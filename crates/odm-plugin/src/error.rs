use thiserror::Error;

/// Errors produced by the plugin subsystem.
#[derive(Debug, Error)]
pub enum PluginError {
    /// An error occurred while parsing or validating a plugin manifest.
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),

    /// The specified plugin role is unsupported.
    #[error("unsupported plugin role '{0}'; only 'resolver' is supported")]
    UnsupportedRole(String),

    /// Plugin process execution failed.
    #[error("plugin process failed: {0}")]
    ProcessFailed(String),

    /// Plugin execution timed out.
    #[error("plugin execution timed out: {0}")]
    Timeout(String),

    /// Plugin output was not valid JSON.
    #[error("invalid JSON from plugin: {0}")]
    InvalidJson(String),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
