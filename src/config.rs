use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};

pub const DEFAULT_API_URL: &str = "https://api-s.anixsekai.com";
pub const BETA_VERSION_CODE: i64 = 26_080_522;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub base_url: String,
    pub token: Option<String>,
    pub beta: bool,
    pub version_code: i64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_API_URL.to_owned(),
            token: env::var("ANIXART_TOKEN").ok().filter(|value| !value.trim().is_empty()),
            beta: true,
            version_code: BETA_VERSION_CODE,
        }
    }
}

impl AppConfig {
    pub fn load() -> io::Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        if let Ok(token) = env::var("ANIXART_TOKEN") {
            if !token.trim().is_empty() {
                config.token = Some(token);
            }
        }

        Ok(config)
    }

    pub fn save(&self) -> io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let body = serde_json::to_vec_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::io::Write;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(&body)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        #[cfg(not(unix))]
        fs::write(&path, body)?;

        Ok(())
    }

    pub fn has_token(&self) -> bool {
        self.token.as_deref().is_some_and(|token| !token.trim().is_empty())
    }
}

pub fn config_path() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("anixart/config.json");
    }

    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".config/anixart-arch/config.json");
    }

    PathBuf::from("anixart-config.json")
}
