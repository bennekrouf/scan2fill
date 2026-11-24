//! scan2fil: photograph a form or a letter that asks for information, and get
//! the questions to answer to fill it in.
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod api0;
mod auth;
mod config;
mod filled;
mod flatten;
mod i18n;
mod layout;
mod photo;
mod settings;
mod sheet;
mod signature;
mod upright;

use api0::{Field, Payer, Reading};
use auth::Provider;
use dioxus::prelude::*;
use i18n::{french, msg, tr};
use settings::Settings;
use signature::Strokes;
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
    // Before any thread starts: it sets the temporary folder on Android.
    settings::prepare_temp_dir();
    dioxus::launch(App);
}

#[derive(Clone, PartialEq)]
enum Screen {
    Home,
    /// The photo or scan being straightened and shrunk.
    Preparing,
    /// The page as it will be read, to check before a credit is spent.
    Review(Arc<photo::Page>),
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
    // French when the phone is: asked now, so messages made off screen
    // follow it too; on Android, the WebView answers a moment later.
    let _ = french();
    i18n::use_webview_language();
    let settings = use_signal(settings::load);
    // The questions and the filled form read the saved signature from here.
    use_context_provider(|| settings);
    // The regions api0 offers, asked once at start; a built-in list until then.
    let mut regions = use_signal(api0::known_regions);
    use_future(move || async move {
        if let Ok(list) = api0::fetch_regions().await {
            regions.set(list);
        }
    });
    let mut error = use_signal(|| None::<String>);
    // The answers typed, one per field, kept apart from the reading.
    let mut answers = use_signal(Vec::<String>::new);
    // The photo the reading is of: the filled form is drawn on it.
    let mut page = use_signal(|| None::<Arc<photo::Page>>);

    let on_photo = move |(bytes, source): (Vec<u8>, photo::Source)| async move {
        error.set(None);
        screen.set(Screen::Preparing);
        let prepared = tokio::task::spawn_blocking(move || photo::prepare(&bytes, source))
            .await
            .map_err(|e| format!("could not prepare the photo: {e}"))
            .and_then(|p| p);
        match prepared {
            Ok(p) => screen.set(Screen::Review(Arc::new(p))),
            Err(e) => {
                error.set(Some(e));
                screen.set(Screen::Home);
            }
        }
    };

    let on_read = move |prepared: Arc<photo::Page>| async move {
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
                Err(e) => {
                    error.set(Some(e));
                    return screen.set(Screen::Home);
                }
            }
        } else {
            error.set(Some(msg("Sign in to read more documents.", "Connectez-vous pour lire plus de documents.")));
            return screen.set(Screen::Home);
        };
        screen.set(Screen::Reading);
        let outcome: Result<(Reading, photo::Page), api0::ReadError> = async {
            let mut prepared = (*prepared).clone();
            // The phone reads the page's text while api0 reads the document:
            // it tells where each answer goes. Without it, the model's guesses
            // are used.
            let text = sheet::TextReading::start(&prepared.jpeg);
            // A region api0 has since closed is not asked.
            let region = api0::chosen(&regions.peek(), &s.region)
                .map(|r| r.id.clone())
                .unwrap_or_else(|| s.region.clone());
            let reading = api0::read_document(&payer, &region, &prepared.jpeg).await?;
            match text.finish().await {
                Ok(lines) => prepared.lines = lines,
                Err(problem) => prepared.text_problem = Some(problem),
            }
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
                    button { class: "link", onclick: move |_| screen.set(Screen::Home), {tr("‹ Back", "‹ Retour")} }
                }
                div { class: "bar-end",
                    if let Some(r) = api0::chosen(&regions.read(), &settings.read().region).cloned() {
                        button { class: "region-flag",
                            title: if french() { format!("Lecture : {}", r.label) } else { format!("Read in {}", r.label) },
                            aria_label: if french() { format!("Région : {}", r.label) } else { format!("Region: {}", r.label) },
                            onclick: move |_| screen.set(Screen::Account),
                            "{r.flag}"
                        }
                    }
                    if !matches!(current, Screen::Account) {
                        button { class: "link", onclick: move |_| screen.set(Screen::Account),
                            if settings.read().signed_in() { {tr("Account", "Compte")} } else { {tr("Sign in", "Se connecter")} }
                        }
                    }
                }
            }
            match current {
                Screen::Home => rsx! { Home { settings, error, on_photo } },
                Screen::Preparing => rsx! {
                    div { class: "center",
                        div { class: "spinner" }
                        p { {tr("Getting the page ready…", "Préparation de la page…")} }
                    }
                },
                Screen::Review(prepared) => rsx! {
                    CheckPage {
                        page: prepared.clone(),
                        trial: !settings.read().signed_in(),
                        on_read: move |_| {
                            // The phone's text reader is started from the
                            // task: its Android bridge must be made now.
                            sheet::warm_up();
                            let p = prepared.clone();
                            spawn(on_read(p));
                        },
                        on_retake: move |_| screen.set(Screen::Home),
                    }
                },
                Screen::Reading => rsx! {
                    div { class: "center",
                        div { class: "spinner" }
                        p { {tr("Reading the document…", "Lecture du document…")} }
                        p { class: "muted", {tr("This takes 10 to 30 seconds.", "Cela prend 10 à 30 secondes.")} }
                    }
                },
                Screen::Result(reading) => rsx! { Questions { reading: *reading, answers, page } },
                Screen::Account => rsx! { AccountScreen { settings, regions } },
            }
        }
    }
}

#[component]
fn Home(
    settings: Signal<Settings>,
    error: Signal<Option<String>>,
    on_photo: EventHandler<(Vec<u8>, photo::Source)>,
) -> Element {
    let s = settings.read().clone();
    let locked = !s.can_read();
    let mut scanning = use_signal(|| false);
    let scan = move |_| async move {
        if scanning() {
            return;
        }
        scanning.set(true);
        error.set(None);
        match sheet::scan().await {
            Ok(Some(bytes)) => on_photo.call((bytes, photo::Source::Scanner)),
            Ok(None) => {}
            Err(e) => error.set(Some(e)),
        }
        scanning.set(false);
    };
    // The photo pickers are plain file inputs that Dioxus must not see: it
    // turns a click on one into a request for a native file dialog, which
    // iOS does not have, so no picker opens. The click is stopped at the
    // input, iOS shows its own picker, and the chosen file comes back here.
    use_future(move || async move {
        let mut picked = document::eval(PICKER_JS);
        while let Ok(b64) = picked.recv::<String>().await {
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) {
                Ok(bytes) => on_photo.call((bytes, photo::Source::Photo)),
                Err(e) => error.set(Some(format!("could not open the photo: {e}"))),
            }
        }
    });
    rsx! {
        main { class: "home",
            p { class: "lead", {tr("Scan a form or a letter that asks you for information. scan2fil lists the questions to answer to fill it in.", "Scannez un formulaire ou un courrier qui vous demande des informations. scan2fil liste les questions auxquelles répondre pour le remplir.")} }
            if !s.signed_in() && !s.trial_used {
                p { class: "note", {tr("Try it on one document, no account needed.", "Essayez sur un document, sans compte.")} }
            }
            if let Some(e) = error() {
                p { class: "error", "{e}" }
            }
            if locked {
                section { class: "card",
                    h3 { {tr("Your free document is used", "Votre document gratuit est utilisé")} }
                    p { class: "muted small", {tr("Sign in to keep going. A free account comes with AI credits every month.", "Connectez-vous pour continuer. Un compte gratuit inclut des crédits IA chaque mois.")} }
                    SignInButtons { settings }
                }
            }
            if sheet::HAS_SCANNER {
                button { class: "big primary", disabled: locked || scanning(), onclick: scan,
                    {tr("📄 Scan a document", "📄 Scanner un document")}
                }
            }
            label { class: if sheet::HAS_SCANNER { "big" } else { "big primary" },
                {tr("🖼 Choose from photos", "🖼 Choisir dans les photos")}
                input { id: "pick-library", r#type: "file", accept: "image/*", disabled: locked }
            }
            if !s.signed_in() && !locked {
                section { class: "card",
                    h3 { {tr("Sign in for more", "Connectez-vous pour plus")} }
                    p { class: "muted small", {tr("A free account reads more documents each month.", "Un compte gratuit lit plus de documents chaque mois.")} }
                    SignInButtons { settings }
                }
            }
            p { class: "muted small", {tr("The photo is sent to api0 to be read, then discarded. Nothing is kept on a server.", "La photo est envoyée à api0 pour être lue, puis supprimée. Rien n'est conservé sur un serveur.")} }
        }
    }
}

/// The page as it will be read: whole, flat and sharp, before a credit is spent.
#[component]
fn CheckPage(
    page: Arc<photo::Page>,
    trial: bool,
    on_read: EventHandler<()>,
    on_retake: EventHandler<()>,
) -> Element {
    use base64::Engine as _;
    let src = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&page.jpeg)
    );
    rsx! {
        main { class: "review",
            h2 { {tr("Check the page", "Vérifiez la page")} }
            p { class: "muted small",
                {tr("The whole page should be there, straight and sharp, so every answer lands on its line. ", "La page doit être entière, droite et nette, pour que chaque réponse tombe sur sa ligne. ")}
                if trial { {tr("Reading it uses your free document.", "La lire utilise votre document gratuit.")} } else { {tr("Reading it uses AI credits.", "La lire utilise des crédits IA.")} }
            }
            img { class: "review-page", src: "{src}", alt: tr("The page to read", "La page à lire") }
            button { class: "big primary", onclick: move |_| on_read.call(()), {tr("Read this page", "Lire cette page")} }
            button { class: "big", onclick: move |_| on_retake.call(()), {tr("Try again", "Réessayer")} }
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
            msg("Signing in…", "Connexion…")
        } else {
            if french() { format!("Continuer avec {}", p.label()) } else { format!("Continue with {}", p.label()) }
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

/// How many page colours the questions take turns with (`.hue-0` … in main.css).
const HUES: usize = 6;

/// Moves to `to` in the questions and back to the top of the page.
fn go(mut step: Signal<usize>, to: usize) {
    step.set(to);
    document::eval("window.scrollTo(0, 0)");
}

/// The reading, one page at a time: what the document is, then each question
/// on a page of its own, then the answers to check and send.
#[component]
fn Questions(
    reading: Reading,
    answers: Signal<Vec<String>>,
    page: Signal<Option<Arc<photo::Page>>>,
) -> Element {
    // 0 is the document, 1 to n the questions, n + 1 the answers.
    let step = use_signal(|| 0usize);
    let show_filled = use_signal(|| false);
    if show_filled() {
        if let Some(p) = page() {
            return rsx! { FilledForm { reading, answers, page: p, show_filled } };
        }
    }
    let total = reading.fields.len();
    let at = step().min(total + 1);
    if at == 0 {
        return rsx! { Intro { reading, step } };
    }
    if at <= total {
        let index = at - 1;
        let field = reading.fields[index].clone();
        return rsx! { QuestionPage { key: "{index}", index, total, field, answers, step } };
    }
    let can_fill = page.read().is_some() && total > 0;
    rsx! { Review { reading, answers, step, can_fill, show_filled } }
}

/// What the document is, before the first question.
#[component]
fn Intro(reading: Reading, step: Signal<usize>) -> Element {
    let doc = reading.document;
    let total = reading.fields.len();
    rsx! {
        main { class: "step intro hue-2",
            p { class: "step-count", {tr("Your document", "Votre document")} }
            h2 { class: "step-question", if doc.title.is_empty() { "Document" } else { "{doc.title}" } }
            if !doc.sender.is_empty() { p { class: "step-label", "{doc.sender}" } }
            if !doc.summary.is_empty() { p { class: "step-text", "{doc.summary}" } }
            dl {
                if !doc.reference.is_empty() { dt { {tr("Reference", "Référence")} } dd { "{doc.reference}" } }
                if !doc.deadline.is_empty() { dt { {tr("Deadline", "Date limite")} } dd { "{doc.deadline}" } }
                if !doc.return_by.is_empty() { dt { {tr("Send back", "À renvoyer")} } dd { "{doc.return_by}" } }
            }
            if total == 0 {
                p { class: "warn", {tr("No field to fill in was found on this photo.", "Aucun champ à remplir n'a été trouvé sur cette photo.")} }
                if !reading.notes.is_empty() {
                    ul { class: "step-text", for n in reading.notes.iter() { li { "{n}" } } }
                }
            } else {
                p { class: "step-text",
                    if total == 1 { {tr("1 question, one page at a time.", "1 question, une page à la fois.")} } else if french() { "{total} questions, une page à la fois." } else { "{total} questions, one page at a time." }
                }
                div { class: "step-nav",
                    span {}
                    button { class: "step-next", onclick: move |_| go(step, 1), {tr("Start ›", "Commencer ›")} }
                }
            }
        }
    }
}

/// One question on a page of its own: the question first, then the answer.
#[component]
fn QuestionPage(
    index: usize,
    total: usize,
    field: Field,
    answers: Signal<Vec<String>>,
    step: Signal<usize>,
) -> Element {
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
    // This page is step index + 1.
    let (back, next) = (index, index + 2);
    let next_label = if index + 1 == total {
        tr("Check my answers ›", "Vérifier mes réponses ›")
    } else if value.trim().is_empty() && !field.required {
        tr("Skip ›", "Passer ›")
    } else {
        tr("Next ›", "Suivant ›")
    };
    let progress = (index + 1) * 100 / total;
    let signs = signature::is_signature(&field);
    let question = if field.question.trim().is_empty() {
        field.label.clone()
    } else {
        field.question.clone()
    };
    let label = field.label.trim();
    let shows_label = !label.is_empty() && label != question.trim();
    rsx! {
        main { key: "{index}", class: "step hue-{index % HUES}",
            div { class: "progress", div { class: "progress-fill", style: "width: {progress}%" } }
            p { class: "step-count",
                if french() { "Question {index + 1} sur {total}" } else { "Question {index + 1} of {total}" }
                if !field.required { span { class: "step-tag", {tr("optional", "facultatif")} } }
            }
            h2 { class: "step-question", "{question}" }
            if shows_label { p { class: "step-label", if french() { "Sur le formulaire : {label}" } else { "On the form: {label}" } } }
            if !field.only_if.is_empty() { p { class: "step-label", if french() { "Seulement si {field.only_if}" } else { "Only if {field.only_if}" } } }
            if signs {
                SignHere { index, answers }
            } else if !field.choices.is_empty() {
                div { class: "step-choices",
                    for c in field.choices.iter().filter(|c| !c.label.trim().is_empty()) {
                        button {
                            class: if value.trim().eq_ignore_ascii_case(c.label.trim()) { "step-choice on" } else { "step-choice" },
                            onclick: {
                                let label = c.label.trim().to_string();
                                move |_| set(label.clone())
                            },
                            "{c.label}"
                        }
                    }
                }
            }
            // Keyed by the question too: the next page would otherwise reuse
            // this input, with the text typed here still in it.
            if field.choices.is_empty() && !signs { input {
                key: "{index}-{rev}",
                class: "step-answer",
                initial_value: "{value}",
                placeholder: if field.hint.is_empty() { tr("Your answer", "Votre réponse").to_string() } else { field.hint.clone() },
                "enterkeyhint": "next",
                oninput: move |e| {
                    let mut a = answers.write();
                    if a.len() <= index { a.resize(index + 1, String::new()); }
                    a[index] = e.value();
                },
                onkeydown: move |e| if e.key() == Key::Named(NamedKey::Enter) { go(step, next) },
            } }
            if !signs && !field.suggested.is_empty() && field.suggested != value {
                button { class: "step-suggest", onclick: {
                    let s = field.suggested.clone();
                    move |_| set(s.clone())
                }, if french() { "Utiliser « {field.suggested} »" } else { "Use “{field.suggested}”" } }
            }
            div { class: "step-nav",
                button { class: "step-back", onclick: move |_| go(step, back), {tr("‹ Back", "‹ Retour")} }
                button { class: "step-next", onclick: move |_| go(step, next), "{next_label}" }
            }
        }
    }
}

/// Every answer at a glance, each one a tap away from its page, then copy or
/// the filled form.
#[component]
fn Review(
    reading: Reading,
    answers: Signal<Vec<String>>,
    step: Signal<usize>,
    can_fill: bool,
    show_filled: Signal<bool>,
) -> Element {
    let mut copied = use_signal(|| false);
    let saved_signature = use_context::<Signal<Settings>>().read().signature.clone();
    let fields = reading.fields.clone();
    let a = answers.read().clone();
    let answer = |i: usize| a.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
    let filled = fields
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let v = answer(i);
            (!v.is_empty()).then(|| format!("{}: {}", f.label.trim_end_matches([':', ' ']), v))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let missing = fields
        .iter()
        .enumerate()
        .filter(|(i, f)| f.required && answer(*i).is_empty())
        .count();
    let total = fields.len();
    rsx! {
        main { class: "step review",
            p { class: "step-count", {tr("All done", "Terminé")} }
            h2 { class: "step-question", {tr("Check your answers", "Vérifiez vos réponses")} }
            if missing == 1 {
                p { class: "warn", {tr("1 required question is still unanswered.", "1 question obligatoire est encore sans réponse.")} }
            } else if missing > 1 {
                p { class: "warn", if french() { "{missing} questions obligatoires sont encore sans réponse." } else { "{missing} required questions are still unanswered." } }
            }
            ul { class: "answers",
                for (i, f) in fields.iter().enumerate() {
                    li { key: "{i}",
                        button { class: "answer-row hue-dot-{i % HUES}", onclick: move |_| go(step, i + 1),
                            span { class: "answer-q", if f.label.trim().is_empty() { "{f.question}" } else { "{f.label}" } }
                            {
                                let v = answer(i);
                                if v.is_empty() {
                                    let gap = if f.required { "answer-v gap" } else { "answer-v skipped" };
                                    rsx! { span { class: gap, if f.required { {tr("Not answered", "Sans réponse")} } else { {tr("Skipped", "Passée")} } } }
                                } else if v == signature::SIGNED && !saved_signature.is_empty() {
                                    rsx! { span { class: "answer-v answer-sig", dangerous_inner_html: "{signature::preview(&saved_signature)}" } }
                                } else {
                                    rsx! { span { class: "answer-v", "{v}" } }
                                }
                            }
                        }
                    }
                }
            }
            if !reading.notes.is_empty() {
                section { class: "card",
                    h3 { {tr("Also", "À faire aussi")} }
                    ul { for n in reading.notes.iter() { li { "{n}" } } }
                }
            }
            if can_fill {
                button { class: "big primary",
                    onclick: move |_| {
                        show_filled.set(true);
                        document::eval("window.scrollTo(0, 0)");
                    },
                    {tr("📄 Show filled form", "📄 Voir le formulaire rempli")}
                }
            }
            button { class: if can_fill { "big" } else { "big primary" },
                onclick: move |_| {
                    let text = serde_json::to_string(&filled).unwrap_or_default();
                    document::eval(&format!("navigator.clipboard.writeText({text})"));
                    copied.set(true);
                },
                if copied() { {tr("Copied ✓", "Copié ✓")} } else { {tr("Copy answers", "Copier les réponses")} }
            }
            button { class: "link", onclick: move |_| go(step, total), {tr("‹ Back to the last question", "‹ Retour à la dernière question")} }
        }
    }
}

/// A field the person signs: their saved signature to use here, or a pad to
/// sign on, which keeps the signature for the next forms too.
#[component]
fn SignHere(index: usize, answers: Signal<Vec<String>>) -> Element {
    let settings = use_context::<Signal<Settings>>();
    let saved = settings.read().signature.clone();
    let signed = answers
        .read()
        .get(index)
        .is_some_and(|a| a == signature::SIGNED);
    let mut set = move |v: &str| {
        let mut a = answers.write();
        if a.len() <= index {
            a.resize(index + 1, String::new());
        }
        a[index] = v.to_string();
    };
    if saved.is_empty() {
        return rsx! {
            div { class: "sign-here",
                p { class: "step-label", {tr("Sign with your finger. scan2fil keeps your signature on this phone and offers it on the next forms.", "Signez avec le doigt. scan2fil garde votre signature sur ce téléphone et la propose sur les prochains formulaires.")} }
                SignatureEditor { on_saved: move |_| set(signature::SIGNED) }
            }
        };
    }
    rsx! {
        div { class: "sign-here",
        button {
            class: if signed { "sig-saved on" } else { "sig-saved" },
            title: if signed { tr("Don't sign here", "Ne pas signer ici") } else { tr("Use my signature", "Utiliser ma signature") },
            onclick: move |_| set(if signed { "" } else { signature::SIGNED }),
            div { class: "sig-ink", dangerous_inner_html: "{signature::preview(&saved)}" }
            if signed { span { class: "sig-check", "✓" } }
        }
        if signed {
            p { class: "step-label", {tr("Your signature goes here on the form.", "Votre signature ira ici sur le formulaire.")} }
            button { class: "step-suggest", onclick: move |_| set(""), {tr("Don't sign here", "Ne pas signer ici")} }
        } else {
            button { class: "step-suggest", onclick: move |_| set(signature::SIGNED), {tr("Use my signature", "Utiliser ma signature")} }
        }
        }
    }
}

/// The pad, and a button that keeps what is drawn on it as the signature.
#[component]
fn SignatureEditor(on_saved: EventHandler<()>) -> Element {
    let settings = use_context::<Signal<Settings>>();
    let mut drawn = use_signal(Strokes::new);
    let mut error = use_signal(|| None::<String>);
    rsx! {
        div { class: "sig-editor",
        signature::Pad { on_change: move |s| drawn.set(s) }
        if let Some(e) = error() { p { class: "error small", "{e}" } }
        button { class: "big primary", disabled: drawn.read().is_empty(),
            onclick: move |_| {
                let mut s = settings.peek().clone();
                s.signature = drawn();
                match commit(settings, s) {
                    Ok(()) => on_saved.call(()),
                    Err(e) => error.set(Some(e)),
                }
            },
            {tr("Save my signature", "Enregistrer ma signature")}
        }
        }
    }
}

/// The answers written on the page photo, with the PDF to share.
#[component]
fn FilledForm(
    reading: Reading,
    answers: Signal<Vec<String>>,
    page: Arc<photo::Page>,
    show_filled: Signal<bool>,
) -> Element {
    let (fields, found) = layout::snapped(&reading.fields, &page.lines);
    // Answers whose label wasn't found on the page sit where the model
    // guessed, which can be a line or two off.
    let guessed = match &page.text_problem {
        Some(problem) => Some(if french() {
            format!("Le téléphone n'a pas pu lire le texte de la page ({problem}) : chaque réponse est placée là où l'IA l'a estimé, parfois une ligne à côté.")
        } else {
            format!("The phone couldn't read the page's text ({problem}), so each answer is placed where the AI estimated: it can be a line off.")
        }),
        None if found == 0 && !reading.fields.is_empty() => Some(
            tr("The printed labels weren't found in the page's text, so each answer is placed where the AI estimated: it can be a line off.", "Les libellés imprimés n'ont pas été trouvés dans le texte de la page : chaque réponse est placée là où l'IA l'a estimé, parfois une ligne à côté.").to_string(),
        ),
        None => None,
    };
    let saved_signature = use_context::<Signal<Settings>>().read().signature.clone();
    let (marks, unplaced) = filled::marks(&fields, &answers.read(), &saved_signature);
    let preview = filled::svg(&page, &marks);
    let mut status = use_signal(|| None::<String>);
    let share = {
        let page = page.clone();
        let marks = marks.clone();
        let name = filled::file_name(&reading.document.title);
        move |_| {
            let path = std::env::temp_dir().join(&name);
            let result = std::fs::write(&path, filled::pdf(&page, &marks))
                .map_err(|e| format!("{} {e}", msg("Could not save the PDF:", "Impossible d'enregistrer le PDF :")))
                .and_then(|()| sheet::share_file(&path));
            status.set(result.err());
        }
    };
    rsx! {
        main { class: "result",
            button { class: "link", onclick: move |_| show_filled.set(false), {tr("‹ Back to the questions", "‹ Retour aux questions")} }
            if marks.is_empty() {
                p { class: "warn", {tr("Answer at least one question to see it on the form.", "Répondez à au moins une question pour la voir sur le formulaire.")} }
            }
            if unplaced > 0 {
                p { class: "muted small", if french() { "{unplaced} réponse(s) n'ont pas pu être placées sur la page : écrivez-les à la main, ou utilisez Copier les réponses." } else { "{unplaced} answer(s) could not be placed on the page: write them by hand, or use Copy answers." } }
            }
            if let Some(g) = guessed { p { class: "warn", "{g}" } }
            div { class: "filled", dangerous_inner_html: "{preview}" }
            p { class: "muted small", {tr("Check each answer sits on its line before you send the form.", "Vérifiez que chaque réponse est sur sa ligne avant d'envoyer le formulaire.")} }
            if let Some(e) = status() { p { class: "error", "{e}" } }
            button { class: "big primary", onclick: share, {tr("Share PDF", "Partager le PDF")} }
        }
    }
}

#[component]
fn AccountScreen(settings: Signal<Settings>, regions: Signal<Vec<api0::Region>>) -> Element {
    let s = settings.read().clone();
    let mut error = use_signal(|| None::<String>);
    let mut redraw = use_signal(|| false);
    let chosen = api0::chosen(&regions.read(), &s.region).map(|r| r.id.clone());
    rsx! {
        main { class: "settings",
            section { class: "card",
                if s.signed_in() {
                    h3 { {tr("Signed in", "Connecté")} }
                    p { "{s.email}" }
                    if let Some(c) = s.credits_left {
                        p { class: "muted small", if french() { "{c} crédits IA restants ce mois-ci." } else { "{c} AI credits left this month." } }
                    }
                    button { class: "big",
                        onclick: move |_| {
                            let mut s = settings.peek().clone();
                            s.licence.clear();
                            s.email.clear();
                            s.credits_left = None;
                            if let Err(e) = commit(settings, s) { error.set(Some(e)); }
                        },
                        {tr("Sign out", "Se déconnecter")}
                    }
                } else {
                    h3 { {tr("Sign in", "Connexion")} }
                    p { class: "muted small",
                        if s.trial_used { {tr("Your free document is used. ", "Votre document gratuit est utilisé. ")} } else { {tr("You can read one document without an account. ", "Vous pouvez lire un document sans compte. ")} }
                        {tr("A free account comes with AI credits every month.", "Un compte gratuit inclut des crédits IA chaque mois.")}
                    }
                    SignInButtons { settings }
                }
            }
            section { class: "card",
                h3 { "Signature" }
                if s.signature.is_empty() || redraw() {
                    p { class: "muted small", {tr("Sign with your finger. It stays on this phone; scan2fil offers it when a form asks for a signature.", "Signez avec le doigt. Elle reste sur ce téléphone ; scan2fil la propose quand un formulaire demande une signature.")} }
                    SignatureEditor { on_saved: move |_| redraw.set(false) }
                    if redraw() {
                        button { class: "link", onclick: move |_| redraw.set(false), {tr("Keep my signature", "Garder ma signature")} }
                    }
                } else {
                    div { class: "sig-saved", div { class: "sig-ink", dangerous_inner_html: "{signature::preview(&s.signature)}" } }
                    div { class: "row",
                        button { class: "big", onclick: move |_| redraw.set(true), {tr("Sign again", "Signer à nouveau")} }
                        button { class: "big",
                            onclick: move |_| {
                                let mut s = settings.peek().clone();
                                s.signature.clear();
                                if let Err(e) = commit(settings, s) { error.set(Some(e)); }
                            },
                            {tr("Delete", "Supprimer")}
                        }
                    }
                }
            }
            section { class: "card",
                h3 { {tr("Region", "Région")} }
                p { class: "muted small", {tr("Where the document is read. api0 picks the model.", "Où le document est lu. api0 choisit le modèle.")} }
                for r in regions() {
                    label { class: if r.available { "radio" } else { "radio off" },
                        input { r#type: "radio", name: "region",
                            disabled: !r.available,
                            checked: chosen.as_deref() == Some(r.id.as_str()),
                            onchange: {
                                let id = r.id.clone();
                                move |_| {
                                    let mut s = settings.peek().clone();
                                    s.region = id.clone();
                                    if let Err(e) = commit(settings, s) { error.set(Some(e)); }
                                }
                            }
                        }
                        "{r.flag} {r.label}"
                        if !r.available { span { class: "muted small", {tr(" · Coming soon", " · Bientôt")} } }
                    }
                }
            }
            About { error }
            if let Some(e) = error() { p { class: "error", "{e}" } }
        }
    }
}

/// The pages the stores ask an app to link to, on Mayorana's site. Without a
/// language, mayorana.ch picks English or French from the browser's.
const ABOUT_LINKS: [(&str, &str, &str); 4] = [
    ("Privacy policy", "Politique de confidentialité", "https://mayorana.ch/privacy#scan2fil"),
    ("Terms of use", "Conditions d'utilisation", "https://mayorana.ch/terms"),
    ("Contact", "Contact", "https://mayorana.ch/contact"),
    ("mayorana.ch", "mayorana.ch", "https://mayorana.ch"),
];

/// Who makes scan2fil, and its legal pages.
#[component]
fn About(error: Signal<Option<String>>) -> Element {
    rsx! {
        section { class: "card about",
            h3 { {tr("About", "À propos")} }
            p { class: "muted small", {tr("scan2fil is made by Mayorana, Switzerland.", "scan2fil est fait par Mayorana, Suisse.")} }
            for (en, fr, url) in ABOUT_LINKS {
                button { class: "about-link",
                    onclick: move |_| {
                        if let Err(e) = sheet::open_link(url) { error.set(Some(e)); }
                    },
                    span { {tr(en, fr)} }
                    span { class: "muted", "›" }
                }
            }
        }
    }
}
