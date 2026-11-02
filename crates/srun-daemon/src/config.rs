use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use srun_core::PortalProfile;

use crate::fsutil::replace_file;

const CONFIG_SCHEMA_VERSION: u32 = 3;
const CHD_PORTAL_URL: &str = "https://beiradius.chd.edu.cn";
const CHD_AC_ID: &str = "5";

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
            guard_enabled: false,
            auto_start_enabled: false,
            portal: Some(PortalProfile {
                base_url: CHD_PORTAL_URL.into(),
                ac_id: CHD_AC_ID.into(),
                domain: String::new(),
            }),
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
    if config.schema_version > CONFIG_SCHEMA_VERSION {
        anyhow::bail!(
            "配置文件版本 {} 高于程序支持的版本 {}",
            config.schema_version,
            CONFIG_SCHEMA_VERSION
        );
    }
    let mut migrated = config.schema_version < CONFIG_SCHEMA_VERSION;
    if config.active_account_id.is_none() {
        config.active_account_id = config
            .accounts
            .iter()
            .find(|account| account.enabled)
            .map(|account| account.id.clone());
    }
    if let Some(portal) = &mut config.portal {
        if portal.base_url.trim_end_matches('/') == CHD_PORTAL_URL
            && matches!(portal.ac_id.as_str(), "" | "1")
        {
            portal.ac_id = CHD_AC_ID.into();
            migrated = true;
        }
        if !portal.domain.is_empty() && !srun_core::valid_portal_domain(&portal.domain) {
            portal.domain.clear();
            migrated = true;
        }
    }
    if migrated {
        for account in &mut config.accounts {
            account.verified = false;
        }
        config.schema_version = CONFIG_SCHEMA_VERSION;
        let _ = save_config(paths, &config);
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
    replace_file(&temporary, path)
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
        let mut loaded = load_config(&paths).unwrap();
        assert_eq!(loaded.accounts[0].id, "first");
        assert_eq!(loaded.active_account_id.as_deref(), Some("first"));
        loaded.guard_enabled = true;
        save_config(&paths, &loaded).unwrap();
        assert!(load_config(&paths).unwrap().guard_enabled);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn chd_default_and_legacy_state_are_migrated() {
        let default_portal = AppConfig::default().portal.unwrap();
        assert_eq!(default_portal.base_url, CHD_PORTAL_URL);
        assert_eq!(default_portal.ac_id, "5");

        let directory = std::env::temp_dir().join(format!(
            "vg-campus-config-migration-test-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = AppPaths::from_root(&directory).unwrap();
        let legacy = serde_json::json!({
            "schema_version": 1,
            "guard_enabled": false,
            "auto_start_enabled": false,
            "portal": {
                "base_url": CHD_PORTAL_URL,
                "ac_id": "1",
                "domain": ""
            },
            "accounts": [{
                "id": "legacy",
                "username": "fixture",
                "enabled": true,
                "verified": true
            }],
            "active_account_id": "legacy"
        });
        fs::write(&paths.config, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let migrated = load_config(&paths).unwrap();
        assert_eq!(migrated.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(migrated.portal.unwrap().ac_id, "5");
        assert!(!migrated.accounts[0].verified);
        fs::remove_dir_all(directory).unwrap();
    }
}
