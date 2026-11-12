//! Plugin discovery and registry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::PluginError;
use crate::manifest::PluginManifest;

/// Overview information about an installed plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Name of the plugin.
    pub name: String,
    /// Semantic version of the plugin.
    pub version: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Role string of the plugin (e.g. "resolver").
    pub role: String,
    /// URL patterns supported by the plugin.
    pub url_patterns: Vec<String>,
    /// Command line executable and arguments to spawn this plugin.
    pub command: Vec<String>,
    /// Root directory of the installed plugin.
    pub path: PathBuf,
}

/// Registry of installed plugins discovered on disk.
#[derive(Debug, Clone)]
pub struct PluginRegistry {
    plugins_dir: PathBuf,
    plugins: Vec<(PluginManifest, PathBuf)>,
}

impl PluginRegistry {
    /// Creates a new `PluginRegistry` pointing to the root plugins directory.
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins_dir,
            plugins: Vec::new(),
        }
    }

    /// Root directory monitored by this registry.
    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    /// Discovers and validates plugins in the plugins directory.
    ///
    /// Scans immediate subdirectories for `plugin.toml`, parses each manifest,
    /// and replaces the internal list of loaded plugins.
    /// Returns the number of successfully discovered plugins.
    pub fn discover(&mut self) -> Result<usize, PluginError> {
        let mut discovered = Vec::new();

        if !self.plugins_dir.exists() {
            self.plugins = discovered;
            return Ok(0);
        }

        let entries = std::fs::read_dir(&self.plugins_dir)?;
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() && !file_type.is_symlink() {
                continue;
            }

            let plugin_dir = entry.path();
            let manifest_path = plugin_dir.join("plugin.toml");
            if manifest_path.is_file() {
                let content = std::fs::read_to_string(&manifest_path)?;
                let manifest = PluginManifest::from_toml_str(&content)?;
                discovered.push((manifest, plugin_dir));
            }
        }

        // Sort plugins by name for deterministic ordering
        discovered.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        let count = discovered.len();
        self.plugins = discovered;
        Ok(count)
    }

    /// Finds an installed plugin whose URL pattern matches the given URL.
    pub fn find_for_url(&self, url: &str) -> Option<(&PluginManifest, &Path)> {
        for (manifest, dir) in &self.plugins {
            if manifest.matches_url(url) {
                return Some((manifest, dir.as_path()));
            }
        }
        None
    }

    /// Returns a list of overview info for all discovered plugins.
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|(manifest, dir)| PluginInfo {
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                description: manifest.description.clone(),
                role: manifest.role.to_string(),
                url_patterns: manifest.url_patterns.clone(),
                command: manifest.command.clone(),
                path: dir.clone(),
            })
            .collect()
    }
}
