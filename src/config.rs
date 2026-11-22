//! The OAuth client ids scan2fil signs in with. They are public by design
//! (a phone app cannot keep a secret); api0 accepts id_tokens issued to them
//! once they are listed in its API0_NATIVE_GOOGLE_CLIENT_IDS and
//! API0_NATIVE_MICROSOFT_CLIENT_IDS.
//!
//! Set at build time, so nothing is committed before the clients exist:
//!   SCAN2FIL_GOOGLE_CLIENT_ID     an iOS client of Google Cloud,
//!                                 `1234-abc.apps.googleusercontent.com`
//!   SCAN2FIL_MICROSOFT_CLIENT_ID  an Entra app registration's id, with the
//!                                 iOS redirect `msauth.com.mayorana.scan2fil://auth`

pub const BUNDLE_ID: &str = "com.mayorana.scan2fil";

pub fn google_client_id() -> &'static str {
    option_env!("SCAN2FIL_GOOGLE_CLIENT_ID").unwrap_or("")
}

pub fn microsoft_client_id() -> &'static str {
    option_env!("SCAN2FIL_MICROSOFT_CLIENT_ID").unwrap_or("")
}
