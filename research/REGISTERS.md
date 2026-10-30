# Sapphire power-protection controller: register notes

Measured on a NITRO+ RX 9070 XT PhantomLink (PCI 1002:7550, subsystem 1DA2:E501, rev C0), VBIOS
023.008.000.068.000001, Adrenalin 26.10.1, TriXX 11.2, powered by a 12V-2x6 cable (GC-HPWR unused).

## Access

The controller sits on the GPU's I2C bus and is reached through AMD's ADL driver API:

| Field | Value |
|---|---|
| Call | `ADL2_Display_WriteAndReadI2C` (TriXX uses `ADL_Display_WriteAndReadI2C`, which forwards to it) |
| Adapter | ADL adapter index of the card (0 on a single-GPU system) |
| Line | 1 |
| Address | 0x50 (ADL's 8-bit form) |
| Action | 3 (read with repeated start: write the register offset, then read) |
| Speed | 150 |
| Size | 1 byte per register |

Take the `Global\Access_ATI_I2C` mutex around each transaction; TriXX and HWiNFO use it to share the bus.
Everything here was only ever read. Nothing was written to the controller.

## Registers TriXX 11.2 uses

Polled about once a second while TriXX runs:

| Reg | Meaning |
|---|---|
| 0x51 | 12V-2x6 connector temperature, °C |
| 0x60 | GC-HPWR connector temperature, °C |
| 0x64 | Status code (see below) |

Status code 0x64, as TriXX interprets it. These are exact values, not bits; 0x03, 0x05, 0x09 etc. all show GOOD:

| Value | TriXX shows |
|---|---|
| 0x00 | Temperature GOOD, Current GOOD |
| 0x01, 0x10 | Temperature DANGER |
| 0x11 | Current DANGER |
| anything else | GOOD |

A plausible reading, unconfirmed: low nibble = 12V-2x6, high nibble = GC-HPWR, 1 = over-temperature, and
both nibbles at 1 = over-current.

Read once at startup by TriXX (meaning unknown, probably configuration for other TriXX pages): 0x0F–0x13
(`00 07 1E 01 01`), 0x15 (0x64), 0x16 (0xC8), 0x1A–0x1C (`FF`), 0x20–0x21 (`00 01`), 0x3E (0x0A).

## Cards TriXX 11.2 enables Power Protection for

All PCI device 0x7550 (Navi 48), subsystem vendor 0x1DA2 (Sapphire):

| Subsystem | Revision | Notes |
|---|---|---|
| E501 | C0 | NITRO+ RX 9070 XT PhantomLink (tested) |
| 501E | C0 | listed next to E501 |
| 4892 | C0 | unknown model (C0 = RX 9070 XT) |
| 4892 | C3 | unknown model (C3 = RX 9070) |

The plain NITRO+ RX 9070 XT (E489) is not on TriXX 11.2's list, although owners reported connector
temperatures in TriXX 11.1.3. Untested.

## Other registers seen changing (not used by TriXX)

Full dumps of 0x00–0xFF at idle and while gaming, plus a side-by-side capture with HWiNFO's board power
(`probe.py`, `correlate.py`):

| Reg | Idle (≈42 W board power) | Gaming (≈330 W) | Notes |
|---|---|---|---|
| 0x54 | 0 | 0 | |
| 0x55–0x59 | 3, 3, 3, 3, 3 | 9–10 each | Rise and fall with load, values nearly equal across the five. Likely per-pin (or per-pin-group) current on the 12V-2x6 connector, but not linear in amps: total 12 V current went from 3.5 A to 27.2 A (×7.8) while these went ×3.2. See the load sweep below. |
| 0x5A–0x5F | 1 each | 1 each (one blip to 2) | Flat on this card. Possibly the GC-HPWR side, which carries no current here. |
| 0x62 | toggles 0/1 | toggles 0/1 | Heartbeat? |
| 0x6E–0x76 | changing | changing | Values move in steps of 60 (mod 256); low bytes of a counter or raw ADC readings? |
| 0x77–0x79 | 0x56 | 0x56 | |
| 0x00–0x0E | toggle 0x00/0x10 | toggle 0x00/0x10 | |

### Load sweep of 0x55–0x59

`load_sweep.py` stepped the GPU through compute load levels (50 s each, averages over the last 25 s),
next to HWiNFO's board power. The last row is the gaming capture from `correlate.py`:

| Load | Board power | 12 V current | Each of 0x55–0x59 |
|---|---|---|---|
| idle | 44 W | 3.6 A | 3 |
| 15 % | 76 W | 6.3 A | 3 |
| 30 % | 62 W | 5.2 A | 3 |
| 50 % | 100 W | 8.3 A | 3 |
| 70 % | 187 W | 15.7 A | 5–6 |
| 85 % | 189 W | 15.8 A | 6 |
| 100 % | 203 W | 17.0 A | 6–7 |
| idle, right after the load | 57 W | 4.8 A | 3.7 (still falling) |
| gaming | 325 W | 27.2 A | 9–10 |

The channels follow load but are not a usable amp reading: they sit at 3 up to about 100 W, they lag
behind the load and fall slowly after it stops (a long average, perhaps for overload protection), and a
straight-line fit against the 12 V current is off by up to 3 A. Board power also includes up to 75 W
through the PCIe slot, which the connector channels don't see. The five channels always read within one
step of each other, consistent with current spread evenly over the pins, so a single channel standing out
might indicate a bad pin; that is untested.

What would help: dumps from other cards on the list, and the same sweep on a card with a GPU power
reading that separates slot and connector.

## Scripts

All need an elevated terminal (TriXX and ADL I2C access) and Python 3 with `frida` and `pillow` where used.

| Script | What it does |
|---|---|
| `sniff_i2c.py` | Attaches to TriXX and logs every ADL I2C call it makes (`--spawn` to catch startup). |
| `probe.py` | Reads all (or selected) controller registers repeatedly into a CSV. Read-only. |
| `correlate.py` | Samples the candidate current registers next to HWiNFO's GPU power readings (needs HWiNFO shared memory). |
| `fake_status.py` | Makes TriXX see chosen values for a register and screenshots its Power Protection panel for each. |
| `load_sweep.py` | Steps the GPU through compute load levels (wgpu) while sampling 0x50–0x5F and HWiNFO's board power. |
