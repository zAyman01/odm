//! Daemon configuration and token management.
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub bind_addr: String,
    pub token: String,
}

/// Errors raised while preparing the daemon's folders and token.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot prepare {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the operating system random source failed: {0}")]
    Random(getrandom::Error),
}

/// Returns a new 256-bit token as 64 lower-case hex characters.
fn generate_token() -> Result<String, ConfigError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(ConfigError::Random)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

impl DaemonConfig {
    pub fn load_or_init(
        custom_config: Option<PathBuf>,
        custom_data: Option<PathBuf>,
        bind: Option<String>,
    ) -> Result<Self, ConfigError> {
        let (config_dir, data_dir) = match (custom_config, custom_data) {
            (Some(c), Some(d)) => (c, d),
            (c, d) => {
                let proj = directories::ProjectDirs::from("", "", "odm");
                let def_config = proj
                    .as_ref()
                    .map(|p| p.config_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("./config"));
                let def_data = proj
                    .as_ref()
                    .map(|p| p.data_dir().to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("./data"));
                (c.unwrap_or(def_config), d.unwrap_or(def_data))
            }
        };

        for dir in [&config_dir, &data_dir] {
            fs::create_dir_all(dir).map_err(|source| ConfigError::Io {
                path: dir.clone(),
                source,
            })?;
        }

        let token_path = config_dir.join("token");
        let stored = fs::read_to_string(&token_path)
            .map(|content| content.trim().to_string())
            .unwrap_or_default();
        let token = if stored.is_empty() {
            let generated = generate_token()?;
            fs::write(&token_path, &generated).map_err(|source| ConfigError::Io {
                path: token_path.clone(),
                source,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600)).map_err(
                    |source| ConfigError::Io {
                        path: token_path.clone(),
                        source,
                    },
                )?;
            }
            generated
        } else {
            stored
        };

        Ok(Self {
            config_dir,
            data_dir,
            bind_addr: bind.unwrap_or_else(|| "127.0.0.1:6820".to_string()),
            token,
        })
    }

    pub fn for_test(dir: &Path, token: &str) -> Self {
        Self {
            config_dir: dir.to_path_buf(),
            data_dir: dir.to_path_buf(),
            bind_addr: "127.0.0.1:0".to_string(),
            token: token.to_string(),
        }
    }
}
