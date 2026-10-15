//! # FinchSpeak Testbed
//!
//! Headless test bench: an in-process server, headless clients built on
//! `finch-client-core`, and voice quality measurements.
//!
//! ## Modules
//!
//! - [`server`] — in-process server on ephemeral ports with a temporary database
//! - [`client`] — headless client with event helpers

pub mod client;
pub mod server;

/// Enable `RUST_LOG`-controlled logging for bench runs (idempotent).
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_test_writer()
        .try_init();
}
