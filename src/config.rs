use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub zotero_db_path: Option<PathBuf>,
    pub zotero_storage_path: Option<PathBuf>,
    pub mirror_root: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    #[serde(default)]
    pub web_api: WebApiConfig,
    #[serde(default)]
    pub helper: HelperConfig,
    #[serde(default)]
    pub lfz: LfzConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebApiConfig {
    pub enabled: bool,
    pub base_url: String,
    pub library_type: String,
    pub library_id: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
}

impl Default for WebApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://api.zotero.org".to_string(),
            library_type: "user".to_string(),
            library_id: None,
            api_key_env: Some("ZOTERO_API_KEY".to_string()),
            api_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelperConfig {
    pub enabled: Option<bool>,
    pub endpoint: String,
    pub token_path: Option<PathBuf>,
}

impl Default for HelperConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            endpoint: "http://127.0.0.1:23119/zcli-helper".to_string(),
            token_path: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LfzConfig {
    pub enabled: Option<bool>,
    pub zotero_data_dir: Option<PathBuf>,
    pub claude_runtime_dir: Option<PathBuf>,
    pub adapter_trace_path: Option<PathBuf>,
}

impl Config {
    pub fn default_path() -> PathBuf {
        env::var_os("ZCLI_CONFIG")
            .map(PathBuf::from)
            .or_else(|| dirs::config_dir().map(|dir| dir.join("zotero-cli").join("config.toml")))
            .unwrap_or_else(|| PathBuf::from("zotero-cli.toml"))
    }

    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = path.map(PathBuf::from).unwrap_or_else(Self::default_path);
        if !path.exists() {
            return Ok(Self::autodetected());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config: Self = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config {}", path.display()))?;
        config.apply_autodetect_defaults();
        Ok(config)
    }

    pub fn write_default(path: &Path, force: bool) -> Result<()> {
        if path.exists() && !force {
            anyhow::bail!("config already exists: {}", path.display());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let config = Self::autodetected();
        fs::write(path, toml::to_string_pretty(&config)?)?;
        Ok(())
    }

    pub fn save(&self, path: &Path, force_parent: bool) -> Result<()> {
        if force_parent {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn autodetected() -> Self {
        let mut config = Self::default();
        config.apply_autodetect_defaults();
        config
    }

    pub fn apply_overrides(
        &mut self,
        db: Option<PathBuf>,
        storage: Option<PathBuf>,
        mirror_root: Option<PathBuf>,
    ) {
        if db.is_some() {
            self.zotero_db_path = db;
        }
        if storage.is_some() {
            self.zotero_storage_path = storage;
        }
        if mirror_root.is_some() {
            self.mirror_root = mirror_root;
        }
    }

    fn apply_autodetect_defaults(&mut self) {
        if self.zotero_db_path.is_none() {
            self.zotero_db_path = detect_zotero_db();
        }
        if self.zotero_storage_path.is_none() {
            self.zotero_storage_path = self
                .zotero_db_path
                .as_ref()
                .and_then(|path| path.parent().map(|parent| parent.join("storage")))
                .filter(|path| path.exists());
        }
        if self.cache_dir.is_none() {
            self.cache_dir = dirs::cache_dir().map(|dir| dir.join("zotero-cli"));
        }
        if self.state_dir.is_none() {
            self.state_dir = env::var_os("ZCLI_STATE_DIR")
                .map(PathBuf::from)
                .or_else(|| dirs::data_local_dir().map(|dir| dir.join("zotero-cli")))
                .or_else(|| dirs::cache_dir().map(|dir| dir.join("zotero-cli").join("state")));
        }
        if self.web_api.base_url.trim().is_empty() {
            self.web_api.base_url = "https://api.zotero.org".to_string();
        }
        if self.web_api.library_type.trim().is_empty() {
            self.web_api.library_type = "user".to_string();
        }
        if self.web_api.api_key_env.is_none() {
            self.web_api.api_key_env = Some("ZOTERO_API_KEY".to_string());
        }
        if self.helper.endpoint.trim().is_empty() {
            self.helper.endpoint = "http://127.0.0.1:23119/zcli-helper".to_string();
        }
        if self.helper.token_path.is_none() {
            self.helper.token_path = self
                .zotero_db_path
                .as_ref()
                .and_then(|p| p.parent().map(|parent| parent.join("zcli-helper-token")));
        }
        if self.lfz.zotero_data_dir.is_none() {
            self.lfz.zotero_data_dir = self
                .zotero_db_path
                .as_ref()
                .and_then(|p| p.parent().map(Path::to_path_buf));
        }
        if self.lfz.claude_runtime_dir.is_none() {
            self.lfz.claude_runtime_dir = dirs::home_dir()
                .map(|home| home.join("Zotero").join("agent-runtime"))
                .filter(|path| path.exists());
        }
    }
}

fn detect_zotero_db() -> Option<PathBuf> {
    if let Some(raw) = env::var_os("ZOTERO_DB_PATH") {
        let path = PathBuf::from(raw);
        if path.exists() {
            return Some(path);
        }
    }
    let home = dirs::home_dir()?;
    let candidates = [
        home.join("Zotero").join("zotero.sqlite"),
        home.join("Library")
            .join("Application Support")
            .join("Zotero")
            .join("Profiles")
            .join("default")
            .join("zotero")
            .join("zotero.sqlite"),
    ];
    candidates.into_iter().find(|path| path.exists())
}
