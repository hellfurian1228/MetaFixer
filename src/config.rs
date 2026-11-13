use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_OCULUS_DIR: &str = r"C:\MetaFixer\Oculus";

#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    pub oculus_dir: Option<String>,
}

pub fn config_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("MetaFixer").join("config.json")
}

fn legacy_config_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    // Previous app folder name; still read so existing installs keep their path.
    PathBuf::from(base).join("RiftSControl").join("config.json")
}

pub fn load_config() -> Config {
    for path in [config_path(), legacy_config_path()] {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return cfg;
            }
        }
    }
    Config::default()
}

pub fn save_config_value(oculus_dir: impl AsRef<Path>) {
    let mut cfg = load_config();
    cfg.oculus_dir = Some(oculus_dir.as_ref().to_string_lossy().into_owned());
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&cfg) {
        let _ = fs::write(path, text);
    }
}

pub fn saved_oculus_dir() -> Option<PathBuf> {
    load_config().oculus_dir.filter(|s| !s.is_empty()).map(PathBuf::from)
}
