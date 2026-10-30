"""Sample the candidate current registers next to HWiNFO's own GPU power/current/voltage readings.

    python -I correlate.py [seconds]
Needs HWiNFO running with Shared Memory Support on. Writes correlate.csv next to this script.
"""
import csv, ctypes as C, mmap, struct, sys, time
from pathlib import Path

SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 60
OUT = Path(__file__).with_name("correlate.csv")
REGS = list(range(0x50, 0x60)) + list(range(0x6E, 0x7A))


class ADLI2C(C.Structure):
    _fields_ = [("iSize", C.c_int), ("iLine", C.c_int), ("iAddress", C.c_int), ("iOffset", C.c_int),
                ("iAction", C.c_int), ("iSpeed", C.c_int), ("iDataSize", C.c_int), ("pcData", C.c_char_p)]


_malloc = C.WINFUNCTYPE(C.c_void_p, C.c_int)(lambda n: C.cdll.msvcrt.malloc(n))
adl = C.CDLL("atiadlxx.dll")
ctx = C.c_void_p()
assert adl.ADL2_Main_Control_Create(_malloc, 1, C.byref(ctx)) == 0
k32 = C.windll.kernel32
k32.CreateMutexW.restype = C.c_void_p
mutex = C.c_void_p(k32.CreateMutexW(None, False, r"Global\Access_ATI_I2C"))


def reg(r):
    buf = C.create_string_buffer(1)
    req = ADLI2C(C.sizeof(ADLI2C), 1, 0x50, r, 3, 150, 1, C.cast(buf, C.c_char_p))
    k32.WaitForSingleObject(mutex, 500)
    try:
        rc = adl.ADL2_Display_WriteAndReadI2C(ctx, 0, C.byref(req))
    finally:
        k32.ReleaseMutex(mutex)
    return buf.raw[0] if rc == 0 else -1


def hwinfo_gpu():
    tag = r"Global\HWiNFO_SENS_SM2"
    hdr = mmap.mmap(-1, 44, tagname=tag, access=mmap.ACCESS_READ)
    _, _, _, _, so, ss, sn, ro, rs, rn = struct.unpack_from("<4sIIqIIIIII", hdr, 0)
    m = mmap.mmap(-1, ro + rs * rn, tagname=tag, access=mmap.ACCESS_READ)
    name = lambda o: m[o:o + 128].split(b"\0")[0].decode("latin1")
    gpu = [i for i in range(sn) if "RX 9070" in name(so + i * ss + 8)]
    out = {}
    for i in range(rn):
        o = ro + i * rs
        t, si = struct.unpack_from("<II", m, o)
        if si in gpu and t in (2, 4, 5):  # volt, current, power
            out[name(o + 12)] = round(struct.unpack_from("<d", m, o + 284)[0], 3)
    return out


first = hwinfo_gpu()
print("HWiNFO GPU readings:", list(first))
with OUT.open("w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["t"] + [f"r{r:02x}" for r in REGS] + list(first))
    end = time.time() + SECONDS
    while time.time() < end:
        hw = hwinfo_gpu()
        w.writerow([f"{time.time():.1f}"] + [reg(r) for r in REGS] + [hw.get(k, "") for k in first])
        f.flush()
        time.sleep(0.5)
print("->", OUT)
