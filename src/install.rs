//! Per-user install: a copy under %LOCALAPPDATA%\Programs and a Task Scheduler task that starts it
//! at logon. A logon task (unlike a Run key entry) does not appear under Task Manager > Startup apps.

use std::path::PathBuf;

use windows::core::BSTR;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::TaskScheduler::{
    ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
};
use windows::Win32::System::Variant::VARIANT;

const TASK_NAME: &str = "phantomlink-sensors";

pub fn install_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("Programs").join("phantomlink-sensors"))
}

/// The root folder of the Task Scheduler library, through its COM API.
fn task_folder() -> Result<ITaskFolder, String> {
    let err = |what: &str, e: windows::core::Error| format!("Task Scheduler: could not {what} ({e})");
    unsafe {
        // Already initialised on this thread is fine; anything else surfaces in CoCreateInstance.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let service: ITaskService =
            CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(|e| err("start", e))?;
        let none = VARIANT::default();
        service.Connect(&none, &none, &none, &none).map_err(|e| err("connect", e))?;
        service.GetFolder(&BSTR::from("\\")).map_err(|e| err("open the task library", e))
    }
}

pub fn is_installed() -> bool {
    task_folder().is_ok_and(|f| unsafe { f.GetTask(&BSTR::from(TASK_NAME)) }.is_ok())
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

    let xml = task_xml(&target.display().to_string(), extra_args)?;
    let none = VARIANT::default();
    unsafe {
        let task = task_folder()?
            .RegisterTask(
                &BSTR::from(TASK_NAME),
                &BSTR::from(xml),
                TASK_CREATE_OR_UPDATE.0,
                &none,
                &none,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &none,
            )
            .map_err(|e| format!("could not create the logon task ({e})"))?;
        task.Run(&none).map_err(|e| format!("created the logon task, but could not start it ({e})"))?;
    }
    Ok(target)
}

/// Stops the background instance, removes the logon task and deletes the installed copy.
pub fn uninstall() -> Result<(), String> {
    crate::stop::stop_running();
    if is_installed() {
        unsafe { task_folder()?.DeleteTask(&BSTR::from(TASK_NAME), 0) }
            .map_err(|e| format!("could not delete the logon task ({e})"))?;
    }
    let dir = install_dir()?;
    if dir.exists() && std::env::current_exe().map_or(true, |e| !e.starts_with(&dir)) {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("could not delete {}: {e}", dir.display()))?;
    }
    Ok(())
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
