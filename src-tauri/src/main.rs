#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod launcher;
mod pipe_client;

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use serde::Deserialize;
use srun_core::PortalProfile;
use srun_ipc::{
    AccountSummary, DaemonCommand, DaemonSnapshot, GuardPhase, PortalSnapshot, PROTOCOL_VERSION,
};
use tauri::{LogicalSize, Manager};
use tauri_plugin_opener::OpenerExt;

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

const DEFAULT_CHD_HOST: &str = "beiradius.chd.edu.cn";
const DEFAULT_CHD_AC_ID: &str = "5";

#[derive(Debug, Deserialize)]
struct StoredAccountMeta {
    id: String,
    username: String,
    enabled: bool,
    verified: bool,
    #[serde(default)]
    verification_failed: bool,
}

#[derive(Debug, Deserialize)]
struct StoredAppConfig {
    #[serde(default)]
    guard_enabled: bool,
    portal: Option<PortalProfile>,
    #[serde(default)]
    accounts: Vec<StoredAccountMeta>,
    active_account_id: Option<String>,
}

fn is_auto_start_task_registered() -> bool {
    let mut command = std::process::Command::new("schtasks.exe");
    command
        .args(["/Query", "/TN", srun_ipc::DAEMON_TASK_NAME])
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

fn data_root_dir() -> PathBuf {
    match std::env::var_os("VG_CAMPUS_DATA_DIR") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("VGCampusWifiAssistant"),
    }
}

#[tauri::command]
fn get_offline_snapshot() -> srun_ipc::DaemonSnapshot {
    let auto_start_enabled = is_auto_start_task_registered();
    let config_path = data_root_dir().join("config.json");
    let parsed = std::fs::read(&config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredAppConfig>(&bytes).ok());

    if let Some(config) = parsed {
        let active_valid = config.active_account_id.as_deref().is_some_and(|id| {
            config
                .accounts
                .iter()
                .any(|account| account.id == id && account.enabled)
        });
        let active_id = if active_valid {
            config.active_account_id
        } else {
            config
                .accounts
                .iter()
                .find(|account| account.enabled)
                .map(|account| account.id.clone())
        };
        let accounts = config
            .accounts
            .into_iter()
            .map(|account| {
                let active = active_id.as_deref() == Some(account.id.as_str());
                AccountSummary {
                    id: account.id,
                    username: account.username,
                    enabled: account.enabled,
                    verified: account.verified,
                    verification_failed: account.verification_failed,
                    active,
                }
            })
            .collect();
        let portal = config.portal.map(|portal| {
            let host = portal.host().unwrap_or_default();
            PortalSnapshot {
                display_name: host.clone(),
                host,
                ac_id: portal.ac_id,
            }
        });
        return DaemonSnapshot {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            guard_enabled: config.guard_enabled,
            auto_start_enabled,
            phase: if config.guard_enabled {
                GuardPhase::Crashed
            } else {
                GuardPhase::Off
            },
            online: false,
            active_account_id: active_id,
            accounts,
            portal,
            ..DaemonSnapshot::default()
        };
    }

    let default_host = DEFAULT_CHD_HOST.to_string();
    DaemonSnapshot {
        protocol_version: PROTOCOL_VERSION,
        daemon_version: env!("CARGO_PKG_VERSION").into(),
        guard_enabled: false,
        auto_start_enabled,
        phase: GuardPhase::Off,
        online: false,
        active_account_id: None,
        accounts: Vec::new(),
        portal: Some(PortalSnapshot {
            host: default_host.clone(),
            ac_id: DEFAULT_CHD_AC_ID.into(),
            display_name: default_host,
        }),
        ..DaemonSnapshot::default()
    }
}

#[tauri::command]
async fn daemon_request(command: serde_json::Value) -> Result<srun_ipc::CommandResponse, String> {
    let command: srun_ipc::DaemonCommand =
        serde_json::from_value(command).map_err(|error| format!("无效的守护命令：{error}"))?;
    let should_autostart = !matches!(
        command,
        DaemonCommand::Ping | DaemonCommand::GetSnapshot | DaemonCommand::Shutdown
    );
    let first_attempt = pipe_client::request(
        REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        command.clone(),
        Duration::from_secs(45),
    )
    .await;
    match first_attempt {
        Ok(response) => Ok(response),
        Err(error) if should_autostart && error == "校园网守护没有运行" => {
            launcher::ensure_running_or_repair().await?;
            pipe_client::request(
                REQUEST_ID.fetch_add(1, Ordering::Relaxed),
                command,
                Duration::from_secs(45),
            )
            .await
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
async fn get_daemon_snapshot() -> Result<srun_ipc::DaemonSnapshot, String> {
    let response = pipe_client::request(
        REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        srun_ipc::DaemonCommand::GetSnapshot,
        Duration::from_secs(2),
    )
    .await?;
    if !response.ok {
        return Err(response
            .message
            .unwrap_or_else(|| "无法读取守护状态".into()));
    }
    response
        .snapshot
        .ok_or_else(|| "守护没有返回状态快照".into())
}

#[tauri::command]
async fn start_daemon() -> Result<(), String> {
    launcher::ensure_running_or_repair().await
}

#[tauri::command]
async fn install_system_integration() -> Result<(), String> {
    launcher::install_system_integration().await
}

#[tauri::command]
async fn discover_portal() -> Result<serde_json::Value, String> {
    let profile = srun_core::discover_portal()
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_value(profile).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_diagnostics(app: tauri::AppHandle) -> Result<(), String> {
    let root = data_root_dir();
    let logs = root.join("logs");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    app.opener()
        .open_path(logs.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_release_page(app: tauri::AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(
            "https://github.com/v1gn0s/srun-campus-wifi-assistant/releases",
            None::<&str>,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_window_theme(window: tauri::WebviewWindow, theme: String) -> Result<(), String> {
    let theme = match theme.as_str() {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        "system" => None,
        _ => return Err("Invalid theme".into()),
    };
    window.set_theme(theme).map_err(|error| error.to_string())
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Windows 11 会给每个窗口外沿画一圈 1px 描边（DWMWA_BORDER_COLOR）。
/// 本应用是无边框满铺窗口，圆角完全来自系统 DWM 裁剪；这圈描边在圆角
/// 处呈现为方形拐角，而它是合成器的实时效果，屏幕截图（PrtSc 等）不会
/// 捕获它，于是出现"屏幕上有方角、截图里只有圆角"的不一致。
/// 设为 DWMWA_COLOR_NONE 关闭描边，让屏幕显示与截图一致。
#[cfg(target_os = "windows")]
fn disable_window_border(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            DwmSetWindowAttribute(
                hwnd.0 as windows_sys::Win32::Foundation::HWND,
                DWMWA_BORDER_COLOR as u32,
                &DWMWA_COLOR_NONE as *const u32 as *const core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                window.app_handle().exit(0);
            }
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                disable_window_border(&window);
                // 配置里 visible:false，等描边关闭、尺寸校准后再显示，
                // 避免启动瞬间闪现描边或尺寸跳变。
                window.show()?;
                if let Some(monitor) = window.current_monitor()? {
                    let work = monitor.work_area();
                    let scale = monitor.scale_factor();
                    let width = (work.size.width as f64 / scale - 24.0).clamp(900.0, 1180.0);
                    let height = (work.size.height as f64 / scale - 24.0).clamp(620.0, 780.0);
                    window.set_size(LogicalSize::new(width, height))?;
                    window.center()?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_window_theme,
            exit_app,
            open_release_page,
            daemon_request,
            get_daemon_snapshot,
            get_offline_snapshot,
            start_daemon,
            install_system_integration,
            discover_portal,
            open_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("Failed to launch VG campus UI");
}
