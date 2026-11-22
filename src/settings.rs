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
    if let Some(files) = files_dir_beside(&std::env::temp_dir()) {
        return files.join("scan2fil").join("settings.json");
    }
    usual_path()
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
        assert_eq!(files, Some(PathBuf::from("/data/user/0/com.mayorana.scan2fil/files")));
    }

    #[test]
    fn no_files_folder_without_a_cache_folder() {
        assert_eq!(files_dir_beside(Path::new("/data/local/tmp")), None);
        assert_eq!(files_dir_beside(Path::new("/")), None);
    }
}
