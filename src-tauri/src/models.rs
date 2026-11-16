use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use serde::{Deserialize, Serialize};
use tauri::{menu::MenuItem, Wry};
use tokio::sync::Mutex;

use crate::settings::OverlaySettings;

pub const EVENT_TRACK_CHANGED: &str = "spotify:track-changed";
pub const EVENT_LYRICS_LOADED: &str = "spotify:lyrics-loaded";
pub const EVENT_PLAYBACK_TICK: &str = "spotify:playback-tick";
pub const EVENT_INTERACTION_MODE: &str = "overlay:interaction-mode";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    /// Identificador normalizado "{artist}-{title}" en minúsculas para caché
    pub id: String,
    pub title: String,
    /// Artista principal únicamente
    pub artist: String,
    pub duration_ms: u64,
}

impl TrackInfo {
    pub fn normalize_id(artist: &str, title: &str) -> String {
        format!(
            "{}-{}",
            artist.trim().to_lowercase(),
            title.trim().to_lowercase()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// Marca de tiempo en milisegundos; 0 si la letra no es sincronizada
    pub time_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsPayload {
    pub track_id: String,
    pub synced: bool,
    pub lines: Vec<LyricLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTick {
    pub position_ms: u64,
    pub is_playing: bool,
}

impl Default for PlaybackTick {
    fn default() -> Self {
        Self {
            position_ms: 0,
            is_playing: false,
        }
    }
}

/// Estado global de la aplicación protegido con `tokio::sync::Mutex`.
pub struct AppState {
    pub lyrics_cache: Mutex<HashMap<String, LyricsPayload>>,
    pub current_track: Mutex<Option<TrackInfo>>,
    pub current_lyrics: Mutex<Option<LyricsPayload>>,
    pub current_playback: Mutex<PlaybackTick>,
    pub locked: Mutex<bool>,
    pub settings: Mutex<OverlaySettings>,
    pub tray_lock_item: Mutex<Option<MenuItem<Wry>>>,
    pub move_debounce_seq: AtomicU64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            lyrics_cache: Mutex::new(HashMap::new()),
            current_track: Mutex::new(None),
            current_lyrics: Mutex::new(None),
            current_playback: Mutex::new(PlaybackTick::default()),
            locked: Mutex::new(false),
            settings: Mutex::new(OverlaySettings::default()),
            tray_lock_item: Mutex::new(None),
            move_debounce_seq: AtomicU64::new(0),
        }
    }
}
