use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::models::LyricsPayload;

fn sanitize_track_id(track_id: &str) -> String {
    track_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn cache_file_path(app: &AppHandle, track_id: &str) -> Result<PathBuf, String> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| format!("Failed to resolve app_cache_dir: {err}"))?;
    Ok(cache_dir
        .join("lyrics")
        .join(format!("{}.json", sanitize_track_id(track_id))))
}

pub fn load_lyrics_from_disk(app: &AppHandle, track_id: &str) -> Option<LyricsPayload> {
    let path = cache_file_path(app, track_id).ok()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<LyricsPayload>(&raw).ok()
}

pub fn save_lyrics_to_disk(app: &AppHandle, payload: &LyricsPayload) -> Result<(), String> {
    let path = cache_file_path(app, &payload.track_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create lyrics cache dir: {err}"))?;
    }

    let json = serde_json::to_string(payload)
        .map_err(|err| format!("Failed to serialize lyrics cache: {err}"))?;

    fs::write(path, json).map_err(|err| format!("Failed to write lyrics cache file: {err}"))
}
