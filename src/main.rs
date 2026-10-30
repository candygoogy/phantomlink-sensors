//! Reads the power connector sensors on a Sapphire NITRO+ RX 9070 XT (the ones TriXX shows
//! under Power Protection) and publishes them to HWiNFO as custom sensors.
//!
//! The values live in a small controller on the card at I2C line 1, address 0x50. TriXX 11.2
//! reads these three registers once a second; see README.md for how they were found.

#![windows_subsystem = "windows"]

mod adl;
mod dialog;
mod hwinfo;
mod install;
mod stop;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

use adl::{AdapterInfo, Adl};

const I2C_LINE: i32 = 1;
const I2C_ADDRESS: i32 = 0x50;
const REG_TEMP_12V_2X6: i32 = 0x51;
const REG_TEMP_GC_HPWR: i32 = 0x60;
/// A status code, not a bit field. TriXX 11.2 shows Temperature DANGER for 0x01 and 0x10 and
/// Current DANGER for 0x11; everything else is GOOD.
const REG_STATUS: i32 = 0x64;

/// Sapphire Navi 48 cards TriXX 11.2 enables Power Protection for: (subsystem device ID, PCI revision).
/// Navi 48 revision C0 is the RX 9070 XT, C3 the RX 9070.
const SUPPORTED: &[(&str, &str)] = &[
    ("E501", "C0"), // NITRO+ RX 9070 XT PhantomLink
    ("501E", "C0"), // listed by TriXX next to E501
    ("4892", "C0"),
    ("4892", "C3"),
];

const USAGE: &str = "\
phantomlink-sensors: Sapphire RX 9070 XT connector temperatures for HWiNFO

  phantomlink-sensors              run in the background and update HWiNFO
  phantomlink-sensors --install    copy to %LOCALAPPDATA%\\Programs, start it at every logon, and start it now
  phantomlink-sensors --uninstall  stop it and remove the logon task, the copy and the HWiNFO sensor
  phantomlink-sensors --stop       stop the background instance
  phantomlink-sensors --once       print the current readings and exit
  phantomlink-sensors --list       list AMD adapters as ADL sees them
  phantomlink-sensors --remove     delete the HWiNFO custom sensor entries
  phantomlink-sensors --version    print the version

  --interval <ms>   polling interval in the background mode (default 2000)
  --adapter <n>     use this ADL adapter index instead of auto-detecting
  --force           run on a Sapphire RX 9070 / 9070 XT that is not on the supported list";

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

struct Args {
    mode: Mode,
    interval: u64,
    adapter: Option<i32>,
    force: bool,
}

#[derive(PartialEq)]
enum Mode {
    Run,
    Once,
    List,
    Remove,
    Version,
    Stop,
    Install,
    Uninstall,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { mode: Mode::Run, interval: 2000, adapter: None, force: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--once" => args.mode = Mode::Once,
            "--list" => args.mode = Mode::List,
            "--remove" => args.mode = Mode::Remove,
            "--version" | "-V" => args.mode = Mode::Version,
            "--stop" => args.mode = Mode::Stop,
            "--install" => args.mode = Mode::Install,
            "--uninstall" => args.mode = Mode::Uninstall,
            "--force" => args.force = true,
            "--interval" => {
                args.interval = it.next().and_then(|v| v.parse().ok()).ok_or("--interval needs milliseconds")?;
                args.interval = args.interval.max(250);
            }
            "--adapter" => args.adapter = Some(it.next().and_then(|v| v.parse().ok()).ok_or("--adapter needs a number")?),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

/// Subsystem device ID and revision from a PNP string like
/// `PCI\VEN_1002&DEV_7550&SUBSYS_E5011DA2&REV_C0\...`, if it is a Sapphire (1DA2) Navi 48.
fn sapphire_navi48_ids(a: &AdapterInfo) -> Option<(String, String)> {
    let pnp = a.pnp().to_ascii_uppercase();
    if !pnp.contains("VEN_1002&DEV_7550") {
        return None;
    }
    let subsys = pnp.split("SUBSYS_").nth(1)?.get(..8)?;
    let rev = pnp.split("&REV_").nth(1)?.get(..2)?;
    (&subsys[4..] == "1DA2").then(|| (subsys[..4].to_string(), rev.to_string()))
}

fn is_supported(ids: &(String, String)) -> bool {
    SUPPORTED.iter().any(|&(s, r)| ids.0 == s && ids.1 == r)
}

fn pick_adapter(adl: &Adl, forced: Option<i32>, force: bool) -> Result<i32, String> {
    if let Some(index) = forced {
        return Ok(index);
    }
    let adapters = adl.adapters()?;
    let sapphire: Vec<_> =
        adapters.iter().filter(|a| a.present != 0).filter_map(|a| sapphire_navi48_ids(a).map(|ids| (a, ids))).collect();
    if let Some((a, _)) = sapphire.iter().find(|(_, ids)| force || is_supported(ids)) {
        return Ok(a.index);
    }
    match sapphire.first() {
        Some((_, (subsys, rev))) => Err(format!(
            "this Sapphire card (subsystem {subsys}, revision {rev}) is not known to have the power protection \
             sensors. If you want to try anyway, run with --force and check that the readings make sense."
        )),
        None => Err("no Sapphire RX 9070 / 9070 XT found (try --list)".into()),
    }
}

struct Reading {
    temp_12v_2x6: u8,
    /// None on cards without a GC-HPWR connector.
    temp_gc_hpwr: Option<u8>,
    status: u8,
}

impl Reading {
    fn temp_danger(&self) -> bool {
        self.status == 0x01 || self.status == 0x10
    }
    fn current_danger(&self) -> bool {
        self.status == 0x11
    }
}

fn read(adl: &Adl, adapter: i32) -> Result<Reading, String> {
    let byte = |reg| {
        adl.read_byte(adapter, I2C_LINE, I2C_ADDRESS, reg)
            .map_err(|rc| format!("I2C read of register 0x{reg:02x} failed (ADL error {rc})"))
    };
    // 0 and 0xFF are what an absent or confused device returns; neither is a real connector temperature.
    let plausible = |t: u8| (1..=150).contains(&t);
    let temp_12v_2x6 = byte(REG_TEMP_12V_2X6)?;
    if !plausible(temp_12v_2x6) {
        return Err(format!("implausible 12V-2x6 temperature {temp_12v_2x6}"));
    }
    let temp_gc_hpwr = Some(byte(REG_TEMP_GC_HPWR)?).filter(|&t| plausible(t));
    Ok(Reading { temp_12v_2x6, temp_gc_hpwr, status: byte(REG_STATUS)? })
}

fn good(danger: bool) -> &'static str {
    if danger {
        "DANGER"
    } else {
        "GOOD"
    }
}

fn run(args: &Args) -> Result<(), String> {
    match args.mode {
        Mode::Version => {
            println!("phantomlink-sensors {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Mode::Remove => {
            hwinfo::remove()?;
            println!("Removed HKCU\\{}. Restart HWiNFO to drop the sensor.", hwinfo::SENSOR_KEY);
            return Ok(());
        }
        Mode::Stop => {
            println!("{}", if stop::stop_running() { "Stopped." } else { "Not running." });
            return Ok(());
        }
        Mode::Uninstall => {
            install::uninstall()?;
            hwinfo::remove()?;
            println!("Uninstalled. Restart HWiNFO to drop the sensor.");
            return Ok(());
        }
        _ => {}
    }

    let adl = Adl::open()?;

    if args.mode == Mode::List {
        for a in adl.adapters()? {
            let note = match sapphire_navi48_ids(&a) {
                Some(ids) if is_supported(&ids) => "  <- supported",
                Some(_) => "  <- Sapphire Navi 48, not on the supported list",
                None => "",
            };
            println!("{:>3}  bus {:<3} present={} {}  {}{note}", a.index, a.bus, a.present, a.name(), a.pnp());
        }
        return Ok(());
    }

    let adapter = pick_adapter(&adl, args.adapter, args.force)?;

    if args.mode == Mode::Install {
        // Make sure the card can actually be read before setting it up to start at every logon.
        read(&adl, adapter)?;
        let mut pass = Vec::new();
        if args.force {
            pass.push("--force".to_string());
        }
        if let Some(a) = args.adapter {
            pass.push(format!("--adapter {a}"));
        }
        if args.interval != 2000 {
            pass.push(format!("--interval {}", args.interval));
        }
        let exe = install::install(&pass.join(" "))?;
        println!("Installed to {} and started.", exe.display());
        println!("It starts at every logon through Task Scheduler (task \"phantomlink-sensors\").");
        println!("Restart HWiNFO once to see the sensor.");
        return Ok(());
    }

    if args.mode == Mode::Once {
        let r = read(&adl, adapter)?;
        println!("adapter {adapter}");
        println!("12V-2x6 connector: {} °C", r.temp_12v_2x6);
        match r.temp_gc_hpwr {
            Some(t) => println!("GC-HPWR connector: {t} °C"),
            None => println!("GC-HPWR connector: not present"),
        }
        println!("temperature:       {}", good(r.temp_danger()));
        println!("current:           {}", good(r.current_danger()));
        println!("status code:       0x{:02x}", r.status);
        return Ok(());
    }

    // Background mode: one instance at a time.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, wide("Local\\phantomlink-sensors").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Err("already running".into());
        }
    }

    // Start from a clean sensor so entries from older versions don't linger.
    let first = read(&adl, adapter)?;
    hwinfo::remove()?;
    let t_12v = hwinfo::Entry::create("Temp0", "12V-2x6 Connector", None)?;
    let t_gc = match first.temp_gc_hpwr {
        Some(_) => Some(hwinfo::Entry::create("Temp1", "GC-HPWR Connector", None)?),
        None => None,
    };
    let temp_danger = hwinfo::Entry::create("Other0", "Temperature Danger", Some("Yes/No"))?;
    let current_danger = hwinfo::Entry::create("Other1", "Current Danger", Some("Yes/No"))?;

    let stop = stop::StopSignal::new();
    let mut reading = Ok(first);
    loop {
        match reading {
            Ok(r) => {
                t_12v.set_value(r.temp_12v_2x6)?;
                if let (Some(entry), Some(t)) = (&t_gc, r.temp_gc_hpwr) {
                    entry.set_value(t)?;
                }
                temp_danger.set_value(r.temp_danger() as u8)?;
                current_danger.set_value(r.current_danger() as u8)?;
            }
            // A missed read now and then (bus busy, driver reset) is not fatal; keep the last values.
            Err(e) => eprintln!("{e}"),
        }
        if stop.wait(args.interval) {
            return Ok(());
        }
        reading = read(&adl, adapter);
    }
}

fn main() {
    // Built as a GUI program so the background mode has no console window; borrow the
    // console of whoever started us so --once and --list can still print. Skip that when
    // output is already redirected, or attaching would point it back at the console.
    let from_terminal = unsafe {
        !GetStdHandle(STD_OUTPUT_HANDLE).is_null() || AttachConsole(ATTACH_PARENT_PROCESS) != 0
    };

    // Double-clicked (no terminal, no arguments, not the copy the logon task starts): ask instead of
    // silently starting in the background.
    if !from_terminal && std::env::args().len() == 1 && !install::is_installed_copy() {
        dialog::run();
        return;
    }

    let result = parse_args().and_then(|args| run(&args));
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
