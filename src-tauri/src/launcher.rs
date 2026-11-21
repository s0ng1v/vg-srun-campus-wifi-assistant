use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::pipe_client;
use srun_ipc::{DaemonCommand, DAEMON_ONDEMAND_TASK_NAME};

pub async fn ensure_running() -> Result<(), String> {
    if pipe_client::request(1, DaemonCommand::Ping, Duration::from_millis(900))
        .await
        .is_ok()
    {
        return Ok(());
    }

    if start_daemon_task() && wait_for_pipe(Duration::from_secs(4)).await {
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

async fn wait_for_pipe(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pipe_client::request(1, DaemonCommand::Ping, Duration::from_millis(600))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

fn start_daemon_task() -> bool {
    let mut command = Command::new("schtasks.exe");
    command
        .args(["/Run", "/TN", DAEMON_ONDEMAND_TASK_NAME])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.status().is_ok_and(|status| status.success())
}

pub async fn ensure_running_or_repair() -> Result<(), String> {
    match ensure_running().await {
        Ok(()) => Ok(()),
        Err(first_error) => {
            repair_data_permissions()
                .await
                .map_err(|repair_error| format!("{first_error}；权限修复失败：{repair_error}"))?;
            ensure_running()
                .await
                .map_err(|retry_error| format!("权限已修复，但守护仍未启动：{retry_error}"))
        }
    }
}

pub async fn repair_data_permissions() -> Result<(), String> {
    let setup = sibling_executable("vg-campus-setup.exe")?;
    let ready_file = ready_file_path("repair");
    let mut command = Command::new(&setup);
    command.arg("repair").arg("--ready-file").arg(&ready_file);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map_err(|error| format!("无法启动数据权限修复程序：{error}"))?;
    wait_for_ready(&ready_file, Duration::from_secs(120)).await
}

pub async fn install_system_integration() -> Result<(), String> {
    let setup = sibling_executable("vg-campus-setup.exe")?;
    let daemon = sibling_executable("vg-campus-daemon.exe")?;
    let notifier = sibling_executable("vg-campus-notifier.exe")?;
    let gui = std::env::current_exe().map_err(|error| format!("无法定位当前程序：{error}"))?;
    let ready_file = ready_file_path("install");
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

    wait_for_ready(&ready_file, Duration::from_secs(120)).await?;
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
    Ok(())
}

async fn wait_for_ready(ready_file: &std::path::Path, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready_file.is_file() {
            let result = std::fs::read_to_string(ready_file).unwrap_or_default();
            let trimmed = result.trim();
            if trimmed.is_empty() {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            let _ = std::fs::remove_file(ready_file);
            if !trimmed.starts_with("ok") {
                return Err(trimmed
                    .strip_prefix("error:")
                    .unwrap_or("系统集成安装失败")
                    .to_owned());
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("等待管理员完成操作超时".into())
}

fn ready_file_path(action: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "vg-campus-setup-{action}-ready-{}-{nonce}.txt",
        std::process::id()
    ))
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
