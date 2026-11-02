use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result};

pub fn resolve(data_root: &Path) -> Result<String> {
    if let Some(sid) = registry_owner_sid().filter(|sid| valid_sid(sid)) {
        return Ok(sid);
    }
    let owner_file = data_root.join("owner.sid");
    let sid = if owner_file.is_file() {
        fs::read_to_string(&owner_file)
            .with_context(|| format!("无法读取 {}", owner_file.display()))?
            .trim()
            .to_owned()
    } else {
        current_user_sid().context("无法确定命名管道所有者")?
    };
    if !valid_sid(&sid) {
        anyhow::bail!("命名管道所有者 SID 无效");
    }
    Ok(sid)
}

fn registry_owner_sid() -> Option<String> {
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};

    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"Software\VGCampusWifiAssistant")
        .and_then(|key| key.get_value::<String, _>("OwnerSid"))
        .ok()
        .map(|sid| sid.trim().to_owned())
}

fn current_user_sid() -> Option<String> {
    let output = Command::new("whoami.exe")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split(',').find_map(|field| {
        let value = field.trim().trim_matches('"');
        value.starts_with("S-").then(|| value.to_owned())
    })
}

fn valid_sid(sid: &str) -> bool {
    sid.starts_with("S-")
        && sid
            .chars()
            .all(|character| character.is_ascii_digit() || character == 'S' || character == '-')
}
