"""Attach to a running TRIXX.exe and log every ADL I2C transaction it makes.

Run from an elevated terminal while TriXX is open on the Power Protection tab:
    <venv>\\Scripts\\python.exe sniff_i2c.py [seconds]
Writes i2c_log.jsonl next to this script.
"""
import json
import sys
import time
from pathlib import Path

import frida

SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 45
LOG = Path(__file__).with_name("i2c_startup.jsonl" if "--spawn" in sys.argv else "i2c_log.jsonl")

AGENT = r"""
// ADLI2C on 32-bit: iSize, iLine, iAddress, iOffset, iAction, iSpeed, iDataSize, char* pcData
function readI2C(p) {
  return {
    size: p.readS32(), line: p.add(4).readS32(), address: p.add(8).readS32(),
    offset: p.add(12).readS32(), action: p.add(16).readS32(), speed: p.add(20).readS32(),
    dataSize: p.add(24).readS32(), data: p.add(28).readPointer(),
  };
}
function hex(ptr, n) {
  if (ptr.isNull() || n <= 0 || n > 4096) return null;
  return Array.from(new Uint8Array(ptr.readByteArray(n)), b => b.toString(16).padStart(2, '0')).join(' ');
}
function hook(mod, name, structArg) {
  let addr;
  try { addr = mod.getExportByName(name); } catch (e) { return; }
  send({ kind: 'hooked', module: mod.name, name });
  Interceptor.attach(addr, {
    onEnter(args) {
      this.adapter = args[structArg - 1].toInt32();
      this.p = args[structArg];
      this.req = readI2C(this.p);
      this.tid = Process.getCurrentThreadId();
      // action 2 = write: capture the outgoing bytes before the call
      this.before = hex(this.req.data, this.req.dataSize);
    },
    onLeave(ret) {
      const r = this.req;
      send({ kind: 'i2c', fn: name, t: Date.now(), tid: this.tid, adapter: this.adapter,
             line: r.line, address: r.address, offset: r.offset, action: r.action,
             speed: r.speed, dataSize: r.dataSize, before: this.before,
             after: hex(r.data, r.dataSize), ret: ret.toInt32() });
    },
  });
}
function hookAll() {
  for (const m of ['atiadlxy.dll', 'atiadlxx.dll']) {
    const mod = Process.findModuleByName(m);
    if (!mod || hooked.has(m)) continue;
    hooked.add(m);
    hook(mod, 'ADL_Display_WriteAndReadI2C', 1);
    hook(mod, 'ADL2_Display_WriteAndReadI2C', 2);
  }
}
const hooked = new Set();
hookAll();
setInterval(hookAll, 20);
send({ kind: 'ready', arch: Process.arch, modules: Process.enumerateModules().map(m => m.name).filter(n => /adl|i2c|ati/i.test(n)) });
"""


def main():
    spawn = "--spawn" in sys.argv
    if spawn:
        pid = frida.spawn(r"C:\Program Files (x86)\Sapphire TRIXX\TRIXX.exe")
        session = frida.attach(pid)
    else:
        session = frida.attach("TRIXX.exe")
    out = LOG.open("a", encoding="utf-8")
    seen = 0

    def on_message(msg, _data):
        nonlocal seen
        if msg["type"] != "send":
            print(msg)
            return
        p = msg["payload"]
        out.write(json.dumps(p) + "\n")
        out.flush()
        if p["kind"] == "i2c":
            seen += 1
            if seen <= 40:
                print(f"line={p['line']} addr=0x{p['address']:02x} off=0x{p['offset']:02x} "
                      f"act={p['action']} n={p['dataSize']} before=[{p['before']}] after=[{p['after']}] ret={p['ret']}")
        else:
            print(p)

    script = session.create_script(AGENT)
    script.on("message", on_message)
    script.load()
    if spawn:
        frida.resume(pid)
    print(f"Logging for {SECONDS}s -> {LOG}")
    time.sleep(SECONDS)
    session.detach()
    print(f"Done, {seen} I2C calls captured.")


if __name__ == "__main__":
    main()
