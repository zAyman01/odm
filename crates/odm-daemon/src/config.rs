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

impl DaemonConfig {
    pub fn load_or_init(
        custom_config: Option<PathBuf>,
        custom_data: Option<PathBuf>,
        bind: Option<String>,
    ) -> Self {
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

        let _ = fs::create_dir_all(&config_dir);
        let _ = fs::create_dir_all(&data_dir);

        let token_path = config_dir.join("token");
        let token = if token_path.exists() {
            fs::read_to_string(&token_path)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            let generated: String = (0..32)
                .map(|_| format!("{:02x}", fastrand::u8(..)))
                .collect();
            let _ = fs::write(&token_path, &generated);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600));
            }
            generated
        };

        Self {
            config_dir,
            data_dir,
            bind_addr: bind.unwrap_or_else(|| "127.0.0.1:6820".to_string()),
            token,
        }
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
