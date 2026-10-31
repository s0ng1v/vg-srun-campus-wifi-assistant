use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use srun_core::PortalProfile;

const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMeta {
    pub id: String,
    pub username: String,
    pub enabled: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub schema_version: u32,
    pub owner_sid: Option<String>,
    pub guard_enabled: bool,
    pub auto_start_enabled: bool,
    pub portal: Option<PortalProfile>,
    pub accounts: Vec<AccountMeta>,
    pub active_account_id: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            owner_sid: None,
            guard_enabled: false,
            auto_start_enabled: false,
            portal: None,
            accounts: Vec::new(),
            active_account_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub credentials: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let root = match env::var_os("VG_CAMPUS_DATA_DIR") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => {
                let program_data = env::var_os("PROGRAMDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
                program_data.join("VGCampusWifiAssistant")
            }
        };
        Self::from_root(root)
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .with_context(|| format!("无法创建数据目录 {}", root.display()))?;
        let logs = root.join("logs");
        fs::create_dir_all(&logs)
            .with_context(|| format!("无法创建日志目录 {}", logs.display()))?;
        Ok(Self {
            config: root.join("config.json"),
            credentials: root.join("credentials.dat"),
            logs,
            root,
        })
    }
}

pub fn load_config(paths: &AppPaths) -> Result<AppConfig> {
    if !paths.config.exists() {
        return Ok(AppConfig::default());
    }
    let bytes = fs::read(&paths.config)
        .with_context(|| format!("无法读取配置 {}", paths.config.display()))?;
    let mut config: AppConfig = serde_json::from_slice(&bytes)
        .with_context(|| format!("配置文件格式无效 {}", paths.config.display()))?;
    if config.schema_version == 0 {
        config.schema_version = CONFIG_SCHEMA_VERSION;
    }
    if config.schema_version > CONFIG_SCHEMA_VERSION {
        anyhow::bail!(
            "配置文件版本 {} 高于程序支持的版本 {}",
            config.schema_version,
            CONFIG_SCHEMA_VERSION
        );
    }
    if config.active_account_id.is_none() {
        config.active_account_id = config
            .accounts
            .iter()
            .find(|account| account.enabled)
            .map(|account| account.id.clone());
    }
    Ok(config)
}

pub fn save_config(paths: &AppPaths, config: &AppConfig) -> Result<()> {
    let data = serde_json::to_vec_pretty(config)?;
    atomic_write(&paths.config, &data)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("data")
    ));
    fs::write(&temporary, data)
        .with_context(|| format!("无法写入临时文件 {}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("无法替换旧文件 {}", path.display()))?;
    }
    fs::rename(&temporary, path).with_context(|| format!("无法提交配置文件 {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip_keeps_account_order() {
        let directory =
            std::env::temp_dir().join(format!("vg-campus-config-test-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&directory).unwrap();
        let config = AppConfig {
            accounts: vec![
                AccountMeta {
                    id: "first".into(),
                    username: "001".into(),
                    enabled: true,
                    verified: true,
                },
                AccountMeta {
                    id: "second".into(),
                    username: "002".into(),
                    enabled: true,
                    verified: false,
                },
            ],
            ..AppConfig::default()
        };
        save_config(&paths, &config).unwrap();
        let loaded = load_config(&paths).unwrap();
        assert_eq!(loaded.accounts[0].id, "first");
        assert_eq!(loaded.active_account_id.as_deref(), Some("first"));
        fs::remove_dir_all(directory).unwrap();
    }
}
