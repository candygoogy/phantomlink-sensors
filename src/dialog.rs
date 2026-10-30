//! What a double-click does: a modern task dialog (heading, details, labelled buttons) instead of a
//! silent background start. Needs Common Controls v6, which the manifest from build.rs selects.

use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS,
    TASKDIALOG_NOTIFICATIONS, TDCBF_CANCEL_BUTTON, TDCBF_CLOSE_BUTTON, TDCBF_OK_BUTTON,
    TDF_ALLOW_DIALOG_CANCELLATION, TDN_CREATED, TD_WARNING_ICON,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON,
    SM_CXSMICON, WM_SETICON,
};

use crate::{wide, Adl, Reading};

const TITLE: &str = "phantomlink-sensors";
const ID_ACTION: i32 = 100;

struct Dialog<'a> {
    heading: &'a str,
    text: &'a str,
    /// Label of the main action button (the default), if any.
    action: Option<&'a str>,
    buttons: TASKDIALOG_COMMON_BUTTON_FLAGS,
    warning: bool,
}

/// Gives the dialog window the app icon (resource 1 from assets/icon.rc) for its title bar and taskbar
/// button; a task dialog doesn't pick it up on its own.
unsafe extern "system" fn on_event(hwnd: HWND, msg: TASKDIALOG_NOTIFICATIONS, _: WPARAM, _: LPARAM, _: isize) -> i32 {
    if msg == TDN_CREATED {
        let module = GetModuleHandleW(std::ptr::null());
        for (which, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
            let px = GetSystemMetrics(metric);
            let icon = LoadImageW(module, 1 as _, IMAGE_ICON, px, px, LR_DEFAULTCOLOR);
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, which as WPARAM, icon as LPARAM);
            }
        }
    }
    0
}

/// Shows the dialog; true if the action button was pressed.
fn show(d: Dialog) -> bool {
    let (title, heading, text) = (wide(TITLE), wide(d.heading), wide(d.text));
    let label = d.action.map(wide);
    let action: Vec<TASKDIALOG_BUTTON> =
        label.iter().map(|l| TASKDIALOG_BUTTON { nButtonID: ID_ACTION, pszButtonText: l.as_ptr() }).collect();

    let mut cfg: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    cfg.cbSize = size_of::<TASKDIALOGCONFIG>() as u32;
    cfg.dwFlags = TDF_ALLOW_DIALOG_CANCELLATION;
    cfg.dwCommonButtons = d.buttons;
    cfg.pszWindowTitle = title.as_ptr();
    if d.warning {
        cfg.Anonymous1 = TASKDIALOGCONFIG_0 { pszMainIcon: TD_WARNING_ICON };
    }
    cfg.pszMainInstruction = heading.as_ptr();
    cfg.pszContent = text.as_ptr();
    cfg.cButtons = action.len() as u32;
    cfg.pButtons = action.as_ptr();
    cfg.nDefaultButton = if action.is_empty() { 0 } else { ID_ACTION };
    cfg.pfCallback = Some(on_event);

    let mut pressed = 0;
    let hr = unsafe { TaskDialogIndirect(&cfg, &mut pressed, null_mut(), null_mut()) };
    hr >= 0 && pressed == ID_ACTION
}

fn done(heading: &str, text: &str) {
    show(Dialog { heading, text, action: None, buttons: TDCBF_OK_BUTTON, warning: false });
}

fn problem(heading: &str, text: &str) {
    show(Dialog { heading, text, action: None, buttons: TDCBF_OK_BUTTON, warning: true });
}

pub fn run() {
    if crate::install::is_installed() {
        let uninstall = show(Dialog {
            heading: "phantomlink-sensors is installed",
            text: "It runs in the background and starts every time you log in.",
            action: Some("Uninstall"),
            buttons: TDCBF_CLOSE_BUTTON,
            warning: false,
        });
        if uninstall {
            match crate::install::uninstall().and_then(|_| crate::hwinfo::remove()) {
                Ok(()) => done("Uninstalled", "Restart HWiNFO to remove the Sapphire Power Protection sensor."),
                Err(e) => problem("Uninstall failed", &e),
            }
        }
        return;
    }

    let reading = Adl::open().and_then(|adl| {
        let adapter = crate::pick_adapter(&adl, None, false)?;
        crate::read(&adl, adapter)
    });
    let r = match reading {
        Ok(r) => r,
        Err(e) => return problem("Can't read the connector sensors", &format!("{e}\n\nNothing was installed.")),
    };

    let install = show(Dialog {
        heading: "Install phantomlink-sensors?",
        text: &format!(
            "Found your Sapphire card. Its power connectors are at:\n\n{}\nThe tool runs in the background, starts \
             when you log in, and adds these readings to HWiNFO. It doesn't need admin rights.",
            describe(&r)
        ),
        action: Some("Install"),
        buttons: TDCBF_CANCEL_BUTTON,
        warning: false,
    });
    if install {
        match crate::install::install("") {
            Ok(_) => done(
                "Installed and running",
                "Restart HWiNFO once to see the Sapphire Power Protection sensor. From now on it starts by itself \
                 when you log in.",
            ),
            Err(e) => problem("Install failed", &e),
        }
    }
}

fn describe(r: &Reading) -> String {
    let mut s = format!("12V-2x6 connector: {} °C\n", r.temp_12v_2x6);
    if let Some(t) = r.temp_gc_hpwr {
        s += &format!("GC-HPWR connector: {t} °C\n");
    }
    s
}
