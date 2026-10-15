//! FinchSpeak desktop client library (Tauri v2).
#![allow(clippy::collapsible_if, clippy::large_enum_variant, clippy::new_without_default)]

pub mod audio_engine;
pub mod commands;
pub mod connection;
pub mod state;

use state::AppState;

pub fn run() {
    // Initialize tracing subscriber
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,finch_app=debug".into()),
        )
        .try_init();

    let state = AppState::new();

    if let Err(e) = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // Auth
            commands::auth::login,
            commands::auth::register,
            commands::auth::logout,
            commands::auth::get_current_user,
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
