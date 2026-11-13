//! Serial links: request/response on USART1 and a line reader for USART3.

use super::proto::{LOG_BAUD, PROTO_BAUD};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct PortInfo {
    pub name: String,
    pub desc: String,
}

pub fn list_ports() -> Vec<PortInfo> {
    let mut v: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.port_name.contains("Bluetooth") && !p.port_name.contains("debug-console"))
        // macOS lists every port twice (tty.* and cu.*); cu.* is the one to open
        .filter(|p| !p.port_name.starts_with("/dev/tty.") )
        .map(|p| {
            let desc = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    let chip = match (u.vid, u.pid) {
                        (0x067B, _) => "Prolific PL2303",
                        (0x1A86, _) => "WCH CH340",
                        (0x10C4, _) => "Silabs CP210x",
                        (0x0403, _) => "FTDI",
                        (0x0483, _) => "ST-Link VCP",
                        _ => "",
                    };
                    let prod = u.product.clone().unwrap_or_default();
                    format!("{} {:04X}:{:04X} {}", chip, u.vid, u.pid, prod).trim().to_string()
                }
                _ => String::new(),
            };
            PortInfo { name: p.port_name, desc }
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

pub struct ProtoPort {
    port: Box<dyn serialport::SerialPort>,
}

impl ProtoPort {
    pub fn open(name: &str) -> Result<ProtoPort, String> {
        let port = serialport::new(name, PROTO_BAUD)
            .timeout(Duration::from_millis(10))
            .open()
            .map_err(|e| format!("{}: {} (порт занят другой программой?)", name, e))?;
        Ok(ProtoPort { port })
    }

    /// Send a frame, collect the reply until the line is idle for 60 ms.
    pub fn xfer(&mut self, data: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
        let _ = self.port.clear(serialport::ClearBuffer::Input);
        self.port.write_all(data).map_err(|e| e.to_string())?;
        let _ = self.port.flush();
        let t0 = Instant::now();
        let mut last: Option<Instant> = None;
        let mut out = vec![];
        let mut buf = [0u8; 256];
        while t0.elapsed() < timeout {
            match self.port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    out.extend_from_slice(&buf[..n]);
                    last = Some(Instant::now());
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.to_string()),
            }
            if last.is_some_and(|l| l.elapsed() > Duration::from_millis(60)) {
                break;
            }
        }
        Ok(out)
    }
}

/// Background reader of the text log; lines go to `sink`.
pub struct LogPort {
    stop: Arc<AtomicBool>,
    pub error: Arc<Mutex<Option<String>>>,
}

impl LogPort {
    pub fn open(name: &str, sink: Arc<Mutex<Vec<String>>>) -> Result<LogPort, String> {
        let mut port = serialport::new(name, LOG_BAUD)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| format!("{}: {} (порт занят другой программой?)", name, e))?;
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (stop2, err2) = (stop.clone(), error.clone());
        std::thread::Builder::new()
            .name("uart3-log".into())
            .spawn(move || {
                let mut line: Vec<u8> = vec![];
                let mut buf = [0u8; 512];
                while !stop2.load(Ordering::Relaxed) {
                    match port.read(&mut buf) {
                        Ok(n) => {
                            for &b in &buf[..n] {
                                if b == b'\n' {
                                    let s = String::from_utf8_lossy(&line).trim_end_matches('\r').to_string();
                                    line.clear();
                                    if !s.is_empty() {
                                        sink.lock().unwrap_or_else(|e| e.into_inner()).push(s);
                                    }
                                } else if line.len() < 512 {
                                    line.push(b);
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            *err2.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                            break;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(LogPort { stop, error })
    }
}

impl Drop for LogPort {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
