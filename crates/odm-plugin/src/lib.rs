//! External-process plugin host for odm.
#![forbid(unsafe_code)]

pub mod error;
pub mod host;
pub mod manifest;
pub mod protocol;

pub use error::PluginError;
pub use host::PluginHost;
pub use manifest::{PluginManifest, PluginRole};
pub use protocol::{PROTOCOL_VERSION, ResolvedDownload, ResolverRequest, ResolverResponse};
