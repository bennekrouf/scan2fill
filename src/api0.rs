//! The one model call: a photo of a document in, the questions to fill it out.
//!
//! api0's gateway is OpenAI-compatible, the same one GitAgent uses: `POST
//! {GATEWAY}/chat/completions`, the licence number as the bearer key, and the
//! region as the model — api0 picks what runs there. The gateway forwards the
//! body as it is, so the photo goes as an OpenAI `image_url` with a data URL.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub const GATEWAY: &str = "https://gateway.api0.ai/v1";

/// What scan2fil calls itself to api0 (`X-Client-App`), for its usage log.
const CLIENT_APP: &str = "scan2fil";

/// A whole form's questions fit well inside this; a reply cut here is reported.
const ANSWER_TOKENS: u32 = 8192;

const TIMEOUT: Duration = Duration::from_secs(120);

/// An api0 region, as api0 lists it (`GET {GATEWAY}/regions`): which ones
/// are open, and the flag and name to show for each. api0 decides; the app
/// only offers what it lists as available.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub struct Region {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub flag: String,
    #[serde(default)]
    pub available: bool,
}

/// What is offered until api0 has answered, or when it cannot be reached.
pub fn known_regions() -> Vec<Region> {
    vec![Region {
        id: "europe".into(),
        label: "Europe".into(),
        flag: "🇪🇺".into(),
        available: true,
    }]
}

/// The regions api0 lists now. Needs no licence.
pub async fn fetch_regions() -> Result<Vec<Region>, String> {
    #[derive(Deserialize)]
    struct List {
        regions: Vec<Region>,
    }
    let list: List = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("{GATEWAY}/regions"))
        .header("X-Client-App", CLIENT_APP)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if list.regions.iter().any(|r| r.available) {
        Ok(list.regions)
    } else {
        Err("api0 lists no open region".into())
    }
}

/// The region a reading goes to: the saved one while api0 keeps it open,
/// else the first one open.
pub fn chosen<'a>(regions: &'a [Region], saved: &str) -> Option<&'a Region> {
    let open = || regions.iter().filter(|r| r.available);
    open().find(|r| r.id == saved).or_else(|| open().next())
}

/// What the document is, as read from the photo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub return_by: String,
    #[serde(default)]
    pub summary: String,
}

/// One blank on the document, as a question to the person filling it in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// The field's label as printed.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub question: String,
    /// Expected format or an example.
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub required: bool,
    /// When the field can be left empty, e.g. "only if you live abroad".
    #[serde(default)]
    pub only_if: String,
    /// A standard example answer, as someone in Paris would give it ("Jean
    /// Dupont", "12 rue de Rivoli, 75001 Paris"), never the document's own data.
    #[serde(default)]
    pub suggested: String,
    /// Where the answer is written on the page, if the model could tell.
    #[serde(default, rename = "box", deserialize_with = "lenient_area")]
    pub area: Option<Area>,
    /// The tick boxes, when the field is a choice between printed options.
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// The reader signs here rather than writes.
    #[serde(default)]
    pub signature: bool,
}

/// One printed option of a choice field, and where its tick box is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub label: String,
    #[serde(default, rename = "box", deserialize_with = "lenient_area")]
    pub area: Option<Area>,
}

/// A rectangle on the page, in fractions of its width and height from the
/// top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    /// `[x, y, width, height]` as the model wrote it: fractions of the page,
    /// or thousandths (the scale some vision models use). Anything else is
    /// dropped rather than drawn in the wrong place.
    pub fn from_numbers(v: &[f64]) -> Option<Area> {
        let [x, y, w, h] = <[f64; 4]>::try_from(v).ok()?;
        let max = x.max(y).max(w).max(h);
        let scale = if max <= 1.0 {
            1.0
        } else if max <= 1000.0 {
            1000.0
        } else {
            return None;
        };
        let (x, y, w, h) = (x / scale, y / scale, w / scale, h / scale);
        let fits = x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0 && x + w <= 1.02 && y + h <= 1.02;
        fits.then_some(Area {
            x: x as f32,
            y: y as f32,
            w: w as f32,
            h: h as f32,
        })
    }
}

fn lenient_area<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Area>, D::Error> {
    let v = Value::deserialize(d)?;
    let numbers: Option<Vec<f64>> = v
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).collect());
    Ok(numbers.and_then(|n| Area::from_numbers(&n)))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub document: Document,
    #[serde(default)]
    pub fields: Vec<Field>,
    /// Anything else the person must do or attach.
    #[serde(default)]
    pub notes: Vec<String>,
}

const SYSTEM: &str = r#"You read a photo of a paper document — a form, or a letter asking for information — for someone who has to fill it in.

Find every blank, line, box or item the reader is asked to fill in or send back, in the order they appear. For each one, write the question to ask the reader so that their answer is exactly what goes in that blank.

Rules:
- Write everything in the document's language.
- "label" is the field's label as printed; "question" is a short, plain question to the reader.
- "hint": the expected format or an example (e.g. an IBAN's shape), or "".
- "required": false when the document says a field is optional or conditional; then "only_if" says when it is needed (e.g. "only if you live abroad").
- "suggested": a standard example answer for this field, as a person living in Paris, France would give it, in the format the field expects: e.g. name "Jean Dupont", first name "Jean", address "12 rue de Rivoli, 75001 Paris", city "Paris", postcode "75001", country "France", phone "06 12 34 56 78", email "jean.dupont@exemple.fr", date of birth "15/03/1985", place of birth "Paris", nationality "française", IBAN "FR76 3000 6000 0112 3456 7890 189". Never copy a name, address or other personal detail printed on the document, such as the addressee's. "" for choice fields, signatures, and fields with no sensible example (a reference only the sender knows, say).
- "document": what the document is ("title"), who sent it ("sender"), any reference or file number ("reference"), the deadline to reply as printed ("deadline"), how to send it back ("return_by": postal address or email as printed), and a one-sentence "summary" of what is asked and why.
- "box": where the reader's answer is written on the page, as [x, y, width, height] in fractions of the photo's width and height (0 to 1), from the photo's top-left corner. Cover only the blank line, empty box or space left for the answer, not the printed label. null when you cannot tell.
- "choices": when the field is a set of printed options to tick or circle, one entry per option: {"label": the option as printed, "box": [x, y, width, height] of its tick box, as above}; the question then asks which one applies. [] otherwise.
- "signature": true when the reader signs there (a signature line or box), with "box" covering the space to sign in; false otherwise.
- "notes": other things the reader must do or attach, each one short (e.g. documents needed in a special case). [] when none.
- If the photo shows no document to fill in, return "fields": [] and say so in "summary".

Reply with one JSON object and nothing else, of this shape:
{"language": "fr", "document": {"title": "", "sender": "", "reference": "", "deadline": "", "return_by": "", "summary": ""}, "fields": [{"label": "", "question": "", "hint": "", "required": true, "only_if": "", "suggested": "", "box": [0.1, 0.2, 0.5, 0.03], "choices": [], "signature": false}], "notes": []}"#;

/// Who pays for a read: the signed-in person's licence, or the one free try
/// this phone gets (`/v1/trial/chat/completions`, once per device).
#[derive(Clone, Debug, PartialEq)]
pub enum Payer {
    Licence(String),
    Trial { device: String },
}

/// Why a read failed, and whether it was because the free try is used.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadError {
    pub message: String,
    pub trial_used: bool,
}

impl From<String> for ReadError {
    fn from(message: String) -> Self {
        ReadError {
            message,
            trial_used: false,
        }
    }
}

impl From<&str> for ReadError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

/// Asks api0 to read `jpeg` and returns the questions.
pub async fn read_document(payer: &Payer, region: &str, jpeg: &[u8]) -> Result<Reading, ReadError> {
    let data_url = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    );
    let body = json!({
        "model": region,
        "stream": false,
        "temperature": 0,
        "max_tokens": ANSWER_TOKENS,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": SYSTEM },
            { "role": "user", "content": [
                { "type": "text", "text": "Here is the document. List the questions to fill it in." },
                { "type": "image_url", "image_url": { "url": data_url } },
            ]},
        ],
    });

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| format!("could not start the connection: {e}"))?;
    let request = match payer {
        Payer::Licence(licence) => client
            .post(format!("{GATEWAY}/chat/completions"))
            .bearer_auth(licence.trim()),
        Payer::Trial { device } => client
            .post(format!("{GATEWAY}/trial/chat/completions"))
            .header("X-Device-Id", device),
    };
    let resp = request
        .header("X-Client-App", CLIENT_APP)
        .header("X-Task", "text")
        .header("X-Feature", "read_document")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                crate::i18n::msg("api0 took too long to answer. Try again.", "api0 a mis trop de temps à répondre. Réessayez.")
            } else {
                format!("{} ({e})", crate::i18n::msg("api0 is unreachable. Check your connection.", "api0 est injoignable. Vérifiez votre connexion."))
            }
        })?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refused(
            status.as_u16(),
            &text,
            matches!(payer, Payer::Trial { .. }),
        ));
    }

    let v: Value =
        serde_json::from_str(&text).map_err(|e| format!("api0 sent invalid JSON: {e}"))?;
    if v["choices"][0]["finish_reason"] == "length" {
        return Err(
            crate::i18n::msg("The answer was cut off before the end. Read the page again: a second read usually completes.", "La réponse a été coupée avant la fin. Relisez la page : une deuxième lecture va en général jusqu'au bout.").into(),
        );
    }
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("api0's answer had no text in it.")?;
    Ok(parse_reading(content)?)
}

/// What to tell the person about a refused call.
fn refused(status: u16, body: &str, trial: bool) -> ReadError {
    let v = serde_json::from_str::<Value>(body).unwrap_or_default();
    let message = v["error"]["message"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(300).collect());
    if trial {
        let used = v["error"]["code"] == "trial_used";
        return ReadError {
            message,
            trial_used: used,
        };
    }
    let message = match status {
        401 | 403 => format!("{} {message}", crate::i18n::msg("api0 did not accept your licence. Sign in again.", "api0 n'a pas accepté votre licence. Reconnectez-vous.")),
        402 => format!("{} {message}", crate::i18n::msg("No AI credits left this month.", "Plus de crédits IA ce mois-ci.")),
        413 => crate::i18n::msg("The photo is too large to send.", "La photo est trop lourde pour être envoyée."),
        _ => format!("api0 returned {status}: {message}"),
    };
    message.into()
}

/// The model's reply as a `Reading`; tolerates a ```json fence around it.
pub fn parse_reading(raw: &str) -> Result<Reading, String> {
    let t = raw.trim();
    let body = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .map(|s| s.trim().trim_end_matches("```").trim())
        .unwrap_or(t);
    serde_json::from_str(body)
        .map_err(|e| format!("The model's answer was not the expected JSON ({e})."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_reply_with_missing_keys() {
        let raw = "```json\n{\"document\":{\"title\":\"Remboursement\"},\"fields\":[{\"label\":\"IBAN\",\"question\":\"Quel est votre IBAN ?\",\"required\":true}]}\n```";
        let r = parse_reading(raw).unwrap();
        assert_eq!(r.document.title, "Remboursement");
        assert_eq!(r.fields.len(), 1);
        assert_eq!(r.fields[0].hint, "");
        assert!(r.notes.is_empty());
    }

    #[test]
    fn reads_boxes_in_fractions_or_thousandths_and_drops_the_rest() {
        let raw = r#"{"fields":[
            {"label":"Nom","box":[0.1,0.2,0.5,0.03]},
            {"label":"Date","box":[100,200,500,30]},
            {"label":"Adresse","box":[0.9,0.2,0.5,0.03]},
            {"label":"NPA","box":null},
            {"label":"Ville","box":[1,2]},
            {"label":"Véhicule","choices":[{"label":"à mon nom","box":[0.2,0.5,0.02,0.02]}]}
        ]}"#;
        let r = parse_reading(raw).unwrap();
        let a = r.fields[0].area.unwrap();
        assert!((a.x - 0.1).abs() < 1e-6 && (a.h - 0.03).abs() < 1e-6);
        let b = r.fields[1].area.unwrap();
        assert!((b.y - 0.2).abs() < 1e-6);
        assert_eq!(r.fields[2].area, None, "runs off the page");
        assert_eq!(r.fields[3].area, None);
        assert_eq!(r.fields[4].area, None);
        assert!(r.fields[5].choices[0].area.is_some());
    }

    #[test]
    fn the_saved_region_is_used_only_while_it_is_open() {
        let list: Vec<Region> = serde_json::from_str(
            r#"[{"id":"europe","label":"Europe","flag":"🇪🇺","available":true},
                {"id":"china","label":"China","flag":"🇨🇳","available":true},
                {"id":"usa","label":"United States","flag":"🇺🇸","available":false}]"#,
        )
        .unwrap();
        assert_eq!(chosen(&list, "china").unwrap().id, "china");
        assert_eq!(chosen(&list, "usa").unwrap().id, "europe", "closed");
        assert_eq!(chosen(&list, "mars").unwrap().id, "europe");
        assert_eq!(chosen(&[], "europe"), None);
    }
}
