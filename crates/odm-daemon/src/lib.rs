//! The odm background service library: REST and WebSocket API over the download engine.
#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod routes;
pub mod scheduler;
pub mod server;
pub mod state;
pub mod ws;
