#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
};

use srun_ipc::{DAEMON_TASK_NAME, NOTIFIER_TASK_NAME};

const APP_ID: &str = "io.github.v1gn0s.vg-campus-wifi-assistant";
const PROTOCOL: &str = "vg-campus-wifi-assistant";

#[derive(Debug, Default)]
struct Options {
    daemon: Option<PathBuf>,
    notifier: Option<PathBuf>,
    gui: Option<PathBuf>,
    owner_sid: Option<String>,
    ready_file: Option<PathBuf>,
    elevated: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log(format!("失败：{error:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    if command == "help" {
        return Ok(());
    }
    let options = parse_options(&arguments[1..])?;

    let result = if needs_admin(command) && !is_admin() && !options.elevated {
        elevate(command, &options)
    } else {
        match command {
            "install" => install(&options),
            "enable" => enable_daemon(),
            "disable" => disable_daemon(),
            "uninstall" => uninstall(),
            _ => anyhow::bail!("未知命令：{command}"),
        }
    };
    if let (Err(error), Some(path)) = (&result, &options.ready_file) {
        let _ = fs::write(path, format!("error:{error:#}"));
    }
    result
}

fn parse_options(arguments: &[String]) -> anyhow::Result<Options> {
    let mut options = Options::default();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        match argument.as_str() {
            "--daemon" => {
                index += 1;
                options.daemon = arguments.get(index).map(PathBuf::from);
            }
            "--notifier" => {
                index += 1;
                options.notifier = arguments.get(index).map(PathBuf::from);
            }
            "--gui" => {
                index += 1;
                options.gui = arguments.get(index).map(PathBuf::from);
            }
            "--owner-sid" => {
                index += 1;
                options.owner_sid = arguments.get(index).cloned();
            }
            "--ready-file" => {
                index += 1;
                options.ready_file = arguments.get(index).map(PathBuf::from);
            }
            "--elevated" => options.elevated = true,
            _ => {}
        }
        index += 1;
    }
    Ok(options)
}

fn install(options: &Options) -> anyhow::Result<()> {
    let daemon = validate_executable(options.daemon.as_deref(), "守护程序")?;
    let notifier = validate_executable(options.notifier.as_deref(), "通知程序")?;
    let gui = validate_executable(options.gui.as_deref(), "图形界面")?;
    let installed = install_files(&daemon, &notifier, &gui)?;
    let owner_sid = options
        .owner_sid
        .clone()
        .or_else(current_user_sid)
        .filter(|sid| valid_sid(sid))
        .ok_or_else(|| anyhow::anyhow!("无法确定当前用户 SID"))?;

    let data_root = data_root();
    fs::create_dir_all(&data_root)?;
    fs::write(data_root.join("owner.sid"), &owner_sid)?;
    restrict_data_directory(&data_root, &owner_sid);

    let daemon_xml = daemon_task_xml(&installed.daemon)?;
    register_task(DAEMON_TASK_NAME, &daemon_xml)?;
    let notifier_xml = notifier_task_xml(&installed.notifier, &owner_sid)?;
    register_task(NOTIFIER_TASK_NAME, &notifier_xml)?;
    register_protocol(&installed.gui)?;
    let _ = run_hidden(Command::new("schtasks.exe").args(["/Run", "/TN", DAEMON_TASK_NAME]));
    log("系统守护任务与通知任务已安装");
    if let Some(path) = &options.ready_file {
        let _ = fs::write(path, "ok");
    }
    Ok(())
}

struct InstalledBinaries {
    daemon: PathBuf,
    notifier: PathBuf,
    gui: PathBuf,
}

fn install_files(daemon: &Path, notifier: &Path, gui: &Path) -> anyhow::Result<InstalledBinaries> {
    let program_files = env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let app_directory = program_files
        .join("VGCampusWifiAssistant")
        .join(format!("app-{}", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&app_directory)?;

    let daemon_target = app_directory.join("vg-campus-daemon.exe");
    let notifier_target = app_directory.join("vg-campus-notifier.exe");
    let gui_target = app_directory.join("vg-campus-assistant.exe");
    copy_if_needed(daemon, &daemon_target)?;
    copy_if_needed(notifier, &notifier_target)?;
    copy_if_needed(gui, &gui_target)?;
    let setup_target = app_directory.join("vg-campus-setup.exe");
    if let Ok(current) = env::current_exe() {
        copy_if_needed(&current, &setup_target)?;
    }
    restrict_install_directory(&app_directory);
    Ok(InstalledBinaries {
        daemon: daemon_target,
        notifier: notifier_target,
        gui: gui_target,
    })
}

fn restrict_install_directory(app_directory: &Path) {
    let path = app_directory.to_string_lossy().to_string();
    let _ = run_hidden(Command::new("icacls.exe").args([
        &path,
        "/inheritance:r",
        "/grant:r",
        "*S-1-5-18:(OI)(CI)F",
        "*S-1-5-32-544:(OI)(CI)F",
        "*S-1-5-32-545:(OI)(CI)RX",
        "/T",
    ]));
}

fn copy_if_needed(source: &Path, target: &Path) -> anyhow::Result<()> {
    if source
        .canonicalize()
        .ok()
        .zip(target.canonicalize().ok())
        .is_some_and(|(source, target)| source == target)
    {
        return Ok(());
    }
    fs::copy(source, target)?;
    Ok(())
}

fn enable_daemon() -> anyhow::Result<()> {
    run_schtasks(["/Change", "/TN", DAEMON_TASK_NAME, "/ENABLE"])?;
    run_schtasks(["/Run", "/TN", DAEMON_TASK_NAME])?;
    Ok(())
}

fn disable_daemon() -> anyhow::Result<()> {
    run_schtasks(["/Change", "/TN", DAEMON_TASK_NAME, "/DISABLE"])?;
    let _ = run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", DAEMON_TASK_NAME]));
    Ok(())
}

fn uninstall() -> anyhow::Result<()> {
    let _ =
        run_hidden(Command::new("schtasks.exe").args(["/Delete", "/TN", NOTIFIER_TASK_NAME, "/F"]));
    let _ =
        run_hidden(Command::new("schtasks.exe").args(["/Delete", "/TN", DAEMON_TASK_NAME, "/F"]));
    let _ = run_hidden(Command::new("reg.exe").args([
        "delete",
        &format!(r"HKLM\Software\Classes\{PROTOCOL}"),
        "/f",
    ]));
    log("系统任务已移除，本地数据未删除");
    Ok(())
}

fn daemon_task_xml(daemon: &Path) -> anyhow::Result<String> {
    let executable = xml_escape(&daemon.to_string_lossy());
    let working_directory =
        xml_escape(&daemon.parent().unwrap_or(Path::new(".")).to_string_lossy());
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>v1gn0s</Author>
    <Description>VG Campus Wi-Fi Assistant system daemon</Description>
  </RegistrationInfo>
  <Triggers>
    <BootTrigger><Enabled>true</Enabled><Delay>PT3S</Delay></BootTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>S-1-5-18</UserId>
      <LogonType>ServiceAccount</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{executable}</Command>
      <Arguments>--onstart</Arguments>
      <WorkingDirectory>{working_directory}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>"#
    ))
}

fn notifier_task_xml(notifier: &Path, owner_sid: &str) -> anyhow::Result<String> {
    let executable = xml_escape(&notifier.to_string_lossy());
    let working_directory = xml_escape(
        &notifier
            .parent()
            .unwrap_or(Path::new("."))
            .to_string_lossy(),
    );
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>v1gn0s</Author>
    <Description>VG Campus Wi-Fi Assistant on-demand notifier</Description>
  </RegistrationInfo>
  <Triggers />
  <Principals>
    <Principal id="Author">
      <UserId>{owner_sid}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>Queue</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT1M</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{executable}</Command>
      <WorkingDirectory>{working_directory}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>"#
    ))
}

fn register_task(name: &str, xml: &str) -> anyhow::Result<()> {
    let path = env::temp_dir().join(format!("{name}-{}.xml", std::process::id()));
    fs::write(&path, xml)?;
    let task_path = path.to_string_lossy().into_owned();
    let status = run_hidden(
        Command::new("schtasks.exe").args(["/Create", "/TN", name, "/XML", &task_path, "/F"]),
    )?;
    let _ = fs::remove_file(path);
    if !status {
        anyhow::bail!("注册计划任务 {name} 失败");
    }
    Ok(())
}

fn run_schtasks<const N: usize>(arguments: [&str; N]) -> anyhow::Result<()> {
    if run_hidden(Command::new("schtasks.exe").args(arguments))? {
        Ok(())
    } else {
        anyhow::bail!("计划任务命令执行失败")
    }
}

fn register_protocol(gui: &Path) -> anyhow::Result<()> {
    let key = format!(r"HKLM\Software\Classes\{PROTOCOL}");
    run_reg([
        "add",
        &key,
        "/ve",
        "/d",
        "URL:VG Campus Wi-Fi Assistant",
        "/f",
    ])?;
    run_reg(["add", &key, "/v", "URL Protocol", "/d", "", "/f"])?;
    let command = format!("\"{}\" \"%1\"", gui.display());
    run_reg([
        "add",
        &format!(r"{key}\shell\open\command"),
        "/ve",
        "/d",
        &command,
        "/f",
    ])?;
    let aumid_key = format!(r"HKLM\Software\Classes\AppUserModelId\{APP_ID}");
    run_reg([
        "add",
        &aumid_key,
        "/v",
        "DisplayName",
        "/d",
        "VG 校园网助手",
        "/f",
    ])?;
    Ok(())
}

fn run_reg<const N: usize>(arguments: [&str; N]) -> anyhow::Result<()> {
    if run_hidden(Command::new("reg.exe").args(arguments))? {
        Ok(())
    } else {
        anyhow::bail!("注册表命令执行失败")
    }
}

fn restrict_data_directory(data_root: &Path, owner_sid: &str) {
    let path = data_root.to_string_lossy().to_string();
    let system = "*S-1-5-18:(OI)(CI)F";
    let administrators = "*S-1-5-32-544:(OI)(CI)F";
    let owner = format!("*{owner_sid}:(OI)(CI)M");
    let _ = run_hidden(Command::new("icacls.exe").args([
        &path,
        "/inheritance:r",
        "/grant:r",
        system,
        administrators,
        &owner,
        "/T",
    ]));
}

fn validate_executable(path: Option<&Path>, label: &str) -> anyhow::Result<PathBuf> {
    let path = path.ok_or_else(|| anyhow::anyhow!("缺少{label}路径"))?;
    if !path.is_file() {
        anyhow::bail!("{label}不存在：{}", path.display());
    }
    Ok(path.canonicalize()?)
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

fn data_root() -> PathBuf {
    match env::var_os("VG_CAMPUS_DATA_DIR") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("VGCampusWifiAssistant"),
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn run_hidden(command: &mut Command) -> std::io::Result<bool> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.status().map(|status| status.success())
}

fn needs_admin(command: &str) -> bool {
    matches!(command, "install" | "enable" | "disable" | "uninstall")
}

fn is_admin() -> bool {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn elevate(command: &str, options: &Options) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let executable = env::current_exe()?;
        let mut arguments = vec![command.to_owned()];
        if let Some(path) = &options.daemon {
            arguments.extend(["--daemon".into(), quote(path)]);
        }
        if let Some(path) = &options.notifier {
            arguments.extend(["--notifier".into(), quote(path)]);
        }
        if let Some(path) = &options.gui {
            arguments.extend(["--gui".into(), quote(path)]);
        }
        let owner_sid = options.owner_sid.clone().or_else(current_user_sid);
        if let Some(sid) = owner_sid {
            arguments.extend(["--owner-sid".into(), sid]);
        }
        if let Some(path) = &options.ready_file {
            arguments.extend(["--ready-file".into(), quote(path)]);
        }
        arguments.push("--elevated".into());

        let file = wide(executable.to_string_lossy().as_ref());
        let operation = wide("runas");
        let parameters = wide(arguments.join(" ").as_str());
        let directory = executable
            .parent()
            .map(|path| wide(path.to_string_lossy().as_ref()));
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                directory
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            anyhow::bail!("无法请求管理员权限");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (command, options);
        anyhow::bail!("系统任务安装只支持 Windows")
    }
}

fn quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn log(message: impl AsRef<str>) {
    let path = env::temp_dir().join("vg-campus-setup.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {}", chrono_like_timestamp(), message.as_ref());
    }
}

fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_task_runs_on_start_without_execution_limit() {
        let xml = daemon_task_xml(Path::new(r"C:\Program Files\VG\daemon.exe")).unwrap();
        assert!(xml.contains("<Arguments>--onstart</Arguments>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains(
            "<RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>"
        ));
    }

    #[test]
    fn notifier_task_is_bound_to_owner_session() {
        let xml = notifier_task_xml(
            Path::new(r"C:\Program Files\VG\notifier.exe"),
            "S-1-5-21-100-200-300-1001",
        )
        .unwrap();
        assert!(xml.contains("<UserId>S-1-5-21-100-200-300-1001</UserId>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(!xml.contains("<BootTrigger>"));
    }

    #[test]
    fn sid_validation_rejects_shell_fragments() {
        assert!(valid_sid("S-1-5-21-100-200-300-1001"));
        assert!(!valid_sid("S-1-5-21-1&calc"));
        assert!(!valid_sid(""));
    }
}
