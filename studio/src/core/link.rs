//! Serial links: request/response on USART1 and a line reader for USART3.

use super::faults::{Line, Step};
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
/// Returns the port and, on macOS, its tty fd (-1 elsewhere).
fn open_port(name: &str, baud: u32, timeout: Duration) -> Result<(Box<dyn serialport::SerialPort>, i32), String> {
    let busy = |e: serialport::Error| format!("{}: {} (порт занят другой программой?)", name, e);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::io::AsRawFd;
        let port = serialport::new(name, baud).timeout(timeout).open_native().map_err(busy)?;
        let fd = port.as_raw_fd();
        termios_speed(fd, baud).map_err(|e| format!("{}: скорость {} не установилась: {}", name, baud, e))?;
        Ok((Box::new(port), fd))
    }
    #[cfg(not(target_os = "macos"))]
    {
        serialport::new(name, baud).timeout(timeout).open().map(|p| (p, -1)).map_err(busy)
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
    /// tty fd: after any `set_*` serialport sets the speed with IOSSIOSPEED
    /// again, so the termios speed has to be restored (see `open_port`).
    #[cfg(target_os = "macos")]
    fd: std::os::unix::io::RawFd,
}

impl ProtoPort {
    pub fn open(name: &str) -> Result<ProtoPort, String> {
        let (port, _fd) = open_port(name, PROTO_BAUD, Duration::from_millis(10))?;
        Ok(ProtoPort {
            port,
            #[cfg(target_os = "macos")]
            fd: _fd,
        })
    }

    /// Switch the line format for fault injection.
    fn set_line(&mut self, l: Line) -> Result<(), String> {
        use serialport::{DataBits, Parity, StopBits};
        let e = |e: serialport::Error| e.to_string();
        let p = &mut self.port;
        p.set_parity(match l {
            Line::ParityEven => Parity::Even,
            Line::ParityOdd => Parity::Odd,
            _ => Parity::None,
        })
        .map_err(e)?;
        p.set_stop_bits(if l == Line::TwoStop { StopBits::Two } else { StopBits::One }).map_err(e)?;
        p.set_data_bits(if l == Line::SevenBits { DataBits::Seven } else { DataBits::Eight }).map_err(e)?;
        let baud = if let Line::Baud(b) = l { b } else { PROTO_BAUD };
        p.set_baud_rate(baud).map_err(e)?;
        #[cfg(target_os = "macos")]
        termios_speed(self.fd, baud).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send a plan (parts, pauses, line format changes, BREAK), put the line
    /// back to 8N1 38400 and collect everything the board answers until it
    /// stays quiet for 200 ms.
    pub fn xfer_plan(&mut self, steps: &[Step], timeout: Duration) -> Result<Vec<u8>, String> {
        let _ = self.port.clear(serialport::ClearBuffer::Input);
        let mut changed = false;
        let mut res = Ok(());
        for s in steps {
            res = match s {
                Step::Line(l) => {
                    changed |= *l != Line::Normal;
                    self.set_line(*l)
                }
                Step::Bytes(b) => self.port.write_all(b).and_then(|_| self.port.flush()).map_err(|e| e.to_string()),
                Step::Pause(ms) => {
                    std::thread::sleep(Duration::from_millis(*ms as u64));
                    Ok(())
                }
                Step::Break(ms) => self.port.set_break().map_err(|e| e.to_string()).map(|_| {
                    std::thread::sleep(Duration::from_millis(*ms as u64));
                    let _ = self.port.clear_break();
                }),
            };
            if res.is_err() {
                break;
            }
        }
        if changed {
            self.set_line(Line::Normal)?;
        }
        res?;
        let mut out = vec![];
        self.read_until(&mut out, timeout, false)?;
        Ok(out)
    }

    /// Read into `out` until the line is quiet for 200 ms or `timeout` runs
    /// out. With `first` it returns as soon as `out` holds a whole reply.
    fn read_until(&mut self, out: &mut Vec<u8>, timeout: Duration, first: bool) -> Result<(), String> {
        let t0 = Instant::now();
        let mut last: Option<Instant> = None;
        let mut buf = [0u8; 256];
        while t0.elapsed() < timeout {
            match self.port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    out.extend_from_slice(&buf[..n]);
                    last = Some(Instant::now());
                    // A whole reply has a CRC of 0. Return it now so a later
                    // gap on the wire cannot be mistaken for "end of frame".
                    if first && out.len() >= 4 && super::proto::crc16(out) == 0 {
                        return Ok(());
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
        Ok(())
    }

    /// Send a frame and read until the reply's CRC checks, or the line stays
    /// quiet for 200 ms. A short idle used to cut a STATUS reply in half.
    pub fn xfer(&mut self, data: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
        let _ = self.port.clear(serialport::ClearBuffer::Input);
        self.port.write_all(data).map_err(|e| e.to_string())?;
        let _ = self.port.flush();
        let mut out = vec![];
        self.read_until(&mut out, timeout, true)?;
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
        let (mut port, _) = open_port(name, LOG_BAUD, Duration::from_millis(50))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::faults::{cases, summary};
    use crate::core::proto::{frame, Status, CMD_STATUS};

    /// `PILL_PORT=/dev/cu.usbserial-120 cargo test --release hardware -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reliability_on_hardware() {
        let Ok(name) = std::env::var("PILL_PORT") else { return };
        let mut p = ProtoPort::open(&name).unwrap();
        let st = |p: &mut ProtoPort| {
            let r = p.xfer(&frame(&[CMD_STATUS]), Duration::from_millis(700)).unwrap();
            (r.len() > 4).then(|| Status::decode(&r[2..r.len() - 2])).flatten()
        };
        let mut prev = st(&mut p).expect("STATUS before");
        let mut bad = 0;
        for c in cases() {
            let rx = p.xfer_plan(&c.steps, Duration::from_millis(700 + crate::core::faults::wait_ms(&c.steps))).unwrap();
            std::thread::sleep(Duration::from_millis(40));
            let after = st(&mut p);
            let ok = c.expect.met(&rx) && after.is_some_and(|a| a.boot_count == prev.boot_count);
            let d = after.map(|a| crate::worker::status_delta(&prev, &a)).unwrap_or("НЕТ ОТВЕТА".into());
            println!("{} {:<22} ждём {:<20} пришло {:<28} {}", if ok { "OK " } else { "-- " }, c.title, c.expect.label(), summary(&rx), d);
            bad += (!ok) as u32;
            if let Some(a) = after {
                prev = a;
            }
        }
        println!("не как ожидалось: {bad}");
    }
}
