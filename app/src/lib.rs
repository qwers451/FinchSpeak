//! FinchSpeak desktop client library (Tauri v2).
#![allow(clippy::collapsible_if, clippy::large_enum_variant, clippy::new_without_default)]

pub mod audio_engine;
pub mod commands;
pub mod connection;
pub mod state;

use state::AppState;
use tauri::Manager;

/// Load pinned server fingerprints from `<app data dir>/known_servers.txt`.
fn load_known_servers(app: &tauri::App) {
    let state = app.state::<AppState>();
    let path = match app.path().app_data_dir() {
        Ok(dir) => dir.join("known_servers.txt"),
        Err(e) => {
            tracing::error!("No app data dir, server fingerprints will not persist: {e}");
            return;
        }
    };
    match finch_client_core::tofu::load_fingerprints(&path) {
        Ok(known) => {
            for (host, fingerprint) in known {
                state.saved_fingerprints.insert(host, fingerprint);
            }
        }
        Err(e) => tracing::error!("Failed to read {path:?}: {e}"),
    }
    *state.known_servers_path.blocking_write() = Some(path);
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
        .manage(state)
        .setup(|app| {
            tracing::info!("Tauri setup initialized");
            load_known_servers(app);
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
        .invoke_handler(tauri::generate_handler![
            // Auth
            commands::auth::login,
            commands::auth::register,
            commands::auth::logout,
            commands::auth::get_current_user,
            commands::auth::get_session_snapshot,
            commands::auth::trust_fingerprint,
            // Chat
            commands::chat::list_channels,
            commands::chat::create_channel,
            commands::chat::fetch_channel_messages,
            commands::chat::send_text_message,
            commands::chat::upload_file,
            // Voice
            commands::voice::join_voice_channel,
            commands::voice::leave_voice_channel,
            commands::voice::set_voice_state,
            commands::voice::get_audio_devices,
            commands::voice::set_speaker_volume,
            // Screen
            commands::screen::register_video_channel,
            commands::screen::subscribe_screen_share,
            commands::screen::unsubscribe_screen_share,
            commands::screen::request_idr_keyframe,
        ])
        .run(tauri::generate_context!())
    {
        tracing::error!("Error while running FinchSpeak application: {e}");
    }
}
