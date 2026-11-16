use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

use crate::lyrics::cache::{load_lyrics_from_disk, save_lyrics_to_disk};
use crate::lyrics::client::LrclibClient;
use crate::models::{
    AppState, PlaybackTick, TrackInfo, EVENT_LYRICS_LOADED, EVENT_PLAYBACK_TICK,
    EVENT_TRACK_CHANGED,
};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub use linux::PlatformMediaProvider;
#[cfg(target_os = "windows")]
pub use windows::PlatformMediaProvider;

/// Abstracción unificada para lectura de metadatos y reloj de reproducción del SO.
pub trait MediaProvider: Send + Sync + 'static {
    /// Obtiene la pista activa de Spotify y su estado de reproducción actual.
    /// Devuelve `Ok(None)` si Spotify está cerrado o no hay pista activa.
    fn read_active_media(&self) -> Result<Option<(TrackInfo, PlaybackTick)>, String>;
}

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const TICK_EMIT_INTERVAL: Duration = Duration::from_millis(1000);
const SEEK_THRESHOLD_MS: u64 = 1500;

/// Inicia el bucle asíncrono en segundo plano que supervisa el reproductor activo,
/// gestiona la caché de LRCLIB y emite los eventos IPC hacia el frontend.
pub fn spawn_media_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let provider = PlatformMediaProvider::new();
        let lrclib_client = match LrclibClient::new() {
            Ok(client) => client,
            Err(err) => {
                eprintln!("[DivineLyrics] Error initializing LRCLIB client: {err}");
                return;
            }
        };

        let mut last_poll_instant = Instant::now();
        let mut last_tick_emitted = Instant::now();

        loop {
            let poll_provider = provider.clone();
            let snapshot_res =
                tokio::task::spawn_blocking(move || poll_provider.read_active_media()).await;

            let now = Instant::now();
            let elapsed_ms = now.duration_since(last_poll_instant).as_millis() as u64;
            last_poll_instant = now;

            if let Ok(Ok(snapshot)) = snapshot_res {
                handle_media_snapshot(
                    &app,
                    &lrclib_client,
                    snapshot,
                    elapsed_ms,
                    &mut last_tick_emitted,
                )
                .await;
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

async fn handle_media_snapshot(
    app: &AppHandle,
    lrclib_client: &LrclibClient,
    snapshot: Option<(TrackInfo, PlaybackTick)>,
    elapsed_ms: u64,
    last_tick_emitted: &mut Instant,
) {
    let state = app.state::<AppState>();

    match snapshot {
        None => {
            let mut track_guard = state.current_track.lock().await;
            if track_guard.is_some() {
                *track_guard = None;
                drop(track_guard);

                *state.current_lyrics.lock().await = None;
                *state.current_playback.lock().await = PlaybackTick::default();

                let _ = app.emit(EVENT_TRACK_CHANGED, Option::<TrackInfo>::None);
            }
        }
        Some((track, playback)) => {
            let previous_track = state.current_track.lock().await.clone();
            let track_changed = previous_track
                .as_ref()
                .map(|prev| prev.id != track.id)
                .unwrap_or(true);

            if track_changed {
                {
                    let mut track_guard = state.current_track.lock().await;
                    *track_guard = Some(track.clone());
                }
                {
                    let mut lyrics_guard = state.current_lyrics.lock().await;
                    *lyrics_guard = None;
                }
                {
                    let mut playback_guard = state.current_playback.lock().await;
                    *playback_guard = playback.clone();
                }

                let _ = app.emit(EVENT_TRACK_CHANGED, Some(track.clone()));
                let _ = app.emit(EVENT_PLAYBACK_TICK, playback.clone());
                *last_tick_emitted = Instant::now();

                // Consultar caché o LRCLIB en tarea asíncrona dedicada
                let app_clone = app.clone();
                let client_clone = lrclib_client.clone();
                let track_clone = track.clone();
                tauri::async_runtime::spawn(async move {
                    resolve_and_emit_lyrics(&app_clone, &client_clone, &track_clone).await;
                });
                return;
            }

            // Misma pista: detectar cambio de play/pause, seek o pulso de ~1000 ms
            let previous_playback = state.current_playback.lock().await.clone();
            let play_state_changed = previous_playback.is_playing != playback.is_playing;

            let expected_pos = if previous_playback.is_playing {
                previous_playback.position_ms.saturating_add(elapsed_ms)
            } else {
                previous_playback.position_ms
            };
            let seek_detected = playback.position_ms.abs_diff(expected_pos) > SEEK_THRESHOLD_MS;

            let periodic_tick_due =
                playback.is_playing && last_tick_emitted.elapsed() >= TICK_EMIT_INTERVAL;

            {
                let mut playback_guard = state.current_playback.lock().await;
                *playback_guard = playback.clone();
            }

            if play_state_changed || seek_detected || periodic_tick_due {
                let _ = app.emit(EVENT_PLAYBACK_TICK, playback);
                *last_tick_emitted = Instant::now();
            }
        }
    }
}

async fn resolve_and_emit_lyrics(
    app: &AppHandle,
    lrclib_client: &LrclibClient,
    track: &TrackInfo,
) {
    let state = app.state::<AppState>();

    // 1. Verificar caché en memoria (RAM) indexada por TrackInfo.id
    let cached_ram = {
        let cache_guard = state.lyrics_cache.lock().await;
        cache_guard.get(&track.id).cloned()
    };

    let payload = match cached_ram {
        Some(cached) => cached,
        None => {
            // 2. Verificar caché en disco (app_cache_dir/lyrics/{track_id}.json)
            if let Some(disk_cached) = load_lyrics_from_disk(app, &track.id) {
                let mut cache_guard = state.lyrics_cache.lock().await;
                cache_guard.insert(track.id.clone(), disk_cached.clone());
                disk_cached
            } else {
                // 3. Consultar API LRCLIB y persistir en RAM + Disco
                match lrclib_client.fetch_lyrics(track).await {
                    Ok(fetched) => {
                        {
                            let mut cache_guard = state.lyrics_cache.lock().await;
                            cache_guard.insert(track.id.clone(), fetched.clone());
                        }
                        if let Err(err) = save_lyrics_to_disk(app, &fetched) {
                            eprintln!(
                                "[DivineLyrics] Failed to persist lyrics cache for '{}': {err}",
                                track.id
                            );
                        }
                        fetched
                    }
                    Err(err) => {
                        eprintln!(
                            "[DivineLyrics] Error fetching lyrics for '{}': {err}",
                            track.id
                        );
                        return;
                    }
                }
            }
        }
    };

    // Confirmar que la pista activa no haya cambiado mientras se descargaba la letra
    let is_still_current = {
        let current_guard = state.current_track.lock().await;
        current_guard
            .as_ref()
            .map(|curr| curr.id == track.id)
            .unwrap_or(false)
    };

    if is_still_current {
        {
            let mut lyrics_guard = state.current_lyrics.lock().await;
            *lyrics_guard = Some(payload.clone());
        }
        let _ = app.emit(EVENT_LYRICS_LOADED, payload);
    }
}
