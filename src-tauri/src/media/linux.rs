#![cfg(target_os = "linux")]

use mpris::{PlaybackStatus, Player, PlayerFinder};

use crate::media::MediaProvider;
use crate::models::{PlaybackTick, TrackInfo};

const SPOTIFY_MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.spotify";

#[derive(Debug, Clone, Default)]
pub struct PlatformMediaProvider;

impl PlatformMediaProvider {
    pub fn new() -> Self {
        Self
    }

    fn find_spotify_player(finder: &PlayerFinder) -> Option<Player> {
        let players = finder.find_all().ok()?;
        players.into_iter().find(|player| {
            player.bus_name().starts_with(SPOTIFY_MPRIS_PREFIX)
                || player.identity().eq_ignore_ascii_case("spotify")
        })
    }
}

impl MediaProvider for PlatformMediaProvider {
    fn read_active_media(&self) -> Result<Option<(TrackInfo, PlaybackTick)>, String> {
        let finder = PlayerFinder::new()
            .map_err(|err| format!("Failed to connect to D-Bus session bus: {err}"))?;

        let Some(player) = Self::find_spotify_player(&finder) else {
            return Ok(None);
        };

        let playback_status = player
            .get_playback_status()
            .map_err(|err| format!("Failed to read MPRIS playback status: {err}"))?;

        if playback_status == PlaybackStatus::Stopped {
            return Ok(None);
        }

        let metadata = player
            .get_metadata()
            .map_err(|err| format!("Failed to read MPRIS track metadata: {err}"))?;

        let Some(raw_title) = metadata.title() else {
            return Ok(None);
        };
        let title = raw_title.trim().to_string();
        if title.is_empty() {
            return Ok(None);
        }

        // Tomar únicamente el artista principal si el bus devuelve múltiples
        let artist = metadata
            .artists()
            .and_then(|artists| artists.into_iter().next())
            .or_else(|| {
                metadata
                    .album_artists()
                    .and_then(|artists| artists.into_iter().next())
            })
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Unknown Artist")
            .to_string();

        let duration_ms = metadata
            .length()
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);

        let position_ms = player
            .get_position()
            .map(|pos| pos.as_millis() as u64)
            .unwrap_or(0);

        let is_playing = playback_status == PlaybackStatus::Playing;
        let id = TrackInfo::normalize_id(&artist, &title);

        let track_info = TrackInfo {
            id,
            title,
            artist,
            duration_ms,
        };

        let playback_tick = PlaybackTick {
            position_ms,
            is_playing,
        };

        Ok(Some((track_info, playback_tick)))
    }
}
