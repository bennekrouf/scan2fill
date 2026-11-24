//! English, or French when the phone is set to French.
//!
//! The screens call [`tr`], which redraws them if the language is found only
//! after they first showed (Android tells it through the WebView). Messages
//! made away from the screens, on another thread say, use [`msg`].

use dioxus::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

static FRENCH_NOW: AtomicBool = AtomicBool::new(false);

static FRENCH: GlobalSignal<bool> = Signal::global(|| {
    let french = system_language().is_some_and(|tag| is_french(&tag));
    FRENCH_NOW.store(french, Ordering::Relaxed);
    french
});

/// `en` or `fr` in the phone's language; in a component, it redraws with it.
pub fn tr(en: &'static str, fr: &'static str) -> &'static str {
    if FRENCH() {
        fr
    } else {
        en
    }
}

/// Whether the screens are in French; in a component, it redraws with it.
pub fn french() -> bool {
    FRENCH()
}

/// `en` or `fr`, from anywhere, without redrawing anything.
pub fn msg(en: &str, fr: &str) -> String {
    if FRENCH_NOW.load(Ordering::Relaxed) {
        fr.to_string()
    } else {
        en.to_string()
    }
}

/// Whether messages made away from the screens are in French.
pub fn french_now() -> bool {
    FRENCH_NOW.load(Ordering::Relaxed)
}

/// Takes the language the WebView reports (`navigator.language`), for the
/// phones whose language the app cannot ask the system for.
pub fn use_webview_language() {
    use_future(|| async {
        if system_language().is_some() {
            return;
        }
        let tag = document::eval("return navigator.language || ''")
            .join::<String>()
            .await;
        if let Ok(tag) = tag {
            let french = is_french(&tag);
            FRENCH_NOW.store(french, Ordering::Relaxed);
            if *FRENCH.peek() != french {
                *FRENCH.write() = french;
            }
        }
    });
}

/// "fr", "fr-FR", "fr_CH.UTF-8" and the like.
fn is_french(tag: &str) -> bool {
    let tag = tag.trim().to_ascii_lowercase();
    tag == "fr" || tag.starts_with("fr-") || tag.starts_with("fr_")
}

/// The person's first language, where the system says it directly. iOS's
/// WebView only reports the languages the app is localised in, so it is asked
/// here instead.
#[cfg(target_os = "ios")]
fn system_language() -> Option<String> {
    let languages = objc2_foundation::NSLocale::preferredLanguages();
    languages.firstObject().map(|l| l.to_string())
}

/// Android gives apps no `LANG`; the WebView tells it instead.
#[cfg(target_os = "android")]
fn system_language() -> Option<String> {
    None
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn system_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_tags() {
        assert!(is_french("fr"));
        assert!(is_french("fr-FR"));
        assert!(is_french("fr-CH"));
        assert!(is_french("fr_FR.UTF-8"));
        assert!(!is_french("en-US"));
        assert!(!is_french("fra"));
        assert!(!is_french(""));
    }
}
