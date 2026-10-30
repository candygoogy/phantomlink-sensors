//! Minimal binding to AMD's ADL (atiadlxx.dll): adapter enumeration and I2C reads.

use std::ffi::{c_char, c_int, c_void};
use std::mem::{size_of, transmute};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, FreeLibrary, HANDLE, HMODULE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use crate::wide;

const ADL_OK: c_int = 0;
/// ADL_DL_I2C_ACTIONREAD_REPEATEDSTART: write the register offset, then read without a stop bit.
const I2C_ACTION_READ_REPEATED_START: c_int = 3;

#[repr(C)]
struct AdlI2c {
    size: c_int,
    line: c_int,
    address: c_int,
    offset: c_int,
    action: c_int,
    speed: c_int,
    data_size: c_int,
    data: *mut c_char,
}

#[repr(C)]
pub struct AdapterInfo {
    size: c_int,
    pub index: c_int,
    udid: [u8; 256],
    pub bus: c_int,
    device: c_int,
    function: c_int,
    vendor_id: c_int,
    name: [u8; 256],
    display_name: [u8; 256],
    pub present: c_int,
    exist: c_int,
    driver_path: [u8; 256],
    driver_path_ext: [u8; 256],
    pnp: [u8; 256],
    os_display_index: c_int,
}

impl AdapterInfo {
    pub fn name(&self) -> String {
        cstr(&self.name)
    }
    pub fn pnp(&self) -> String {
        cstr(&self.pnp)
    }
}

fn cstr(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

type MallocCallback = unsafe extern "system" fn(c_int) -> *mut c_void;
type MainControlCreate = unsafe extern "C" fn(MallocCallback, c_int, *mut *mut c_void) -> c_int;
type MainControlDestroy = unsafe extern "C" fn(*mut c_void) -> c_int;
type NumberOfAdaptersGet = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
type AdapterInfoGet = unsafe extern "C" fn(*mut c_void, *mut AdapterInfo, c_int) -> c_int;
type WriteAndReadI2c = unsafe extern "C" fn(*mut c_void, c_int, *mut AdlI2c) -> c_int;

extern "C" {
    fn malloc(size: usize) -> *mut c_void;
}

unsafe extern "system" fn adl_malloc(size: c_int) -> *mut c_void {
    malloc(size.max(0) as usize)
}

pub struct Adl {
    ctx: *mut c_void,
    destroy: MainControlDestroy,
    number_of_adapters: NumberOfAdaptersGet,
    adapter_info: AdapterInfoGet,
    i2c: WriteAndReadI2c,
    /// Shared lock that HWiNFO, TriXX and other monitoring tools take around ATI I2C traffic.
    i2c_mutex: HANDLE,
    /// Released on drop, so reconnecting after a driver update loads the new atiadlxx.dll.
    lib: HMODULE,
}

macro_rules! proc {
    ($lib:expr, $name:literal, $ty:ty) => {{
        let p = GetProcAddress($lib, concat!($name, "\0").as_ptr());
        match p {
            Some(f) => transmute::<unsafe extern "system" fn() -> isize, $ty>(f),
            None => return Err(concat!("atiadlxx.dll has no ", $name).into()),
        }
    }};
}

impl Adl {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let lib = LoadLibraryW(wide("atiadlxx.dll").as_ptr());
            if lib.is_null() {
                return Err("could not load atiadlxx.dll (is the AMD driver installed?)".into());
            }
            let adl = Self::init(lib);
            if adl.is_err() {
                FreeLibrary(lib);
            }
            adl
        }
    }

    unsafe fn init(lib: HMODULE) -> Result<Self, String> {
        unsafe {
            let create = proc!(lib, "ADL2_Main_Control_Create", MainControlCreate);
            let mut ctx = null_mut();
            let rc = create(adl_malloc, 1, &mut ctx);
            if rc != ADL_OK {
                return Err(format!("ADL2_Main_Control_Create failed ({rc})"));
            }
            let i2c_mutex = CreateMutexW(std::ptr::null(), 0, wide("Global\\Access_ATI_I2C").as_ptr());
            Ok(Self {
                ctx,
                destroy: proc!(lib, "ADL2_Main_Control_Destroy", MainControlDestroy),
                number_of_adapters: proc!(lib, "ADL2_Adapter_NumberOfAdapters_Get", NumberOfAdaptersGet),
                adapter_info: proc!(lib, "ADL2_Adapter_AdapterInfo_Get", AdapterInfoGet),
                i2c: proc!(lib, "ADL2_Display_WriteAndReadI2C", WriteAndReadI2c),
                i2c_mutex,
                lib,
            })
        }
    }

    pub fn adapters(&self) -> Result<Vec<AdapterInfo>, String> {
        unsafe {
            let mut n = 0;
            let rc = (self.number_of_adapters)(self.ctx, &mut n);
            if rc != ADL_OK || n <= 0 {
                return Err(format!("no ADL adapters ({rc})"));
            }
            let mut list: Vec<AdapterInfo> = (0..n).map(|_| std::mem::zeroed()).collect();
            for a in &mut list {
                a.size = size_of::<AdapterInfo>() as c_int;
            }
            let bytes = (size_of::<AdapterInfo>() * list.len()) as c_int;
            let rc = (self.adapter_info)(self.ctx, list.as_mut_ptr(), bytes);
            if rc != ADL_OK {
                return Err(format!("ADL2_Adapter_AdapterInfo_Get failed ({rc})"));
            }
            Ok(list)
        }
    }

    /// Reads one byte from `register` of the device at `address` on I2C `line`.
    pub fn read_byte(&self, adapter: c_int, line: c_int, address: c_int, register: c_int) -> Result<u8, c_int> {
        let mut byte = 0u8;
        let mut req = AdlI2c {
            size: size_of::<AdlI2c>() as c_int,
            line,
            address,
            offset: register,
            action: I2C_ACTION_READ_REPEATED_START,
            speed: 150,
            data_size: 1,
            data: (&mut byte as *mut u8).cast(),
        };
        let _guard = self.lock_i2c();
        let rc = unsafe { (self.i2c)(self.ctx, adapter, &mut req) };
        if rc == ADL_OK {
            Ok(byte)
        } else {
            Err(rc)
        }
    }

    fn lock_i2c(&self) -> Option<I2cGuard> {
        if self.i2c_mutex.is_null() {
            return None;
        }
        // If another tool holds the bus for too long, go ahead anyway rather than stall forever.
        match unsafe { WaitForSingleObject(self.i2c_mutex, 500) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Some(I2cGuard(self.i2c_mutex)),
            _ => None,
        }
    }
}

struct I2cGuard(HANDLE);

impl Drop for I2cGuard {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0) };
    }
}

impl Drop for Adl {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.ctx);
            if !self.i2c_mutex.is_null() {
                CloseHandle(self.i2c_mutex);
            }
            FreeLibrary(self.lib);
        }
    }
}
