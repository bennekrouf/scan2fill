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
const ANSWER_TOKENS: u32 = 4096;

const TIMEOUT: Duration = Duration::from_secs(120);

/// An api0 region. Only those whose model reads images are offered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub id: &'static str,
    pub display: &'static str,
}

/// China's model (DeepSeek) reads no images, so it is not offered.
pub const REGIONS: &[Region] = &[
    Region { id: "europe", display: "🇪🇺 Europe" },
    Region { id: "usa", display: "🇺🇸 United States" },
];

pub fn region(id: &str) -> &'static Region {
    REGIONS.iter().find(|r| r.id == id).unwrap_or(&REGIONS[0])
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
    /// A value the document itself already shows (the addressee's name, say).
    #[serde(default)]
    pub suggested: String,
    /// Where the answer is written on the page, if the model could tell.
    #[serde(default, rename = "box", deserialize_with = "lenient_area")]
    pub area: Option<Area>,
    /// The tick boxes, when the field is a choice between printed options.
    #[serde(default)]
    pub choices: Vec<Choice>,
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
        let scale = if max <= 1.0 { 1.0 } else if max <= 1000.0 { 1000.0 } else { return None };
        let (x, y, w, h) = (x / scale, y / scale, w / scale, h / scale);
        let fits = x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0 && x + w <= 1.02 && y + h <= 1.02;
        fits.then_some(Area { x: x as f32, y: y as f32, w: w as f32, h: h as f32 })
    }
}

fn lenient_area<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Area>, D::Error> {
    let v = Value::deserialize(d)?;
    let numbers: Option<Vec<f64>> = v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect());
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
- "suggested": a value the document itself already shows for this field (for example the addressee's name and address printed on the letter); otherwise "". Never invent personal data.
- "document": what the document is ("title"), who sent it ("sender"), any reference or file number ("reference"), the deadline to reply as printed ("deadline"), how to send it back ("return_by": postal address or email as printed), and a one-sentence "summary" of what is asked and why.
- "box": where the reader's answer is written on the page, as [x, y, width, height] in fractions of the photo's width and height (0 to 1), from the photo's top-left corner. Cover only the blank line, empty box or space left for the answer, not the printed label. null when you cannot tell.
- "choices": when the field is a set of printed options to tick or circle, one entry per option: {"label": the option as printed, "box": [x, y, width, height] of its tick box, as above}; the question then asks which one applies. [] otherwise.
- "notes": other things the reader must do or attach, each one short (e.g. documents needed in a special case). [] when none.
- If the photo shows no document to fill in, return "fields": [] and say so in "summary".

Reply with one JSON object and nothing else, of this shape:
{"language": "fr", "document": {"title": "", "sender": "", "reference": "", "deadline": "", "return_by": "", "summary": ""}, "fields": [{"label": "", "question": "", "hint": "", "required": true, "only_if": "", "suggested": "", "box": [0.1, 0.2, 0.5, 0.03], "choices": []}], "notes": []}"#;

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
        ReadError { message, trial_used: false }
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
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "api0 took too long to answer. Try again.".to_string()
            } else {
                format!("api0 is unreachable. Check your connection. ({e})")
            }
        })?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refused(status.as_u16(), &text, matches!(payer, Payer::Trial { .. })));
    }

    let v: Value = serde_json::from_str(&text).map_err(|e| format!("api0 sent invalid JSON: {e}"))?;
    if v["choices"][0]["finish_reason"] == "length" {
        return Err("The answer was cut off: the document has too many fields for one read.".into());
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
        return ReadError { message, trial_used: used };
    }
    let message = match status {
        401 | 403 => format!("api0 did not accept your licence. Sign in again. {message}"),
        402 => format!("No AI credits left this month. {message}"),
        413 => "The photo is too large to send.".into(),
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
    serde_json::from_str(body).map_err(|e| format!("The model's answer was not the expected JSON ({e})."))
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
    fn unknown_region_falls_back_to_europe() {
        assert_eq!(region("china").id, "europe");
        assert_eq!(region("usa").id, "usa");
    }
}
