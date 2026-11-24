//! The system sign-in sheet, and the id that marks this phone for the trial.
//!
//! On iOS, ASWebAuthenticationSession shows the provider's page in a secure
//! browser the app cannot read, and hands back only the redirect to
//! `scheme`. The device id lives in the Keychain, which keeps it when the app
//! is deleted and installed again. The share sheet hands the filled form's
//! PDF to Files, Mail, Print and the rest.
//!
//! On Android, the same jobs are done by the Kotlin module in `android/`:
//! a Custom Tab for the sign-in page, Android's share sheet, and the browser
//! for the About links.

#[cfg(target_os = "ios")]
pub use ios::{device_id, find_page, open, open_link, scan, share_file};

#[cfg(target_os = "android")]
pub use android::{open, open_link, scan, share_file};

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
        /// Opens a web page in the browser; empty when it opened.
        pub fn view_url(this: &Scan2filPlatform, url: &str) -> String;
        /// Opens Google's document scanner; empty when it opened.
        pub fn start_scan(this: &Scan2filPlatform) -> String;
        /// Empty while the person is still scanning; see `scan_answer`.
        pub fn scan_result(this: &Scan2filPlatform) -> String;
        /// Starts reading the text printed on the JPEG at `path`; empty when started.
        pub fn start_read_text(this: &Scan2filPlatform, path: &str) -> String;
        /// Empty while the text is still being read; see `text_answer`.
        pub fn read_text_result(this: &Scan2filPlatform) -> String;
    }

    /// The bridge to the Kotlin side, made once and kept. Making it looks the
    /// Kotlin class up, which only works on the thread a tap arrives on (a
    /// task resumed elsewhere has no access to the app's classes); calling it
    /// afterwards works from anywhere. [`warm_up`] makes it during a tap.
    fn platform() -> Result<&'static Scan2filPlatform, String> {
        static PLATFORM: std::sync::OnceLock<Scan2filPlatform> = std::sync::OnceLock::new();
        if let Some(p) = PLATFORM.get() {
            return Ok(p);
        }
        let made = Scan2filPlatform::new()?;
        Ok(PLATFORM.get_or_init(|| made))
    }

    /// Makes the bridge while a tap is being handled, for the tasks it starts.
    pub fn warm_up() {
        let _ = platform();
    }

    /// The provider's page in a Custom Tab; returns the redirect to `scheme`.
    ///
    /// The answer is asked for every quarter second: the Kotlin side cannot
    /// call back into Rust through this ffi macro.
    pub async fn open(url: &str, scheme: &str) -> Result<String, String> {
        const STEP: std::time::Duration = std::time::Duration::from_millis(250);
        // A person who leaves the page open this long has gone.
        const GIVE_UP: u32 = 4 * 60 * 10;
        let platform = platform()?;
        let problem = start_sign_in(platform, url)?;
        if !problem.is_empty() {
            return Err(problem);
        }
        for _ in 0..GIVE_UP {
            tokio::time::sleep(STEP).await;
            if let Some(result) = super::sign_in_answer(&sign_in_result(platform)?, scheme) {
                return result;
            }
        }
        Err(crate::i18n::msg("Sign-in took too long. Try again.", "La connexion a pris trop de temps. Réessayez."))
    }

    /// Asked for every quarter second: the Kotlin side cannot call back into
    /// Rust through this ffi macro.
    const STEP: std::time::Duration = std::time::Duration::from_millis(250);

    /// Google's document scanner: the page, flat and cropped, as a JPEG;
    /// `None` when the person closed the scanner.
    pub async fn scan() -> Result<Option<Vec<u8>>, String> {
        // A person who leaves the scanner open this long has gone.
        const GIVE_UP: u32 = 4 * 60 * 30;
        let platform = platform()?;
        let problem = start_scan(platform)?;
        if !problem.is_empty() {
            return Err(problem);
        }
        for _ in 0..GIVE_UP {
            tokio::time::sleep(STEP).await;
            if let Some(result) = super::scan_answer(&scan_result(platform)?) {
                return match result? {
                    Some(path) => {
                        let bytes = std::fs::read(&path)
                            .map_err(|e| format!("could not open the scanned page: {e}"))?;
                        let _ = std::fs::remove_file(&path);
                        Ok(Some(bytes))
                    }
                    None => Ok(None),
                };
            }
        }
        Err(crate::i18n::msg("The scan took too long. Try again.", "Le scan a pris trop de temps. Réessayez."))
    }

    /// Google's text reader at work on a page, on the phone.
    pub struct TextReading(Result<&'static Scan2filPlatform, String>);

    impl TextReading {
        pub fn start(jpeg: &[u8]) -> Self {
            let path = std::env::temp_dir().join("scan2fil-page.jpg");
            let started = (|| {
                let platform = platform()?;
                std::fs::write(&path, jpeg).map_err(|e| format!("could not save the page: {e}"))?;
                let problem = start_read_text(platform, &path.to_string_lossy())?;
                if problem.is_empty() {
                    Ok(platform)
                } else {
                    Err(problem)
                }
            })();
            TextReading(started)
        }

        /// The page's lines, or why they couldn't be read.
        pub async fn finish(self) -> Result<Vec<crate::layout::Line>, String> {
            // Reading a page takes a second or two; past this, the model's
            // places are used.
            const GIVE_UP: u32 = 4 * 30;
            let platform = self.0?;
            for _ in 0..GIVE_UP {
                match super::text_answer(&read_text_result(platform)?) {
                    None => tokio::time::sleep(STEP).await,
                    Some(lines) => return lines,
                }
            }
            Err("reading the page's text took too long".into())
        }
    }

    /// A web page in the browser, such as the privacy policy.
    pub fn open_link(url: &str) -> Result<(), String> {
        let platform = platform()?;
        let problem = view_url(platform, url)?;
        if problem.is_empty() {
            Ok(())
        } else {
            Err(problem)
        }
    }

    /// Android's share sheet, for the filled form's PDF in the cache folder.
    pub fn share_file(path: &std::path::Path) -> Result<(), String> {
        let platform = platform()?;
        let problem = share_pdf(platform, &path.to_string_lossy())?;
        if problem.is_empty() {
            Ok(())
        } else {
            Err(problem)
        }
    }
}

#[cfg(target_os = "ios")]
pub use ios::TextReading;
#[cfg(target_os = "android")]
pub use android::{warm_up, TextReading};

/// Only Android needs its bridge made during a tap.
#[cfg(not(target_os = "android"))]
pub fn warm_up() {}

/// Off the phone there is no text reader: answers keep the model's places.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub struct TextReading;

#[cfg(not(any(target_os = "ios", target_os = "android")))]
impl TextReading {
    pub fn start(_jpeg: &[u8]) -> Self {
        TextReading
    }

    pub async fn finish(self) -> Result<Vec<crate::layout::Line>, String> {
        Err("this computer has no text reader".into())
    }
}

/// Whether this platform has a document scanner: off the phone, a photo is
/// chosen instead.
pub const HAS_SCANNER: bool = cfg!(any(target_os = "ios", target_os = "android"));

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub async fn scan() -> Result<Option<Vec<u8>>, String> {
    Err(crate::i18n::msg("Scanning works in the phone app. Choose a photo instead.", "Le scan marche dans l'app du téléphone. Choisissez plutôt une photo."))
}

/// Off iOS the phone does not look for the page in a photo: `flatten::detect`
/// does.
#[cfg(not(target_os = "ios"))]
pub fn find_page(_jpeg: &[u8]) -> Option<crate::flatten::Corners> {
    None
}

/// What the Android scanner reported: `None` while it is still open, the
/// scanned file, or `Ok(None)` when the person closed it.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn scan_answer(answer: &str) -> Option<Result<Option<std::path::PathBuf>, String>> {
    match answer {
        "" => None,
        "cancelled" => Some(Ok(None)),
        _ => Some(match answer.strip_prefix("file:") {
            Some(path) => Ok(Some(path.into())),
            None => Err(answer.to_string()),
        }),
    }
}

/// What the Android text reader reported: `None` while it is still reading,
/// the page's lines, or why it couldn't read them.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn text_answer(answer: &str) -> Option<Result<Vec<crate::layout::Line>, String>> {
    use crate::api0::Area;
    use crate::layout::{Line, Word};
    #[derive(serde::Deserialize)]
    struct W {
        t: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    }
    if answer.is_empty() {
        return None;
    }
    let Some(json) = answer.strip_prefix("lines:") else {
        return Some(Err(answer.to_string()));
    };
    Some(
        serde_json::from_str::<Vec<Vec<W>>>(json)
            .map(|lines| {
                lines
                    .into_iter()
                    .map(|words| Line {
                        words: words
                            .into_iter()
                            .map(|w| Word {
                                text: w.t,
                                area: Area { x: w.x, y: w.y, w: w.w, h: w.h },
                            })
                            .collect(),
                    })
                    .collect()
            })
            .map_err(|e| format!("could not read the page's text: {e}")),
    )
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
        .map_err(|e| {
            format!(
                "The PDF is saved at {}, but could not be opened: {e}",
                path.display()
            )
        })
}

/// What the Android sign-in reported: `None` while it is still going on.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn sign_in_answer(answer: &str, scheme: &str) -> Option<Result<String, String>> {
    match answer {
        "" => None,
        "cancelled" => Some(Err(crate::i18n::msg("Sign-in cancelled.", "Connexion annulée."))),
        _ => Some(match answer.strip_prefix("url:") {
            Some(url) if url.starts_with(&format!("{scheme}:")) => Ok(url.to_string()),
            Some(_) => Err(crate::i18n::msg("The sign-in came back for another provider. Try again.", "La connexion est revenue pour un autre fournisseur. Réessayez.")),
            None => Err(answer.to_string()),
        }),
    }
}

/// On the desktop, a web page opens in the default browser.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn open_link(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{url} could not be opened: {e}"))
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
    use objc2::runtime::AnyObject;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_authentication_services::{
        ASWebAuthenticationPresentationContextProviding, ASWebAuthenticationSession,
    };
    use objc2_foundation::{NSArray, NSError, NSString, NSURL};
    use objc2_ui_kit::{
        UIActivityViewController, UIApplication, UIDevice, UIModalPresentationStyle,
        UIUserInterfaceIdiom, UIWindow, UIWindowScene,
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
                key_window(self.mtm())
                    .map(Retained::autorelease_return)
                    .unwrap_or(std::ptr::null_mut())
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
    fn read_text(jpeg: &[u8]) -> Result<Vec<crate::layout::Line>, String> {
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
            .map_err(|e| {
                format!(
                    "could not read the page's text: {}",
                    e.localizedDescription()
                )
            })?;

        // Vision's boxes are fractions of the image from its bottom-left.
        let area = |r: objc2_core_foundation::CGRect| Area {
            x: r.origin.x as f32,
            y: (1.0 - r.origin.y - r.size.height) as f32,
            w: r.size.width as f32,
            h: r.size.height as f32,
        };
        let mut lines = Vec::new();
        for obs in request.results().iter().flat_map(|r| r.iter()) {
            let Some(best) = obs.topCandidates(1).firstObject() else {
                continue;
            };
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
                    let found: Result<
                        objc2::rc::Retained<VNRectangleObservation>,
                        objc2::rc::Retained<NSError>,
                    > = unsafe { msg_send![&*best, boundingBoxForRange: range, error: _] };
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
                    words.push(Word {
                        text: word.to_string(),
                        area: a,
                    });
                }
                at16 += piece.encode_utf16().count();
            }
            lines.push(Line { words });
        }
        Ok(lines)
    }

    /// Apple's text reader at work on a page, on a thread of its own.
    pub struct TextReading(oneshot::Receiver<Result<Vec<crate::layout::Line>, String>>);

    impl TextReading {
        pub fn start(jpeg: &[u8]) -> Self {
            let (tx, rx) = oneshot::channel();
            let jpeg = jpeg.to_vec();
            std::thread::spawn(move || {
                let _ = tx.send(read_text(&jpeg));
            });
            TextReading(rx)
        }

        /// The page's lines, or why they couldn't be read.
        pub async fn finish(self) -> Result<Vec<crate::layout::Line>, String> {
            self.0
                .await
                .unwrap_or_else(|_| Err("the text reader stopped".into()))
        }
    }

    /// The page's corners on a photo, found by Vision. Its document detector
    /// is asked first; where it can't run (it needs the Neural Engine: the
    /// simulator answers with the whole frame and no confidence), the
    /// classic rectangle detector, which runs anywhere, takes over.
    pub fn find_page(jpeg: &[u8]) -> Option<crate::flatten::Corners> {
        use objc2_foundation::{NSData, NSDictionary};
        use objc2_vision::{
            VNDetectDocumentSegmentationRequest, VNDetectRectanglesRequest, VNImageRequestHandler,
            VNRectangleObservation, VNRequest,
        };

        let data = NSData::with_bytes(jpeg);
        let handler = VNImageRequestHandler::initWithData_options(
            VNImageRequestHandler::alloc(),
            &data,
            &NSDictionary::new(),
        );
        let document = unsafe { VNDetectDocumentSegmentationRequest::new() };
        let rectangle = unsafe { VNDetectRectanglesRequest::new() };
        unsafe {
            // A sheet of paper, A4 or letter, seen at an angle, filling a
            // good part of the photo.
            rectangle.setMinimumAspectRatio(0.3);
            rectangle.setMaximumAspectRatio(1.0);
            rectangle.setMinimumSize(0.3);
            rectangle.setQuadratureTolerance(30.0);
            rectangle.setMinimumConfidence(0.6);
            rectangle.setMaximumObservations(1);
        }
        let requests: [&VNRequest; 2] = [&document, &rectangle];
        if let Err(e) = handler.performRequests_error(&NSArray::from_slice(&requests)) {
            eprintln!("scan2fil: finding the page failed: {}", e.localizedDescription());
            return None;
        }
        let confident = |found: Option<Retained<VNRectangleObservation>>| {
            found.filter(|o| unsafe { o.confidence() } >= 0.6)
        };
        let found = confident(unsafe { document.results() }.and_then(|r| r.firstObject()))
            .or_else(|| confident(unsafe { rectangle.results() }.and_then(|r| r.firstObject())))?;
        // Vision's points are fractions of the image from its bottom-left.
        let point = |p: objc2_core_foundation::CGPoint| (p.x as f32, (1.0 - p.y) as f32);
        Some(unsafe {
            crate::flatten::Corners {
                top_left: point(found.topLeft()),
                top_right: point(found.topRight()),
                bottom_right: point(found.bottomRight()),
                bottom_left: point(found.bottomLeft()),
            }
        })
    }

    // VisionKit's document camera. No Rust crate covers VisionKit, so the
    // class is declared here and its framework linked by hand.
    #[link(name = "VisionKit", kind = "framework")]
    unsafe extern "C" {}

    objc2::extern_class!(
        #[unsafe(super(objc2_ui_kit::UIViewController, objc2_ui_kit::UIResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "VNDocumentCameraViewController"]
        struct DocumentCamera;
    );

    type ScanResult = Result<Option<Vec<u8>>, String>;

    pub struct ScanDelegateIvars {
        tx: RefCell<Option<oneshot::Sender<ScanResult>>>,
    }

    define_class!(
        /// Hears back from the document camera.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "Scan2filScanDelegate"]
        #[ivars = ScanDelegateIvars]
        struct ScanDelegate;

        unsafe impl NSObjectProtocol for ScanDelegate {}

        impl ScanDelegate {
            #[unsafe(method(documentCameraViewController:didFinishWithScan:))]
            fn did_finish(&self, camera: &objc2_ui_kit::UIViewController, scan: &AnyObject) {
                let pages: usize = unsafe { msg_send![scan, pageCount] };
                let result = if pages == 0 {
                    Ok(None)
                } else {
                    // The first page: scan2fil reads one page at a time.
                    let image: Retained<objc2_ui_kit::UIImage> =
                        unsafe { msg_send![scan, imageOfPageAtIndex: 0usize] };
                    image
                        .jpeg_representation(0.9)
                        .map(|data| Some(data.to_vec()))
                        .ok_or_else(|| "could not keep the scanned page".to_string())
                };
                camera.dismissViewControllerAnimated_completion(true, None);
                self.send(result);
            }

            #[unsafe(method(documentCameraViewControllerDidCancel:))]
            fn did_cancel(&self, camera: &objc2_ui_kit::UIViewController) {
                camera.dismissViewControllerAnimated_completion(true, None);
                self.send(Ok(None));
            }

            #[unsafe(method(documentCameraViewController:didFailWithError:))]
            fn did_fail(&self, camera: &objc2_ui_kit::UIViewController, error: &NSError) {
                camera.dismissViewControllerAnimated_completion(true, None);
                self.send(Err(format!("The scan failed: {}", error.localizedDescription())));
            }
        }
    );

    impl ScanDelegate {
        fn new(mtm: MainThreadMarker, tx: oneshot::Sender<ScanResult>) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ScanDelegateIvars {
                tx: RefCell::new(Some(tx)),
            });
            unsafe { msg_send![super(this), init] }
        }

        fn send(&self, result: ScanResult) {
            if let Some(tx) = self.ivars().tx.borrow_mut().take() {
                let _ = tx.send(result);
            }
        }
    }

    thread_local! {
        /// The camera's delegate while it is open: the camera only holds it weakly.
        static SCANNING: RefCell<Option<Retained<ScanDelegate>>> = const { RefCell::new(None) };
    }

    /// The document camera: the page, flat and cropped, as a JPEG; `None`
    /// when the person closed it.
    pub async fn scan() -> Result<Option<Vec<u8>>, String> {
        let mtm = MainThreadMarker::new().ok_or("Scanning must start from the app's screen.")?;
        let supported: bool = unsafe { msg_send![DocumentCamera::class(), isSupported] };
        if !supported {
            return Err(crate::i18n::msg("This device can't scan documents. Choose a photo instead.", "Cet appareil ne peut pas scanner de documents. Choisissez plutôt une photo."));
        }
        let (tx, rx) = oneshot::channel();
        let delegate = ScanDelegate::new(mtm, tx);
        let camera: Retained<DocumentCamera> = unsafe { msg_send![DocumentCamera::alloc(mtm), init] };
        let _: () = unsafe { msg_send![&*camera, setDelegate: &*delegate] };
        let mut top = key_window(mtm)
            .and_then(|w| w.rootViewController())
            .ok_or("The scanner has no screen to show on.")?;
        while let Some(next) = top.presentedViewController() {
            top = next;
        }
        SCANNING.with(|s| *s.borrow_mut() = Some(delegate));
        top.presentViewController_animated_completion(&camera, true, None);
        let result = rx
            .await
            .unwrap_or_else(|_| Err(crate::i18n::msg("The scan was interrupted.", "Le scan a été interrompu.")));
        SCANNING.with(|s| *s.borrow_mut() = None);
        result
    }

    /// Opens a web page in Safari.
    pub fn open_link(url: &str) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("Links open from the app's screen.")?;
        let link = NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| format!("{url} is not a link."))?;
        let options = objc2_foundation::NSDictionary::new();
        // SAFETY: an empty options dictionary, of the type openURL expects.
        unsafe {
            UIApplication::sharedApplication(mtm)
                .openURL_options_completionHandler(&link, &options, None)
        };
        Ok(())
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
            if let (Some(popover), Some(view)) = (sheet.popoverPresentationController(), top.view())
            {
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
        let ns_url = NSURL::URLWithString(&NSString::from_str(url))
            .ok_or("The sign-in address is invalid.")?;

        let (tx, rx) = oneshot::channel::<Result<String, String>>();
        let tx = Mutex::new(Some(tx));
        let handler = RcBlock::new(move |callback: *mut NSURL, error: *mut NSError| {
            let result = if let Some(url) = unsafe { callback.as_ref() } {
                url.absoluteString()
                    .map(|s| s.to_string())
                    .ok_or_else(|| "The sign-in sent back nothing.".to_string())
            } else if let Some(err) = unsafe { error.as_ref() } {
                // Code 1: the person closed the sheet.
                if err.code() == 1 {
                    Err(crate::i18n::msg("Sign-in cancelled.", "Connexion annulée."))
                } else {
                    Err(format!("Sign-in failed: {}", err.localizedDescription()))
                }
            } else {
                Err(crate::i18n::msg("Sign-in failed.", "La connexion a échoué."))
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
            return Err(crate::i18n::msg("The sign-in sheet could not open.", "La fenêtre de connexion n'a pas pu s'ouvrir."));
        }

        let result = rx
            .await
            .unwrap_or_else(|_| Err(crate::i18n::msg("Sign-in was interrupted.", "La connexion a été interrompue.")));
        RUNNING.with(|r| *r.borrow_mut() = None);
        drop(handler);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{scan_answer, sign_in_answer, text_answer};

    #[test]
    fn the_scanner_hands_back_its_file_or_nothing() {
        assert_eq!(scan_answer(""), None);
        assert_eq!(scan_answer("cancelled"), Some(Ok(None)));
        assert_eq!(
            scan_answer("file:/data/user/0/x/cache/scan.jpg"),
            Some(Ok(Some("/data/user/0/x/cache/scan.jpg".into())))
        );
        assert_eq!(scan_answer("No scanner on this phone."), Some(Err("No scanner on this phone.".into())));
    }

    #[test]
    fn the_text_reader_hands_back_lines_of_words() {
        assert!(text_answer("").is_none());
        let lines = text_answer(r#"lines:[[{"t":"Titulaire","x":0.1,"y":0.4,"w":0.12,"h":0.02},{"t":"du","x":0.23,"y":0.4,"w":0.03,"h":0.02}],[]]"#)
            .unwrap()
            .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].words[1].text, "du");
        assert!((lines[0].words[0].area.x - 0.1).abs() < 1e-6);
        assert!(text_answer("The text reader is not ready.").unwrap().is_err());
    }

    const SCHEME: &str = "msauth.com.mayorana.scan2fil";

    #[test]
    fn nothing_yet_means_keep_waiting() {
        assert_eq!(sign_in_answer("", SCHEME), None);
    }

    #[test]
    fn the_redirect_is_handed_back() {
        let url = "msauth.com.mayorana.scan2fil://auth?code=abc&state=xyz";
        assert_eq!(
            sign_in_answer(&format!("url:{url}"), SCHEME),
            Some(Ok(url.to_string()))
        );
    }

    #[test]
    fn a_redirect_for_the_other_provider_is_refused() {
        let other = "url:com.googleusercontent.apps.123:/oauth2redirect?code=abc";
        assert!(matches!(sign_in_answer(other, SCHEME), Some(Err(_))));
    }

    #[test]
    fn closing_the_page_cancels() {
        assert_eq!(
            sign_in_answer("cancelled", SCHEME),
            Some(Err("Sign-in cancelled.".into()))
        );
    }

    #[test]
    fn a_problem_is_passed_on() {
        let problem = "No browser could open the sign-in page.";
        assert_eq!(sign_in_answer(problem, SCHEME), Some(Err(problem.into())));
    }
}
