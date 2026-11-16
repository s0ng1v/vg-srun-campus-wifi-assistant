#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod launcher;
mod pipe_client;

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use tauri::{LogicalSize, Manager};
use tauri_plugin_opener::OpenerExt;

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[tauri::command]
async fn daemon_request(command: serde_json::Value) -> Result<srun_ipc::CommandResponse, String> {
    let command: srun_ipc::DaemonCommand =
        serde_json::from_value(command).map_err(|error| format!("无效的守护命令：{error}"))?;
    pipe_client::request(
        REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        command,
        Duration::from_secs(45),
    )
    .await
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
    let root = match std::env::var_os("VG_CAMPUS_DATA_DIR") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("VGCampusWifiAssistant"),
    };
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
            start_daemon,
            install_system_integration,
            discover_portal,
            open_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("Failed to launch VG campus UI");
}
