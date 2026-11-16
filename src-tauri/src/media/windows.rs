#![cfg(target_os = "windows")]

use std::time::{SystemTime, UNIX_EPOCH};

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

use crate::media::MediaProvider;
use crate::models::{PlaybackTick, TrackInfo};

/// Diferencia en intervalos de 100ns entre la época FILETIME (1601-01-01) y UNIX_EPOCH (1970-01-01).
const WINDOWS_TO_UNIX_EPOCH_100NS: i64 = 116_444_736_000_000_000;

#[derive(Debug, Clone, Default)]
pub struct PlatformMediaProvider;

impl PlatformMediaProvider {
    pub fn new() -> Self {
        Self
    }

    fn find_spotify_session(
        manager: &GlobalSystemMediaTransportControlsSessionManager,
    ) -> Result<Option<GlobalSystemMediaTransportControlsSession>, String> {
        let sessions = manager
            .GetSessions()
            .map_err(|err| format!("SMTC GetSessions failed: {err}"))?;

        for session in sessions {
            if let Ok(app_id) = session.SourceAppUserModelId() {
                if app_id.to_string().to_lowercase().contains("spotify") {
                    return Ok(Some(session));
                }
            }
        }

        Ok(None)
    }

    fn compute_elapsed_since_last_update_ms(last_updated_100ns: i64) -> u64 {
        let Ok(since_unix) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return 0;
        };
        let now_100ns = (since_unix.as_nanos() / 100) as i64 + WINDOWS_TO_UNIX_EPOCH_100NS;
        let diff_100ns = now_100ns.saturating_sub(last_updated_100ns);
        if diff_100ns > 0 {
            (diff_100ns / 10_000) as u64
        } else {
            0
        }
    }
}

impl MediaProvider for PlatformMediaProvider {
    fn read_active_media(&self) -> Result<Option<(TrackInfo, PlaybackTick)>, String> {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|err| format!("SMTC RequestAsync failed: {err}"))?
            .get()
            .map_err(|err| format!("SMTC RequestAsync().get() failed: {err}"))?;

        let Some(session) = Self::find_spotify_session(&manager)? else {
            return Ok(None);
        };

        let playback_info = session
            .GetPlaybackInfo()
            .map_err(|err| format!("SMTC GetPlaybackInfo failed: {err}"))?;
        let status = playback_info
            .PlaybackStatus()
            .map_err(|err| format!("SMTC PlaybackStatus failed: {err}"))?;

        if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed
            || status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped
        {
            return Ok(None);
        }

        let is_playing = status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;

        let properties = session
            .TryGetMediaPropertiesAsync()
            .map_err(|err| format!("SMTC TryGetMediaPropertiesAsync failed: {err}"))?
            .get()
            .map_err(|err| format!("SMTC TryGetMediaPropertiesAsync().get() failed: {err}"))?;

        let title = properties
            .Title()
            .map(|h| h.to_string())
            .unwrap_or_default()
            .trim()
            .to_string();
        if title.is_empty() {
            return Ok(None);
        }

        let artist = properties
            .Artist()
            .map(|h| h.to_string())
            .unwrap_or_default()
            .trim()
            .to_string();
        let artist = if artist.is_empty() {
            "Unknown Artist".to_string()
        } else {
            artist
        };

        let timeline = session
            .GetTimelineProperties()
            .map_err(|err| format!("SMTC GetTimelineProperties failed: {err}"))?;

        let end_time_ms = (timeline
            .EndTime()
            .map(|t| t.Duration)
            .unwrap_or(0)
            .max(0)
            / 10_000) as u64;
        let start_time_ms = (timeline
            .StartTime()
            .map(|t| t.Duration)
            .unwrap_or(0)
            .max(0)
            / 10_000) as u64;
        let duration_ms = end_time_ms.saturating_sub(start_time_ms);

        let base_pos_ms = (timeline
            .Position()
            .map(|t| t.Duration)
            .unwrap_or(0)
            .max(0)
            / 10_000) as u64;

        let position_ms = if is_playing {
            let extra_ms = timeline
                .LastUpdatedTime()
                .map(|dt| Self::compute_elapsed_since_last_update_ms(dt.UniversalTime))
                .unwrap_or(0);
            let computed = base_pos_ms.saturating_add(extra_ms);
            if duration_ms > 0 {
                computed.min(duration_ms)
            } else {
                computed
            }
        } else {
            base_pos_ms
        };

        let id = TrackInfo::normalize_id(&artist, &title);

        Ok(Some((
            TrackInfo {
                id,
                title,
                artist,
                duration_ms,
            },
            PlaybackTick {
                position_ms,
                is_playing,
            },
        )))
    }
}
