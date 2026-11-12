use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::PluginError;

/// The role of a plugin within odm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginRole {
    /// URL resolver plugin that resolves high-level URLs into direct media/download streams.
    Resolver,
}

impl PluginRole {
    /// Returns the string representation of the plugin role.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolver => "resolver",
        }
    }
}

impl std::fmt::Display for PluginRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for PluginRole {
    type Err = PluginError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "resolver" => Ok(Self::Resolver),
            other => Err(PluginError::UnsupportedRole(other.to_string())),
        }
    }
}

/// Parsed and validated manifest of an odm plugin (`plugin.toml`).
#[derive(Debug, Clone, Serialize)]
pub struct PluginManifest {
    /// Name of the plugin.
    pub name: String,
    /// Semantic version of the plugin.
    pub version: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Role of the plugin (currently only `resolver`).
    pub role: PluginRole,
    /// URL patterns (wildcards or regex) that this plugin handles.
    pub url_patterns: Vec<String>,
    /// Command line executable and arguments to spawn this plugin.
    pub command: Vec<String>,
    /// Precompiled regexes corresponding to each item in `url_patterns`.
    #[serde(skip)]
    compiled_patterns: Vec<Regex>,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    plugin: RawManifest,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    name: String,
    version: String,
    description: Option<String>,
    role: String,
    #[serde(default)]
    url_patterns: Vec<String>,
    #[serde(deserialize_with = "deserialize_command")]
    command: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CommandConfig {
    Single(String),
    Multiple(Vec<String>),
}

fn deserialize_command<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match CommandConfig::deserialize(deserializer)? {
        CommandConfig::Single(s) => {
            if s.trim().is_empty() {
                Err(serde::de::Error::custom("command cannot be empty"))
            } else {
                Ok(vec![s])
            }
        }
        CommandConfig::Multiple(v) => {
            if v.is_empty() {
                Err(serde::de::Error::custom("command array cannot be empty"))
            } else if v.iter().any(|arg| arg.trim().is_empty()) {
                Err(serde::de::Error::custom("command elements cannot be empty"))
            } else {
                Ok(v)
            }
        }
    }
}

fn is_regex_meta(c: char) -> bool {
    matches!(
        c,
        '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
    )
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::with_capacity(pattern.len() * 2 + 2);
    regex.push('^');
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '\\' => {
                if let Some(&next) = chars.peek() {
                    chars.next();
                    match next {
                        '*' => regex.push_str(r"\*"),
                        '?' => regex.push_str(r"\?"),
                        other => {
                            regex.push('\\');
                            regex.push(other);
                        }
                    }
                } else {
                    regex.push_str(r"\\");
                }
            }
            c if is_regex_meta(c) => {
                regex.push('\\');
                regex.push(c);
            }
            c => regex.push(c),
        }
    }
    regex.push('$');
    regex
}

fn compile_pattern(pattern: &str) -> Result<Regex, PluginError> {
    let re_str = if pattern.starts_with('^') || pattern.starts_with("(?") {
        pattern.to_string()
    } else {
        glob_to_regex(pattern)
    };

    Regex::new(&re_str).map_err(|e| {
        PluginError::InvalidManifest(format!("failed to compile URL pattern '{pattern}': {e}"))
    })
}

impl PluginManifest {
    /// Create a new `PluginManifest` with validated fields and compiled URL patterns.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        description: Option<String>,
        role: PluginRole,
        url_patterns: Vec<String>,
        command: Vec<String>,
    ) -> Result<Self, PluginError> {
        let name = name.into();
        let version = version.into();

        if name.trim().is_empty() {
            return Err(PluginError::InvalidManifest(
                "plugin name cannot be empty".to_string(),
            ));
        }
        if version.trim().is_empty() {
            return Err(PluginError::InvalidManifest(
                "plugin version cannot be empty".to_string(),
            ));
        }
        if command.is_empty() || command.iter().any(|c| c.trim().is_empty()) {
            return Err(PluginError::InvalidManifest(
                "plugin command cannot be empty".to_string(),
            ));
        }

        let mut compiled_patterns = Vec::with_capacity(url_patterns.len());
        for pattern in &url_patterns {
            compiled_patterns.push(compile_pattern(pattern)?);
        }

        Ok(Self {
            name,
            version,
            description,
            role,
            url_patterns,
            command,
            compiled_patterns,
        })
    }

    /// Parse a `plugin.toml` string and validate its fields and patterns.
    pub fn from_toml_str(content: &str) -> Result<Self, PluginError> {
        let file: ManifestFile =
            toml::from_str(content).map_err(|e| PluginError::InvalidManifest(e.to_string()))?;

        let role: PluginRole = file.plugin.role.parse()?;

        Self::new(
            file.plugin.name,
            file.plugin.version,
            file.plugin.description,
            role,
            file.plugin.url_patterns,
            file.plugin.command,
        )
    }

    /// Check if the given URL matches any of the plugin's configured URL patterns.
    pub fn matches_url(&self, url: &str) -> bool {
        self.compiled_patterns.iter().any(|re| re.is_match(url))
    }

    /// Returns the compiled regular expressions for the URL patterns.
    pub fn compiled_patterns(&self) -> &[Regex] {
        &self.compiled_patterns
    }
}
