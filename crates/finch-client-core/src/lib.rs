//! # FinchSpeak Client Core
//!
//! UI-independent client logic shared by the Tauri app and headless test harnesses.
//!
//! ## Modules
//!
//! - [`api`] — REST API client (auth, channels, history, files)
//! - [`session`] — QUIC session: authentication, control stream, media events
//! - [`tofu`] — Trust-On-First-Use certificate verification
//! - [`error`] — Client error type

pub mod api;
pub mod error;
pub mod session;
pub mod tofu;
