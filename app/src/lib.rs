//! FinchSpeak desktop client library (Tauri v2).
#![allow(clippy::collapsible_if, clippy::large_enum_variant, clippy::new_without_default)]

pub mod audio_engine;
pub mod audio_diagnostics;
mod capture_privacy;
pub mod updater;
pub mod commands;
mod tray;
pub mod hotkeys;
pub mod connection;
pub mod state;
pub mod activity;
pub mod credentials;
pub mod music;
mod music_clock;

use state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, RunEvent};

/// An invite link that arrived before the window was listening; the window takes it once at start.
static PENDING_INVITE: Mutex<Option<String>> = Mutex::new(None);

/// The `finchspeak://` argument of a launch or of a second launch that was handed to the running instance.
pub fn invite_argument(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().find(|arg| arg.len() <= 4096 && arg.to_ascii_lowercase().starts_with("finchspeak://"))
}

/// Keeps a cold-start link until the window asks for it once. Only the launch path calls this.
fn stash_invite(slot: &Mutex<Option<String>>, link: String) {
    if let Ok(mut held) = slot.lock() {
        *held = Some(link);
    }
}

/// Hands over the stashed link and forgets it.
fn take_invite(slot: &Mutex<Option<String>>) -> Option<String> {
    slot.lock().ok().and_then(|mut held| held.take())
}

/// Tells a running window about a link. The window validates it; nothing is trusted here.
fn announce_invite(app: &AppHandle, link: String) {
    if let Err(e) = app.emit("invite-link", link) {
        tracing::warn!("Could not announce the invite link: {e}");
    }
}

/// The invite link waiting from the last launch, if any; read once by the window at start.
pub fn take_pending_invite() -> Option<String> {
    take_invite(&PENDING_INVITE)
}

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

/// True when the app was launched by the Windows autostart entry and must stay in the tray.
pub fn should_start_hidden(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|a| a == "--minimized")
}

/// Set once the quit cleanup has started; the first exit request starts it.
static QUIT_CLEANUP_STARTED: AtomicBool = AtomicBool::new(false);
/// Set when the quit cleanup has finished; the exit it repeats is then let through.
static QUIT_CLEANUP_DONE: AtomicBool = AtomicBool::new(false);

/// Every exit (tray Quit, Windows log-off, updater) first leaves the voice room and closes the
/// session, so others see us leave at once. The exit is held until that finishes, then repeated.
fn on_run_event(app: &AppHandle, event: RunEvent) {
    let RunEvent::ExitRequested { api, .. } = event else { return };
    if QUIT_CLEANUP_DONE.load(Ordering::SeqCst) {
        // The cleanup already ran; an update queued since then still starts before this exit.
        updater::start_queued_installer();
        return;
    }
    api.prevent_exit();
    if QUIT_CLEANUP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let state = app.state::<AppState>().inner().clone();
    let app = app.clone();
    // Last resort: a blocking step that never reaches an await point must not keep the app alive.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(3));
        updater::start_queued_installer();
        std::process::exit(0);
    });
    tauri::async_runtime::spawn(async move {
        connection::shutdown_session(&state).await;
        QUIT_CLEANUP_DONE.store(true, Ordering::SeqCst);
        updater::start_queued_installer();
        app.exit(0);
    });
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

    match tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(link) = invite_argument(argv) {
                announce_invite(app, link);
            }
            tray::show_window(app)
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| hotkeys::handle(app, shortcut, event.state()))
                .build(),
        )
        .manage(state)
        .setup(|app| {
            tracing::info!("Tauri setup initialized");
            load_known_servers(app)?;
            #[cfg(windows)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                // Registers finchspeak:// for this user so that links open FinchSpeak (installer-independent).
                if let Err(e) = app.deep_link().register_all() {
                    tracing::warn!("Could not register the finchspeak:// links: {e}");
                }
            }
            if let Some(link) = invite_argument(std::env::args()) {
                stash_invite(&PENDING_INVITE, link.clone());
                announce_invite(app.handle(), link);
            }
            connection::spawn_sleep_watch(app.state::<AppState>().inner().clone());
            let language = match app.state::<AppState>().known_servers_path.blocking_read().as_deref().and_then(|path| path.parent()) {
                Some(dir) => tray::saved_language(dir),
                None => "ru".to_string(),
            };
            if let Err(e) = tray::setup(app.handle(), &language) {
                tracing::warn!("Could not create the tray icon: {e}");
            }
            let hidden = should_start_hidden(std::env::args());
            if let Some(window) = app.get_webview_window("main") {
                if hidden {
                    tracing::info!("Started with --minimized, staying in the tray");
                } else {
                    tracing::info!("Main window found, ensuring show and focus");
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            } else {
                tracing::warn!("Main window not created by config, creating manually");
                let window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                    .title("FinchSpeak")
                    .inner_size(1280.0, 800.0)
                    .min_inner_size(900.0, 600.0)
                    .build()?;
                if !hidden {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // The close button always keeps FinchSpeak running in the tray; only the tray's Quit really closes.
                let state = window.app_handle().state::<AppState>();
                if !state.quitting.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::update::app_version,
            commands::update::check_update,
            commands::update::install_update,
            commands::window::set_tray_state,
            commands::window::set_attention,
            commands::window::show_main_window,
            commands::window::system_idle_seconds,
            // Auth
            commands::auth::login,
            commands::auth::register,
            commands::auth::resume_session,
            commands::auth::logout,
            commands::auth::discard_session,
            commands::auth::list_sessions,
            commands::auth::end_session,
            commands::auth::end_other_sessions,
            commands::auth::reconnect_now,
            commands::auth::get_current_user,
            commands::auth::get_session_snapshot,
            commands::auth::trust_fingerprint,
            commands::auth::forget_server_fingerprint,
            commands::auth::server_info,
            commands::window::take_invite_link,
            commands::chat::mark_channel_read,
            commands::chat::upload_avatar,
            commands::chat::remove_avatar,
            commands::chat::download_avatar,
            commands::chat::upload_join_sound,
            commands::chat::remove_join_sound,
            commands::chat::download_join_sound,
            commands::chat::set_status,
            activity::set_activity_sharing,
            commands::chat::search_messages,
            commands::admin::audit_log,
            commands::admin::server_overview,
            commands::admin::set_registration,
            commands::admin::backup_now,
            commands::admin::moderate_voice,
            commands::admin::move_voice,
            commands::voice::test_microphone,
            commands::voice::start_audio_diagnostics,
            commands::voice::stop_audio_diagnostics,
            commands::voice::get_audio_diagnostics,
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
            commands::voice::set_spatial_audio,
            commands::voice::set_speaker_pan,
            commands::voice::get_voice_settings,
            commands::voice::set_voice_settings,
            commands::voice::set_hotkey,
            // Administration
            commands::admin::create_invite,
            commands::admin::list_invites,
            commands::admin::revoke_invite,
            commands::admin::admin_users,
            commands::admin::set_user_role,
            commands::admin::kick_user,
            commands::admin::ban_user,
            commands::admin::unban_user,
            commands::admin::reset_user_password,
            commands::admin::update_channel,
            commands::admin::delete_channel,
            commands::admin::reorder_channels,
            // Screen
            commands::screen::register_video_channel,
            commands::screen::subscribe_screen_share,
            commands::screen::unsubscribe_screen_share,
            commands::screen::request_idr_keyframe,
            commands::screen::list_screen_sources,
            commands::screen::process_loopback_supported,
            commands::screen::start_screen_share,
            commands::screen::stop_screen_share,
        ])
        .build(tauri::generate_context!())
    {
        Ok(app) => app.run(on_run_event),
        Err(e) => tracing::error!("Error while running FinchSpeak application: {e}"),
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

#[cfg(test)]
mod tests {
    use super::{invite_argument, should_start_hidden, stash_invite, take_invite};

    #[test]
    fn a_stashed_invite_is_taken_once() {
        let slot = std::sync::Mutex::new(None);
        stash_invite(&slot, "finchspeak://join?x".to_string());
        assert_eq!(take_invite(&slot), Some("finchspeak://join?x".to_string()));
        assert_eq!(take_invite(&slot), None);
    }

    #[test]
    fn invite_argument_takes_only_finchspeak_links() {
        let args = ["finch-app.exe", "--minimized", "finchspeak://join?server=x&code=y"].map(String::from);
        assert_eq!(invite_argument(args), Some("finchspeak://join?server=x&code=y".to_string()));
        assert_eq!(invite_argument(["finch-app.exe".to_string(), "https://example.com".to_string()]), None);
        assert_eq!(invite_argument(["finch-app.exe".to_string(), format!("finchspeak://{}", "a".repeat(5000))]), None);
    }

    #[test]
    fn should_start_hidden_only_with_flag() {
        assert!(!should_start_hidden(["finch-app.exe".to_string()]));
        assert!(should_start_hidden(["finch-app.exe".to_string(), "--minimized".to_string()]));
    }
}
