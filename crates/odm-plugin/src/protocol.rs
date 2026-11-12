use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Current protocol version string sent in resolver requests.
pub const PROTOCOL_VERSION: &str = "1";

/// Request payload sent to a plugin's stdin when resolving a URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolverRequest {
    /// Protocol version identifier.
    pub version: String,
    /// The target URL to resolve.
    pub url: String,
}

impl ResolverRequest {
    /// Creates a new `ResolverRequest` with default protocol version.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION.to_string(),
            url: url.into(),
        }
    }
}

/// A single direct download stream resolved by a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDownload {
    /// Direct HTTP/HTTPS download URL.
    pub url: String,
    /// Optional HTTP headers required for downloading.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional target filename suggested by the resolver.
    #[serde(default)]
    pub filename: Option<String>,
}

impl ResolvedDownload {
    /// Creates a new `ResolvedDownload` with the given URL and empty headers.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: HashMap::new(),
            filename: None,
        }
    }
}

/// Response payload expected on stdout from a resolver plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolverResponse {
    /// List of direct download streams produced by resolving the requested URL.
    pub downloads: Vec<ResolvedDownload>,
}

impl ResolverResponse {
    /// Creates a new `ResolverResponse` containing the given list of downloads.
    pub fn new(downloads: Vec<ResolvedDownload>) -> Self {
        Self { downloads }
    }
}
