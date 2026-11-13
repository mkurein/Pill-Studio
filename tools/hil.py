#!/usr/bin/env python3
"""
Hardware-in-the-loop tool for the primGPT firmware on two boards:
STM32F103C8T (Blue Pill) and STM32F411CEU6 (Black Pill). The board is picked
from the linker script in the repo root, or env HIL_BOARD=f103|f411.

Lets a human or an AI agent build, flash, talk to and inspect the real board,
and run a full self-check with a machine-readable verdict.

  .venv/bin/python tools/hil.py probe            # what is connected?
  .venv/bin/python tools/hil.py test             # build + flash + full test suite
  .venv/bin/python tools/hil.py test --no-flash  # test what is already on the chip
  .venv/bin/python tools/hil.py ping | status | led on|off|auto | fram 0|1|2
  .venv/bin/python tools/hil.py log 5            # text log (USART3 / USART6) for 5 s
  .venv/bin/python tools/hil.py diag | regs | gpio | read <symbol|0xaddr> [words]

Physical channels:
  SWD (ST-Link + OpenOCD) : flash, CPU registers, RAM (g_diag), GPIO registers
  USART1 38400 (PA9/PA10) : binary protocol, see Core/Inc/DataFile.h
  log UART 115200         : text log (BOOT/FRAM/DIAG/FAULT lines)
                            F103: USART3 TX PB10, F411: USART6 TX PA11

Ports come from tools/hil.json, env HIL_PROTO_PORT / HIL_LOG_PORT, or
`hil.py detect` (writes tools/hil.json). Exit code 0 = all checks passed.
"""

import argparse
import glob
import json
import os
import re
import struct
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BOARDS = {
    # GPIO register offsets: F1 = CRL, CRH, IDR, ODR; F4 = MODER, OTYPER, OSPEEDR, PUPDR, IDR, ODR
    "f103": {"name": "Blue Pill STM32F103C8T6", "elf": "primGPT.elf", "target": "target/stm32f1x.cfg",
             "gpioc": 0x40011000, "idr": 0x08, "odr": 0x0C, "cfg_reg": ("GPIOC_CRH", 0x04),
             "dev_id": 0x410, "log_uart": "USART3 (PB10)"},
    "f411": {"name": "Black Pill STM32F411CEU6", "elf": "primGPT_F411.elf", "target": "target/stm32f4x.cfg",
             "gpioc": 0x40020800, "idr": 0x10, "odr": 0x14, "cfg_reg": ("GPIOC_MODER", 0x00),
             "dev_id": 0x431, "log_uart": "USART6 (PA11)"},
}


def pick_board():
    env = os.environ.get("HIL_BOARD", "").lower()
    if env in BOARDS:
        return env
    return "f411" if os.path.exists(os.path.join(ROOT, "STM32F411CEUX_FLASH.ld")) else "f103"


BOARD_ID = pick_board()
BOARD = BOARDS[BOARD_ID]
ELF = os.path.join(ROOT, "build", BOARD["elf"])
CFG_FILE = os.path.join(ROOT, "tools", "hil.json")

PROTO_BAUD = 38400
LOG_BAUD = 115200

# --- protocol (keep in sync with Core/Inc/DataFile.h) -------------------------
ID_BU = 0x01
CMD_PING, CMD_FRAM, CMD_STATUS, CMD_RESET, CMD_LED, CMD_LED_GET = 0x01, 0x02, 0x03, 0x0F, 0x70, 0x71
ST_NAMES = {0x00: "OK", 0x01: "FAIL", 0x02: "BAD_ARG", 0xE1: "BAD_CRC", 0xE2: "UNKNOWN_CMD", 0xE3: "OVERFLOW"}

STATUS_FMT = "<6I3H6I2B"
STATUS_FIELDS = ["uptime_ms", "reset_cause", "boot_count", "heartbeat", "heap_free", "heap_min",
                 "stack_free_default", "stack_free_led", "stack_free_uart",
                 "rx_frames", "rx_crc_err", "uart_err", "queue_full", "fram_ok", "fram_fail",
                 "led_odr", "led_manual"]

# g_diag layout (keep in sync with Core/Inc/diag.h)
DIAG_FIELDS = ["magic", "boot_count", "reset_cause", "heartbeat",
               "fault_magic", "error_code", "fault_pc", "fault_lr", "fault_cfsr", "fault_hfsr",
               "fault_bfar", "fault_mmfar", "fault_task0", "fault_task1", "fault_task2", "fault_task3",
               "rx_frames", "rx_crc_err", "rx_overflow", "uart_err", "queue_full", "last_cmd",
               "fram_ok", "fram_fail"]
DIAG_ERR = {0: "NONE", 1: "Error_Handler", 2: "HardFault", 3: "MemManage", 4: "BusFault",
            5: "UsageFault", 6: "StackOverflow", 7: "MallocFailed"}

GPIOC_BASE = BOARD["gpioc"]


def crc16(data: bytes) -> int:
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


def frame(*payload: int) -> bytes:
    body = bytes(payload)
    c = crc16(body)
    return body + bytes([c & 0xFF, c >> 8])


# --- config -------------------------------------------------------------------
def load_cfg():
    cfg = {"proto_port": None, "log_port": None,
           "openocd": ["-f", "interface/stlink.cfg", "-c", "transport select hla_swd",
                       "-f", BOARD["target"]]}
    if os.path.exists(CFG_FILE):
        with open(CFG_FILE) as f:
            cfg.update(json.load(f))
    cfg["proto_port"] = os.environ.get("HIL_PROTO_PORT", cfg["proto_port"])
    cfg["log_port"] = os.environ.get("HIL_LOG_PORT", cfg["log_port"])
    return cfg


def serial_ports():
    pats = ["/dev/cu.usbserial*", "/dev/cu.usbmodem*", "/dev/cu.wchusbserial*", "/dev/cu.SLAB*",
            "/dev/ttyUSB*", "/dev/ttyACM*"]
    return sorted({p for pat in pats for p in glob.glob(pat)})


def open_serial(port, baud, timeout=0.2):
    import serial  # pyserial
    return serial.Serial(port, baud, timeout=timeout)


# --- SWD via OpenOCD ----------------------------------------------------------
class SwdError(RuntimeError):
    pass


def openocd(cfg, commands, timeout=40):
    """Run one OpenOCD session executing `commands`; returns combined output."""
    args = ["openocd"] + cfg["openocd"] + ["-c", "init"]
    for c in commands:
        args += ["-c", c]
    args += ["-c", "shutdown"]
    try:
        p = subprocess.run(args, capture_output=True, text=True, timeout=timeout, cwd=ROOT)
    except subprocess.TimeoutExpired:
        raise SwdError("openocd timeout")
    out = p.stdout + p.stderr
    if "unable to connect to the target" in out or "init mode failed" in out:
        raise SwdError("ST-Link found but target does not answer on SWD "
                       "(check SWDIO/SWCLK/GND wiring, power, BOOT0=0; "
                       "try --under-reset with NRST wired). OpenOCD: " + last_error(out))
    if "open failed" in out or "No ST-Link" in out or "LIBUSB_ERROR" in out:
        raise SwdError("ST-Link not found: " + last_error(out))
    return out


def last_error(out):
    errs = [l for l in out.splitlines() if l.startswith("Error")]
    return errs[-1] if errs else out.strip().splitlines()[-1] if out.strip() else "?"


def parse_mdw(out):
    words = {}
    for m in re.finditer(r"^0x([0-9a-fA-F]+):((?:\s+[0-9a-fA-F]{8})+)", out, re.M):
        addr = int(m.group(1), 16)
        for i, w in enumerate(m.group(2).split()):
            words[addr + 4 * i] = int(w, 16)
    return words


def read_words(cfg, addr, count):
    out = openocd(cfg, [f"mdw 0x{addr:08x} {count}"])
    w = parse_mdw(out)
    try:
        return [w[addr + 4 * i] for i in range(count)]
    except KeyError:
        raise SwdError("memory read failed: " + last_error(out))


def symbol(name):
    if not os.path.exists(ELF):
        raise SystemExit("no ELF, run `make` first")
    out = subprocess.run(["arm-none-eabi-nm", "-S", ELF], capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = line.split()
        if parts[-1] == name:
            size = int(parts[1], 16) if len(parts) == 4 else 4
            return int(parts[0], 16), size
    raise SystemExit(f"symbol {name} not found in ELF")


def read_diag(cfg):
    addr, size = symbol("g_diag")
    words = read_words(cfg, addr, size // 4)
    d = dict(zip(DIAG_FIELDS, words))
    task = b"".join(struct.pack("<I", d.pop(f"fault_task{i}")) for i in range(4))
    d["fault_task"] = task.split(b"\0")[0].decode(errors="replace")
    d["valid"] = d["magic"] == 0xD1A60002
    d["fault_recorded"] = d["fault_magic"] == 0xFA017ED
    d["error_name"] = DIAG_ERR.get(d["error_code"], "?")
    return d


def addr2line(pc):
    out = subprocess.run(["arm-none-eabi-addr2line", "-f", "-C", "-e", ELF, hex(pc)],
                         capture_output=True, text=True).stdout.split()
    return " ".join(out)


def read_regs(cfg):
    out = openocd(cfg, ["halt", "reg pc", "reg lr", "reg sp", "reg xPSR",
                        "mdw 0xE000ED28 1", "mdw 0xE000ED2C 1", "resume"])
    regs = {}
    for name in ["pc", "lr", "sp", "xPSR"]:
        m = re.search(rf"^{name} \(/32\): 0x([0-9a-fA-F]+)", out, re.M)
        if m:
            regs[name] = int(m.group(1), 16)
    w = parse_mdw(out)
    regs["CFSR"] = w.get(0xE000ED28)
    regs["HFSR"] = w.get(0xE000ED2C)
    if "pc" in regs and os.path.exists(ELF):
        regs["pc_where"] = addr2line(regs["pc"])
    return regs


def read_gpio(cfg):
    c = read_words(cfg, GPIOC_BASE, 6)
    w = lambda off: c[off // 4]
    odr, idr = w(BOARD["odr"]), w(BOARD["idr"])
    name, off = BOARD["cfg_reg"]
    return {"PC13_odr": (odr >> 13) & 1, "PC13_idr": (idr >> 13) & 1,
            "PC15_odr": (odr >> 15) & 1, name: w(off)}


# --- UART protocol ------------------------------------------------------------
class Proto:
    def __init__(self, port):
        self.s = open_serial(port, PROTO_BAUD, timeout=0.05)

    def close(self):
        self.s.close()

    def xfer_raw(self, data: bytes, wait=0.5):
        self.s.reset_input_buffer()
        self.s.write(data)
        buf, t0, last = b"", time.time(), None
        # reply is complete when the line is idle for 60 ms
        while time.time() - t0 < wait:
            chunk = self.s.read(256)
            if chunk:
                buf += chunk
                last = time.time()
            elif last and time.time() - last > 0.06:
                break
        return buf

    def cmd(self, *payload, wait=0.5):
        raw = self.xfer_raw(frame(*payload), wait)
        return decode_reply(raw)


def decode_reply(raw: bytes):
    r = {"raw": raw.hex(), "ok_crc": False, "status": None, "payload": b""}
    if len(raw) >= 4:
        r["ok_crc"] = crc16(raw) == 0
        r["id"] = raw[0]
        r["status"] = raw[1]
        r["status_name"] = ST_NAMES.get(raw[1], hex(raw[1]))
        r["payload"] = raw[2:-2]
    return r


def decode_status(payload: bytes):
    n = struct.calcsize(STATUS_FMT)
    if len(payload) != n:
        return None
    return dict(zip(STATUS_FIELDS, struct.unpack(STATUS_FMT, payload)))


def read_log(port, seconds):
    s = open_serial(port, LOG_BAUD, timeout=0.1)
    s.reset_input_buffer()
    buf, t0 = b"", time.time()
    while time.time() - t0 < seconds:
        buf += s.read(512)
    s.close()
    return buf.decode(errors="replace")


# --- build / flash ------------------------------------------------------------
def build():
    p = subprocess.run(["make", "-j8"], cwd=ROOT, capture_output=True, text=True)
    out = p.stdout + p.stderr
    diags = [l for l in out.splitlines() if re.search(r"(error|warning):", l)]
    mem = [l.strip() for l in out.splitlines() if re.match(r"\s+(RAM|FLASH):", l)]
    return p.returncode == 0, diags, mem, out


def flash(cfg, under_reset=False):
    cmds = []
    if under_reset:
        cmds.append("reset_config srst_only srst_nogate connect_assert_srst")
    out = openocd(cfg, cmds + [f"program {{{ELF}}} verify reset"], timeout=90)
    ok = "Verified OK" in out or "verified" in out.lower()
    return ok, out


# --- test suite ---------------------------------------------------------------
class Report:
    def __init__(self):
        self.items = []

    def add(self, name, ok, detail=None):
        status = "PASS" if ok is True else "FAIL" if ok is False else "SKIP"
        self.items.append({"test": name, "result": status, "detail": detail})
        mark = {"PASS": "  ok ", "FAIL": "FAIL ", "SKIP": "skip "}[status]
        print(f"[{mark}] {name}" + (f"  -> {detail}" if detail not in (None, "") else ""), flush=True)
        return ok

    @property
    def failed(self):
        return [i for i in self.items if i["result"] == "FAIL"]


def run_tests(cfg, args):
    rep = Report()
    swd = True

    if not args.no_build:
        ok, diags, mem, out = build()
        rep.add("build", ok, "; ".join(diags[:5] + mem) or None)
        if not ok:
            print(out[-3000:])
            return rep
        rep.add("build: no warnings", not diags, diags[:5] or None)

    if not args.no_flash:
        try:
            ok, out = flash(cfg, args.under_reset)
            rep.add("flash + verify", ok, None if ok else last_error(out))
            if not ok:
                return rep
        except SwdError as e:
            rep.add("flash + verify", False, str(e))
            swd = False
            if not cfg["proto_port"]:
                return rep

    if swd and not args.no_flash:
        try:
            idc = read_words(cfg, 0xE0042000, 1)[0]
            rep.add(f"swd: chip is {BOARD['name']} (DEV_ID 0x{BOARD['dev_id']:03x})",
                    (idc & 0xFFF) == BOARD["dev_id"], hex(idc))
        except SwdError as e:
            rep.add("swd: IDCODE read", False, str(e))

    # boot log (log UART) right after reset
    log_text = ""
    if cfg["log_port"] and not args.no_flash and swd:
        log_text = read_log(cfg["log_port"], 2.5)
        m = re.search(r"BOOT fw=(.+?) rst=", log_text)
        rep.add("log: BOOT line", bool(m), m.group(1) if m else repr(log_text[-200:]))
        rep.add("log: no FAULT", "FAULT" not in log_text)
        mf = re.search(r"FRAM probe=(\w+) sr=(\S+)", log_text)
        rep.add("log: FRAM probe at boot", (mf.group(1) == "OK") if mf else None,
                mf.group(0) if mf else "no FRAM line")
    else:
        time.sleep(1.5)   # let the firmware boot

    p = None
    if cfg["proto_port"]:
        p = Proto(cfg["proto_port"])
        r = p.cmd(CMD_PING)
        ver = r["payload"].decode(errors="replace")
        rep.add("uart1: PING", r["ok_crc"] and r["status"] == 0, ver or r["raw"] or "no reply")
        if not r["raw"]:
            rep.add("uart1: link", False, f"no reply on {cfg['proto_port']} @ {PROTO_BAUD} "
                    "(check TX/RX crossed, GND, port is USART1 PA9/PA10)")
            p.close()
            p = None
    else:
        rep.add("uart1: protocol tests", None, "proto_port not configured (hil.py detect)")

    if p:
        # LED on/off, verified physically on the GPIO output register over SWD
        for arg, name, expect_odr in ((1, "on", 0), (0, "off", 1)):
            r = p.cmd(CMD_LED, arg)
            ok = r["ok_crc"] and r["status"] == 0 and len(r["payload"]) == 0
            rep.add(f"uart1: LED {name} reply", ok, r["raw"])
            r2 = p.cmd(CMD_LED_GET)
            rep.add(f"uart1: LED {name} readback", r2["payload"][:2] == bytes([expect_odr, 1]),
                    r2["payload"].hex())
            if swd:
                try:
                    g = read_gpio(cfg)
                    rep.add(f"swd: PC13 ODR after LED {name}", g["PC13_odr"] == expect_odr, g)
                except SwdError as e:
                    swd = False
                    rep.add("swd: GPIO read", False, str(e))
        p.cmd(CMD_LED, 2)   # back to blinking

        r = p.cmd(CMD_LED, 9)
        rep.add("uart1: bad arg -> BAD_ARG", r["status"] == 0x02, r.get("status_name"))

        bad = bytearray(frame(CMD_PING))
        bad[-1] ^= 0x55
        r = decode_reply(p.xfer_raw(bytes(bad)))
        rep.add("uart1: bad CRC -> BAD_CRC", r["ok_crc"] and r["status"] == 0xE1, r.get("status_name"))

        r = p.cmd(0x55)
        rep.add("uart1: unknown cmd -> UNKNOWN_CMD", r["status"] == 0xE2, r.get("status_name"))

        r = decode_reply(p.xfer_raw(bytes(range(150)), wait=1.0))
        rep.add("uart1: 150-byte frame -> OVERFLOW", r["status"] == 0xE3, r.get("status_name"))

        # long valid frame (90 bytes) must not be split
        r = p.cmd(CMD_PING, *([0xAA] * 87))
        rep.add("uart1: 90-byte valid frame accepted", r["ok_crc"] and r["status"] == 0, r.get("status_name"))

        # FRAM
        r = p.cmd(CMD_FRAM, 1)
        rep.add("fram: probe (WEL set/clear)", r["status"] == 0, "SR=" + r["payload"].hex())
        r = p.cmd(CMD_FRAM, 0, wait=1.0)
        rep.add("fram: write/read 'KUREIN'", r["status"] == 0, r["payload"])
        r = p.cmd(CMD_FRAM, 2, wait=1.0)
        pl = r["payload"]
        rep.add("fram: 64-byte pattern test", r["status"] == 0,
                f"mismatch={pl[0]} first={pl[1]} exp={pl[2]:02x} got={pl[3]:02x}" if len(pl) == 4 else r["raw"])

        # stress: 50 pings back to back
        bad_n = 0
        for _ in range(50):
            r = p.cmd(CMD_PING, wait=0.3)
            bad_n += not (r["ok_crc"] and r["status"] == 0)
        rep.add("uart1: 50x PING stress", bad_n == 0, f"{bad_n} failed")

        r = p.cmd(CMD_STATUS)
        st = decode_status(r["payload"])
        rep.add("uart1: STATUS decode", st is not None, st)
        if st:
            rep.add("status: heap_min > 512 B", st["heap_min"] > 512, st["heap_min"])
            low = {k: st[k] for k in ("stack_free_default", "stack_free_led", "stack_free_uart") if st[k] < 128}
            rep.add("status: every task stack free >= 128 B", not low, low or None)
            rep.add("status: no UART errors / drops", st["uart_err"] == 0 and st["queue_full"] == 0,
                    {"uart_err": st["uart_err"], "queue_full": st["queue_full"]})
            hb0 = st["heartbeat"]
            time.sleep(2.2)
            st2 = decode_status(p.cmd(CMD_STATUS)["payload"])
            rep.add("status: heartbeat advances (RTOS alive)", st2 and st2["heartbeat"] >= hb0 + 2,
                    f"{hb0} -> {st2 and st2['heartbeat']}")

        # reset via command: reboot must be visible as boot_count+1
        if st:
            r = p.cmd(CMD_RESET)
            time.sleep(1.5)
            st3 = decode_status(p.cmd(CMD_STATUS)["payload"])
            rep.add("uart1: RESET cmd -> reboot", st3 and st3["boot_count"] == st["boot_count"] + 1,
                    st3 and {"boot_count": st3["boot_count"], "reset_cause": hex(st3["reset_cause"])})
        p.close()

    if swd:
        try:
            d = read_diag(cfg)
            rep.add("swd: g_diag valid", d["valid"], hex(d["magic"]))
            rep.add("swd: no fault recorded", not d["fault_recorded"],
                    None if not d["fault_recorded"] else
                    f"{d['error_name']} pc={addr2line(d['fault_pc'])} task={d['fault_task']}")
        except SwdError as e:
            rep.add("swd: diag read", False, str(e))
    else:
        rep.add("swd: diag read", None, "SWD unavailable")
    return rep


# --- detect -------------------------------------------------------------------
def detect(cfg):
    found: dict = {"proto_port": None, "log_port": None}
    for port in serial_ports():
        try:
            txt = read_log(port, 2.2)
            if re.search(r"Tick:|BOOT fw=|DIAG ", txt):
                found["log_port"] = port
                print(f"{port}: log @ {LOG_BAUD}")
                continue
            p = Proto(port)
            r = p.cmd(CMD_PING)
            p.close()
            if r["ok_crc"]:
                found["proto_port"] = port
                print(f"{port}: USART1 protocol @ {PROTO_BAUD}, fw={r['payload'].decode(errors='replace')}")
            else:
                print(f"{port}: silent (raw={r['raw']!r})")
        except Exception as e:  # noqa
            print(f"{port}: {e}")
    cur = {}
    if os.path.exists(CFG_FILE):
        cur = json.load(open(CFG_FILE))
    for k, v in found.items():
        if v:
            cur[k] = v
    json.dump(cur, open(CFG_FILE, "w"), indent=2)
    print("saved", CFG_FILE, cur)


# --- CLI ----------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--json", action="store_true", help="print machine-readable result")
    ap.add_argument("--under-reset", action="store_true", help="connect under reset (needs NRST wired)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("probe")
    sub.add_parser("detect")
    sub.add_parser("build")
    f = sub.add_parser("flash"); f.add_argument("--no-build", action="store_true")
    t = sub.add_parser("test")
    t.add_argument("--no-build", action="store_true")
    t.add_argument("--no-flash", action="store_true")
    sub.add_parser("ping")
    sub.add_parser("status")
    sub.add_parser("reset")
    l = sub.add_parser("led"); l.add_argument("state", choices=["on", "off", "auto", "get"])
    fr = sub.add_parser("fram"); fr.add_argument("sub", type=int, choices=[0, 1, 2], nargs="?", default=1)
    raw = sub.add_parser("raw", help="send hex bytes, CRC appended"); raw.add_argument("hex")
    lg = sub.add_parser("log"); lg.add_argument("seconds", type=float, nargs="?", default=5)
    sub.add_parser("diag")
    sub.add_parser("regs")
    sub.add_parser("gpio")
    rd = sub.add_parser("read"); rd.add_argument("what"); rd.add_argument("words", type=int, nargs="?")
    a = ap.parse_args()
    cfg = load_cfg()

    def out(obj):
        if isinstance(obj, dict) and "payload" in obj and isinstance(obj["payload"], bytes):
            obj = dict(obj, payload=obj["payload"].hex())
        print(json.dumps(obj, indent=2, default=str))

    def need_proto():
        if not cfg["proto_port"]:
            raise SystemExit("USART1 port unknown: run `hil.py detect` or set HIL_PROTO_PORT")
        return Proto(cfg["proto_port"])

    try:
        if a.cmd == "probe":
            res = {"serial_ports": serial_ports(), "config": {k: cfg[k] for k in ("proto_port", "log_port")}}
            try:
                o = openocd(cfg, ["mdw 0xE0042000 1"])
                idc = parse_mdw(o).get(0xE0042000)
                res["board"] = {"id": BOARD_ID, "name": BOARD["name"]}
                res["swd"] = {"ok": True, "DBGMCU_IDCODE": hex(idc) if idc is not None else None,
                              "chip_matches_board": idc is not None and (idc & 0xFFF) == BOARD["dev_id"],
                              "voltage": (re.search(r"Target voltage: ([\d.]+)", o) or [None, None])[1]}
            except SwdError as e:
                res["swd"] = {"ok": False, "error": str(e)}
            out(res)
            sys.exit(0 if res["swd"]["ok"] else 1)
        if a.cmd == "detect":
            detect(cfg)
        elif a.cmd == "build":
            ok, diags, mem, o = build()
            out({"ok": ok, "diagnostics": diags, "memory": mem})
            sys.exit(0 if ok else 1)
        elif a.cmd == "flash":
            if not a.no_build:
                ok, diags, mem, o = build()
                if not ok:
                    print(o[-3000:]); sys.exit(1)
            ok, o = flash(cfg, a.under_reset)
            print("flash:", "OK" if ok else "FAILED\n" + o[-2000:])
            sys.exit(0 if ok else 1)
        elif a.cmd == "test":
            rep = run_tests(cfg, a)
            n_fail = len(rep.failed)
            n_pass = sum(i["result"] == "PASS" for i in rep.items)
            n_skip = sum(i["result"] == "SKIP" for i in rep.items)
            print(f"\nRESULT: {'PASS' if not n_fail else 'FAIL'}  pass={n_pass} fail={n_fail} skip={n_skip}")
            path = os.path.join(ROOT, "build", "hil_report.json")
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w") as fh:
                json.dump(rep.items, fh, indent=2, default=str)
            print("report:", path)
            sys.exit(1 if n_fail else 0)
        elif a.cmd == "ping":
            out(need_proto().cmd(CMD_PING))
        elif a.cmd == "status":
            r = need_proto().cmd(CMD_STATUS)
            out(decode_status(r["payload"]) or r)
        elif a.cmd == "reset":
            out(need_proto().cmd(CMD_RESET))
        elif a.cmd == "led":
            p = need_proto()
            if a.state == "get":
                out(p.cmd(CMD_LED_GET))
            else:
                out(p.cmd(CMD_LED, {"off": 0, "on": 1, "auto": 2}[a.state]))
        elif a.cmd == "fram":
            out(need_proto().cmd(CMD_FRAM, a.sub, wait=1.0))
        elif a.cmd == "raw":
            out(need_proto().cmd(*bytes.fromhex(a.hex), wait=1.0))
        elif a.cmd == "log":
            if not cfg["log_port"]:
                raise SystemExit("log port unknown: run `hil.py detect` or set HIL_LOG_PORT")
            print(read_log(cfg["log_port"], a.seconds))
        elif a.cmd == "diag":
            d = read_diag(cfg)
            if d["fault_recorded"]:
                d["fault_where"] = addr2line(d["fault_pc"])
                d["fault_caller"] = addr2line(d["fault_lr"])
            out({k: (hex(v) if isinstance(v, int) and k.startswith(("fault_", "magic", "reset")) else v)
                 for k, v in d.items()})
        elif a.cmd == "regs":
            out({k: hex(v) if isinstance(v, int) else v for k, v in read_regs(cfg).items()})
        elif a.cmd == "gpio":
            out(read_gpio(cfg))
        elif a.cmd == "read":
            if a.what.startswith("0x"):
                addr, size = int(a.what, 16), 4 * (a.words or 1)
            else:
                addr, size = symbol(a.what)
            n = a.words or max(1, (size + 3) // 4)
            words = read_words(cfg, addr, n)
            out({"addr": hex(addr), "words": [hex(w) for w in words]})
    except SwdError as e:
        print("SWD ERROR:", e, file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
