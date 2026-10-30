"""Step the GPU through load levels with a compute shader while sampling the candidate current
registers (0x50-0x5F) and HWiNFO's board power / 12 V, to calibrate the per-pin channels.

    python -I load_sweep.py [seconds_per_level]
Writes load_sweep.csv next to this script. Needs HWiNFO running with shared memory on.
"""
import csv
import ctypes as C
import mmap
import struct
import sys
import threading
import time
from pathlib import Path

import wgpu

LEVEL_S = float(sys.argv[1]) if len(sys.argv) > 1 else 40
LEVELS = [0.0, 0.15, 0.30, 0.50, 0.70, 0.85, 1.0, 0.0]
OUT = Path(__file__).with_name("load_sweep.csv")

# ---------------------------------------------------------------- GPU load
SHADER = """
@group(0) @binding(0) var<storage, read_write> sink: array<f32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    var a = vec4<f32>(f32(id.x) * 1e-6, 0.5, 0.25, 0.125);
    let b = vec4<f32>(0.9999, 1.0001, 0.99995, 1.00005);
    for (var i = 0u; i < 4096u; i++) { a = fma(a, b, vec4<f32>(1e-7)); }
    sink[id.x] = a.x + a.y + a.z + a.w;
}
"""
adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
device = adapter.request_device_sync()
GROUPS = 4096
buf = device.create_buffer(size=GROUPS * 256 * 4, usage=wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC)
# Reading 16 bytes back blocks until the GPU is done (on_submitted_work_done_sync is broken in wgpu 0.32).
probe = device.create_buffer(size=16, usage=wgpu.BufferUsage.COPY_DST | wgpu.BufferUsage.COPY_SRC)
module = device.create_shader_module(code=SHADER)
pipeline = device.create_compute_pipeline(layout="auto", compute={"module": module, "entry_point": "main"})
bind = device.create_bind_group(layout=pipeline.get_bind_group_layout(0),
                                entries=[{"binding": 0, "resource": {"buffer": buf}}])


def dispatch(n=8):
    enc = device.create_command_encoder()
    p = enc.begin_compute_pass()
    p.set_pipeline(pipeline)
    p.set_bind_group(0, bind)
    for _ in range(n):
        p.dispatch_workgroups(GROUPS)
    p.end()
    enc.copy_buffer_to_buffer(buf, 0, probe, 0, 16)
    device.queue.submit([enc.finish()])
    device.queue.read_buffer(probe)


level = 0.0
running = True


def load_loop():
    # 50 ms periods: busy for level * period, idle for the rest.
    while running:
        start = time.perf_counter()
        if level > 0:
            while time.perf_counter() - start < 0.05 * level:
                dispatch()
        rest = 0.05 - (time.perf_counter() - start)
        if rest > 0:
            time.sleep(rest)


# ---------------------------------------------------------------- sensors
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
    b = C.create_string_buffer(1)
    req = ADLI2C(C.sizeof(ADLI2C), 1, 0x50, r, 3, 150, 1, C.cast(b, C.c_char_p))
    k32.WaitForSingleObject(mutex, 500)
    try:
        rc = adl.ADL2_Display_WriteAndReadI2C(ctx, 0, C.byref(req))
    finally:
        k32.ReleaseMutex(mutex)
    return b.raw[0] if rc == 0 else None


WANT = ["Total Board Power (TBP)", "+12V SOC Input Voltage", "Total Graphics Power (TGP)"]


def hwinfo():
    tag = r"Global\HWiNFO_SENS_SM2"
    hdr = mmap.mmap(-1, 44, tagname=tag, access=mmap.ACCESS_READ)
    _, _, _, _, _, _, _, ro, rs, rn = struct.unpack_from("<4sIIqIIIIII", hdr, 0)
    m = mmap.mmap(-1, ro + rs * rn, tagname=tag, access=mmap.ACCESS_READ)
    out = {}
    for i in range(rn):
        o = ro + i * rs
        name = m[o + 12:o + 140].split(b"\0")[0].decode("latin1")
        if name in WANT and name not in out:
            out[name] = round(struct.unpack_from("<d", m, o + 284)[0], 2)
    return out


dispatch()  # compile and warm up
threading.Thread(target=load_loop, daemon=True).start()
regs = list(range(0x50, 0x60))
with OUT.open("w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["t", "level"] + [f"r{r:02x}" for r in regs] + WANT)
    t0 = time.time()
    for lv in LEVELS:
        level = lv
        end = time.time() + LEVEL_S
        while time.time() < end:
            hw = hwinfo()
            w.writerow([round(time.time() - t0, 1), lv] + [reg(r) for r in regs] + [hw.get(k) for k in WANT])
            f.flush()
            time.sleep(1)
        print(f"level {lv:.2f} done", flush=True)
running = False
print("->", OUT)
