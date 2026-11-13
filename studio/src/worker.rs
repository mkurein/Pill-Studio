//! I/O thread. The UI never touches a port, OpenOCD or a process: it sends
//! `Job`s, the worker runs them and publishes `State` + streamed events into an
//! inbox that the UI drains once per frame (pattern from DigiPot Studio).

use crate::core::config::Config;
use crate::core::elf::{self, Elf};
use crate::core::link::{LogPort, ProtoPort};
use crate::core::openocd::{OcdConfig, OpenOcd};
use crate::core::proto::{self, *};
use crate::core::sim::SimBoard;
use crate::core::svd::Peripheral;
use crate::core::{Diag, DIAG_WORDS};
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Ok,
    Info,
    Warn,
    Err,
}

pub enum Job {
    ProtoOpen(String),
    ProtoSim,
    ProtoClose,
    LogOpen(String),
    LogSim,
    LogClose,
    /// Request body (CRC is appended unless `raw`).
    Send { body: Vec<u8>, raw: bool },
    Poll(bool),
    PollMs(u64),
    Stress(u32),
    SwdOpen(OcdConfig),
    SwdSim,
    SwdClose,
    /// halt / resume / "reset run" / step …
    SwdCtl(&'static str),
    SwdRegs,
    ReadDiag,
    ReadWords(Vec<u32>),
    WriteWord(u32, u32),
    OcdExec(String),
    LoadElf(PathBuf),
    Build(Config),
    Flash(Config),
    HilTest(Config),
    SimFram(bool),
}

#[derive(Clone, Debug)]
pub struct Txn {
    pub id: u64,
    pub t: f64,
    pub tx: Vec<u8>,
    pub rx: Vec<u8>,
    pub label: String,
    pub verdict: String,
    pub ok: bool,
    pub ms: f64,
    pub poll: bool,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub t: f64,
    pub text: String,
    pub kind: LogKind,
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub t: f64,
    pub st: Status,
}

#[derive(Clone, Default)]
pub struct State {
    pub proto: Option<String>,
    pub log: Option<String>,
    pub fw: Option<String>,
    pub status: Option<Status>,
    pub status_t: f64,
    pub polling: bool,
    pub poll_ms: u64,
    pub swd: Option<String>,
    pub swd_state: String,
    pub swd_voltage: Option<String>,
    pub idcode: Option<u32>,
    pub regs: Vec<(String, u32)>,
    pub diag: Option<Diag>,
    pub mem: HashMap<u32, u32>,
    pub mem_t: f64,
    pub elf: Option<Arc<Elf>>,
    pub job: Option<String>,
    pub job_out: Vec<String>,
    pub job_ok: Option<bool>,
    pub ocd_console: Vec<String>,
    pub sim_fram: bool,
}

#[derive(Default)]
struct Inbox {
    state: Option<State>,
    txns: Vec<Txn>,
    lines: Vec<LogLine>,
    samples: Vec<Sample>,
    toasts: Vec<(Tone, String)>,
}

pub struct Update {
    pub state: Option<State>,
    pub txns: Vec<Txn>,
    pub lines: Vec<LogLine>,
    pub samples: Vec<Sample>,
    pub toasts: Vec<(Tone, String)>,
}

pub struct Worker {
    tx: Sender<Job>,
    inbox: Arc<Mutex<Inbox>>,
    pending: Arc<AtomicUsize>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

enum Proto {
    Serial(ProtoPort),
    Sim,
}
enum Swd {
    Real(Box<OpenOcd>),
    Sim,
}
enum LogSrc {
    Serial(LogPort, Arc<Mutex<Vec<String>>>),
    Sim,
}

struct BgJob {
    name: String,
    out: Arc<Mutex<Vec<String>>>,
    done: Arc<Mutex<Option<bool>>>,
    reload_elf: bool,
}

struct Eng {
    t0: Instant,
    st: State,
    svd: Arc<Vec<Peripheral>>,
    sim: SimBoard,
    proto: Option<Proto>,
    log: Option<LogSrc>,
    swd: Option<Swd>,
    last_poll: Instant,
    next_id: u64,
    bg: Option<BgJob>,
    project: PathBuf,
    toolchain: String,
    out: Inbox,
}

impl Worker {
    pub fn spawn(ctx: egui::Context, svd: Arc<Vec<Peripheral>>, cfg: &Config) -> Worker {
        let (tx, rx) = mpsc::channel::<Job>();
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let pending = Arc::new(AtomicUsize::new(0));
        let (inbox2, pending2) = (inbox.clone(), pending.clone());
        let project = PathBuf::from(&cfg.project_dir);
        let toolchain = cfg.toolchain_dir.clone();
        let poll_ms = cfg.poll_ms.max(200);
        std::thread::Builder::new()
            .name("io-worker".into())
            .spawn(move || {
                let mut e = Eng {
                    t0: Instant::now(),
                    st: State { poll_ms, sim_fram: true, ..Default::default() },
                    svd,
                    sim: SimBoard::new(),
                    proto: None,
                    log: None,
                    swd: None,
                    last_poll: Instant::now(),
                    next_id: 1,
                    bg: None,
                    project: project.clone(),
                    toolchain,
                    out: Inbox::default(),
                };
                for p in elf::candidates(&project) {
                    if p.exists() {
                        e.load_elf(&p, false);
                        break;
                    }
                }
                loop {
                    let first = match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(j) => Some(j),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let mut jobs: Vec<Job> = first.into_iter().collect();
                    jobs.extend(rx.try_iter());
                    let taken = jobs.len();
                    for j in jobs {
                        e.run(j);
                    }
                    e.tick();
                    let changed = e.publish(&inbox2);
                    pending2.fetch_sub(taken, Ordering::SeqCst);
                    if changed || taken > 0 {
                        ctx.request_repaint();
                    }
                }
            })
            .expect("failed to start the I/O thread");
        Worker { tx, inbox, pending }
    }

    pub fn send(&self, job: Job) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        if self.tx.send(job).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn busy(&self) -> bool {
        self.pending.load(Ordering::SeqCst) > 0
    }

    pub fn take(&self) -> Update {
        let mut ib = lock(&self.inbox);
        Update {
            state: ib.state.take(),
            txns: std::mem::take(&mut ib.txns),
            lines: std::mem::take(&mut ib.lines),
            samples: std::mem::take(&mut ib.samples),
            toasts: std::mem::take(&mut ib.toasts),
        }
    }
}

impl Eng {
    fn now(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }
    fn toast(&mut self, t: Tone, s: impl Into<String>) {
        self.out.toasts.push((t, s.into()));
    }

    fn publish(&mut self, inbox: &Mutex<Inbox>) -> bool {
        let changed = !self.out.txns.is_empty()
            || !self.out.lines.is_empty()
            || !self.out.toasts.is_empty()
            || !self.out.samples.is_empty();
        let mut ib = lock(inbox);
        ib.state = Some(self.st.clone());
        ib.txns.append(&mut self.out.txns);
        ib.lines.append(&mut self.out.lines);
        ib.samples.append(&mut self.out.samples);
        ib.toasts.append(&mut self.out.toasts);
        changed || self.bg.is_some()
    }

    fn push_line(&mut self, text: String) {
        let kind = classify_log(&text);
        if kind == LogKind::Fault || kind == LogKind::PrevFault {
            self.toast(Tone::Err, format!("UART3: {}", text));
        }
        let t = self.now();
        self.out.lines.push(LogLine { t, text, kind });
    }

    fn tick(&mut self) {
        // simulated log
        if matches!(self.log, Some(LogSrc::Sim)) {
            for l in self.sim.poll_log() {
                self.push_line(l);
            }
        } else {
            self.sim.poll_log();
        }
        // serial log
        let mut lines = vec![];
        let mut err = None;
        if let Some(LogSrc::Serial(port, sink)) = &self.log {
            lines = std::mem::take(&mut *lock(sink));
            err = lock(&port.error).take();
        }
        for l in lines {
            self.push_line(l);
        }
        if let Some(e) = err {
            self.toast(Tone::Err, format!("Лог UART3 остановлен: {}", e));
            self.log = None;
            self.st.log = None;
        }
        // STATUS polling
        if self.st.polling && self.proto.is_some() && self.last_poll.elapsed() >= Duration::from_millis(self.st.poll_ms) {
            self.last_poll = Instant::now();
            self.transact(vec![CMD_STATUS], false, true);
        }
        // SWD liveness
        let mut dead = false;
        if let Some(Swd::Real(o)) = &mut self.swd {
            if !o.alive() {
                dead = true;
            }
        }
        if dead {
            self.swd = None;
            self.st.swd = None;
            self.toast(Tone::Err, "OpenOCD завершился — SWD отключён");
        }
        // background process
        if let Some(bg) = &self.bg {
            self.st.job_out = lock(&bg.out).clone();
            let done = *lock(&bg.done);
            if let Some(ok) = done {
                let name = bg.name.clone();
                let reload = bg.reload_elf;
                self.st.job = None;
                self.st.job_ok = Some(ok);
                self.bg = None;
                self.toast(if ok { Tone::Ok } else { Tone::Err }, format!("{}: {}", name, if ok { "успешно" } else { "ошибка" }));
                if reload && ok {
                    self.reload_elf();
                }
            }
        }
    }

    fn reload_elf(&mut self) {
        for p in elf::candidates(&self.project) {
            if p.exists() {
                self.load_elf(&p, true);
                return;
            }
        }
    }

    fn load_elf(&mut self, p: &std::path::Path, toast: bool) {
        match Elf::load(p) {
            Ok(e) => {
                if toast {
                    self.toast(Tone::Ok, format!("ELF загружен: {} символов", e.syms.len()));
                }
                self.st.elf = Some(Arc::new(e));
            }
            Err(e) => self.toast(Tone::Err, e),
        }
    }

    /// One request/response on USART1, logged as a transaction.
    fn transact(&mut self, body: Vec<u8>, raw: bool, poll: bool) -> Option<Reply> {
        let tx = if raw { body.clone() } else { frame(&body) };
        let t = self.now();
        let t1 = Instant::now();
        let rx = match self.proto.as_mut()? {
            Proto::Sim => {
                std::thread::sleep(Duration::from_millis(3 + tx.len() as u64 / 4));
                self.sim.xfer(&tx)
            }
            Proto::Serial(p) => match p.xfer(&tx, Duration::from_millis(700)) {
                Ok(v) => v,
                Err(e) => {
                    self.toast(Tone::Err, format!("UART1: {}", e));
                    self.proto = None;
                    self.st.proto = None;
                    return None;
                }
            },
        };
        let ms = t1.elapsed().as_secs_f64() * 1000.0;
        let label = if raw { format!("сырые байты ({} B)", tx.len()) } else { describe_request(&body) };
        let decoded = decode_reply(&rx);
        let (ok, verdict, reply) = match &decoded {
            Decoded::Ok(r) => {
                (r.status == ST_OK, format!("{} — {}", status_name(r.status), status_ru(r.status)), Some(r.clone()))
            }
            Decoded::Silent => (false, "нет ответа (проверьте TX/RX, скорость 38400, питание)".into(), None),
            Decoded::Garbage(e) => (false, e.clone(), None),
        };
        if let Some(r) = &reply {
            self.apply_reply(&body, r, raw);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.out.txns.push(Txn { id, t, tx, rx, label, verdict, ok, ms, poll });
        reply
    }

    fn apply_reply(&mut self, body: &[u8], r: &Reply, raw: bool) {
        if raw || r.status != ST_OK {
            return;
        }
        match body.first().copied() {
            Some(CMD_PING) => self.st.fw = Some(String::from_utf8_lossy(&r.payload).to_string()),
            Some(CMD_STATUS) => {
                if let Some(s) = Status::decode(&r.payload) {
                    let t = self.now();
                    if let Some(prev) = self.st.status {
                        if s.boot_count != prev.boot_count {
                            self.toast(
                                Tone::Warn,
                                format!("Плата перезагрузилась: {}", proto::reset_cause_ru(s.reset_cause)),
                            );
                        }
                    }
                    self.st.status = Some(s);
                    self.st.status_t = t;
                    self.out.samples.push(Sample { t, st: s });
                }
            }
            Some(CMD_RESET) => {
                self.st.fw = None;
            }
            _ => {}
        }
    }

    fn swd_read(&mut self, addr: u32, count: u32) -> Result<Vec<u32>, String> {
        match self.swd.as_mut() {
            None => Err("SWD не подключён".into()),
            Some(Swd::Sim) => Ok((0..count).map(|i| self.sim.read_word(&self.svd, addr + 4 * i)).collect()),
            Some(Swd::Real(o)) => o.read_words(addr, count),
        }
    }

    fn swd_refresh_state(&mut self) {
        self.st.swd_state = match self.swd.as_mut() {
            Some(Swd::Real(o)) => o.state(),
            Some(Swd::Sim) => "running".into(),
            None => String::new(),
        };
    }

    fn spawn_bg(&mut self, name: &str, mut cmd: Command, reload_elf: bool) {
        if self.bg.is_some() {
            self.toast(Tone::Warn, "Дождитесь окончания текущей операции");
            return;
        }
        let out = Arc::new(Mutex::new(vec![format!("$ {:?}", cmd)]));
        let done = Arc::new(Mutex::new(None));
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        match cmd.spawn() {
            Err(e) => {
                lock(&out).push(format!("не удалось запустить: {}", e));
                self.st.job_out = lock(&out).clone();
                self.st.job_ok = Some(false);
                self.toast(Tone::Err, format!("{}: {}", name, e));
            }
            Ok(mut child) => {
                let (o2, d2) = (out.clone(), done.clone());
                let so = child.stdout.take();
                let se = child.stderr.take();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    let o3 = o2.clone();
                    let h = se.map(|se| {
                        std::thread::spawn(move || {
                            for l in std::io::BufReader::new(se).lines().map_while(Result::ok) {
                                lock(&o3).push(l);
                            }
                        })
                    });
                    if let Some(so) = so {
                        for l in std::io::BufReader::new(so).lines().map_while(Result::ok) {
                            lock(&o2).push(l);
                        }
                    }
                    if let Some(h) = h {
                        let _ = h.join();
                    }
                    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
                    *lock(&d2) = Some(ok);
                });
                self.st.job = Some(name.to_string());
                self.st.job_ok = None;
                self.bg = Some(BgJob { name: name.to_string(), out, done, reload_elf });
            }
        }
    }

    fn shell(cmdline: &str, dir: &str, toolchain: &str) -> Command {
        let mut c = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", cmdline]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", cmdline]);
            c
        };
        c.current_dir(dir);
        if !toolchain.is_empty() {
            let sep = if cfg!(windows) { ";" } else { ":" };
            let path = std::env::var("PATH").unwrap_or_default();
            c.env("PATH", format!("{}{}{}", toolchain, sep, path));
        } else if cfg!(target_os = "macos") {
            // GUI apps start without the shell PATH: add Homebrew
            let path = std::env::var("PATH").unwrap_or_default();
            c.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{}", path));
        }
        c
    }

    fn run(&mut self, job: Job) {
        match job {
            Job::ProtoOpen(name) => match ProtoPort::open(&name) {
                Ok(p) => {
                    self.proto = Some(Proto::Serial(p));
                    self.st.proto = Some(name.clone());
                    self.st.fw = None;
                    let r = self.transact(vec![CMD_PING], false, false);
                    match r {
                        Some(_) => self.toast(Tone::Ok, format!("UART1 {}: плата ответила", name)),
                        None => self.toast(
                            Tone::Warn,
                            format!("UART1 {} открыт, но PING без ответа — проверьте провода TX↔RX и что это USART1", name),
                        ),
                    }
                }
                Err(e) => self.toast(Tone::Err, e),
            },
            Job::ProtoSim => {
                self.proto = Some(Proto::Sim);
                self.st.proto = Some("виртуальная плата".into());
                self.transact(vec![CMD_PING], false, false);
                self.toast(Tone::Info, "UART1 → виртуальная плата");
            }
            Job::ProtoClose => {
                self.proto = None;
                self.st.proto = None;
                self.st.polling = false;
            }
            Job::LogOpen(name) => {
                let sink = Arc::new(Mutex::new(vec![]));
                match LogPort::open(&name, sink.clone()) {
                    Ok(p) => {
                        self.log = Some(LogSrc::Serial(p, sink));
                        self.st.log = Some(name.clone());
                        self.toast(Tone::Ok, format!("Лог UART3: {}", name));
                    }
                    Err(e) => self.toast(Tone::Err, e),
                }
            }
            Job::LogSim => {
                self.log = Some(LogSrc::Sim);
                self.st.log = Some("виртуальная плата".into());
            }
            Job::LogClose => {
                self.log = None;
                self.st.log = None;
            }
            Job::Send { body, raw } => {
                if self.proto.is_none() {
                    self.toast(Tone::Warn, "UART1 не подключён");
                } else {
                    self.transact(body, raw, false);
                }
            }
            Job::Poll(on) => {
                self.st.polling = on && self.proto.is_some();
                if on && self.proto.is_none() {
                    self.toast(Tone::Warn, "Сначала подключите UART1");
                }
            }
            Job::PollMs(ms) => self.st.poll_ms = ms.max(200),
            Job::Stress(n) => {
                if self.proto.is_none() {
                    self.toast(Tone::Warn, "UART1 не подключён");
                    return;
                }
                let t1 = Instant::now();
                let mut bad = 0;
                for i in 0..n {
                    let ok = self.transact(vec![CMD_PING], false, true).is_some_and(|r| r.status == ST_OK);
                    if !ok {
                        bad += 1;
                    }
                    if self.proto.is_none() {
                        bad += n - i - 1;
                        break;
                    }
                }
                let avg = t1.elapsed().as_secs_f64() * 1000.0 / n.max(1) as f64;
                self.toast(
                    if bad == 0 { Tone::Ok } else { Tone::Err },
                    format!("Стресс-тест: {} PING, ошибок {}, в среднем {:.1} мс на обмен", n, bad, avg),
                );
            }
            Job::SwdOpen(cfg) => {
                self.swd = None;
                match OpenOcd::start(&cfg) {
                    Ok(mut o) => {
                        self.st.swd_voltage = o.target_voltage();
                        self.st.idcode = o.read_words(0xE004_2000, 1).ok().map(|v| v[0]);
                        self.st.ocd_console = o.log_tail(30);
                        self.swd = Some(Swd::Real(Box::new(o)));
                        self.st.swd = Some("ST-Link (OpenOCD)".into());
                        self.swd_refresh_state();
                        self.toast(Tone::Ok, "SWD подключён");
                    }
                    Err(e) => {
                        self.st.swd = None;
                        self.toast(Tone::Err, e);
                    }
                }
            }
            Job::SwdSim => {
                self.swd = Some(Swd::Sim);
                self.st.swd = Some("виртуальная плата".into());
                self.st.swd_voltage = Some("3.30 В".into());
                self.st.idcode = Some(0x2000_0410);
                self.st.swd_state = "running".into();
            }
            Job::SwdClose => {
                self.swd = None;
                self.st.swd = None;
                self.st.regs.clear();
            }
            Job::SwdCtl(c) => {
                match self.swd.as_mut() {
                    Some(Swd::Real(o)) => {
                        if let Err(e) = o.eval(c) {
                            self.toast(Tone::Err, format!("{}: {}", c, e));
                        }
                    }
                    Some(Swd::Sim) => {
                        if c.starts_with("reset") {
                            self.sim.xfer(&frame(&[CMD_RESET]));
                        }
                        self.toast(Tone::Info, format!("Симулятор: «{}» имитирован", c));
                    }
                    None => self.toast(Tone::Warn, "SWD не подключён"),
                }
                self.swd_refresh_state();
                self.run(Job::SwdRegs);
            }
            Job::SwdRegs => {
                let regs = match self.swd.as_mut() {
                    Some(Swd::Real(o)) => o.core_regs(),
                    Some(Swd::Sim) => Ok(vec![
                        ("pc".into(), self.st.elf.as_ref().and_then(|e| e.sym("prvIdleTask")).map(|s| s.addr + 0x10).unwrap_or(0x0800_1234)),
                        ("lr".into(), 0xFFFF_FFFD),
                        ("sp".into(), 0x2000_2F80),
                        ("xPSR".into(), 0x6100_0000),
                        ("msp".into(), 0x2000_4FD0),
                        ("psp".into(), 0x2000_2F80),
                        ("control".into(), 2),
                    ]),
                    None => Err("SWD не подключён".into()),
                };
                match regs {
                    Ok(r) => self.st.regs = r,
                    Err(e) => self.toast(Tone::Err, e),
                }
                self.swd_refresh_state();
            }
            Job::ReadDiag => {
                if matches!(self.swd, Some(Swd::Sim)) {
                    let s = self.sim.status();
                    self.st.diag = Some(Diag {
                        magic: crate::core::DIAG_MAGIC,
                        boot_count: s.boot_count,
                        reset_cause: s.reset_cause,
                        heartbeat: s.heartbeat,
                        rx_frames: s.rx_frames,
                        rx_crc_err: s.rx_crc_err,
                        fram_ok: s.fram_ok,
                        fram_fail: s.fram_fail,
                        ..Default::default()
                    });
                    return;
                }
                let Some(sym) = self.st.elf.as_ref().and_then(|e| e.sym("g_diag")).cloned() else {
                    self.toast(Tone::Warn, "В ELF нет символа g_diag — соберите прошивку этой ветки");
                    return;
                };
                match self.swd_read(sym.addr, DIAG_WORDS) {
                    Ok(w) => self.st.diag = Diag::from_words(&w),
                    Err(e) => self.toast(Tone::Err, e),
                }
            }
            Job::ReadWords(addrs) => {
                // group consecutive addresses into block reads
                let mut sorted = addrs;
                sorted.sort_unstable();
                sorted.dedup();
                let mut i = 0;
                while i < sorted.len() {
                    let start = sorted[i];
                    let mut n = 1;
                    while i + n < sorted.len() && sorted[i + n] == start + 4 * n as u32 && n < 64 {
                        n += 1;
                    }
                    match self.swd_read(start, n as u32) {
                        Ok(v) => {
                            for (k, w) in v.into_iter().enumerate() {
                                self.st.mem.insert(start + 4 * k as u32, w);
                            }
                        }
                        Err(e) => {
                            self.toast(Tone::Err, e);
                            break;
                        }
                    }
                    i += n;
                }
                self.st.mem_t = self.now();
            }
            Job::WriteWord(addr, val) => {
                let r = match self.swd.as_mut() {
                    Some(Swd::Real(o)) => o.write_word(addr, val),
                    Some(Swd::Sim) => Err("в симуляторе запись регистров не моделируется".into()),
                    None => Err("SWD не подключён".into()),
                };
                match r {
                    Ok(()) => {
                        self.toast(Tone::Ok, format!("0x{:08X} ← 0x{:08X}", addr, val));
                        self.run(Job::ReadWords(vec![addr]));
                    }
                    Err(e) => self.toast(Tone::Err, e),
                }
            }
            Job::OcdExec(c) => {
                let r = match self.swd.as_mut() {
                    Some(Swd::Real(o)) => o.eval(&c),
                    _ => Err("нужен реальный SWD (OpenOCD)".into()),
                };
                self.st.ocd_console.push(format!("> {}", c));
                match r {
                    Ok(s) => self.st.ocd_console.extend(s.lines().map(String::from)),
                    Err(e) => self.st.ocd_console.push(format!("ошибка: {}", e)),
                }
                let n = self.st.ocd_console.len();
                if n > 400 {
                    self.st.ocd_console.drain(..n - 400);
                }
            }
            Job::LoadElf(p) => self.load_elf(&p, true),
            Job::Build(cfg) => {
                let c = Self::shell(&cfg.build_cmd, &cfg.project_dir, &cfg.toolchain_dir);
                self.toolchain = cfg.toolchain_dir.clone();
                self.spawn_bg("Сборка", c, true);
            }
            Job::Flash(cfg) => {
                let elf = self.st.elf.as_ref().map(|e| e.path.clone());
                let Some(elf) = elf else {
                    self.toast(Tone::Err, "Нет ELF: сначала соберите прошивку");
                    return;
                };
                if self.swd.is_none() || matches!(self.swd, Some(Swd::Sim)) {
                    self.run(Job::SwdOpen(cfg.ocd.clone()));
                }
                let path = elf.to_string_lossy().replace('\\', "/");
                let r = match self.swd.as_mut() {
                    Some(Swd::Real(o)) => o.eval(&format!("program {{{}}} verify reset", path)),
                    _ => Err("SWD не подключён".into()),
                };
                self.st.job_out = vec![format!("program {} verify reset", path)];
                match r {
                    Ok(s) => {
                        self.st.job_out.extend(s.lines().map(String::from));
                        self.st.job_ok = Some(true);
                        self.toast(Tone::Ok, "Прошито и проверено, плата перезапущена");
                    }
                    Err(e) => {
                        self.st.job_out.push(e.clone());
                        self.st.job_ok = Some(false);
                        self.toast(Tone::Err, format!("Прошивка: {}", e));
                    }
                }
                self.swd_refresh_state();
            }
            Job::HilTest(cfg) => {
                // hil.py needs the ports and the ST-Link for itself
                self.proto = None;
                self.log = None;
                self.swd = None;
                self.st.proto = None;
                self.st.log = None;
                self.st.swd = None;
                self.st.polling = false;
                let py = cfg.python();
                let mut c = Command::new(if py.exists() { py.to_string_lossy().to_string() } else { "python3".into() });
                c.arg("tools/hil.py").arg("test").current_dir(&cfg.project_dir);
                if !cfg.proto_port.is_empty() {
                    c.env("HIL_PROTO_PORT", &cfg.proto_port);
                }
                if !cfg.log_port.is_empty() {
                    c.env("HIL_LOG_PORT", &cfg.log_port);
                }
                let path = std::env::var("PATH").unwrap_or_default();
                let extra = if cfg.toolchain_dir.is_empty() { "/opt/homebrew/bin:/usr/local/bin".to_string() } else { cfg.toolchain_dir.clone() };
                c.env("PATH", format!("{}{}{}", extra, if cfg!(windows) { ";" } else { ":" }, path));
                self.toast(Tone::Info, "Порты и ST-Link освобождены для автотеста hil.py");
                self.spawn_bg("Автотест hil.py", c, true);
            }
            Job::SimFram(on) => {
                self.sim.fram_present = on;
                self.st.sim_fram = on;
            }
        }
    }
}
