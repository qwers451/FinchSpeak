//! FinchSpeak desktop client library (Tauri v2).
#![allow(clippy::collapsible_if, clippy::large_enum_variant, clippy::new_without_default)]

pub mod audio_engine;
mod capture_privacy;
pub mod updater;
pub mod commands;
mod tray;
pub mod connection;
pub mod state;

use state::AppState;
use tauri::Manager;

/// Load pinned server fingerprints from `<app data dir>/known_servers.txt`.
fn profile_directory(default: std::path::PathBuf, explicit: Option<std::ffi::OsString>) -> std::io::Result<std::path::PathBuf> {
    match explicit {
        None => Ok(default),
        Some(value) => {
            let path = std::path::PathBuf::from(value);
            if !path.is_absolute() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "FINCHSPEAK_PROFILE_DIR must be absolute")); }
            Ok(path)
        }
    }
}
fn load_known_servers(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let path = profile_directory(app.path().app_data_dir()?, std::env::var_os("FINCHSPEAK_PROFILE_DIR"))?.join("known_servers.txt");
    match finch_client_core::tofu::load_fingerprints(&path) {
        Ok(known) => {
            for (host, fingerprint) in known {
                state.saved_fingerprints.insert(host, fingerprint);
            }
        }
        Err(e) => tracing::error!("Failed to read {path:?}: {e}"),
    }
    *state.known_servers_path.blocking_write() = Some(path);
    Ok(())
}

pub fn run() {
    // Initialize tracing subscriber
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,finch_app=debug".into()),
        )
        .try_init();

    tracing::info!("Initializing FinchSpeak client application...");

    let state = AppState::new();

    if let Err(e) = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    let settings = app.state::<AppState>().voice_settings.clone();
                    settings.set_key_down(event.state() == ShortcutState::Pressed);
                })
                .build(),
        )
        .manage(state)
        .setup(|app| {
            tracing::info!("Tauri setup initialized");
            load_known_servers(app)?;
            if let Err(e) = tray::setup(app.handle()) {
                tracing::warn!("Could not create the tray icon: {e}");
            }
            if let Some(window) = app.get_webview_window("main") {
                tracing::info!("Main window found, ensuring show and focus");
                let _ = window.show();
                let _ = window.set_focus();
            } else {
                tracing::warn!("Main window not created by config, creating manually");
                let window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                    .title("FinchSpeak")
                    .inner_size(1280.0, 800.0)
                    .min_inner_size(900.0, 600.0)
                    .build()?;
                let _ = window.show();
                let _ = window.set_focus();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use std::sync::atomic::Ordering;
                let state = window.app_handle().state::<AppState>();
                if state.close_to_tray.load(Ordering::SeqCst) && !state.quitting.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::update::app_version,
            commands::update::check_update,
            commands::update::install_update,
            commands::window::set_close_to_tray,
            commands::window::set_tray_state,
            commands::window::show_main_window,
            // Auth
            commands::auth::login,
            commands::auth::register,
            commands::auth::logout,
            commands::auth::get_current_user,
            commands::auth::get_session_snapshot,
            commands::auth::trust_fingerprint,
            commands::auth::forget_server_fingerprint,
            commands::auth::server_info,
            commands::chat::mark_channel_read,
            commands::chat::upload_avatar,
            commands::chat::remove_avatar,
            commands::chat::download_avatar,
            commands::chat::set_status,
            commands::chat::search_messages,
            commands::admin::audit_log,
            commands::admin::server_overview,
            commands::admin::set_registration,
            commands::admin::backup_now,
            commands::admin::moderate_voice,
            commands::voice::test_microphone,
            commands::screen::capture_thumbnail,
            commands::chat::download_thumb,
            // Chat
            commands::chat::list_channels,
            commands::chat::create_channel,
            commands::chat::fetch_channel_messages,
            commands::chat::send_text_message,
            commands::chat::upload_file,
            commands::chat::download_file,
            commands::chat::edit_message,
            commands::chat::delete_message,
            commands::chat::set_reaction,
            commands::chat::send_typing,
            commands::chat::update_profile,
            commands::chat::change_password,
            // Voice
            commands::voice::join_voice_channel,
            commands::voice::leave_voice_channel,
            commands::voice::set_voice_state,
            commands::voice::get_audio_devices,
            commands::voice::set_audio_devices,
            commands::voice::set_speaker_volume,
            commands::voice::get_voice_settings,
            commands::voice::set_voice_settings,
            commands::voice::set_push_to_talk_key,
            // Administration
            commands::admin::create_invite,
            commands::admin::list_invites,
            commands::admin::revoke_invite,
            commands::admin::admin_users,
            commands::admin::set_user_role,
            commands::admin::kick_user,
            commands::admin::ban_user,
            commands::admin::unban_user,
            commands::admin::update_channel,
            commands::admin::delete_channel,
            commands::admin::reorder_channels,
            // Screen
            commands::screen::register_video_channel,
            commands::screen::subscribe_screen_share,
            commands::screen::unsubscribe_screen_share,
            commands::screen::request_idr_keyframe,
            commands::screen::list_screen_sources,
            commands::screen::start_screen_share,
            commands::screen::stop_screen_share,
        ])
        .run(tauri::generate_context!())
    {
        tracing::error!("Error while running FinchSpeak application: {e}");
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[test]
    fn explicit_profile_is_absolute_and_never_falls_back() {
        let default = std::path::PathBuf::from(r"C:\default");
        assert_eq!(profile_directory(default.clone(), None).unwrap(), default);
        let isolated = std::ffi::OsString::from(r"C:\QA Ж\a");
        assert_eq!(profile_directory(default.clone(), Some(isolated.clone())).unwrap(), std::path::PathBuf::from(isolated));
        assert!(profile_directory(default.clone(), Some("relative".into())).is_err());
        assert!(profile_directory(default, Some("".into())).is_err());
    }
}
