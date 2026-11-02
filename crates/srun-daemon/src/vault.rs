use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::fsutil::replace_file;
use crate::secret::{protect_local_machine, unprotect_local_machine};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialRecord {
    pub id: String,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredVault {
    Records(Vec<CredentialRecord>),
    LegacyContainer { records: Vec<CredentialRecord> },
}

#[derive(Serialize)]
pub struct CredentialVault {
    records: Vec<CredentialRecord>,
}

impl CredentialVault {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                records: Vec::new(),
            });
        }
        let encrypted =
            fs::read(path).with_context(|| format!("无法读取凭据文件 {}", path.display()))?;
        let plaintext = unprotect_local_machine(&encrypted)?;
        let records = match serde_json::from_slice(&plaintext).context("凭据内容格式无效")?
        {
            StoredVault::Records(records) => records,
            StoredVault::LegacyContainer { records } => records,
        };
        Ok(Self { records })
    }

    pub fn get(&self, id: &str) -> Option<&CredentialRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    pub fn upsert(&mut self, record: CredentialRecord) -> Result<()> {
        if record.username.trim().is_empty() || record.password.is_empty() {
            anyhow::bail!("账号和密码不能为空");
        }
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.id == record.id)
        {
            *existing = record;
        } else {
            self.records.push(record);
        }
        Ok(())
    }

    pub fn remove(&mut self, id: &str) {
        self.records.retain(|record| record.id != id);
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let plaintext = serde_json::to_vec(&self.records)?;
        let encrypted = protect_local_machine(&plaintext)?;
        let temporary = path.with_extension("dat.tmp");
        fs::write(&temporary, encrypted)
            .with_context(|| format!("无法写入临时凭据文件 {}", temporary.display()))?;
        replace_file(&temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_upsert_and_remove_are_deterministic() {
        let mut vault = CredentialVault {
            records: Vec::new(),
        };
        vault
            .upsert(CredentialRecord {
                id: "a".into(),
                username: "001".into(),
                password: "fixture".into(),
            })
            .unwrap();
        vault
            .upsert(CredentialRecord {
                id: "a".into(),
                username: "002".into(),
                password: "fixture-new".into(),
            })
            .unwrap();
        assert_eq!(vault.records.len(), 1);
        assert_eq!(vault.get("a").unwrap().username, "002");
        vault.remove("a");
        assert!(vault.get("a").is_none());
    }

    #[test]
    fn encrypted_vault_can_be_replaced_and_reloaded() {
        let directory =
            std::env::temp_dir().join(format!("vg-campus-vault-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("credentials.dat");
        let mut vault = CredentialVault {
            records: Vec::new(),
        };
        vault
            .upsert(CredentialRecord {
                id: "a".into(),
                username: "001".into(),
                password: "fixture".into(),
            })
            .unwrap();
        vault.save(&path).unwrap();
        vault
            .upsert(CredentialRecord {
                id: "b".into(),
                username: "002".into(),
                password: "fixture-2".into(),
            })
            .unwrap();
        vault.save(&path).unwrap();
        let loaded = CredentialVault::load(&path).unwrap();
        assert_eq!(loaded.records.len(), 2);
        assert_eq!(loaded.get("b").unwrap().username, "002");
        fs::remove_dir_all(directory).unwrap();
    }
}
