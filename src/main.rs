//! scan2fil: photograph a form or a letter that asks for information, and get
//! the questions to answer to fill it in.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod api0;
mod auth;
mod config;
mod filled;
mod layout;
mod photo;
mod settings;
mod sheet;

use api0::{Field, Payer, Reading};
use auth::Provider;
use dioxus::prelude::*;
use settings::Settings;
use std::sync::Arc;

const STYLE: Asset = asset!("/assets/main.css");
const LOGO: Asset = asset!("/assets/logo.svg");

/// Catches the photo inputs at the document, before Dioxus's own click
/// listener, whenever they are on screen; the chosen file goes back as base64
/// to the Home screen showing now.
const PICKER_JS: &str = r#"
window.__scan2filSend = (b64) => dioxus.send(b64);
if (!window.__scan2filPicker) {
  window.__scan2filPicker = true;
  const isPicker = (t) => t instanceof HTMLInputElement && t.type === "file" && t.id.startsWith("pick-");
  document.addEventListener("click", (e) => { if (isPicker(e.target)) e.stopPropagation(); }, true);
  document.addEventListener("change", (e) => {
    const input = e.target;
    if (!isPicker(input)) return;
    e.stopPropagation();
    const file = input.files && input.files[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      input.value = "";
      window.__scan2filSend(String(reader.result).split(",")[1] || "");
    };
    reader.readAsDataURL(file);
  }, true);
}
"#;

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, PartialEq)]
enum Screen {
    Home,
    Reading,
    Result(Box<Reading>),
    Account,
}

/// Saves `s` and makes it current; a save that fails is shown, not lost.
fn commit(mut settings: Signal<Settings>, s: Settings) -> Result<(), String> {
    let saved = settings::save(&s);
    settings.set(s);
    saved
}

#[component]
fn App() -> Element {
    let mut screen = use_signal(|| Screen::Home);
    let settings = use_signal(settings::load);
    let mut error = use_signal(|| None::<String>);
    // The answers typed, one per field, kept apart from the reading.
    let mut answers = use_signal(Vec::<String>::new);
    // The photo the reading is of: the filled form is drawn on it.
    let mut page = use_signal(|| None::<Arc<photo::Page>>);

    let on_photo = move |bytes: Vec<u8>| async move {
        error.set(None);
        let s = settings.peek().clone();
        let payer = if s.signed_in() {
            Payer::Licence(s.licence.clone())
        } else if !s.trial_used {
            match sheet::device_id() {
                Ok(device) => {
                    // Kept in the settings in memory too: saving them later
                    // must not erase an id kept only there.
                    if s.device_id != device {
                        let mut kept = settings.peek().clone();
                        kept.device_id = device.clone();
                        let _ = commit(settings, kept);
                    }
                    Payer::Trial { device }
                }
                Err(e) => return error.set(Some(e)),
            }
        } else {
            return error.set(Some("Sign in to read more documents.".into()));
        };
        screen.set(Screen::Reading);
        let outcome: Result<(Reading, photo::Page), api0::ReadError> = async {
            let mut prepared = tokio::task::spawn_blocking(move || photo::prepare(&bytes))
                .await
                .map_err(|e| format!("could not prepare the photo: {e}"))??;
            // The phone reads the page's text while api0 reads the document:
            // it tells where each answer goes. Without it, the model's guesses
            // are used.
            let jpeg = prepared.jpeg.clone();
            let text = tokio::task::spawn_blocking(move || sheet::read_text(&jpeg));
            // A region saved by an older build may no longer be offered.
            let reading = api0::read_document(&payer, api0::region(&s.region).id, &prepared.jpeg).await?;
            prepared.lines = text.await.ok().and_then(Result::ok).unwrap_or_default();
            Ok((reading, prepared))
        }
        .await;

        let trial = matches!(payer, Payer::Trial { .. });
        match outcome {
            Ok((reading, prepared)) => {
                page.set(Some(Arc::new(prepared)));
                if trial {
                    let mut s = settings.peek().clone();
                    s.trial_used = true;
                    let _ = commit(settings, s);
                }
                answers.set(reading.fields.iter().map(|f| f.suggested.clone()).collect());
                screen.set(Screen::Result(Box::new(reading)));
            }
            Err(e) => {
                if e.trial_used {
                    let mut s = settings.peek().clone();
                    s.trial_used = true;
                    let _ = commit(settings, s);
                }
                error.set(Some(e.message));
                screen.set(Screen::Home);
            }
        }
    };

    let current = screen.read().clone();
    rsx! {
        document::Stylesheet { href: STYLE }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        div { class: "app",
            header { class: "bar",
                if matches!(current, Screen::Home) {
                    div { class: "brand",
                        img { src: LOGO, alt: "" }
                        h1 { "scan2fil" }
                    }
                } else {
                    button { class: "link", onclick: move |_| screen.set(Screen::Home), "‹ Back" }
                }
                if !matches!(current, Screen::Account) {
                    button { class: "link", onclick: move |_| screen.set(Screen::Account),
                        if settings.read().signed_in() { "Account" } else { "Sign in" }
                    }
                }
            }
            match current {
                Screen::Home => rsx! { Home { settings, error, on_photo } },
                Screen::Reading => rsx! {
                    div { class: "center",
                        div { class: "spinner" }
                        p { "Reading the document…" }
                        p { class: "muted", "This takes 10 to 30 seconds." }
                    }
                },
                Screen::Result(reading) => rsx! { Questions { reading: *reading, answers, page } },
                Screen::Account => rsx! { AccountScreen { settings } },
            }
        }
    }
}

#[component]
fn Home(settings: Signal<Settings>, error: Signal<Option<String>>, on_photo: EventHandler<Vec<u8>>) -> Element {
    let s = settings.read().clone();
    let locked = !s.can_read();
    // The photo pickers are plain file inputs that Dioxus must not see: it
    // turns a click on one into a request for a native file dialog, which
    // iOS does not have, so no picker opens. The click is stopped at the
    // input, iOS shows its own picker, and the chosen file comes back here.
    use_future(move || async move {
        let mut picked = document::eval(PICKER_JS);
        while let Ok(b64) = picked.recv::<String>().await {
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) {
                Ok(bytes) => on_photo.call(bytes),
                Err(e) => error.set(Some(format!("could not open the photo: {e}"))),
            }
        }
    });
    rsx! {
        main { class: "home",
            p { class: "lead", "Photograph a form or a letter that asks you for information. scan2fil lists the questions to answer to fill it in." }
            if !s.signed_in() && !s.trial_used {
                p { class: "note", "Try it on one document, no account needed." }
            }
            if let Some(e) = error() {
                p { class: "error", "{e}" }
            }
            if locked {
                section { class: "card",
                    h3 { "Your free document is used" }
                    p { class: "muted small", "Sign in to keep going. A free account comes with AI credits every month." }
                    SignInButtons { settings }
                }
            }
            label { class: "big primary",
                "📷 Take a photo"
                input { id: "pick-camera", r#type: "file", accept: "image/*", "capture": "environment", disabled: locked }
            }
            label { class: "big",
                "🖼 Choose from photos"
                input { id: "pick-library", r#type: "file", accept: "image/*", disabled: locked }
            }
            if !s.signed_in() && !locked {
                section { class: "card",
                    h3 { "Sign in for more" }
                    p { class: "muted small", "A free account reads more documents each month." }
                    SignInButtons { settings }
                }
            }
            p { class: "muted small", "The photo is sent to api0 to be read, then discarded. Nothing is kept on a server." }
        }
    }
}

#[component]
fn SignInButtons(settings: Signal<Settings>) -> Element {
    let mut busy = use_signal(|| None::<Provider>);
    let mut error = use_signal(|| None::<String>);
    let sign_in = move |provider: Provider| async move {
        if busy.peek().is_some() {
            return;
        }
        busy.set(Some(provider));
        error.set(None);
        match auth::sign_in(provider).await {
            Ok(account) => {
                let mut s = settings.peek().clone();
                s.licence = account.licence;
                s.email = account.email;
                s.credits_left = account.credits_left;
                if let Err(e) = commit(settings, s) {
                    error.set(Some(e));
                }
            }
            Err(e) => error.set(Some(e)),
        }
        busy.set(None);
    };
    let label = |p: Provider| {
        if busy() == Some(p) {
            "Signing in…".to_string()
        } else {
            format!("Continue with {}", p.label())
        }
    };
    rsx! {
        div { class: "signin",
            button { class: "big provider google", disabled: busy().is_some(),
                onclick: move |_| sign_in(Provider::Google),
                span { class: "mark", "G" } "{label(Provider::Google)}"
            }
            button { class: "big provider microsoft", disabled: busy().is_some(),
                onclick: move |_| sign_in(Provider::Microsoft),
                span { class: "mark ms",
                    span {} span {} span {} span {}
                }
                "{label(Provider::Microsoft)}"
            }
            if let Some(e) = error() { p { class: "error small", "{e}" } }
        }
    }
}

#[component]
fn Questions(reading: Reading, answers: Signal<Vec<String>>, page: Signal<Option<Arc<photo::Page>>>) -> Element {
    let doc = reading.document.clone();
    let mut copied = use_signal(|| false);
    let mut show_filled = use_signal(|| false);
    if show_filled() {
        if let Some(p) = page() {
            return rsx! { FilledForm { reading, answers, page: p, show_filled } };
        }
    }
    let fields = reading.fields.clone();
    let filled = {
        let a = answers.read();
        fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let v = a.get(i).map(|s| s.trim()).unwrap_or("");
                (!v.is_empty()).then(|| format!("{}: {}", f.label.trim_end_matches([':', ' ']), v))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let missing = fields
        .iter()
        .enumerate()
        .filter(|(i, f)| f.required && answers.read().get(*i).is_none_or(|v| v.trim().is_empty()))
        .count();

    rsx! {
        main { class: "result",
            section { class: "card doc",
                h2 { if doc.title.is_empty() { "Document" } else { "{doc.title}" } }
                if !doc.sender.is_empty() { p { class: "muted", "{doc.sender}" } }
                if !doc.summary.is_empty() { p { "{doc.summary}" } }
                dl {
                    if !doc.reference.is_empty() { dt { "Reference" } dd { "{doc.reference}" } }
                    if !doc.deadline.is_empty() { dt { "Deadline" } dd { "{doc.deadline}" } }
                    if !doc.return_by.is_empty() { dt { "Send back" } dd { "{doc.return_by}" } }
                }
            }
            if fields.is_empty() {
                p { class: "warn", "No field to fill in was found on this photo." }
            }
            for (i, field) in fields.into_iter().enumerate() {
                QuestionCard { key: "{i}", index: i, field, answers }
            }
            if !reading.notes.is_empty() {
                section { class: "card",
                    h3 { "Also" }
                    ul { for n in reading.notes.iter() { li { "{n}" } } }
                }
            }
            section { class: "card",
                h3 { "Your answers" }
                if missing > 0 {
                    p { class: "muted", "{missing} required question(s) still unanswered." }
                }
                textarea { class: "summary", readonly: true, rows: 8, value: "{filled}" }
                button { class: "big primary",
                    onclick: move |_| {
                        let text = serde_json::to_string(&filled).unwrap_or_default();
                        document::eval(&format!("navigator.clipboard.writeText({text})"));
                        copied.set(true);
                    },
                    if copied() { "Copied ✓" } else { "Copy answers" }
                }
                if page.read().is_some() && !reading.fields.is_empty() {
                    button { class: "big",
                        onclick: move |_| {
                            show_filled.set(true);
                            document::eval("window.scrollTo(0, 0)");
                        },
                        "📄 Show filled form"
                    }
                }
            }
        }
    }
}

#[component]
fn QuestionCard(index: usize, field: Field, answers: Signal<Vec<String>>) -> Element {
    let value = answers.read().get(index).cloned().unwrap_or_default();
    // The input keeps its own text while it is typed in: setting its value
    // on each keystroke races the typing and drops letters. A button that
    // fills it in makes a new input instead (`rev`).
    let mut rev = use_signal(|| 0u32);
    let mut set = move |v: String| {
        let mut a = answers.write();
        if a.len() <= index {
            a.resize(index + 1, String::new());
        }
        a[index] = v;
        drop(a);
        rev += 1;
    };
    rsx! {
        section { class: "card q",
            div { class: "q-head",
                span { class: "label", "{field.label}" }
                if !field.required { span { class: "tag", "optional" } }
            }
            p { class: "question", "{field.question}" }
            if !field.only_if.is_empty() { p { class: "muted small", "Only if: {field.only_if}" } }
            if !field.choices.is_empty() {
                div { class: "choices",
                    for c in field.choices.iter().filter(|c| !c.label.trim().is_empty()) {
                        button {
                            class: if value.trim().eq_ignore_ascii_case(c.label.trim()) { "chip on" } else { "chip" },
                            onclick: {
                                let label = c.label.trim().to_string();
                                move |_| set(label.clone())
                            },
                            "{c.label}"
                        }
                    }
                }
            }
            input {
                key: "{rev}",
                class: "answer",
                initial_value: "{value}",
                placeholder: "{field.hint}",
                oninput: move |e| {
                    let mut a = answers.write();
                    if a.len() <= index { a.resize(index + 1, String::new()); }
                    a[index] = e.value();
                },
            }
            if !field.suggested.is_empty() && field.suggested != value {
                button { class: "chip", onclick: {
                    let s = field.suggested.clone();
                    move |_| set(s.clone())
                }, "Use “{field.suggested}”" }
            }
        }
    }
}

/// The answers written on the page photo, with the PDF to share.
#[component]
fn FilledForm(reading: Reading, answers: Signal<Vec<String>>, page: Arc<photo::Page>, show_filled: Signal<bool>) -> Element {
    let fields = layout::snap(&reading.fields, &page.lines);
    let (marks, unplaced) = filled::marks(&fields, &answers.read());
    let preview = filled::svg(&page, &marks);
    let mut status = use_signal(|| None::<String>);
    let share = {
        let page = page.clone();
        let marks = marks.clone();
        let name = filled::file_name(&reading.document.title);
        move |_| {
            let path = std::env::temp_dir().join(&name);
            let result = std::fs::write(&path, filled::pdf(&page, &marks))
                .map_err(|e| format!("could not save the PDF: {e}"))
                .and_then(|()| sheet::share_file(&path));
            status.set(result.err());
        }
    };
    rsx! {
        main { class: "result",
            button { class: "link", onclick: move |_| show_filled.set(false), "‹ Back to the questions" }
            if marks.is_empty() {
                p { class: "warn", "Answer at least one question to see it on the form." }
            }
            if unplaced > 0 {
                p { class: "muted small", "{unplaced} answer(s) could not be placed on the page: write them by hand, or use Copy answers." }
            }
            div { class: "filled", dangerous_inner_html: "{preview}" }
            p { class: "muted small", "Check each answer sits on its line before you send the form." }
            if let Some(e) = status() { p { class: "error", "{e}" } }
            button { class: "big primary", onclick: share, "Share PDF" }
        }
    }
}

#[component]
fn AccountScreen(settings: Signal<Settings>) -> Element {
    let s = settings.read().clone();
    let mut error = use_signal(|| None::<String>);
    rsx! {
        main { class: "settings",
            section { class: "card",
                if s.signed_in() {
                    h3 { "Signed in" }
                    p { "{s.email}" }
                    if let Some(c) = s.credits_left {
                        p { class: "muted small", "{c} AI credits left this month." }
                    }
                    button { class: "big",
                        onclick: move |_| {
                            let mut s = settings.peek().clone();
                            s.licence.clear();
                            s.email.clear();
                            s.credits_left = None;
                            if let Err(e) = commit(settings, s) { error.set(Some(e)); }
                        },
                        "Sign out"
                    }
                } else {
                    h3 { "Sign in" }
                    p { class: "muted small",
                        if s.trial_used { "Your free document is used. " } else { "You can read one document without an account. " }
                        "A free account comes with AI credits every month."
                    }
                    SignInButtons { settings }
                }
            }
            section { class: "card",
                h3 { "Region" }
                p { class: "muted small", "Where the document is read. api0 picks the model." }
                for r in api0::REGIONS.iter() {
                    label { class: "radio",
                        input { r#type: "radio", name: "region", checked: s.region == r.id,
                            onchange: move |_| {
                                let mut s = settings.peek().clone();
                                s.region = r.id.to_string();
                                if let Err(e) = commit(settings, s) { error.set(Some(e)); }
                            }
                        }
                        "{r.display}"
                    }
                }
            }
            if let Some(e) = error() { p { class: "error", "{e}" } }
        }
    }
}
