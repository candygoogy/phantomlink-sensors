//! Lets `--stop` end the background instance through a named event it waits on between polls.

use std::ptr::null;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE};

use crate::wide;

const STOP_EVENT: &str = "Local\\phantomlink-sensors-stop";

/// Asks a running background instance to exit and gives it a moment to do so.
pub fn stop_running() -> bool {
    unsafe {
        let event = OpenEventW(EVENT_MODIFY_STATE, 0, wide(STOP_EVENT).as_ptr());
        if event.is_null() {
            return false;
        }
        SetEvent(event);
        CloseHandle(event);
    }
    std::thread::sleep(Duration::from_millis(1500));
    true
}

/// Event the background loop waits on between polls; set by `stop_running`.
pub struct StopSignal(HANDLE);

impl StopSignal {
    pub fn new() -> Self {
        Self(unsafe { CreateEventW(null(), 1, 0, wide(STOP_EVENT).as_ptr()) })
    }

    /// Sleeps for `ms`, returning true early if a stop was requested.
    pub fn wait(&self, ms: u64) -> bool {
        if self.0.is_null() {
            std::thread::sleep(Duration::from_millis(ms));
            return false;
        }
        unsafe { WaitForSingleObject(self.0, ms as u32) == WAIT_OBJECT_0 }
    }
}

impl Drop for StopSignal {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}
