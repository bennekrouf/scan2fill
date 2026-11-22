//! The system sign-in sheet, and the id that marks this phone for the trial.
//!
//! On iOS, ASWebAuthenticationSession shows the provider's page in a secure
//! browser the app cannot read, and hands back only the redirect to
//! `scheme`. The device id lives in the Keychain, which keeps it when the app
//! is deleted and installed again. The share sheet hands the filled form's
//! PDF to Files, Mail, Print and the rest.
//!
//! On Android, the same jobs are done by the Kotlin module in `android/`:
//! a Custom Tab for the sign-in page, and Android's share sheet.

#[cfg(target_os = "ios")]
pub use ios::{device_id, open, read_text, share_file};

#[cfg(target_os = "android")]
pub use android::{open, share_file};

/// On Android, the Gradle module in `android/` is bundled with the app because
/// of this declaration. Its manifest gives the WebView's camera button what it
/// needs (a FileProvider, and leave to find the camera app) and catches the
/// sign-in redirects; its Kotlin class opens the sign-in page and the share sheet.
#[cfg(target_os = "android")]
mod android {
    // No leading slash, unlike the macro's docs: with one, the macro misses
    // build.gradle.kts, looks the class up as com.example.Scan2filPlatform,
    // and the app crashes on the first call.
    #[manganis::ffi("android")]
    extern "Kotlin" {
        pub type Scan2filPlatform;
        // A string, not a `Result`: the ffi macro only carries plain values
        // (and in this dioxus alpha, no `bool` return either).
        /// Empty when the share sheet opened, otherwise what went wrong.
        pub fn share_pdf(this: &Scan2filPlatform, path: &str) -> String;
        /// Opens the sign-in page; empty when it opened.
        pub fn start_sign_in(this: &Scan2filPlatform, url: &str) -> String;
        /// Empty while the person is still signing in; see `sign_in_answer`.
        pub fn sign_in_result(this: &Scan2filPlatform) -> String;
    }

    /// The provider's page in a Custom Tab; returns the redirect to `scheme`.
    ///
    /// The answer is asked for every quarter second: the Kotlin side cannot
    /// call back into Rust through this ffi macro.
    pub async fn open(url: &str, scheme: &str) -> Result<String, String> {
        const STEP: std::time::Duration = std::time::Duration::from_millis(250);
        // A person who leaves the page open this long has gone.
        const GIVE_UP: u32 = 4 * 60 * 10;
        let platform = Scan2filPlatform::new()?;
        let problem = start_sign_in(&platform, url)?;
        if !problem.is_empty() {
            return Err(problem);
        }
        for _ in 0..GIVE_UP {
            tokio::time::sleep(STEP).await;
            if let Some(result) = super::sign_in_answer(&sign_in_result(&platform)?, scheme) {
                return result;
            }
        }
        Err("Sign-in took too long. Try again.".into())
    }

    /// Android's share sheet, for the filled form's PDF in the cache folder.
    pub fn share_file(path: &std::path::Path) -> Result<(), String> {
        let platform = Scan2filPlatform::new()?;
        let problem = share_pdf(&platform, &path.to_string_lossy())?;
        if problem.is_empty() {
            Ok(())
        } else {
            Err(problem)
        }
    }
}

/// Off iOS there is no text reader: answers keep the model's places.
#[cfg(not(target_os = "ios"))]
pub fn read_text(_jpeg: &[u8]) -> Result<Vec<crate::layout::Line>, String> {
    Ok(Vec::new())
}

/// On the desktop, the PDF is opened in the default viewer.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn share_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("The PDF is saved at {}, but could not be opened: {e}", path.display()))
}

/// What the Android sign-in reported: `None` while it is still going on.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn sign_in_answer(answer: &str, scheme: &str) -> Option<Result<String, String>> {
    match answer {
        "" => None,
        "cancelled" => Some(Err("Sign-in cancelled.".into())),
        _ => Some(match answer.strip_prefix("url:") {
            Some(url) if url.starts_with(&format!("{scheme}:")) => Ok(url.to_string()),
            Some(_) => Err("The sign-in came back for another provider. Try again.".into()),
            None => Err(answer.to_string()),
        }),
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub async fn open(_url: &str, _scheme: &str) -> Result<String, String> {
    Err("Sign-in with Google or Microsoft works in the phone app.".into())
}

/// Off iOS, the id is kept with the settings.
#[cfg(not(target_os = "ios"))]
pub fn device_id() -> Result<String, String> {
    settings_device_id()
}

/// The id kept with the settings: lost when the app is removed.
#[cfg_attr(target_os = "ios", allow(dead_code))]
fn settings_device_id() -> Result<String, String> {
    let mut s = crate::settings::load();
    if s.device_id.is_empty() {
        s.device_id = new_id()?;
        crate::settings::save(&s)?;
    }
    Ok(s.device_id)
}

pub fn new_id() -> Result<String, String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| format!("no randomness available: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(target_os = "ios")]
mod ios {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, MainThreadMarker, MainThreadOnly};
    use objc2_authentication_services::{
        ASWebAuthenticationPresentationContextProviding, ASWebAuthenticationSession,
    };
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSError, NSString, NSURL};
    use objc2_ui_kit::{
        UIActivityViewController, UIApplication, UIDevice, UIModalPresentationStyle, UIUserInterfaceIdiom, UIWindow,
        UIWindowScene,
    };
    use std::cell::RefCell;
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    const KEYCHAIN_SERVICE: &str = "com.mayorana.scan2fil";
    const KEYCHAIN_ACCOUNT: &str = "device-id";
    /// errSecMissingEntitlement
    const ERR_SEC_MISSING_ENTITLEMENT: i32 = -34018;

    pub fn device_id() -> Result<String, String> {
        use security_framework::passwords::{get_generic_password, set_generic_password};
        if let Ok(bytes) = get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            if let Ok(id) = String::from_utf8(bytes) {
                if !id.is_empty() {
                    return Ok(id);
                }
            }
        }
        let id = super::new_id()?;
        match set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, id.as_bytes()) {
            Ok(()) => Ok(id),
            // A simulator build is signed without entitlements, and has no
            // keychain: keep the id with the settings there instead.
            Err(e) if e.code() == ERR_SEC_MISSING_ENTITLEMENT => super::settings_device_id(),
            Err(e) => Err(format!("could not keep this phone's id: {e}")),
        }
    }

    pub struct AnchorIvars;

    define_class!(
        /// Tells the sign-in sheet which window to show over.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "Scan2filSignInAnchor"]
        #[ivars = AnchorIvars]
        struct Anchor;

        unsafe impl NSObjectProtocol for Anchor {}

        unsafe impl ASWebAuthenticationPresentationContextProviding for Anchor {
            #[unsafe(method(presentationAnchorForWebAuthenticationSession:))]
            fn presentation_anchor(&self, _session: &ASWebAuthenticationSession) -> *mut UIWindow {
                key_window(self.mtm()).map(Retained::autorelease_return).unwrap_or(std::ptr::null_mut())
            }
        }
    );

    impl Anchor {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(AnchorIvars);
            unsafe { msg_send![super(this), init] }
        }
    }

    fn key_window(mtm: MainThreadMarker) -> Option<Retained<UIWindow>> {
        let app = UIApplication::sharedApplication(mtm);
        for scene in app.connectedScenes().iter() {
            if let Ok(window_scene) = scene.downcast::<UIWindowScene>() {
                if let Some(w) = window_scene.keyWindow() {
                    return Some(w);
                }
                if let Some(w) = window_scene.windows().firstObject() {
                    return Some(w);
                }
            }
        }
        None
    }

    /// Every word printed on the photo, with its box, read by Apple's Vision
    /// on the phone itself.
    pub fn read_text(jpeg: &[u8]) -> Result<Vec<crate::layout::Line>, String> {
        use crate::api0::Area;
        use crate::layout::{Line, Word};
        use objc2_foundation::{NSData, NSDictionary, NSRange};
        use objc2_vision::{
            VNImageRequestHandler, VNRecognizeTextRequest, VNRectangleObservation, VNRequest,
            VNRequestTextRecognitionLevel,
        };

        let data = NSData::with_bytes(jpeg);
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &NSDictionary::new(),
        );
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);
        let as_request: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[as_request]))
            .map_err(|e| format!("could not read the page's text: {}", e.localizedDescription()))?;

        // Vision's boxes are fractions of the image from its bottom-left.
        let area = |r: objc2_core_foundation::CGRect| Area {
            x: r.origin.x as f32,
            y: (1.0 - r.origin.y - r.size.height) as f32,
            w: r.size.width as f32,
            h: r.size.height as f32,
        };
        let mut lines = Vec::new();
        for obs in request.results().iter().flat_map(|r| r.iter()) {
            let Some(best) = obs.topCandidates(1).firstObject() else { continue };
            let text = best.string().to_string();
            let whole = area(unsafe { obs.boundingBox() });
            let total16 = text.encode_utf16().count().max(1);
            let mut words = Vec::new();
            let mut at16 = 0usize;
            for piece in text.split_inclusive(char::is_whitespace) {
                let word = piece.trim_end();
                let len16 = word.encode_utf16().count();
                if len16 > 0 {
                    let range = NSRange::new(at16, len16);
                    let found: Result<objc2::rc::Retained<VNRectangleObservation>, objc2::rc::Retained<NSError>> =
                        unsafe { msg_send![&*best, boundingBoxForRange: range, error: _] };
                    let a = match found {
                        Ok(r) => area(unsafe { r.boundingBox() }),
                        // Without its own box, a word gets its share of the line's.
                        Err(_) => Area {
                            x: whole.x + whole.w * at16 as f32 / total16 as f32,
                            y: whole.y,
                            w: whole.w * len16 as f32 / total16 as f32,
                            h: whole.h,
                        },
                    };
                    words.push(Word { text: word.to_string(), area: a });
                }
                at16 += piece.encode_utf16().count();
            }
            lines.push(Line { words });
        }
        Ok(lines)
    }

    /// Shows the system share sheet for the file at `path`.
    pub fn share_file(path: &std::path::Path) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("Sharing must start from the app's screen.")?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let item: &AnyObject = &url;
        let items = NSArray::from_slice(&[item]);
        let sheet = unsafe {
            UIActivityViewController::initWithActivityItems_applicationActivities(
                UIActivityViewController::alloc(mtm),
                &items,
                None,
            )
        };
        let mut top = key_window(mtm)
            .and_then(|w| w.rootViewController())
            .ok_or("The share sheet has no screen to show on.")?;
        while let Some(next) = top.presentedViewController() {
            top = next;
        }
        // On an iPad the sheet is a popover and needs something to point at.
        // On an iPhone it is asked for as a sheet from the bottom: left as a
        // popover (iOS 26's default) it opens half off the screen.
        if UIDevice::currentDevice(mtm).userInterfaceIdiom() == UIUserInterfaceIdiom::Pad {
            if let (Some(popover), Some(view)) = (sheet.popoverPresentationController(), top.view()) {
                popover.setSourceView(Some(&view));
            }
        } else {
            sheet.setModalPresentationStyle(UIModalPresentationStyle::PageSheet);
        }
        top.presentViewController_animated_completion(&sheet, true, None);
        Ok(())
    }

    thread_local! {
        /// The running sheet and its anchor: the session must outlive start(),
        /// and it only holds the anchor weakly.
        static RUNNING: RefCell<Option<(Retained<ASWebAuthenticationSession>, Retained<Anchor>)>> =
            const { RefCell::new(None) };
    }

    /// Shows `url` in the sign-in sheet and returns the URL it was sent back
    /// to on `scheme`.
    pub async fn open(url: &str, scheme: &str) -> Result<String, String> {
        let mtm = MainThreadMarker::new().ok_or("Sign-in must start from the app's screen.")?;
        let ns_url = NSURL::URLWithString(&NSString::from_str(url)).ok_or("The sign-in address is invalid.")?;

        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        let tx = Mutex::new(Some(tx));
        let handler = RcBlock::new(move |callback: *mut NSURL, error: *mut NSError| {
            let result = if let Some(url) = unsafe { callback.as_ref() } {
                url.absoluteString().map(|s| s.to_string()).ok_or_else(|| "The sign-in sent back nothing.".to_string())
            } else if let Some(err) = unsafe { error.as_ref() } {
                // Code 1: the person closed the sheet.
                if err.code() == 1 {
                    Err("Sign-in cancelled.".to_string())
                } else {
                    Err(format!("Sign-in failed: {}", err.localizedDescription()))
                }
            } else {
                Err("Sign-in failed.".to_string())
            };
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(result);
            }
        });

        #[allow(deprecated)]
        let session = unsafe {
            ASWebAuthenticationSession::initWithURL_callbackURLScheme_completionHandler(
                ASWebAuthenticationSession::alloc(),
                &ns_url,
                Some(&NSString::from_str(scheme)),
                RcBlock::as_ptr(&handler),
            )
        };
        let anchor = Anchor::new(mtm);
        unsafe {
            session.setPresentationContextProvider(Some(ProtocolObject::from_ref(&*anchor)));
            session.setPrefersEphemeralWebBrowserSession(false);
        }
        RUNNING.with(|r| *r.borrow_mut() = Some((session.clone(), anchor)));
        if !unsafe { session.start() } {
            RUNNING.with(|r| *r.borrow_mut() = None);
            return Err("The sign-in sheet could not open.".into());
        }

        let result = rx.await.unwrap_or_else(|_| Err("Sign-in was interrupted.".into()));
        RUNNING.with(|r| *r.borrow_mut() = None);
        drop(handler);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::sign_in_answer;

    const SCHEME: &str = "msauth.com.mayorana.scan2fil";

    #[test]
    fn nothing_yet_means_keep_waiting() {
        assert_eq!(sign_in_answer("", SCHEME), None);
    }

    #[test]
    fn the_redirect_is_handed_back() {
        let url = "msauth.com.mayorana.scan2fil://auth?code=abc&state=xyz";
        assert_eq!(sign_in_answer(&format!("url:{url}"), SCHEME), Some(Ok(url.to_string())));
    }

    #[test]
    fn a_redirect_for_the_other_provider_is_refused() {
        let other = "url:com.googleusercontent.apps.123:/oauth2redirect?code=abc";
        assert!(matches!(sign_in_answer(other, SCHEME), Some(Err(_))));
    }

    #[test]
    fn closing_the_page_cancels() {
        assert_eq!(sign_in_answer("cancelled", SCHEME), Some(Err("Sign-in cancelled.".into())));
    }

    #[test]
    fn a_problem_is_passed_on() {
        let problem = "No browser could open the sign-in page.";
        assert_eq!(sign_in_answer(problem, SCHEME), Some(Err(problem.into())));
    }
}
