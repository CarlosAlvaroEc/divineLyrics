use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::models::{
    AppState, LyricsPayload, PlaybackTick, TrackInfo, EVENT_INTERACTION_MODE,
};
use crate::settings::{normalize_style, save_settings};

/// Devuelve el estado actual en memoria al montar el frontend:
/// `Option<(TrackInfo, Option<LyricsPayload>, PlaybackTick)>`.
#[tauri::command]
pub async fn get_initial_state(
    state: State<'_, AppState>,
) -> Result<Option<(TrackInfo, Option<LyricsPayload>, PlaybackTick)>, String> {
    let track = state.current_track.lock().await.clone();
    let Some(track_info) = track else {
        return Ok(None);
    };

    let lyrics = state.current_lyrics.lock().await.clone();
    let playback = state.current_playback.lock().await.clone();

    Ok(Some((track_info, lyrics, playback)))
}

/// Ejecuta `window.set_ignore_cursor_events(locked)` y emite `overlay:interaction-mode`.
#[tauri::command]
pub async fn toggle_click_through(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    locked: bool,
) -> Result<(), String> {
    apply_click_through(&app, &window, &state, locked).await
}

/// Helper compartido entre el comando IPC y el menú del System Tray.
pub async fn apply_click_through(
    app: &AppHandle,
    window: &WebviewWindow,
    state: &AppState,
    locked: bool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::WidgetExt;

        let gtk_win = window
            .gtk_window()
            .map_err(|err| format!("Failed to access GTK window: {err}"))?;

        if locked {
            let rect = gtk::gdk::cairo::RectangleInt::new(0, 0, 40, 40);
            let region = gtk::gdk::cairo::Region::create_rectangle(&rect);
            gtk_win.input_shape_combine_region(Some(&region));
        } else {
            gtk_win.input_shape_combine_region(None);
            window
                .set_ignore_cursor_events(false)
                .map_err(|err| format!("Failed to disable ignore cursor events: {err}"))?;
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        window
            .set_ignore_cursor_events(locked)
            .map_err(|err| format!("Failed to set ignore cursor events: {err}"))?;
    }

    if !locked {
        window
            .set_focus()
            .map_err(|err| format!("Failed to focus unlocked overlay window: {err}"))?;
    }

    // Algunos gestores de ventanas en Linux resetean los hints de apilamiento al cambiar el input shape
    window
        .set_always_on_top(true)
        .map_err(|err| format!("Failed to re-assert always on top: {err}"))?;

    {
        let mut locked_guard = state.locked.lock().await;
        *locked_guard = locked;
    }

    // Actualizar etiqueta del ítem en el System Tray
    if let Some(tray_item) = state.tray_lock_item.lock().await.as_ref() {
        let label = if locked {
            "Unlock Overlay"
        } else {
            "Lock Overlay"
        };
        let _ = tray_item.set_text(label);
    }

    app.emit(EVENT_INTERACTION_MODE, locked)
        .map_err(|err| format!("Failed to emit interaction mode event: {err}"))?;

    Ok(())
}

/// Obtiene el preset de estilo guardado en `settings.json`.
#[tauri::command]
pub async fn get_style_preference(state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.settings.lock().await;
    Ok(settings.style.clone())
}

/// Guarda el preset de estilo elegido en `settings.json`.
#[tauri::command]
pub async fn save_style_preference(
    app: AppHandle,
    state: State<'_, AppState>,
    style: String,
) -> Result<String, String> {
    let normalized = normalize_style(&style);
    let snapshot = {
        let mut settings = state.settings.lock().await;
        settings.style = normalized.clone();
        settings.clone()
    };

    save_settings(&app, &snapshot)?;
    Ok(normalized)
}

/// Alterna el modo click-through desde el icono de la bandeja del sistema (Tray).
pub async fn toggle_click_through_from_tray(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let next_locked = {
        let current = *state.locked.lock().await;
        !current
    };

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main overlay window not found".to_string())?;

    apply_click_through(app, &window, &state, next_locked).await?;
    Ok(next_locked)
}
