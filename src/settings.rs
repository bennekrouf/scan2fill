//! Who is signed in, the region, and whether the free try is used, kept in
//! the app's own data folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// The signed-in person's licence; empty when no one is signed in.
    #[serde(default)]
    pub licence: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub credits_left: Option<i64>,
    #[serde(default = "default_region")]
    pub region: String,
    /// The one free document has been read on this phone.
    #[serde(default)]
    pub trial_used: bool,
    /// Off iOS only; on iOS the id lives in the Keychain.
    #[serde(default)]
    pub device_id: String,
    /// The person's signature as pen strokes (`signature.rs`); empty until
    /// they draw one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signature: crate::signature::Strokes,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            licence: String::new(),
            email: String::new(),
            credits_left: None,
            region: default_region(),
            trial_used: false,
            device_id: String::new(),
            signature: Vec::new(),
        }
    }
}

impl Settings {
    pub fn signed_in(&self) -> bool {
        !self.licence.trim().is_empty()
    }

    /// Whether a photo can be read now: signed in, or the free try is left.
    pub fn can_read(&self) -> bool {
        self.signed_in() || !self.trial_used
    }
}

fn default_region() -> String {
    "europe".into()
}

/// On iOS the app's sandbox (`HOME` is its container); on Android the app's
/// files folder; elsewhere the usual data folder.
fn path() -> PathBuf {
    #[cfg(target_os = "android")]
    if let Some(files) = android_files_dir() {
        return files.join("scan2fil").join("settings.json");
    }
    usual_path()
}

/// The app's own files folder, which Android keeps until the app is
/// uninstalled. Found from the cache folder when `TMPDIR` points there, else
/// from the package name and the Android user the app runs as: Rust's
/// `temp_dir()` is `/data/local/tmp` when `TMPDIR` is unset, which an app
/// cannot write to, so the sign-in was never kept.
#[cfg(target_os = "android")]
fn android_files_dir() -> Option<PathBuf> {
    if let Some(files) = files_dir_beside(&std::env::temp_dir()) {
        return Some(files);
    }
    let cmdline = std::fs::read("/proc/self/cmdline").ok()?;
    let package = String::from_utf8_lossy(cmdline.split(|b| *b == 0).next()?).into_owned();
    // A service in its own process is named "package:service".
    let package = package.split(':').next()?.trim().to_string();
    if package.is_empty() || package.contains('/') {
        return None;
    }
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let uid: u32 = status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(app_files_dir(&package, uid))
}

/// `/data/user/<n>/<package>/files`, where Android user n's apps run with
/// uids from n × 100000.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn app_files_dir(package: &str, uid: u32) -> PathBuf {
    PathBuf::from(format!("/data/user/{}/{package}/files", uid / 100_000))
}

/// Gives Rust's temporary folder, `/data/local/tmp` on Android when `TMPDIR`
/// is unset, the app's cache folder instead: the photo for the text reader and
/// the PDF to share are written there.
pub fn prepare_temp_dir() {
    #[cfg(target_os = "android")]
    if files_dir_beside(&std::env::temp_dir()).is_none() {
        if let Some(cache) = android_files_dir().and_then(|f| Some(f.parent()?.join("cache"))) {
            let _ = std::fs::create_dir_all(&cache);
            // Set once at start, before any other thread reads the environment.
            std::env::set_var("TMPDIR", &cache);
        }
    }
}

/// Android gives apps no `HOME`, so this lands in the cache folder there,
/// which the system may empty: signing the person out and giving the free
/// try back. Only kept on Android to move a file saved there before.
fn usual_path() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("scan2fil").join("settings.json")
}

/// An Android app's temporary folder is its cache, `/data/user/<n>/<package>/cache`;
/// the files folder the system leaves alone sits beside it.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn files_dir_beside(cache: &Path) -> Option<PathBuf> {
    if cache.file_name()? != "cache" {
        return None;
    }
    Some(cache.parent()?.join("files"))
}

/// Settings saved in the cache folder by earlier versions move to the files folder.
#[cfg(target_os = "android")]
fn move_out_of_cache() {
    let (old, new) = (usual_path(), path());
    if old == new || new.exists() || !old.exists() {
        return;
    }
    if let Some(dir) = new.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::copy(&old, &new).is_ok() {
        let _ = std::fs::remove_file(&old);
    }
}

pub fn load() -> Settings {
    #[cfg(target_os = "android")]
    move_out_of_cache();
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not save: {e}"))?;
    }
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(p, text).map_err(|e| format!("could not save: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_folder_sits_beside_the_cache() {
        let files = files_dir_beside(Path::new("/data/user/0/com.mayorana.scan2fil/cache"));
        assert_eq!(
            files,
            Some(PathBuf::from("/data/user/0/com.mayorana.scan2fil/files"))
        );
    }

    #[test]
    fn files_folder_of_each_android_user() {
        assert_eq!(
            app_files_dir("com.mayorana.scan2fil", 10_123),
            PathBuf::from("/data/user/0/com.mayorana.scan2fil/files")
        );
        assert_eq!(
            app_files_dir("com.mayorana.scan2fil", 1_010_123),
            PathBuf::from("/data/user/10/com.mayorana.scan2fil/files")
        );
    }

    #[test]
    fn no_files_folder_without_a_cache_folder() {
        assert_eq!(files_dir_beside(Path::new("/data/local/tmp")), None);
        assert_eq!(files_dir_beside(Path::new("/")), None);
    }
}
