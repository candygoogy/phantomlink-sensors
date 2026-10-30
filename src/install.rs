//! Per-user install: a copy under %LOCALAPPDATA%\Programs and a Task Scheduler task that starts it
//! at logon. A logon task (unlike a Run key entry) does not appear under Task Manager > Startup apps.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const TASK_NAME: &str = "phantomlink-sensors";
/// Keeps schtasks (a console program) from flashing a window when we run without a console.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn install_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("Programs").join("phantomlink-sensors"))
}

pub fn is_installed() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME]).is_ok()
}

/// True for the copy the logon task starts.
pub fn is_installed_copy() -> bool {
    match (std::env::current_exe(), install_dir()) {
        (Ok(exe), Ok(dir)) => exe.starts_with(dir),
        _ => false,
    }
}

/// Copies this exe into the install folder, registers the logon task and starts it now.
pub fn install(extra_args: &str) -> Result<PathBuf, String> {
    let dir = install_dir()?;
    let target = dir.join("phantomlink-sensors.exe");
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    // Whatever copy is running now (possibly an older installed one holding the file open) makes way.
    crate::stop::stop_running();
    if current != target {
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        std::fs::copy(&current, &target).map_err(|e| format!("could not copy to {}: {e}", target.display()))?;
    }

    let xml_path = std::env::temp_dir().join("phantomlink-sensors-task.xml");
    write_utf16(&xml_path, &task_xml(&target.display().to_string(), extra_args)?)?;
    let created = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &xml_path.display().to_string(), "/F"]);
    let _ = std::fs::remove_file(&xml_path);
    created?;
    schtasks(&["/Run", "/TN", TASK_NAME])?;
    Ok(target)
}

/// Stops the background instance, removes the logon task and deletes the installed copy.
pub fn uninstall() -> Result<(), String> {
    crate::stop::stop_running();
    if is_installed() {
        schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?;
    }
    let dir = install_dir()?;
    if dir.exists() && std::env::current_exe().map_or(true, |e| !e.starts_with(&dir)) {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("could not delete {}: {e}", dir.display()))?;
    }
    Ok(())
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    let out = Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run schtasks: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("schtasks {} failed: {}", args[0], String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn task_xml(exe: &str, args: &str) -> Result<String, String> {
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user = std::env::var("USERNAME").map_err(|_| "USERNAME is not set")?;
    let user = escape(&if domain.is_empty() { user } else { format!("{domain}\\{user}") });
    let (exe, args) = (escape(exe), escape(args));
    // No time limit, keep running on battery, start once the user logs on (with a short delay so
    // the display driver is up), never a second copy.
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Publishes the Sapphire power connector sensors to HWiNFO (phantomlink-sensors).</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT15S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <StartWhenAvailable>true</StartWhenAvailable>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{args}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    ))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn write_utf16(path: &std::path::Path, text: &str) -> Result<(), String> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))
}
