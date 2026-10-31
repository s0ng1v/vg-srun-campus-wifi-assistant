use std::{
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
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
        thread::sleep(Duration::from_millis(150));
    }
    Err("校园网守护启动后没有响应".into())
}

fn daemon_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("VG_CAMPUS_DAEMON_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位当前程序：{error}"))?;
    let sibling = executable
        .parent()
        .ok_or_else(|| "无法定位程序目录".to_string())?
        .join("vg-campus-daemon.exe");
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(format!("找不到守护程序 {}", sibling.display()))
    }
}
