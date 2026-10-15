//! # FinchSpeak Client Core
//!
//! UI-independent client logic shared by the Tauri app and headless test harnesses.
//!
//! ## Modules
//!
//! - [`api`] — REST API client (auth, channels, history, files)
//! - [`screen`] — screen share packetizer, viewer state machine and capture driver
//! - [`session`] — QUIC session: authentication, control stream, media events
//! - [`tofu`] — Trust-On-First-Use certificate verification
//! - [`error`] — Client error type

pub mod api;
pub mod error;
pub mod screen;
pub mod session;
pub mod tofu;
