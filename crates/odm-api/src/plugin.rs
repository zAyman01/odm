//! Plugin API types.

use serde::{Deserialize, Serialize};

/// Information about an installed plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginDto {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub role: String,
    pub url_patterns: Vec<String>,
    pub command: Vec<String>,
}

/// Response payload for plugin list queries (`GET /api/v1/plugins`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginListResponse {
    pub plugins: Vec<PluginDto>,
}
