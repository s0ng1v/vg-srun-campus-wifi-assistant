use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::pipe_client;
use srun_ipc::DaemonCommand;

pub async fn ensure_running() -> Result<(), String> {
    if pipe_client::request(1, DaemonCommand::Ping, Duration::from_millis(900))
        .await
        .is_ok()
    {
        return Ok(());
    }

    let daemon = daemon_path()?;
    let mut command = Command::new(&daemon);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map_err(|error| format!("无法启动校园网守护 {}：{error}", daemon.display()))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if pipe_client::request(1, DaemonCommand::Ping, Duration::from_millis(600))
            .await
            .is_ok()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err("校园网守护启动后没有响应".into())
}

pub async fn install_system_integration() -> Result<(), String> {
    let setup = sibling_executable("vg-campus-setup.exe")?;
    let daemon = sibling_executable("vg-campus-daemon.exe")?;
    let notifier = sibling_executable("vg-campus-notifier.exe")?;
    let gui = std::env::current_exe().map_err(|error| format!("无法定位当前程序：{error}"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let ready_file = std::env::temp_dir().join(format!(
        "vg-campus-setup-ready-{}-{nonce}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&ready_file);

    let mut command = Command::new(&setup);
    command
        .arg("install")
        .arg("--daemon")
        .arg(&daemon)
        .arg("--notifier")
        .arg(&notifier)
        .arg("--gui")
        .arg(&gui)
        .arg("--ready-file")
        .arg(&ready_file);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map_err(|error| format!("无法启动系统集成安装器：{error}"))?;

    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if ready_file.is_file() {
            let result = std::fs::read_to_string(&ready_file).unwrap_or_default();
            let _ = std::fs::remove_file(&ready_file);
            if !result.trim().starts_with("ok") {
                return Err(result
                    .trim()
                    .strip_prefix("error:")
                    .unwrap_or("系统集成安装失败")
                    .to_owned());
            }
            ensure_running().await?;
            let response = pipe_client::request(
                1,
                DaemonCommand::SetAutoStart { enabled: true },
                Duration::from_secs(10),
            )
            .await?;
            if !response.ok {
                return Err(response
                    .message
                    .unwrap_or_else(|| "无法启用开机守护".into()));
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("等待管理员完成系统集成安装超时".into())
}

fn daemon_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("VG_CAMPUS_DAEMON_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    sibling_executable("vg-campus-daemon.exe")
}

fn sibling_executable(name: &str) -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位当前程序：{error}"))?;
    let sibling = executable
        .parent()
        .ok_or_else(|| "无法定位程序目录".to_string())?
        .join(name);
    sibling
        .is_file()
        .then_some(sibling.clone())
        .ok_or_else(|| format!("找不到程序 {}", sibling.display()))
}
