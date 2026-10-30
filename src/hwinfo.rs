//! HWiNFO custom sensors: HKCU\Software\HWiNFO64\Sensors\Custom\<sensor>\<Type><n>
//! with string values "Name" and "Value". HWiNFO picks up new sensors when it starts
//! and re-reads "Value" on every polling cycle.

use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};

use crate::wide;

pub const SENSOR_KEY: &str = r"Software\HWiNFO64\Sensors\Custom\Sapphire Power Protection";

pub struct Entry {
    key: HKEY,
}

impl Entry {
    /// `slot` is the HWiNFO type and index, e.g. "Temp0" or "Other0".
    pub fn create(slot: &str, name: &str, unit: Option<&str>) -> Result<Self, String> {
        let path = format!(r"{SENSOR_KEY}\{slot}");
        let mut key = null_mut();
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut key,
                null_mut(),
            )
        };
        if rc != 0 {
            return Err(format!("could not create HKCU\\{path} (error {rc})"));
        }
        let entry = Self { key };
        entry.set("Name", name)?;
        if let Some(unit) = unit {
            entry.set("Unit", unit)?;
        }
        Ok(entry)
    }

    pub fn set_value(&self, value: impl ToString) -> Result<(), String> {
        self.set("Value", &value.to_string())
    }

    fn set(&self, field: &str, data: &str) -> Result<(), String> {
        let data = wide(data);
        let rc = unsafe {
            RegSetValueExW(
                self.key,
                wide(field).as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("could not write {field} (error {rc})"))
        }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.key) };
    }
}

pub fn remove() -> Result<(), String> {
    let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(SENSOR_KEY).as_ptr()) };
    // 2 = ERROR_FILE_NOT_FOUND: nothing to remove.
    if rc == 0 || rc == 2 {
        Ok(())
    } else {
        Err(format!("could not delete HKCU\\{SENSOR_KEY} (error {rc})"))
    }
}
