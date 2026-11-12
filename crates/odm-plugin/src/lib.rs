//! External-process plugin host for odm.
#![forbid(unsafe_code)]

pub mod error;
pub mod manifest;

pub use error::PluginError;
pub use manifest::{PluginManifest, PluginRole};
