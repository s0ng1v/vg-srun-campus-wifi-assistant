#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::SystemTime,
};

use anyhow::Context;
use srun_ipc::{DAEMON_ONDEMAND_TASK_NAME, DAEMON_TASK_NAME, NOTIFIER_TASK_NAME};

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
    log(format!(
        "command={command} token_elevated={} elevated_flag={}",
        is_admin(),
        options.elevated
    ));

    let result = if needs_admin(command) && !is_admin() && !options.elevated {
        elevate(command, &options)
    } else {
        match command {
            "install" => install(&options),
            "repair" => repair(&options),
            "enable" => enable_daemon(),
            "disable" => disable_daemon(),
            "uninstall" => uninstall(),
            _ => anyhow::bail!("未知命令：{command}"),
        }
    };
    if let (Err(error), Some(path)) = (&result, &options.ready_file) {
        write_ready_file(path, &format!("error:{error:#}"));
    }
    result
}

fn write_ready_file(path: &Path, content: &str) {
    let temp = path.with_extension("tmp");
    if fs::write(&temp, content).is_ok() && fs::rename(&temp, path).is_ok() {
        return;
    }
    let _ = fs::remove_file(&temp);
    let _ = fs::write(path, content);
}

fn repair(options: &Options) -> anyhow::Result<()> {
    let owner_sid = options
        .owner_sid
        .clone()
        .or_else(current_user_sid)
        .filter(|sid| valid_sid(sid))
        .ok_or_else(|| anyhow::anyhow!("无法确定当前用户 SID"))?;
    let data_root = data_root();
    secure_data_directory(&data_root, &owner_sid)?;
    register_owner_sid(&owner_sid)?;
    ensure_owner_sid(&data_root.join("owner.sid"), &owner_sid);
    if let Some(path) = &options.ready_file {
        write_ready_file(path, "ok");
    }
    log("数据目录权限修复完成");
    Ok(())
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
    log("开始安装系统集成");
    let daemon = validate_executable(options.daemon.as_deref(), "守护程序")?;
    let notifier = validate_executable(options.notifier.as_deref(), "通知程序")?;
    let gui = validate_executable(options.gui.as_deref(), "图形界面")?;
    let installed =
        install_files(&daemon, &notifier, &gui).with_context(|| "复制受保护程序文件失败")?;
    let owner_sid = options
        .owner_sid
        .clone()
        .or_else(current_user_sid)
        .filter(|sid| valid_sid(sid))
        .ok_or_else(|| anyhow::anyhow!("无法确定当前用户 SID"))?;

    let data_root = data_root();
    ensure_data_directory(&data_root)?;
    log("开始修复数据目录权限");
    secure_data_directory(&data_root, &owner_sid)?;
    register_owner_sid(&owner_sid)?;
    ensure_owner_sid(&data_root.join("owner.sid"), &owner_sid);

    let daemon_xml = daemon_task_xml(&installed.daemon, &owner_sid, true)?;
    log("开始注册开机守护任务");
    register_task(DAEMON_TASK_NAME, &daemon_xml)?;
    let ondemand_xml = daemon_task_xml(&installed.daemon, &owner_sid, false)?;
    register_task(DAEMON_ONDEMAND_TASK_NAME, &ondemand_xml)?;
    let notifier_xml = notifier_task_xml(&installed.notifier, &owner_sid)?;
    register_task(NOTIFIER_TASK_NAME, &notifier_xml)?;
    register_protocol(&installed.gui)?;
    // 重新安装/升级时先结束仍在运行的旧守护进程，否则旧版本会继续占用本地管道，
    // 新版本要等到下次重启才会生效。结束命令返回后进程可能尚未完全退出，
    // 稍等片刻再启动，并在失败时补试一次（已运行时会直接报错，可忽略）。
    let _ =
        run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", DAEMON_ONDEMAND_TASK_NAME]));
    let _ = run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", DAEMON_TASK_NAME]));
    std::thread::sleep(std::time::Duration::from_millis(900));
    let _ = run_hidden(Command::new("schtasks.exe").args(["/Run", "/TN", DAEMON_TASK_NAME]));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let _ = run_hidden(Command::new("schtasks.exe").args(["/Run", "/TN", DAEMON_TASK_NAME]));
    log("系统守护任务与通知任务已安装");
    if let Some(path) = &options.ready_file {
        write_ready_file(path, "ok");
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
    let app_directory = program_files.join("VGCampusWifiAssistant").join(format!(
        "app-{}-{}",
        env!("CARGO_PKG_VERSION"),
        build_directory_stamp()
    ));
    fs::create_dir_all(&app_directory)
        .with_context(|| format!("无法创建程序目录 {}", app_directory.display()))?;

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
    Ok(InstalledBinaries {
        daemon: daemon_target,
        notifier: notifier_target,
        gui: gui_target,
    })
}

fn build_directory_stamp() -> u64 {
    env::current_exe()
        .and_then(fs::metadata)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
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
    fs::copy(source, target)
        .with_context(|| format!("无法复制 {} 到 {}", source.display(), target.display()))?;
    Ok(())
}

fn enable_daemon() -> anyhow::Result<()> {
    run_schtasks(["/Change", "/TN", DAEMON_TASK_NAME, "/ENABLE"])?;
    run_schtasks(["/Run", "/TN", DAEMON_TASK_NAME])?;
    Ok(())
}

fn disable_daemon() -> anyhow::Result<()> {
    run_schtasks(["/Change", "/TN", DAEMON_TASK_NAME, "/DISABLE"])?;
    let _ = run_hidden(Command::new("schtasks.exe").args([
        "/Change",
        "/TN",
        DAEMON_ONDEMAND_TASK_NAME,
        "/DISABLE",
    ]));
    let _ = run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", DAEMON_TASK_NAME]));
    Ok(())
}

fn uninstall() -> anyhow::Result<()> {
    let _ =
        run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", DAEMON_ONDEMAND_TASK_NAME]));
    let _ = run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", DAEMON_TASK_NAME]));
    let _ = run_hidden(Command::new("schtasks.exe").args(["/End", "/TN", NOTIFIER_TASK_NAME]));
    let _ =
        run_hidden(Command::new("schtasks.exe").args(["/Delete", "/TN", NOTIFIER_TASK_NAME, "/F"]));
    let _ =
        run_hidden(Command::new("schtasks.exe").args(["/Delete", "/TN", DAEMON_TASK_NAME, "/F"]));
    let _ = run_hidden(Command::new("schtasks.exe").args([
        "/Delete",
        "/TN",
        DAEMON_ONDEMAND_TASK_NAME,
        "/F",
    ]));
    let _ = run_hidden(Command::new("reg.exe").args([
        "delete",
        &format!(r"HKLM\Software\Classes\{PROTOCOL}"),
        "/f",
    ]));
    let _ = run_hidden(Command::new("reg.exe").args([
        "delete",
        &format!(r"HKLM\Software\Classes\AppUserModelId\{APP_ID}"),
        "/f",
    ]));
    log("系统任务已移除，本地数据未删除");
    Ok(())
}

fn daemon_task_xml(daemon: &Path, owner_sid: &str, boot: bool) -> anyhow::Result<String> {
    let executable = xml_escape(&daemon.to_string_lossy());
    let working_directory =
        xml_escape(&daemon.parent().unwrap_or(Path::new(".")).to_string_lossy());
    let triggers = if boot {
        "<Triggers><BootTrigger><Enabled>true</Enabled><Delay>PT3S</Delay></BootTrigger></Triggers>"
    } else {
        "<Triggers />"
    };
    let arguments = if boot {
        "<Arguments>--onstart</Arguments>"
    } else {
        ""
    };
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>v1gn0s</Author>
    <Description>VG Campus Wi-Fi Assistant system daemon</Description>
    <SecurityDescriptor>D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{owner_sid})</SecurityDescriptor>
  </RegistrationInfo>
  {triggers}
  <Principals>
    <Principal id="Author">
      <UserId>S-1-5-18</UserId>
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
  <Actions>
    <Exec>
      <Command>{executable}</Command>
      {arguments}
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
    let security_descriptor = format!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{owner_sid})");
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>v1gn0s</Author>
    <Description>VG Campus Wi-Fi Assistant on-demand notifier</Description>
    <SecurityDescriptor>{security_descriptor}</SecurityDescriptor>
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
  <Actions>
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
    let mut encoded = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&path, encoded)?;
    let task_path = path.to_string_lossy().into_owned();
    let output = Command::new("schtasks.exe")
        .args(["/Create", "/TN", name, "/XML", &task_path, "/F"])
        .stdin(Stdio::null())
        .output()?;
    let detail = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        let _ = fs::remove_file(path);
        log(format!("计划任务 {name} 注册成功（schtasks）"));
        return Ok(());
    }
    log(format!("schtasks 注册计划任务 {name} 失败：{detail}"));

    let script = "$xml = Get-Content -Raw -LiteralPath $env:VG_TASK_XML; Register-ScheduledTask -TaskName $env:VG_TASK_NAME -Xml $xml -Force | Out-Null";
    let mut fallback = Command::new("powershell.exe");
    fallback
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("VG_TASK_XML", &task_path)
        .env("VG_TASK_NAME", name)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        fallback.creation_flags(0x0800_0000);
    }
    let fallback_output = fallback.output()?;
    let _ = fs::remove_file(path);
    let fallback_detail = format!(
        "{}{}",
        String::from_utf8_lossy(&fallback_output.stdout),
        String::from_utf8_lossy(&fallback_output.stderr)
    );
    if !fallback_output.status.success() {
        log(format!(
            "Register-ScheduledTask 注册计划任务 {name} 失败：{fallback_detail}"
        ));
        anyhow::bail!(
            "注册计划任务 {name} 失败：schtasks={}；PowerShell={}",
            detail.trim(),
            fallback_detail.trim()
        );
    }
    log(format!(
        "计划任务 {name} 注册成功（Register-ScheduledTask）"
    ));
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

fn secure_data_directory(data_root: &Path, owner_sid: &str) -> anyhow::Result<()> {
    let path = data_root.to_string_lossy().to_string();
    let system = "*S-1-5-18:(OI)(CI)F";
    let administrators = "*S-1-5-32-544:(OI)(CI)F";
    let owner = format!("*{owner_sid}:(OI)(CI)M");
    let _ = run_logged(
        Command::new("takeown.exe").args(["/F", &path, "/R", "/A", "/D", "Y"]),
        "takeown data directory",
    );
    let _ = run_logged(
        Command::new("icacls.exe").args([&path, "/reset", "/T", "/C"]),
        "icacls reset data directory",
    );
    let granted = run_logged(
        Command::new("icacls.exe").args([
            &path,
            "/inheritance:r",
            "/grant:r",
            system,
            administrators,
            &owner,
            "/T",
            "/C",
        ]),
        "icacls grant data directory",
    )?;
    if !granted {
        anyhow::bail!("无法修复数据目录访问权限：{path}");
    }
    apply_data_dacl(data_root, owner_sid)?;
    let integrity_set = run_logged(
        Command::new("icacls.exe").args([&path, "/setintegritylevel", "M", "/T", "/C"]),
        "icacls set integrity",
    )?;
    if !integrity_set {
        log(format!("部分数据文件仍保持高完整性级别：{path}"));
    }
    for name in [
        "config.json.tmp",
        "credentials.dat.tmp",
        "notification-event.tmp",
    ] {
        let _ = fs::remove_file(data_root.join(name));
    }
    Ok(())
}

fn apply_data_dacl(data_root: &Path, owner_sid: &str) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SetFileSecurityW,
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };

    fn apply(path: &Path, sddl: &str) -> anyhow::Result<()> {
        let wide_sddl: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_path: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let mut size = 0u32;
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide_sddl.as_ptr(),
                1,
                &mut descriptor,
                &mut size,
            )
        };
        if converted == 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("无法生成 ACL：{}", path.display()));
        }
        let applied = unsafe {
            SetFileSecurityW(
                wide_path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        if applied == 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("无法设置 ACL：{}", path.display()));
        }
        Ok(())
    }

    let directory_sddl = format!("D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GA;;;{owner_sid})");
    let file_sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{owner_sid})");
    apply(data_root, &directory_sddl)?;
    for entry in fs::read_dir(data_root)? {
        let entry = entry?;
        let path = entry.path();
        let sddl = if path.is_dir() {
            &directory_sddl
        } else {
            &file_sddl
        };
        apply(&path, sddl)?;
        if path.is_dir() {
            for child in fs::read_dir(path)? {
                apply(&child?.path(), &file_sddl)?;
            }
        }
    }
    Ok(())
}

fn ensure_data_directory(data_root: &Path) -> anyhow::Result<()> {
    match fs::metadata(data_root) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => anyhow::bail!("数据路径不是目录：{}", data_root.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(data_root)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn run_logged(command: &mut Command, label: &str) -> std::io::Result<bool> {
    let output = command.stdin(Stdio::null()).output()?;
    let detail = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    log(format!("{label}: {}", detail.trim()));
    Ok(output.status.success())
}

fn ensure_owner_sid(path: &Path, owner_sid: &str) {
    if fs::read_to_string(path)
        .ok()
        .is_some_and(|current| current.trim().eq_ignore_ascii_case(owner_sid))
    {
        return;
    }
    if let Err(error) = fs::write(path, owner_sid) {
        log(format!("无法写入 {}：{error}", path.display()));
    }
}

fn register_owner_sid(owner_sid: &str) -> anyhow::Result<()> {
    run_reg([
        "add",
        r"HKLM\Software\VGCampusWifiAssistant",
        "/v",
        "OwnerSid",
        "/t",
        "REG_SZ",
        "/d",
        owner_sid,
        "/f",
    ])
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
    matches!(
        command,
        "install" | "repair" | "enable" | "disable" | "uninstall"
    )
}

fn is_admin() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let succeeded = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        unsafe {
            CloseHandle(token);
        }
        succeeded != 0 && elevation.TokenIsElevated != 0
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
        let xml = daemon_task_xml(
            Path::new(r"C:\Program Files\VG\daemon.exe"),
            "S-1-5-21-100-200-300-1001",
            true,
        )
        .unwrap();
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
    fn daemon_on_demand_task_has_no_boot_trigger_or_arg() {
        let xml = daemon_task_xml(
            Path::new(r"C:\Program Files\VG\daemon.exe"),
            "S-1-5-21-100-200-300-1001",
            false,
        )
        .unwrap();
        assert!(xml.contains("<Triggers />"));
        assert!(!xml.contains("--onstart"));
        assert!(xml.contains("S-1-5-21-100-200-300-1001"));
    }

    #[test]
    fn sid_validation_rejects_shell_fragments() {
        assert!(valid_sid("S-1-5-21-100-200-300-1001"));
        assert!(!valid_sid("S-1-5-21-1&calc"));
        assert!(!valid_sid(""));
    }
}
