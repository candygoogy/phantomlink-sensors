"""Find out how TriXX interprets values of a register (default: status register 0x64).

Starts TriXX under Frida (run elevated), opens its Power Protection tab, then makes TriXX's
reads of the register return each test value in turn and screenshots the status panel for each,
stacked into fake_status_<reg><tag>.png. Only TriXX's view of the value is changed; nothing is
written to the card. TriXX is closed afterwards.

    python -I fake_status.py [register-hex] [values-hex,comma,separated] [tag]
"""
import ctypes as C
import ctypes.wintypes as W
import sys
import time
from pathlib import Path

import frida
from PIL import Image, ImageDraw

HERE = Path(__file__).parent
TRIXX = r"C:\Program Files (x86)\Sapphire TRIXX\TRIXX.exe"
REGISTER = int(sys.argv[1], 16) if len(sys.argv) > 1 else 0x64
VALUES = [int(v, 16) for v in sys.argv[2].split(",")] if len(sys.argv) > 2 else [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF]
TAG = sys.argv[3] if len(sys.argv) > 3 else ""

AGENT = r"""
let fake = -1, reg = -1;
rpc.exports = { set(r, v) { reg = r; fake = v; } };
function hook(mod, name, structArg) {
  let addr; try { addr = mod.getExportByName(name); } catch (e) { return; }
  Interceptor.attach(addr, {
    onEnter(args) { this.p = args[structArg]; },
    onLeave() {
      const p = this.p;
      if (fake >= 0 && p.add(12).readS32() === reg && p.add(8).readS32() === 0x50) p.add(28).readPointer().writeU8(fake);
    },
  });
}
const done = new Set();
setInterval(() => {
  const mod = Process.findModuleByName('atiadlxx.dll');
  if (!mod || done.has('x')) return;
  done.add('x');
  // TriXX calls ADL_ (which forwards to ADL2_); patching the inner one covers both.
  hook(mod, 'ADL2_Display_WriteAndReadI2C', 2);
}, 20);
"""

user32 = C.windll.user32
user32.SetProcessDPIAware()


class RECT(C.Structure):
    _fields_ = [("l", C.c_long), ("t", C.c_long), ("r", C.c_long), ("b", C.c_long)]


def main_window(pid):
    found = []

    @C.WINFUNCTYPE(C.c_bool, W.HWND, W.LPARAM)
    def cb(h, _):
        p = W.DWORD()
        user32.GetWindowThreadProcessId(h, C.byref(p))
        r = RECT()
        user32.GetWindowRect(h, C.byref(r))
        if p.value == pid and user32.IsWindowVisible(h) and (r.r - r.l) > 400:
            found.append(h)
        return True

    user32.EnumWindows(cb, 0)
    return found[0] if found else None


def capture(hwnd):
    user32.ShowWindow(hwnd, 4)  # SW_SHOWNOACTIVATE
    r = RECT()
    user32.GetWindowRect(hwnd, C.byref(r))
    w, h = r.r - r.l, r.b - r.t
    gdi = C.windll.gdi32
    hdc = user32.GetWindowDC(hwnd)
    mdc = gdi.CreateCompatibleDC(hdc)
    bmp = gdi.CreateCompatibleBitmap(hdc, w, h)
    gdi.SelectObject(mdc, bmp)
    user32.PrintWindow(hwnd, mdc, 2)
    buf = C.create_string_buffer(w * h * 4)
    gdi.GetBitmapBits(bmp, len(buf), buf)
    gdi.DeleteObject(bmp); gdi.DeleteDC(mdc); user32.ReleaseDC(hwnd, hdc)
    return Image.frombuffer("RGBA", (w, h), buf.raw, "raw", "BGRA", 0, 1).convert("RGB")


def open_power_tab(pid):
    import subprocess
    ps = ("Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes; "
          "$A=[Windows.Automation.AutomationElement]; $W=[Windows.Automation.TreeWalker]::ControlViewWalker; "
          f"$root=$A::FromHandle((Get-Process -Id {pid}).MainWindowHandle); "
          "$e=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition) | "
          "? { $_.Current.Name -match 'POWER PROTECTION' } | Select-Object -First 1; "
          "$t=$W.GetParent($e); $t.GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select()")
    subprocess.run(["powershell", "-NoProfile", "-Command", ps], check=False)


def main():
    pid = frida.spawn(TRIXX)
    session = frida.attach(pid)
    script = session.create_script(AGENT)
    script.load()
    frida.resume(pid)
    time.sleep(12)
    open_power_tab(pid)
    time.sleep(2)
    hwnd = main_window(pid)
    shots = []
    for v in VALUES:
        script.exports_sync.set(REGISTER, v)
        time.sleep(4)
        img = capture(hwnd)
        W_, H_ = img.size
        # status panel: roughly the band from "Temperature" to "Current" rows
        shots.append((v, img.crop((0, int(H_ * 0.30), int(W_ * 0.75), int(H_ * 0.56)))))
    script.exports_sync.set(REGISTER, -1)
    # Detaching from TriXX can hang; we are done with it anyway.
    import subprocess
    subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True)
    sheet = Image.new("RGB", (shots[0][1].width, sum(s.height for _, s in shots)), "white")
    y = 0
    for v, s in shots:
        sheet.paste(s, (0, y))
        ImageDraw.Draw(sheet).text((s.width - 160, y + 10), f"0x{REGISTER:02x} = 0x{v:02x}", fill="yellow")
        y += s.height
    out = HERE / f"fake_status_{REGISTER:02x}{TAG}.png"
    sheet.save(out)
    print(out, flush=True)
    import os
    os._exit(0)


if __name__ == "__main__":
    main()
