use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};

const LOG_RETENTION_DAYS: u64 = 7;
const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init(log_directory: &Path) -> Result<()> {
    fs::create_dir_all(log_directory)
        .with_context(|| format!("无法创建日志目录 {}", log_directory.display()))?;
    prune_old_logs(log_directory);
    let today = chrono::Local::now().format("%Y-%m-%d");
    let path = log_directory.join(format!("daemon-{today}.log"));
    if path.metadata().map(|metadata| metadata.len()).unwrap_or(0) >= LOG_MAX_BYTES {
        let rotated = log_directory.join(format!("daemon-{today}-1.log"));
        let _ = fs::rename(&path, rotated);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("无法打开日志文件 {}", path.display()))?;
    LOG_FILE
        .set(Mutex::new(file))
        .map_err(|_| anyhow::anyhow!("日志系统已初始化"))?;
    Ok(())
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write("WARN", message.as_ref());
}

pub fn debug(message: impl AsRef<str>) {
    write("DEBUG", message.as_ref());
}

fn write(level: &str, message: &str) {
    let Some(file) = LOG_FILE.get() else {
        return;
    };
    let Ok(mut file) = file.lock() else {
        return;
    };
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let _ = writeln!(file, "{timestamp} [{level}] {message}");
    let _ = file.flush();
}

fn prune_old_logs(directory: &Path) {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_daemon_log = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("daemon.log")
                    || (name.starts_with("daemon-") && name.ends_with(".log"))
            });
        if !is_daemon_log {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < cutoff {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_dated_daemon_log_filenames() {
        let directory =
            std::env::temp_dir().join(format!("vg-campus-log-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let recent = directory.join("daemon-2026-11-21.log");
        fs::write(&recent, "ok").unwrap();
        prune_old_logs(&directory);
        assert!(recent.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
