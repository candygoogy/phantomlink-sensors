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
const ID_INSTALL: i32 = 100;
const ID_UNINSTALL: i32 = 101;

struct Dialog<'a> {
    heading: &'a str,
    text: &'a str,
    /// Custom buttons (ID, label); the first one is the default.
    actions: &'a [(i32, &'a str)],
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

/// Shows the dialog and returns the ID of the button pressed (0 if it couldn't be shown).
fn show(d: Dialog) -> i32 {
    let (title, heading, text) = (wide(TITLE), wide(d.heading), wide(d.text));
    let labels: Vec<_> = d.actions.iter().map(|&(_, l)| wide(l)).collect();
    let action: Vec<TASKDIALOG_BUTTON> = d
        .actions
        .iter()
        .zip(&labels)
        .map(|(&(id, _), l)| TASKDIALOG_BUTTON { nButtonID: id, pszButtonText: l.as_ptr() })
        .collect();

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
    cfg.nDefaultButton = d.actions.first().map_or(0, |&(id, _)| id);
    cfg.pfCallback = Some(on_event);

    let mut pressed = 0;
    let hr = unsafe { TaskDialogIndirect(&cfg, &mut pressed, null_mut(), null_mut()) };
    if hr >= 0 {
        pressed
    } else {
        0
    }
}

fn done(heading: &str, text: &str) {
    show(Dialog { heading, text, actions: &[], buttons: TDCBF_OK_BUTTON, warning: false });
}

fn problem(heading: &str, text: &str) {
    show(Dialog { heading, text, actions: &[], buttons: TDCBF_OK_BUTTON, warning: true });
}

/// Reads the card; on failure tells the user and returns None.
fn check_card() -> Option<Reading> {
    let reading = Adl::open().and_then(|adl| {
        let adapter = crate::pick_adapter(&adl, None, false)?;
        crate::read(&adl, adapter)
    });
    match reading {
        Ok(r) => Some(r),
        Err(e) => {
            problem("Can't read the connector sensors", &format!("{e}\n\nNothing was installed."));
            None
        }
    }
}

fn install_now(heading: &str, text: &str) {
    match crate::install::install("") {
        Ok(_) => done(heading, text),
        Err(e) => problem("Install failed", &e),
    }
}

pub fn run() {
    if crate::install::is_installed() {
        // A different file than the installed copy (usually a newer release) can replace it.
        let update = !crate::install::is_same_as_installed();
        let text = if update {
            format!(
                "It runs in the background and starts every time you log in.\n\nThe file you opened is a different \
                 version ({}). Install it to replace the installed copy.",
                env!("CARGO_PKG_VERSION")
            )
        } else {
            "It runs in the background and starts every time you log in.".to_string()
        };
        let actions: &[(i32, &str)] = if update {
            &[(ID_INSTALL, "Install this version"), (ID_UNINSTALL, "Uninstall")]
        } else {
            &[(ID_UNINSTALL, "Uninstall")]
        };
        let pressed = show(Dialog {
            heading: "phantomlink-sensors is installed",
            text: &text,
            actions,
            buttons: TDCBF_CLOSE_BUTTON,
            warning: false,
        });
        match pressed {
            ID_INSTALL => {
                if check_card().is_some() {
                    install_now(
                        &format!("Updated to {}", env!("CARGO_PKG_VERSION")),
                        "The new version is running. HWiNFO keeps showing the sensor; no restart needed.",
                    );
                }
            }
            ID_UNINSTALL => match crate::install::uninstall().and_then(|_| crate::hwinfo::remove()) {
                Ok(()) => done("Uninstalled", "Restart HWiNFO to remove the Sapphire Power Protection sensor."),
                Err(e) => problem("Uninstall failed", &e),
            },
            _ => {}
        }
        return;
    }

    let Some(r) = check_card() else { return };
    let pressed = show(Dialog {
        heading: "Install phantomlink-sensors?",
        text: &format!(
            "Found your Sapphire card. Its power connectors are at:\n\n{}\nThe tool runs in the background, starts \
             when you log in, and adds these readings to HWiNFO. It doesn't need admin rights.",
            describe(&r)
        ),
        actions: &[(ID_INSTALL, "Install")],
        buttons: TDCBF_CANCEL_BUTTON,
        warning: false,
    });
    if pressed == ID_INSTALL {
        install_now(
            "Installed and running",
            "Restart HWiNFO once to see the Sapphire Power Protection sensor. From now on it starts by itself \
             when you log in.",
        );
    }
}

fn describe(r: &Reading) -> String {
    let mut s = format!("12V-2x6 connector: {} °C\n", r.temp_12v_2x6);
    if let Some(t) = r.temp_gc_hpwr {
        s += &format!("GC-HPWR connector: {t} °C\n");
    }
    s
}
