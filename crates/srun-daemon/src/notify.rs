use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use srun_ipc::NOTIFIER_TASK_NAME;

use crate::fsutil::replace_file;
use crate::logging;

#[derive(Debug, Clone, Copy)]
pub enum NotificationEvent {
    NetworkDown,
    NetworkRestored,
    CredentialsRequired,
    BalanceInsufficient,
    DeviceLimit,
}

impl NotificationEvent {
    fn code(self) -> &'static str {
        match self {
            Self::NetworkDown => "network_down",
            Self::NetworkRestored => "network_restored",
            Self::CredentialsRequired => "credentials_required",
            Self::BalanceInsufficient => "balance_insufficient",
            Self::DeviceLimit => "device_limit",
        }
    }
}

pub fn dispatch(data_root: &Path, event: NotificationEvent) {
    let queue = data_root.join("notification-event.txt");
    let temporary = data_root.join("notification-event.tmp");
    let written = std::fs::write(&temporary, event.code())
        .map_err(anyhow::Error::from)
        .and_then(|_| replace_file(&temporary, &queue));
    if let Err(error) = written {
        logging::warn(format!("无法写入通知事件：{error}"));
        return;
    }
    if run_notifier_task() {
        return;
    }
    if let Some(notifier) = notifier_path() {
        let mut command = Command::new(notifier);
        command
            .arg("--event")
            .arg(event.code())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let _ = command.spawn();
    }
}

fn run_notifier_task() -> bool {
    let mut command = Command::new("schtasks.exe");
    command
        .args(["/Run", "/TN", NOTIFIER_TASK_NAME])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.status().is_ok_and(|status| status.success())
}

fn notifier_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("VG_CAMPUS_NOTIFIER_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let executable = std::env::current_exe().ok()?;
    let sibling = executable.parent()?.join("vg-campus-notifier.exe");
    sibling.is_file().then_some(sibling)
}
