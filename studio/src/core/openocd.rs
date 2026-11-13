//! SWD through OpenOCD: the studio starts `openocd` as a server and talks to it
//! over the TCL RPC port (commands terminated by 0x1A). Works the same on
//! macOS and Windows; the target keeps running while memory is read.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TCL_PORT: u16 = 6667;

pub struct OpenOcd {
    child: Child,
    sock: TcpStream,
    pub log: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OcdConfig {
    pub exe: String,
    /// Extra arguments before `init` (interface/target).
    pub args: Vec<String>,
    pub under_reset: bool,
}

impl Default for OcdConfig {
    fn default() -> Self {
        OcdConfig {
            exe: "openocd".into(),
            args: vec![
                "-f".into(),
                "interface/stlink.cfg".into(),
                "-c".into(),
                "transport select hla_swd".into(),
                "-f".into(),
                "target/stm32f1x.cfg".into(),
            ],
            under_reset: false,
        }
    }
}

fn last_error(lines: &[String]) -> String {
    lines.iter().rev().find(|l| l.starts_with("Error")).cloned().unwrap_or_else(|| {
        lines.last().cloned().unwrap_or_else(|| "OpenOCD завершился без вывода".into())
    })
}

impl OpenOcd {
    pub fn start(cfg: &OcdConfig) -> Result<OpenOcd, String> {
        let mut cmd = Command::new(&cfg.exe);
        cmd.args(&cfg.args);
        if cfg.under_reset {
            cmd.args(["-c", "reset_config srst_only srst_nogate connect_assert_srst"]);
        }
        cmd.args(["-c", &format!("tcl_port {}", TCL_PORT), "-c", "telnet_port disabled", "-c", "gdb_port disabled"]);
        cmd.args(["-c", "init"]);
        cmd.stdout(Stdio::null()).stderr(Stdio::piped()).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("не удалось запустить «{}»: {} (путь — в Настройках)", cfg.exe, e))?;
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        if let Some(err) = child.stderr.take() {
            let log2 = log.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    let mut l = log2.lock().unwrap_or_else(|e| e.into_inner());
                    l.push(line);
                    if l.len() > 500 {
                        l.drain(..100);
                    }
                }
            });
        }
        let t0 = Instant::now();
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                let lines = log.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let mut msg = last_error(&lines);
                if msg.contains("unable to connect to the target") || msg.contains("init mode failed") {
                    msg = format!(
                        "ST-Link найден, но микроконтроллер не отвечает по SWD: проверьте SWDIO/SWCLK/GND, питание, BOOT0 = 0; \
                         при наличии провода NRST включите «подключение под сбросом». ({})",
                        msg
                    );
                }
                return Err(format!("OpenOCD ({}): {}", status, msg));
            }
            if let Ok(sock) = TcpStream::connect(("127.0.0.1", TCL_PORT)) {
                sock.set_read_timeout(Some(Duration::from_secs(30))).ok();
                sock.set_nodelay(true).ok();
                let mut o = OpenOcd { child, sock, log };
                // wait until the target is examined
                o.cmd("targets").ok();
                return Ok(o);
            }
            if t0.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                return Err("OpenOCD не открыл TCL-порт за 10 с".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Run a TCL command, return its result text.
    pub fn cmd(&mut self, c: &str) -> Result<String, String> {
        let mut msg = c.as_bytes().to_vec();
        msg.push(0x1A);
        self.sock.write_all(&msg).map_err(|e| format!("OpenOCD: {}", e))?;
        let mut out = vec![];
        let mut buf = [0u8; 4096];
        loop {
            let n = self.sock.read(&mut buf).map_err(|e| format!("OpenOCD: {}", e))?;
            if n == 0 {
                return Err("OpenOCD закрыл соединение".into());
            }
            out.extend_from_slice(&buf[..n]);
            if out.last() == Some(&0x1A) {
                out.pop();
                break;
            }
        }
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// Like `cmd`, but turns TCL errors into Err (wrapped with `catch`).
    pub fn eval(&mut self, c: &str) -> Result<String, String> {
        let r = self.cmd(&format!("if {{[catch {{{}}} res]}} {{return \"ERR:$res\"}} else {{return $res}}", c))?;
        match r.strip_prefix("ERR:") {
            Some(e) => Err(e.trim().to_string()),
            None => Ok(r),
        }
    }

    pub fn read_words(&mut self, addr: u32, count: u32) -> Result<Vec<u32>, String> {
        let r = self.eval(&format!("read_memory 0x{:08x} 32 {}", addr, count))?;
        let v: Result<Vec<u32>, _> = r
            .split_whitespace()
            .map(|t| u32::from_str_radix(t.trim_start_matches("0x"), 16))
            .collect();
        let v = v.map_err(|_| format!("не разобрать ответ read_memory: {}", r))?;
        if v.len() != count as usize {
            return Err(format!("прочитано {} слов из {}", v.len(), count));
        }
        Ok(v)
    }

    pub fn write_word(&mut self, addr: u32, val: u32) -> Result<(), String> {
        self.eval(&format!("write_memory 0x{:08x} 32 {{0x{:08x}}}", addr, val)).map(|_| ())
    }

    pub fn state(&mut self) -> String {
        self.eval("[target current] curstate").unwrap_or_else(|e| format!("? ({})", e))
    }

    /// Core registers (halts the CPU if needed, restores the run state).
    pub fn core_regs(&mut self) -> Result<Vec<(String, u32)>, String> {
        let was_running = self.state() == "running";
        if was_running {
            self.eval("halt")?;
        }
        let mut out = vec![];
        for r in ["pc", "lr", "sp", "xPSR", "msp", "psp", "primask", "basepri", "control", "r0", "r1", "r2", "r3", "r12"]
        {
            let s = self.eval(&format!("reg {}", r)).unwrap_or_default();
            // "pc (/32): 0x08001234"
            if let Some(v) = s.rsplit("0x").next().and_then(|h| u32::from_str_radix(h.trim(), 16).ok()) {
                out.push((r.to_string(), v));
            }
        }
        if was_running {
            self.eval("resume")?;
        }
        Ok(out)
    }

    pub fn target_voltage(&mut self) -> Option<String> {
        let lines = self.log.lock().unwrap_or_else(|e| e.into_inner()).clone();
        lines.iter().rev().find_map(|l| l.split("Target voltage:").nth(1).map(|v| format!("{} В", v.trim())))
    }

    pub fn log_tail(&self, n: usize) -> Vec<String> {
        let l = self.log.lock().unwrap_or_else(|e| e.into_inner());
        l.iter().rev().take(n).rev().cloned().collect()
    }
}

impl Drop for OpenOcd {
    fn drop(&mut self) {
        let _ = self.cmd("shutdown");
        std::thread::sleep(Duration::from_millis(100));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Cortex-M3 fault status registers (CFSR) → Russian.
pub fn cfsr_ru(cfsr: u32) -> Vec<&'static str> {
    let bits: [(u32, &str); 15] = [
        (0, "IACCVIOL — выполнение из запрещённой области"),
        (1, "DACCVIOL — доступ к данным в запрещённой области"),
        (3, "MUNSTKERR — сбой при выходе из прерывания"),
        (4, "MSTKERR — сбой при входе в прерывание (стек?)"),
        (7, "MMARVALID — адрес в MMFAR достоверен"),
        (8, "IBUSERR — ошибка шины при выборке команды"),
        (9, "PRECISERR — точная ошибка шины данных (адрес в BFAR)"),
        (10, "IMPRECISERR — неточная ошибка шины (запись в несуществующий адрес)"),
        (11, "UNSTKERR — ошибка шины при выходе из прерывания"),
        (12, "STKERR — ошибка шины при сохранении контекста (переполнение стека?)"),
        (15, "BFARVALID — адрес в BFAR достоверен"),
        (16, "UNDEFINSTR — неизвестная команда"),
        (17, "INVSTATE — попытка выйти из Thumb (вызов по чётному адресу/NULL)"),
        (18, "INVPC — неверный EXC_RETURN"),
        (25, "DIVBYZERO — деление на ноль"),
    ];
    let mut v: Vec<&str> = bits.iter().filter(|(b, _)| cfsr & (1 << b) != 0).map(|(_, s)| *s).collect();
    if cfsr & (1 << 19) != 0 {
        v.push("NOCP — сопроцессор (FPU) недоступен");
    }
    if cfsr & (1 << 24) != 0 {
        v.push("UNALIGNED — невыровненный доступ");
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfsr_decoding() {
        let v = cfsr_ru(1 << 25 | 1 << 9);
        assert_eq!(v.len(), 2);
        assert!(v.iter().any(|s| s.contains("деление")));
    }
}
