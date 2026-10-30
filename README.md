# phantomlink-sensors

Shows the power connector temperatures of a Sapphire NITRO+ RX 9070 XT PhantomLink in
[HWiNFO](https://www.hwinfo.com/), the same readings Sapphire TriXX shows under **Power Protection**.

```
∨ Sapphire Power Protection
    12V-2x6 Connector     41.0 °C
    GC-HPWR Connector     44.0 °C
    Temperature Danger    No
    Current Danger        No
```

Once they are in HWiNFO, you can graph them, log them, set alerts, put them in the tray or in the RTSS
on-screen display like any other sensor. It only needs the AMD Adrenalin driver; TriXX doesn't need to be
installed or running.

Unofficial. Not affiliated with or endorsed by Sapphire or HWiNFO.

## Supported cards

The tool runs on the cards Sapphire TriXX 11.2 enables Power Protection for (PCI device `1002:7550`, Navi 48):

| Subsystem | Revision | Card |
|---|---|---|
| `1DA2:E501` | C0 | NITRO+ RX 9070 XT PhantomLink (tested) |
| `1DA2:501E` | C0 | listed by TriXX next to E501 |
| `1DA2:4892` | C0 / C3 | listed by TriXX, model unknown (RX 9070 XT / RX 9070) |

Run `phantomlink-sensors --list` to see your card's IDs. Other Sapphire RX 9070 / 9070 XT cards, such as
the plain NITRO+ (`E489`), may have the same sensor chip; you can try with `--force`. If you do, please
open an issue with the output of `--list` and `--once`, whether it works or not.

## Install

1. Download `phantomlink-sensors.exe` from the [latest release](../../releases/latest).
2. In a terminal in the download folder, check that it can read your card:
   ```
   phantomlink-sensors --once
   ```
3. Install it:
   ```
   phantomlink-sensors --install
   ```
   This copies it to `%LOCALAPPDATA%\Programs\phantomlink-sensors`, adds a Task Scheduler task that starts it
   at every logon, and starts it now. It runs with normal user rights (no admin, no UAC prompt) and has no
   window. It does not appear under Task Manager > Startup apps; you will find it in Task Scheduler.
4. Restart HWiNFO once. HWiNFO looks for custom sensors only when it starts.

The sensor appears near the bottom of HWiNFO's sensor list. HWiNFO always puts custom sensors after its
own while the **Fixed Order** layout setting is on (the default). You can turn Fixed Order off and drag it
up under your GPU, but that also turns off HWiNFO's collapsible groups (such as GPU Memory Temperatures).

To remove everything (the task, the installed copy and the HWiNFO sensor):

```
phantomlink-sensors --uninstall
```

## Usage

```
phantomlink-sensors              run in the background and update HWiNFO every 2 s
phantomlink-sensors --install    copy to %LOCALAPPDATA%\Programs, start it at every logon, and start it now
phantomlink-sensors --uninstall  stop it and remove the logon task, the copy and the HWiNFO sensor
phantomlink-sensors --stop       stop the background instance
phantomlink-sensors --once       print the current readings and exit
phantomlink-sensors --list       list AMD adapters as ADL sees them
phantomlink-sensors --remove     delete the HWiNFO custom sensor entries
phantomlink-sensors --version    print the version

  --interval <ms>   polling interval (default 2000)
  --adapter <n>     use this ADL adapter index instead of auto-detecting
  --force           run on a Sapphire RX 9070 / 9070 XT that is not on the supported list
```

## What the readings mean

| HWiNFO entry | Meaning |
|---|---|
| 12V-2x6 Connector | Temperature at the 12V-2x6 (16-pin) power connector, °C |
| GC-HPWR Connector | Temperature at the PhantomLink GC-HPWR connector, °C (warm even when unused, it sits near the VRM) |
| Temperature Danger | Yes when the card reports a connector over-temperature (TriXX: Temperature DANGER) |
| Current Danger | Yes when the card reports an over-current condition (TriXX: Current DANGER) |

The Danger entries follow TriXX exactly: the card's status register reads 0x01 or 0x10 for
over-temperature and 0x11 for over-current. Sapphire says the card's own protection works regardless of
any software; this tool only reports.

## How it works

The card has a small controller on its I2C bus (line 1, address 0x50) that measures the connectors.
The tool reads three of its registers through AMD's driver API (`ADL2_Display_WriteAndReadI2C`), the same
reads TriXX makes, and writes the values to HWiNFO's
[custom sensor](https://www.hwinfo.com/forum/threads/custom-user-sensors-in-hwinfo.5817/) registry keys
(`HKCU\Software\HWiNFO64\Sensors\Custom`).

- It only reads. It never writes to the card.
- It takes the `Global\Access_ATI_I2C` lock around each read, so it doesn't collide with TriXX, HWiNFO or
  other monitoring tools on the same bus.
- It refuses to run on cards that are not on the list above unless you pass `--force`, because address
  0x50 can be a different chip on other boards.

The registers were found by logging TriXX's driver calls and checking them against its Power Protection
page. Register notes, measurements and the scripts used are in [`research/`](research/). The controller
also has channels that follow the 12V-2x6 load (likely per-pin current), but their scale is not known yet.
Help welcome.

## Build

```
cargo build --release
```

Windows only, Rust stable, target `x86_64-pc-windows-msvc`. Releases are built by GitHub Actions from the
tagged source ([`release.yml`](.github/workflows/release.yml)), with a SHA-256 checksum next to the exe.
The exe is not code-signed, so SmartScreen may warn on first run.

## License

[MIT](LICENSE)
