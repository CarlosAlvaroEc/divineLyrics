pub mod commands;
pub mod lyrics;
pub mod media;
pub mod models;
pub mod settings;

use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, Monitor, PhysicalPosition, WebviewWindow, WindowEvent,
};

use crate::models::AppState;
use crate::settings::{load_settings, save_settings};

const TRAY_ITEM_TOGGLE_LOCK: &str = "toggle_lock";
const TRAY_ITEM_QUIT: &str = "quit";

fn is_position_within_monitors(x: i32, y: i32, monitors: &[Monitor]) -> bool {
    monitors.iter().any(|monitor| {
        let pos = monitor.position();
        let size = monitor.size();
        let min_x = pos.x;
        let min_y = pos.y;
        let max_x = pos.x + size.width as i32 - 40;
        let max_y = pos.y + size.height as i32 - 40;
        x >= min_x && x <= max_x && y >= min_y && y <= max_y
    })
}

fn position_window_from_settings_or_default(
    main_window: &WebviewWindow,
    saved_x: Option<i32>,
    saved_y: Option<i32>,
) -> tauri::Result<()> {
    if let (Some(x), Some(y)) = (saved_x, saved_y) {
        let monitors = main_window.available_monitors().unwrap_or_default();
        if is_position_within_monitors(x, y, &monitors) {
            return main_window.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
        }
    }

    if let Some(monitor) = main_window.primary_monitor()? {
        let monitor_pos = monitor.position();
        let monitor_size = monitor.size();
        let window_size = main_window.outer_size()?;
        let margin_px = (24.0 * monitor.scale_factor()).round() as i32;

        let x = monitor_pos.x + monitor_size.width as i32 - window_size.width as i32 - margin_px;
        let y = monitor_pos.y + monitor_size.height as i32 - window_size.height as i32 - margin_px;

        main_window.set_position(tauri::Position::Physical(PhysicalPosition::new(
            x.max(monitor_pos.x),
            y.max(monitor_pos.y),
        )))?;
    }

    Ok(())
}

fn schedule_save_window_position(app: &AppHandle, x: i32, y: i32) {
    let state = app.state::<AppState>();
    let seq = state.move_debounce_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let state = app_handle.state::<AppState>();
        if state.move_debounce_seq.load(Ordering::SeqCst) != seq {
            return;
        }

        let snapshot = {
            let mut settings = state.settings.lock().await;
            settings.x = Some(x);
            settings.y = Some(y);
            settings.clone()
        };

        if let Err(err) = save_settings(&app_handle, &snapshot) {
            eprintln!("[DivineLyrics] Failed to save window position: {err}");
        }
    });
}

#[cfg(target_os = "windows")]
const HOTSPOT_SIZE_LOGICAL: f64 = 40.0;
#[cfg(target_os = "windows")]
const HIT_TEST_INTERVAL: Duration = Duration::from_millis(60);

/// Hit-testing ligero en Windows cada 60ms para que la pestaña de 40x40px en la esquina superior izquierda
/// siga siendo clicable aunque `set_ignore_cursor_events(true)` esté activo en el resto del overlay.
#[cfg(target_os = "windows")]
fn spawn_hotspot_hit_testing(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_locked = false;
        let mut currently_ignoring = false;

        loop {
            tokio::time::sleep(HIT_TEST_INTERVAL).await;

            let is_locked = {
                let state = app.state::<AppState>();
                let locked = *state.locked.lock().await;
                locked
            };

            if is_locked != last_locked {
                currently_ignoring = is_locked;
                last_locked = is_locked;
            }

            let Some(main_window) = app.get_webview_window("main") else {
                continue;
            };

            if !is_locked {
                if currently_ignoring {
                    let _ = main_window.set_ignore_cursor_events(false);
                    currently_ignoring = false;
                }
                continue;
            }

            let (Ok(cursor_pos), Ok(win_pos), Ok(scale)) = (
                app.cursor_position(),
                main_window.outer_position(),
                main_window.scale_factor(),
            ) else {
                continue;
            };

            let scale = if scale > 0.0 { scale } else { 1.0 };
            let hotspot_phys = HOTSPOT_SIZE_LOGICAL * scale;
            let rel_x = cursor_pos.x - win_pos.x as f64;
            let rel_y = cursor_pos.y - win_pos.y as f64;

            let in_hotspot = rel_x >= 0.0
                && rel_x <= hotspot_phys
                && rel_y >= 0.0
                && rel_y <= hotspot_phys;

            if in_hotspot && currently_ignoring {
                if main_window.set_ignore_cursor_events(false).is_ok() {
                    currently_ignoring = false;
                }
            } else if !in_hotspot && !currently_ignoring {
                if main_window.set_ignore_cursor_events(true).is_ok() {
                    currently_ignoring = true;
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_initial_state,
            commands::toggle_click_through,
            commands::get_style_preference,
            commands::save_style_preference
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::Moved(position) = event {
                    schedule_save_window_position(window.app_handle(), position.x, position.y);
                }
            }
        })
        .setup(move |app| {
            // Cargar configuración persistida en settings.json
            let initial_settings = load_settings(app.handle());
            {
                let state = app.state::<AppState>();
                let mut settings_guard = state.settings.blocking_lock();
                *settings_guard = initial_settings.clone();
            }

            let toggle_lock_item = MenuItem::with_id(
                app,
                TRAY_ITEM_TOGGLE_LOCK,
                "Lock Overlay",
                true,
                None::<&str>,
            )?;
            let quit_item =
                MenuItem::with_id(app, TRAY_ITEM_QUIT, "Quit", true, None::<&str>)?;

            {
                let state = app.state::<AppState>();
                let mut tray_item_guard = state.tray_lock_item.blocking_lock();
                *tray_item_guard = Some(toggle_lock_item.clone());
            }

            let tray_menu = Menu::with_items(app, &[&toggle_lock_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("DivineLyrics")
                .on_menu_event(|app_handle, event| match event.id.as_ref() {
                    TRAY_ITEM_TOGGLE_LOCK => {
                        let handle = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(err) =
                                commands::toggle_click_through_from_tray(&handle).await
                            {
                                eprintln!(
                                    "[DivineLyrics] Tray toggle_click_through error: {err}"
                                );
                            }
                        });
                    }
                    TRAY_ITEM_QUIT => {
                        app_handle.exit(0);
                    }
                    _ => {}
                });

            if let Some(default_icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(default_icon);
            }

            let _tray = tray_builder.build(app)?;

            // Reforzar transparencia RGBA, Always-On-Top y restaurar posición
            if let Some(main_window) = app.get_webview_window("main") {
                #[cfg(target_os = "linux")]
                {
                    use gtk::prelude::WidgetExt;
                    use webkit2gtk::WebViewExt;

                    if let Ok(gtk_win) = main_window.gtk_window() {
                        if let Some(visual) = gtk_win.screen().and_then(|s| s.rgba_visual()) {
                            gtk_win.set_visual(Some(&visual));
                        }
                        gtk_win.set_app_paintable(true);
                        gtk_win.input_shape_combine_region(None);
                    }

                    let _ = main_window.with_webview(|webview| {
                        let wv = webview.inner();
                        wv.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
                    });
                }

                // Arrancar desbloqueado por defecto (is_locked = false)
                main_window.set_ignore_cursor_events(false)?;
                main_window.set_always_on_top(true)?;
                main_window.set_visible_on_all_workspaces(true)?;
                main_window.set_skip_taskbar(true)?;

                position_window_from_settings_or_default(
                    &main_window,
                    initial_settings.x,
                    initial_settings.y,
                )?;
            }

            // En Windows, iniciar hit-testing de la pestaña en esquina superior izquierda (40x40px)
            #[cfg(target_os = "windows")]
            spawn_hotspot_hit_testing(app.handle().clone());

            // Iniciar monitor de medios del sistema operativo (MPRIS en Linux / Stub en Windows)
            media::spawn_media_watcher(app.handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DivineLyrics Tauri application");
}
