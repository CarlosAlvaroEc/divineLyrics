use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE_NAME: &str = "settings.json";
pub const DEFAULT_STYLE: &str = "classic";
pub const VALID_STYLES: &[&str] = &["editorial", "typewriter", "classic", "single"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    pub x: Option<i32>,
    pub y: Option<i32>,
    #[serde(default = "default_style")]
    pub style: String,
}

fn default_style() -> String {
    DEFAULT_STYLE.to_string()
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            style: default_style(),
        }
    }
}

pub fn normalize_style(style: &str) -> String {
    let candidate = style.trim().to_lowercase();
    if VALID_STYLES.contains(&candidate.as_str()) {
        candidate
    } else {
        DEFAULT_STYLE.to_string()
    }
}

fn settings_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|err| format!("Failed to resolve app_config_dir: {err}"))?;
    Ok(config_dir.join(SETTINGS_FILE_NAME))
}

pub fn load_settings(app: &AppHandle) -> OverlaySettings {
    let Ok(path) = settings_file_path(app) else {
        return OverlaySettings::default();
    };

    let Ok(raw) = fs::read_to_string(path) else {
        return OverlaySettings::default();
    };

    let mut parsed: OverlaySettings = serde_json::from_str(&raw).unwrap_or_default();
    parsed.style = normalize_style(&parsed.style);
    parsed
}

pub fn save_settings(app: &AppHandle, settings: &OverlaySettings) -> Result<(), String> {
    let path = settings_file_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create config directory: {err}"))?;
    }

    let json = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Failed to serialize settings: {err}"))?;

    fs::write(path, json).map_err(|err| format!("Failed to write settings.json: {err}"))
}
