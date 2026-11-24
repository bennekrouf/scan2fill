//! Signing in with Google or Microsoft, and the licence that comes with it.
//!
//! 1. The provider's sign-in page opens in the system sign-in sheet
//!    (ASWebAuthenticationSession), as an OAuth public client: authorization
//!    code with PKCE, and a nonce made for this sign-in.
//! 2. The code is exchanged for an id_token, with the PKCE verifier.
//! 3. api0 checks the id_token (`POST /api/auth/native`) and returns its own
//!    session token for the verified address.
//! 4. With that token, `GET /api/plans/subscription` returns the person's
//!    licence; a newcomer gets the free allowance there.
//!
//! The licence is what every model call is paid with afterwards; the session
//! token is not kept.

use crate::config;
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;

const API: &str = "https://gateway.api0.ai/api";
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Google,
    Microsoft,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Google => "Google",
            Provider::Microsoft => "Microsoft",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Microsoft => "microsoft",
        }
    }

    fn client_id(self) -> &'static str {
        match self {
            Provider::Google => config::google_client_id(),
            Provider::Microsoft => config::microsoft_client_id(),
        }
    }

    /// The scheme the sign-in sheet waits for, and the redirect URI with it.
    fn redirect(self) -> (String, String) {
        match self {
            // Google's iOS clients redirect to the client id reversed.
            Provider::Google => {
                let id = self
                    .client_id()
                    .trim_end_matches(".apps.googleusercontent.com");
                let scheme = format!("com.googleusercontent.apps.{id}");
                let uri = format!("{scheme}:/oauth2redirect");
                (scheme, uri)
            }
            Provider::Microsoft => {
                let scheme = format!("msauth.{}", config::BUNDLE_ID);
                let uri = format!("{scheme}://auth");
                (scheme, uri)
            }
        }
    }

    fn authorize_url(self) -> &'static str {
        match self {
            Provider::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Provider::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Provider::Google => "https://oauth2.googleapis.com/token",
            Provider::Microsoft => "https://login.microsoftonline.com/common/oauth2/v2.0/token",
        }
    }
}

/// Who signed in, and the licence their calls are paid with.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub email: String,
    pub licence: String,
    pub credits_left: Option<i64>,
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| format!("no randomness available: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

fn challenge_of(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn encode(s: &str) -> String {
    // Enough for the values here: ids, URIs, scopes and base64url.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The query parameter `name` of `url`, decoded.
pub fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| decode(v))
    })
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Signs in with `provider` and returns the account and its licence.
pub async fn sign_in(provider: Provider) -> Result<Account, String> {
    if provider.client_id().is_empty() {
        return Err(if crate::i18n::french_now() {
            format!("La connexion {} n'est pas encore prête dans cette version.", provider.label())
        } else {
            format!("{} sign-in is not set up in this build yet.", provider.label())
        });
    }
    let verifier = random_token(48)?;
    let nonce = random_token(24)?;
    let state = random_token(24)?;
    let (scheme, redirect) = provider.redirect();

    let mut url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&nonce={}",
        provider.authorize_url(),
        encode(provider.client_id()),
        encode(&redirect),
        encode("openid email profile"),
        challenge_of(&verifier),
        state,
        nonce,
    );
    match provider {
        Provider::Google => url.push_str("&prompt=select_account"),
        Provider::Microsoft => url.push_str("&response_mode=query&prompt=select_account"),
    }

    let callback = crate::sheet::open(&url, &scheme).await?;
    if let Some(err) = query_param(&callback, "error") {
        let detail = query_param(&callback, "error_description").unwrap_or_default();
        return Err(
            format!("{} refused the sign-in: {err} {detail}", provider.label())
                .trim()
                .to_string(),
        );
    }
    if query_param(&callback, "state").as_deref() != Some(state.as_str()) {
        return Err(crate::i18n::msg("The sign-in answer did not match this request. Try again.", "La réponse de connexion ne correspond pas à cette demande. Réessayez."));
    }
    let code = query_param(&callback, "code").ok_or("The sign-in returned no code.")?;

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", redirect),
        ("client_id", provider.client_id().to_string()),
        ("code_verifier", verifier),
    ];
    if provider == Provider::Microsoft {
        form.push(("scope", "openid email profile".into()));
    }
    let resp = client
        .post(provider.token_url())
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("{} could not be reached: {e}", provider.label()))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        let why = body["error_description"]
            .as_str()
            .or(body["error"].as_str())
            .unwrap_or("");
        return Err(format!(
            "{} refused the sign-in ({status}). {why}",
            provider.label()
        ));
    }
    let id_token = body["id_token"]
        .as_str()
        .ok_or("The sign-in returned no identity.")?;

    // api0: id_token → session → licence.
    let resp = client
        .post(format!("{API}/auth/native"))
        .json(&serde_json::json!({ "provider": provider.key(), "id_token": id_token, "nonce": nonce }))
        .send()
        .await
        .map_err(|e| format!("api0 could not be reached: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(api0_error(&body, status.as_u16()));
    }
    let session = body["token"].as_str().ok_or("api0 returned no session.")?;
    let email = body["email"].as_str().unwrap_or_default().to_string();

    let resp = client
        .get(format!("{API}/plans/subscription"))
        .header("X-Google-Auth", session)
        .header("X-Client-App", "scan2fil")
        .send()
        .await
        .map_err(|e| format!("api0 could not be reached: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(api0_error(&body, status.as_u16()));
    }
    let sub = &body["subscription"];
    let licence = sub["licence"]
        .as_str()
        .filter(|l| !l.is_empty())
        .ok_or_else(|| crate::i18n::msg("Your account has no licence yet. Try again in a minute.", "Votre compte n'a pas encore de licence. Réessayez dans une minute."))?
        .to_string();
    Ok(Account {
        email,
        licence,
        credits_left: sub["credits_left"].as_i64(),
    })
}

fn api0_error(body: &Value, status: u16) -> String {
    let message = body["error"]["message"]
        .as_str()
        .or(body["message"].as_str())
        .or(body["error"].as_str())
        .unwrap_or("");
    format!("api0 refused the sign-in ({status}). {message}")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_callback() {
        let cb = "msauth.com.mayorana.scan2fil://auth?code=M.C5%2Fabc&state=xyz#";
        assert_eq!(query_param(cb, "code").unwrap(), "M.C5/abc");
        assert_eq!(query_param(cb, "state").unwrap(), "xyz");
        assert!(query_param(cb, "error").is_none());
    }

    #[test]
    fn pkce_challenge_is_rfc7636() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn redirects_follow_each_provider_rule() {
        let (scheme, uri) = Provider::Microsoft.redirect();
        assert_eq!(scheme, "msauth.com.mayorana.scan2fil");
        assert_eq!(uri, "msauth.com.mayorana.scan2fil://auth");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }
}
