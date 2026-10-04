//! MinRegistry: a self-hosted, single-tenant OCI container registry.
//!
//! The crate is a binary; this library target exists so integration tests can
//! build the application in-process. Everything is `pub(crate)` unless tests
//! or `main.rs` need it.

#![deny(unsafe_code)]

pub(crate) mod api;
pub mod app;
pub(crate) mod audit;
pub(crate) mod auth;
pub mod cli;
pub mod config;
pub(crate) mod db;
pub(crate) mod digest;
pub(crate) mod error;
pub(crate) mod gc;
pub(crate) mod registry;
pub(crate) mod storage;
pub(crate) mod tasks;
pub(crate) mod time;
pub(crate) mod ui;

pub use api::openapi_json;
pub use app::App;
pub use config::{CoreConfig, ServeConfig};

/// TLS for outgoing HTTPS (GitHub, S3) uses rustls with the `ring` provider.
pub(crate) fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
