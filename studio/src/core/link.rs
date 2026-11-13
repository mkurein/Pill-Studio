//! Serial links: request/response on USART1 and a line reader for USART3.

use super::proto::{LOG_BAUD, PROTO_BAUD};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Open a port at `baud`, 8N1.
///
/// On macOS `serialport` sets the speed with the IOSSIOSPEED ioctl. Apple's
/// driver for Prolific PL2303 adapters does not apply it: termios reports
/// 38400 while the wire runs at another rate, so the board sees garbage
/// (uart_err, rx_crc_err) and never answers. pyserial (hil.py) sets standard
/// speeds through termios and works. Setting the speed again with tcsetattr
/// makes every adapter run at the rate asked for.
fn open_port(name: &str, baud: u32, timeout: Duration) -> Result<Box<dyn serialport::SerialPort>, String> {
    let busy = |e: serialport::Error| format!("{}: {} (порт занят другой программой?)", name, e);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        let port = serialport::new(name, baud).timeout(timeout).open_native().map_err(busy)?;
        termios_speed(port.as_raw_fd(), baud).map_err(|e| format!("{}: скорость {} не установилась: {}", name, baud, e))?;
        Ok(Box::new(port))
    }
    #[cfg(not(target_os = "macos"))]
    {
        serialport::new(name, baud).timeout(timeout).open().map_err(busy)
    }
}

#[cfg(target_os = "macos")]
fn termios_speed(fd: std::os::unix::io::RawFd, baud: u32) -> std::io::Result<()> {
    let speed = match baud {
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        _ => return Ok(()), // non-standard: only IOSSIOSPEED can set it
    };
    // SAFETY: fd is the open tty owned by the caller; termios is plain data.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        libc::cfsetispeed(&mut t, speed);
        libc::cfsetospeed(&mut t, speed);
        if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

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
        let port = open_port(name, PROTO_BAUD, Duration::from_millis(10))?;
        Ok(ProtoPort { port })
    }

    /// Send a frame and read until the reply's CRC checks, or the line stays
    /// quiet for 200 ms. A short idle used to cut a STATUS reply in half.
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
                    // A whole reply has a CRC of 0. Return it now so a later
                    // gap on the wire cannot be mistaken for "end of frame".
                    if out.len() >= 4 && super::proto::crc16(&out) == 0 {
                        return Ok(out);
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.to_string()),
            }
            // USB-UART often splits a 60-byte STATUS across packets with a
            // pause longer than 60 ms. Wait out that pause before giving up.
            if last.is_some_and(|l| l.elapsed() > Duration::from_millis(200)) {
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
        let mut port = open_port(name, LOG_BAUD, Duration::from_millis(50))?;
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
