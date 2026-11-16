use std::time::Duration;
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::lyrics::parser::build_lyrics_payload;
use crate::models::{LyricsPayload, TrackInfo};

const LRCLIB_GET_ENDPOINT: &str = "https://lrclib.net/api/get";
const USER_AGENT: &str =
    "DivineLyrics/0.1.0 (https://github.com/divinelyrics/divine-lyrics; Tauri v2 Desktop Overlay)";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
    #[serde(default)]
    pub instrumental: bool,
}

#[derive(Clone)]
pub struct LrclibClient {
    http: Client,
}

impl LrclibClient {
    pub fn new() -> Result<Self, String> {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|err| format!("Failed to build LRCLIB HTTP client: {err}"))?;

        Ok(Self { http })
    }

    /// Consulta `GET https://lrclib.net/api/get` con `track_name`, `artist_name`
    /// y `duration` (en segundos).
    pub async fn fetch_lyrics(&self, track: &TrackInfo) -> Result<LyricsPayload, String> {
        let duration_secs = (track.duration_ms / 1000).max(1);
        let duration_param = duration_secs.to_string();

        let response = self
            .http
            .get(LRCLIB_GET_ENDPOINT)
            .query(&[
                ("track_name", track.title.as_str()),
                ("artist_name", track.artist.as_str()),
                ("duration", duration_param.as_str()),
            ])
            .send()
            .await
            .map_err(|err| format!("LRCLIB request failed: {err}"))?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(LyricsPayload {
                track_id: track.id.clone(),
                synced: false,
                lines: Vec::new(),
            });
        }

        let response = response
            .error_for_status()
            .map_err(|err| format!("LRCLIB HTTP error: {err}"))?;

        let body: LrclibResponse = response
            .json()
            .await
            .map_err(|err| format!("Failed to deserialize LRCLIB response: {err}"))?;

        if body.instrumental
            && body.synced_lyrics.as_deref().unwrap_or("").is_empty()
            && body.plain_lyrics.as_deref().unwrap_or("").is_empty()
        {
            return Ok(build_lyrics_payload(
                &track.id,
                None,
                Some("♪ Pista instrumental ♪"),
            ));
        }

        Ok(build_lyrics_payload(
            &track.id,
            body.synced_lyrics.as_deref(),
            body.plain_lyrics.as_deref(),
        ))
    }
}
