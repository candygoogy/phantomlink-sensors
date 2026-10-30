"""Read-only dump of every register (0x00-0xFF) of the Sapphire power-protection controller
(ADL adapter 0, I2C line 1, address 0x50), repeated, to find values that change with load.

    python -I probe.py <label> [rounds] [interval_s]

Appends rows to probe_<label>.csv: t, round, then one column per register (-1 = read failed).
"""
import csv
import ctypes as C
import sys
import time
from pathlib import Path

LABEL = sys.argv[1] if len(sys.argv) > 1 else "idle"
ROUNDS = int(sys.argv[2]) if len(sys.argv) > 2 else 3
INTERVAL = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
OUT = Path(__file__).with_name(f"probe_{LABEL}.csv")
# Optional 4th argument: register ranges like "50-7f,c0-ff" (default: all 256).
REGS = list(range(256))
if len(sys.argv) > 4:
    REGS = [r for part in sys.argv[4].split(",") for a, b in [part.split("-")] for r in range(int(a, 16), int(b, 16) + 1)]


class ADLI2C(C.Structure):
    _fields_ = [("iSize", C.c_int), ("iLine", C.c_int), ("iAddress", C.c_int), ("iOffset", C.c_int),
                ("iAction", C.c_int), ("iSpeed", C.c_int), ("iDataSize", C.c_int), ("pcData", C.c_char_p)]


MALLOC = C.WINFUNCTYPE(C.c_void_p, C.c_int)
_malloc = MALLOC(lambda n: C.cdll.msvcrt.malloc(n))
adl = C.CDLL("atiadlxx.dll")
ctx = C.c_void_p()
assert adl.ADL2_Main_Control_Create(_malloc, 1, C.byref(ctx)) == 0
kernel32 = C.windll.kernel32
kernel32.CreateMutexW.restype = C.c_void_p
mutex = kernel32.CreateMutexW(None, False, "Global\\Access_ATI_I2C")


def read(reg):
    buf = C.create_string_buffer(1)
    req = ADLI2C(C.sizeof(ADLI2C), 1, 0x50, reg, 3, 150, 1, C.cast(buf, C.c_char_p))
    kernel32.WaitForSingleObject(C.c_void_p(mutex), 500)
    try:
        rc = adl.ADL2_Display_WriteAndReadI2C(ctx, 0, C.byref(req))
    finally:
        kernel32.ReleaseMutex(C.c_void_p(mutex))
    return buf.raw[0] if rc == 0 else -1


new = not OUT.exists()
with OUT.open("a", newline="") as f:
    w = csv.writer(f)
    if new:
        w.writerow(["t", "round"] + [f"r{r:02x}" for r in REGS])
    for i in range(ROUNDS):
        row = [read(r) for r in REGS]
        w.writerow([f"{time.time():.1f}", i] + row)
        f.flush()
        if i == 0 and len(REGS) == 256:
            for base in range(0, 256, 16):
                print(f"{base:02x}: " + " ".join("--" if v < 0 else f"{v:02x}" for v in row[base:base + 16]))
        if i + 1 < ROUNDS:
            time.sleep(INTERVAL)
print(f"{ROUNDS} rounds -> {OUT}")
