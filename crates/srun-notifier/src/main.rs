#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_toast {
    use std::{env, thread, time::Duration};

    use windows::{
        core::HSTRING,
        Data::Xml::Dom::XmlDocument,
        Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        UI::Notifications::{ToastNotification, ToastNotificationManager},
    };
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    const AUMID: &str = "io.github.v1gn0s.vg-campus-wifi-assistant";

    pub fn show(event: &str) -> windows::core::Result<()> {
        register_aumid();
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        let result = show_toast(event);
        unsafe { RoUninitialize() };
        result
    }

    fn show_toast(event: &str) -> windows::core::Result<()> {
        let (title, body) = content(event);
        let xml = format!(
            r#"<toast activationType="protocol" launch="vg-campus-wifi-assistant://status">
              <visual>
                <binding template="ToastGeneric">
                  <text>{title}</text>
                  <text>{body}</text>
                </binding>
              </visual>
              <actions>
                <action content="打开 VG 校园网助手" activationType="protocol" arguments="vg-campus-wifi-assistant://status" />
              </actions>
            </toast>"#
        );
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(xml))?;
        let notification = ToastNotification::CreateToastNotification(&document)?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
        notifier.Show(&notification)?;
        thread::sleep(Duration::from_millis(350));
        Ok(())
    }

    fn register_aumid() {
        let Ok((hkcu, _)) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(format!(r"Software\Classes\AppUserModelId\{AUMID}"))
        else {
            return;
        };
        let _ = hkcu.set_value("DisplayName", &HSTRING::from("VG 校园网助手").to_string());
        if let Ok(executable) = env::current_exe() {
            let _ = hkcu.set_value("IconUri", &executable.to_string_lossy().to_string());
        }
    }

    fn content(event: &str) -> (&'static str, &'static str) {
        match event {
            "network_down" => (
                "校园网连接异常",
                "连续 3 分钟未能恢复，网络守护仍在自动重试。",
            ),
            "network_restored" => ("校园网已恢复", "网络守护已重新连接校园网。"),
            "credentials_required" => (
                "需要更新校园网账号",
                "账号或密码认证失败，请打开 VG 校园网助手处理。",
            ),
            "balance_insufficient" => (
                "校园网余额不足",
                "当前账号无法继续认证，请充值后等待自动恢复。",
            ),
            "device_limit" => (
                "校园网设备数已满",
                "当前账号在线设备过多，请释放设备后重试。",
            ),
            "guard_crashed" => (
                "网络守护已停止",
                "后台守护异常退出，请打开 VG 校园网助手重新启动。",
            ),
            _ => ("VG 校园网助手", "网络守护需要你的关注。"),
        }
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    {
        let arguments: Vec<String> = std::env::args().collect();
        let event = arguments
            .iter()
            .position(|argument| argument == "--event")
            .and_then(|index| arguments.get(index + 1))
            .cloned()
            .or_else(|| {
                let root = std::env::var_os("VG_CAMPUS_DATA_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::path::PathBuf::from(
                            std::env::var_os("PROGRAMDATA")
                                .unwrap_or_else(|| r"C:\ProgramData".into()),
                        )
                        .join("VGCampusWifiAssistant")
                    });
                std::fs::read_to_string(root.join("notification-event.txt"))
                    .ok()
                    .map(|value| value.trim().to_owned())
            })
            .unwrap_or_default();
        match windows_toast::show(&event) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                let path = std::env::temp_dir().join("vg-campus-notifier-error.log");
                let _ = std::fs::write(path, format!("{error:?}"));
                std::process::ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(windows))]
    {
        std::process::ExitCode::FAILURE
    }
}
