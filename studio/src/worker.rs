//! I/O thread. The UI never touches a port, OpenOCD or a process: it sends
//! `Job`s, the worker runs them and publishes `State` + streamed events into an
//! inbox that the UI drains once per frame (pattern from DigiPot Studio).

use crate::core::config::{self, Config, Run};
use crate::core::debug::{self, AsmLine, StepKind};
use crate::core::elf::{self, Elf};
use crate::core::faults::{self, Step};
use crate::core::link::{LogPort, ProtoPort};
use crate::core::boards;
use crate::core::openocd::{OcdConfig, OpenOcd};
use crate::core::proto::{self, *};
use crate::core::sim::SimBoard;
use crate::core::svd::Peripheral;
use crate::core::{Diag, DIAG_WORDS};
use eframe::egui;
use std::collections::{HashMap, VecDeque};
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
    /// Bytes with pauses, line format changes and BREAK (fault injection).
    Plan { label: String, steps: Vec<Step> },
    /// Run these cases of `faults::cases()`, each followed by STATUS.
    Robust(Vec<usize>),
    RobustStop,
    /// Reset the board halted and run it from one of these addresses to the
    /// next with breakpoints, recording the order and the cycle counter.
    BootTrace(Vec<u32>),
    /// Compare the board's Flash with the loaded ELF.
    VerifyFlash,
    SwdOpen(OcdConfig),
    SwdSim,
    SwdClose,
    /// halt / resume / "reset run" / step …
    SwdCtl(&'static str),
    /// Into a call, over a call, or out to the caller. Only while halted.
    SwdStep(StepKind),
    SwdRegs,
    /// Hardware breakpoint at a Thumb address (bit 0 already cleared).
    SwdBpSet(u32),
    SwdBpClr(u32),
    /// Editor CLI for "open the stop line" (`None` = off).
    Editor(Option<String>),
    ReadDiag,
    ReadWords(Vec<u32>),
    WriteWord(u32, u32),
    OcdExec(String),
    LoadElf(PathBuf),
    /// Another project of the same board became active: reload its ELF.
    Project(String),
    /// Build, and flash right after a successful build.
    BuildFlash(Config),
    Build(Config),
    Flash(Config),
    HilTest(Config),
    SimFram(bool),
    /// Drop every virtual-board link. Real UART and OpenOCD stay open.
    SimStop,
    /// Another board was selected (`boards::cur()` is already switched): close
    /// every link, take its SVD and project, reload the ELF.
    Board(Arc<Vec<Peripheral>>, String),
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
    /// Instructions around PC, filled only while the core is halted.
    pub disasm: Vec<AsmLine>,
    /// PC and callers (exception frame or LR).
    pub frames: Vec<u32>,
    pub breaks: Vec<u32>,
    pub diag: Option<Diag>,
    pub mem: HashMap<u32, u32>,
    pub mem_t: f64,
    pub elf: Option<Arc<Elf>>,
    pub job: Option<String>,
    pub job_out: Vec<String>,
    pub job_ok: Option<bool>,
    /// Git state of the project: "main a1b2c3d +изм.", empty outside git.
    pub git: String,
    /// Newest change of the project sources (Core/, Makefile, *.ld), unix s.
    pub src_mtime: Option<u64>,
    /// When the loaded ELF was written, unix s.
    pub elf_mtime: Option<u64>,
    pub ocd_console: Vec<String>,
    pub sim_fram: bool,
    /// Results of the reliability test, in the order run.
    pub robust: Vec<RobustRow>,
    /// Cases still queued (progress while the test runs).
    pub robust_left: usize,
    /// Counter changes over the whole run.
    pub robust_total: Option<String>,
    /// Steps reached by the last live boot trace.
    pub boot_trace: Vec<crate::core::boot::Hit>,
    /// How that trace ended ("" = not run yet).
    pub boot_note: String,
    /// Flash of the connected board compared with the loaded ELF.
    pub flash_check: Option<FlashCheck>,
    /// Last TASKS reply and the one before it (CPU share = their difference).
    pub tasks: Option<Tasks>,
    pub tasks_prev: Option<Tasks>,
    /// The firmware answered UNKNOWN_CMD to TASKS: it is older than the command.
    pub tasks_missing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashCheck {
    pub same: bool,
    /// First address that differs.
    pub at: Option<u32>,
    pub bytes: u32,
    /// Build time of the ELF that was compared.
    pub elf_mtime: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct RobustRow {
    pub case: usize,
    pub got: String,
    /// What changed in STATUS: "crc +1, uart +2".
    pub delta: String,
    pub alive: bool,
    pub ok: bool,
}

/// Counter changes between two STATUS replies, only the ones that moved.
/// `b` must be the STATUS read right after the case.
pub fn status_delta(a: &Status, b: &Status) -> String {
    let d = |x: u32, y: u32| y.wrapping_sub(x) as i64;
    let mut v = vec![];
    for (name, x, y) in [
        // `b` itself is a STATUS reply, so its own frame is not counted
        ("кадров", a.rx_frames.wrapping_add(1), b.rx_frames),
        ("crc", a.rx_crc_err, b.rx_crc_err),
        ("uart", a.uart_err, b.uart_err),
        ("очередь", a.queue_full, b.queue_full),
    ] {
        let n = d(x, y);
        if n != 0 {
            v.push(format!("{name} +{n}"));
        }
    }
    if b.boot_count != a.boot_count {
        v.push("ПЕРЕЗАГРУЗКА".into());
    }
    if v.is_empty() {
        "—".into()
    } else {
        v.join(", ")
    }
}

impl State {
    /// True when UART1, UART3 or SWD is the virtual board.
    pub fn sim_running(&self) -> bool {
        self.proto.as_deref() == Some("виртуальная плата")
            || self.log.as_deref() == Some("виртуальная плата")
            || self.swd.as_deref() == Some("виртуальная плата")
    }
}

#[derive(Default)]
struct Inbox {
    state: Option<State>,
    txns: Vec<Txn>,
    lines: Vec<LogLine>,
    samples: Vec<Sample>,
    toasts: Vec<(Tone, String)>,
    runs: Vec<Run>,
}

pub struct Update {
    pub state: Option<State>,
    pub txns: Vec<Txn>,
    pub lines: Vec<LogLine>,
    pub samples: Vec<Sample>,
    pub toasts: Vec<(Tone, String)>,
    /// Finished build / flash / autotest runs, for the saved history.
    pub runs: Vec<Run>,
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
    started: Instant,
    git: String,
    /// "Собрать и прошить": flash with this config after a good build.
    then_flash: Option<Box<Config>>,
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
    /// Cursor / VS Code CLI; on every stop it gets `-g file:line`.
    editor: Option<String>,
    /// Last line sent to the editor, so a repeated stop does not steal focus again.
    last_jump: Option<(String, u32)>,
    /// PC of the last stop the studio has shown. A new PC while halted = someone
    /// else (Cursor over GDB) stepped the core.
    last_halt_pc: Option<u32>,
    last_state_poll: Instant,
    /// Reliability cases still to run and STATUS before the next one.
    robust_q: VecDeque<usize>,
    robust_prev: Option<Status>,
    robust_first: Option<Status>,
    last_watch: Instant,
    /// State changed without new txns/lines/toasts: the UI still has to repaint.
    repaint: bool,
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
        let editor = cfg.editor();
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
                    editor,
                    last_jump: None,
                    last_halt_pc: None,
                    last_state_poll: Instant::now(),
                    robust_q: VecDeque::new(),
                    robust_prev: None,
                    robust_first: None,
                    last_watch: Instant::now() - Duration::from_secs(10),
                    repaint: false,
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
            runs: std::mem::take(&mut ib.runs),
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
        ib.runs.append(&mut self.out.runs);
        changed || self.bg.is_some() || std::mem::take(&mut self.repaint)
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
        if !self.robust_q.is_empty() {
            self.robust_step();
        } else if self.st.polling && self.proto.is_some() && self.last_poll.elapsed() >= Duration::from_millis(self.st.poll_ms) {
            self.last_poll = Instant::now();
            self.transact(vec![CMD_STATUS], false, true);
            if !self.st.tasks_missing && self.proto.is_some() {
                let r = self.transact(vec![CMD_TASKS], false, true);
                self.st.tasks_missing = r.is_some_and(|r| r.status == ST_UNKNOWN_CMD);
            }
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
        self.watch_stops();
        if self.last_watch.elapsed() >= Duration::from_secs(2) {
            self.last_watch = Instant::now();
            self.watch_project();
        }
        // background process
        if let Some(bg) = &self.bg {
            self.st.job_out = lock(&bg.out).clone();
            let done = *lock(&bg.done);
            if let Some(ok) = done {
                let name = bg.name.clone();
                let reload = bg.reload_elf;
                let run = Run {
                    t: config::now_secs(),
                    dir: self.project.to_string_lossy().into_owned(),
                    board: boards::cur().id,
                    what: name.clone(),
                    ok,
                    secs: bg.started.elapsed().as_secs_f32(),
                    git: bg.git.clone(),
                };
                let then = self.bg.take().and_then(|b| b.then_flash);
                self.out.runs.push(run);
                self.st.job = None;
                self.st.job_ok = Some(ok);
                self.toast(if ok { Tone::Ok } else { Tone::Err }, format!("{}: {}", name, if ok { "успешно" } else { "ошибка" }));
                if reload && ok {
                    self.reload_elf();
                }
                self.last_watch = Instant::now() - Duration::from_secs(10);
                if let (true, Some(cfg)) = (ok, then) {
                    let log = std::mem::take(&mut self.st.job_out);
                    self.run(Job::Flash(*cfg));
                    let flash = std::mem::take(&mut self.st.job_out);
                    self.st.job_out = log.into_iter().chain(std::iter::once(String::new())).chain(flash).collect();
                }
            }
        }
    }

    /// The core can also be stopped from Cursor (GDB on the same OpenOCD) or by a
    /// breakpoint. Every 300 ms: a new stop or a new PC while halted → read registers,
    /// refresh the studio and open the line in the editor.
    fn watch_stops(&mut self) {
        if !matches!(self.swd, Some(Swd::Real(_))) || self.last_state_poll.elapsed() < Duration::from_millis(300) {
            return;
        }
        self.last_state_poll = Instant::now();
        self.swd_refresh_state();
        if self.st.swd_state != "halted" {
            self.last_halt_pc = None;
            self.last_jump = None;
            return;
        }
        let Ok(pc) = self.read_pc() else { return };
        if self.last_halt_pc != Some(pc) {
            self.last_halt_pc = Some(pc);
            self.run(Job::SwdRegs);
        }
    }

    /// `cursor -g file:line` for the current PC. Quiet when following is off,
    /// there is no ELF / DWARF line, or the file is not on this machine.
    fn follow_editor(&mut self) {
        let pc = self.reg("pc") & !1;
        self.last_halt_pc = Some(pc);
        let Some(exe) = self.editor.clone() else { return };
        let Some(elf) = self.st.elf.clone() else { return };
        let Some(p) = elf::place(&elf, &self.toolchain, pc) else { return };
        let mut file = PathBuf::from(&p.file);
        if file.is_relative() {
            file = self.project.join(file);
        }
        if !file.is_file() {
            return;
        }
        let key = (file.to_string_lossy().into_owned(), p.line);
        if self.last_jump.as_ref() == Some(&key) {
            return;
        }
        let mut c = Command::new(&exe);
        c.arg("-g").arg(format!("{}:{}", key.0, key.1));
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        match c.spawn() {
            Ok(mut child) => {
                // the launcher exits at once; wait so it does not stay a zombie
                std::thread::spawn(move || child.wait());
                self.last_jump = Some(key);
            }
            Err(e) => {
                self.editor = None;
                self.toast(Tone::Warn, format!("Не удалось открыть строку в редакторе ({exe}): {e}. Путь — в Настройках."));
            }
        }
    }

    /// Sources newer than the ELF = edited (in Cursor) after the last build.
    /// Git is asked again only when the sources change.
    fn watch_project(&mut self) {
        let src = newest_source(&self.project);
        if src != self.st.src_mtime || self.st.git.is_empty() {
            self.st.git = git_state(&self.project);
        }
        let elf = self.st.elf.as_ref().and_then(|e| mtime(&e.path));
        self.repaint |= (src, elf) != (self.st.src_mtime, self.st.elf_mtime);
        self.st.src_mtime = src;
        self.st.elf_mtime = elf;
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
                self.st.elf_mtime = mtime(p);
                // a trace belongs to the build it was taken on
                self.st.boot_trace.clear();
                self.st.boot_note.clear();
                if toast {
                    self.toast(Tone::Ok, format!("ELF загружен: {} символов", e.syms.len()));
                }
                self.st.elf = Some(Arc::new(e));
                self.verify_flash();
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
        if !poll && !ok {
            self.toast(Tone::Err, format!("{label}: {verdict}"));
        } else if !poll && body.first().copied() == Some(CMD_STATUS) {
            if let Some(r) = &reply {
                if Status::decode(&r.payload).is_none() {
                    self.toast(Tone::Err, format!("STATUS: в ответе {} байт, нужно 56", r.payload.len()));
                }
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.out.txns.push(Txn { id, t, tx, rx, label, verdict, ok, ms, poll });
        reply
    }

    /// A fault-injection plan on USART1; every reply that came back goes into
    /// one journal row.
    fn transact_plan(&mut self, label: String, steps: &[Step], toast: bool) -> Option<Vec<u8>> {
        let tx = faults::tx_bytes(steps);
        let t = self.now();
        let t1 = Instant::now();
        let rx = match self.proto.as_mut()? {
            Proto::Sim => {
                std::thread::sleep(Duration::from_millis(3 + tx.len() as u64 / 4 + faults::wait_ms(steps)));
                self.sim.xfer_plan(steps)
            }
            Proto::Serial(p) => match p.xfer_plan(steps, Duration::from_millis(700 + faults::wait_ms(steps))) {
                Ok(v) => v,
                Err(e) => {
                    self.toast(Tone::Err, format!("UART1: {}", e));
                    return None;
                }
            },
        };
        let ms = t1.elapsed().as_secs_f64() * 1000.0;
        let (replies, junk) = faults::split_replies(&rx);
        let ok = !replies.is_empty() && junk == 0 && replies.iter().all(|r| r.status == ST_OK);
        let verdict = faults::summary(&rx);
        if toast && !ok {
            self.toast(Tone::Warn, format!("{label}: {verdict}"));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.out.txns.push(Txn { id, t, tx, rx: rx.clone(), label, verdict, ok, ms, poll: false });
        Some(rx)
    }

    /// STATUS without a journal row; None when the board does not answer.
    fn status_now(&mut self) -> Option<Status> {
        let r = self.transact(vec![CMD_STATUS], false, true)?;
        if r.status != ST_OK {
            return None;
        }
        Status::decode(&r.payload)
    }

    /// One reliability case: send it, then STATUS proves the board is alive
    /// and shows which counters moved.
    fn robust_step(&mut self) {
        let Some(i) = self.robust_q.pop_front() else { return };
        self.st.robust_left = self.robust_q.len();
        if self.proto.is_none() {
            self.robust_q.clear();
            self.st.robust_left = 0;
            return;
        }
        if self.robust_prev.is_none() {
            self.robust_prev = self.status_now();
            self.robust_first = self.robust_prev;
        }
        let cases = faults::cases();
        let Some(c) = cases.get(i) else { return };
        let rx = self.transact_plan(format!("тест: {}", c.title), &c.steps, false).unwrap_or_default();
        let expected = c.expect.met(&rx);
        // let the board finish the frame (20 ms gap) and any reply
        std::thread::sleep(Duration::from_millis(40));
        let after = self.status_now().or_else(|| {
            std::thread::sleep(Duration::from_millis(300));
            self.status_now()
        });
        let rebooted = matches!((&self.robust_prev, &after), (Some(a), Some(b)) if a.boot_count != b.boot_count);
        let delta = match (&self.robust_prev, &after) {
            (Some(a), Some(b)) => status_delta(a, b),
            _ => "—".into(),
        };
        let alive = after.is_some() && !rebooted;
        let row = RobustRow {
            case: i,
            got: faults::summary(&rx),
            delta,
            alive,
            ok: expected && alive,
        };
        self.st.robust.retain(|r| r.case != i);
        self.st.robust.push(row);
        if after.is_some() {
            self.robust_prev = after;
        }
        if self.robust_q.is_empty() {
            let rows = &self.st.robust;
            let bad = rows.iter().filter(|r| !r.ok).count();
            let dead = rows.iter().filter(|r| !r.alive).count();
            self.st.robust_total = match (&self.robust_first, &self.robust_prev) {
                (Some(a), Some(b)) => Some(status_delta(a, b)),
                _ => None,
            };
            self.robust_prev = None;
            self.robust_first = None;
            let msg = if dead > 0 {
                format!("Тест надёжности: плата перестала отвечать или перезагрузилась в {dead} случаях")
            } else if bad > 0 {
                format!("Тест надёжности: {} из {} случаев ответили не так, как ожидалось; плата жива", bad, rows.len())
            } else {
                format!("Тест надёжности: все {} случаев пройдены, плата жива", rows.len())
            };
            self.toast(if dead > 0 { Tone::Err } else if bad > 0 { Tone::Warn } else { Tone::Ok }, msg);
        }
    }

    /// Read the board's Flash (the core keeps running) and compare it with
    /// the code and constants of the loaded ELF.
    fn verify_flash(&mut self) {
        self.st.flash_check = None;
        let Some(elf) = self.st.elf.clone() else { return };
        let Some(Swd::Real(o)) = self.swd.as_mut() else { return };
        let mut bytes = 0u32;
        let mut at = None;
        'all: for (base, data) in &elf.flash_image {
            for (i, block) in data.chunks(1024).enumerate() {
                let addr = base + (i * 1024) as u32;
                let Ok(words) = o.read_words(addr, block.len().div_ceil(4) as u32) else { return };
                let got: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
                if let Some(k) = block.iter().zip(&got).position(|(a, b)| a != b) {
                    at = Some(addr + k as u32);
                    break 'all;
                }
                bytes += block.len() as u32;
            }
        }
        self.st.flash_check = Some(FlashCheck { same: at.is_none(), at, bytes, elf_mtime: self.st.elf_mtime });
        self.repaint = true;
    }

    /// Live boot trace over SWD. Only a few hardware breakpoints exist, so
    /// they are set on the next few steps not reached yet; whichever halts
    /// the core is recorded, and so the real order shows up by itself.
    fn boot_trace(&mut self, addrs: Vec<u32>) -> Result<String, String> {
        use crate::core::boot::Hit;
        const DEMCR: u32 = 0xE000_EDFC;
        const DWT_CTRL: u32 = 0xE000_1000;
        const DWT_CYCCNT: u32 = 0xE000_1004;
        const WINDOW: usize = 4;
        let Some(Swd::Real(o)) = self.swd.as_mut() else { return Err("нужен ST-Link: на виртуальной плате старт не трассируется".into()) };
        o.eval("reset halt")?;
        // cycle counter: trace on, counter zeroed and running
        let demcr = o.read_words(DEMCR, 1)?[0];
        o.write_word(DEMCR, demcr | 1 << 24)?;
        o.write_word(DWT_CYCCNT, 0)?;
        let ctrl = o.read_words(DWT_CTRL, 1)?[0];
        o.write_word(DWT_CTRL, ctrl | 1)?;
        let mut left: Vec<u32> = addrs.iter().map(|a| a & !1).collect();
        let mut hits: Vec<Hit> = vec![];
        let mut note = String::new();
        let pc = |o: &mut OpenOcd| -> Result<u32, String> {
            let s = o.eval("reg pc")?;
            let hex = s.rsplit("0x").next().unwrap_or("").split_whitespace().next().unwrap_or("").to_string();
            u32::from_str_radix(hex.trim(), 16).map(|a| a & !1).map_err(|_| format!("не разобрать PC: {s}"))
        };
        // `reset halt` stops on the first instruction, which is a step itself
        let at = pc(o)?;
        if let Some(i) = left.iter().position(|a| *a == at) {
            hits.push(Hit { addr: left.remove(i), order: 0, cycles: 0 });
        }
        while !left.is_empty() {
            let set: Vec<u32> = left.iter().take(WINDOW).copied().collect();
            let mut placed = vec![];
            let mut fail = None;
            for a in &set {
                match o.eval(&format!("bp 0x{a:08x} 2 hw")) {
                    Ok(_) => placed.push(*a),
                    Err(e) => {
                        fail = Some(e);
                        break;
                    }
                }
            }
            let mut halted = false;
            if fail.is_none() {
                o.eval("resume")?;
                let t0 = Instant::now();
                while t0.elapsed() < Duration::from_millis(2500) {
                    if o.state() == "halted" {
                        halted = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
            }
            let stop = if halted { pc(o).ok() } else { None };
            for a in &placed {
                let _ = o.eval(&format!("rbp 0x{a:08x}"));
            }
            if let Some(e) = fail {
                let _ = o.eval("resume");
                return Err(format!("точка останова не поставилась ({e}). Снимите свои точки останова и повторите."));
            }
            match stop.and_then(|at| left.iter().position(|a| *a == at)) {
                Some(i) => {
                    let cycles = o.read_words(DWT_CYCCNT, 1)?[0];
                    hits.push(Hit { addr: left.remove(i), order: hits.len(), cycles });
                }
                None => {
                    note = match stop {
                        Some(at) => format!("ядро остановилось в другом месте (0x{at:08X}) — возможно, сбой или своя точка останова"),
                        None => "за 2.5 с следующий шаг не достигнут: программа не дошла до него или шаг выполняется в другом порядке".to_string(),
                    };
                    break;
                }
            }
        }
        let _ = o.eval("resume");
        self.st.boot_trace = hits;
        let n = self.st.boot_trace.len();
        Ok(if note.is_empty() { format!("пройдено шагов: {n}, плата запущена") } else { format!("пройдено шагов: {n}; {note}. Плата запущена.") })
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
            Some(CMD_TASKS) => {
                if let Some(t) = Tasks::decode(&r.payload) {
                    // a reboot restarts the counters: the old sample is no base for a share
                    let rebooted = self.st.tasks.as_ref().is_some_and(|o| t.run_us < o.run_us && o.run_us < 0xF000_0000);
                    self.st.tasks_prev = if rebooted { None } else { self.st.tasks.take() };
                    self.st.tasks = Some(t);
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

    fn reg(&self, name: &str) -> u32 {
        self.st.regs.iter().find(|r| r.0 == name).map(|r| r.1).unwrap_or(0)
    }

    /// Disassembly and call stack. Only while halted: reading them on a running
    /// core would stop the firmware.
    fn swd_listing(&mut self) {
        if self.st.swd_state != "halted" {
            return;
        }
        let pc = self.reg("pc");
        let lr = self.reg("lr");
        let sp = self.reg("sp") & !3;
        if let Err(e) = self.fill_disasm(pc) {
            self.st.disasm.clear();
            self.toast(Tone::Warn, e);
        }
        let stack = match self.swd.as_mut() {
            Some(Swd::Real(o)) => o.read_words(sp, 8).unwrap_or_default(),
            _ => return,
        };
        self.st.frames = debug::cortex_frames(pc, lr, &stack);
    }

    fn ocd_eval(&mut self, c: &str) -> Result<(), String> {
        match self.swd.as_mut() {
            Some(Swd::Real(o)) => o.eval(c).map(|_| ()),
            _ => Err("SWD не подключён".into()),
        }
    }

    /// Temporary breakpoint, run, wait, remove it. The core is halted again afterwards.
    fn run_to(&mut self, addr: u32) -> Result<(), String> {
        let addr = addr & !1;
        self.ocd_eval(&format!("bp 0x{addr:08x} 2 hw"))?;
        let resume = self.ocd_eval("resume");
        let wait = self.ocd_eval("wait_halt 1500");
        let _ = self.ocd_eval(&format!("rbp 0x{addr:08x}"));
        if resume.is_err() || wait.is_err() {
            let _ = self.ocd_eval("halt");
        }
        resume.map_err(|e| format!("не удалось дойти до 0x{addr:08X}: {e}"))?;
        wait.map_err(|_| format!("вызов не вернулся за 1,5 с (0x{addr:08X}). Ядро остановлено."))
    }

    /// One instruction. A step into WFI must not block the worker for half a minute.
    fn step_insn(&mut self) -> Result<(), String> {
        if let Some(Swd::Real(o)) = self.swd.as_mut() {
            o.set_read_timeout(Duration::from_millis(1200));
        }
        let r = self.ocd_eval("step");
        if let Some(Swd::Real(o)) = self.swd.as_mut() {
            o.set_read_timeout(Duration::from_secs(30));
        }
        match r {
            Err(e) if cmd_timed_out(&e) => {
                let _ = self.ocd_eval("halt");
                Err("шаг завис: ядро ждёт событие (WFI). Оно остановлено. Поставьте точку и нажмите Продолжить.".into())
            }
            other => other,
        }
    }

    fn read_pc(&mut self) -> Result<u32, String> {
        let s = match self.swd.as_mut() {
            Some(Swd::Real(o)) => o.eval("reg pc")?,
            _ => return Err("SWD не подключён".into()),
        };
        let hex = s.rsplit("0x").next().unwrap_or("");
        let hex = hex.split_whitespace().next().unwrap_or("");
        u32::from_str_radix(hex.trim(), 16).map(|a| a & !1).map_err(|_| format!("не разобрать PC: {s}"))
    }

    /// Instruction at `pc` from the ELF (`objdump`). OpenOCD's disassembler is not used:
    /// this ST build has no Capstone and answers with an error.
    fn insn_text(&mut self, pc: u32) -> Result<String, String> {
        let pc = pc & !1;
        if let Some(text) = self.st.disasm.iter().find(|l| l.addr == pc).map(|l| l.text.clone()) {
            return Ok(text);
        }
        self.fill_disasm(pc).ok();
        Ok(self.st.disasm.iter().find(|l| l.addr == pc).map(|l| l.text.clone()).unwrap_or_default())
    }

    fn fill_disasm(&mut self, pc: u32) -> Result<(), String> {
        let Some(path) = self.st.elf.as_ref().map(|e| e.path.clone()) else {
            return Err("ELF не загружен — список инструкций не из чего собрать. Сначала «Собрать».".into());
        };
        let start = pc.saturating_sub(16) & !1;
        let text = elf::objdump_range(&path, &self.toolchain, start, pc.saturating_add(80))?;
        let lines = debug::parse_disasm(&text);
        if lines.is_empty() {
            return Err("В ELF нет инструкций вокруг PC.".into());
        }
        self.st.disasm = lines;
        Ok(())
    }

    /// File and 1-based line at PC. `None` without an ELF or without DWARF for this address.
    fn line_at(&mut self) -> Option<(String, u32)> {
        let pc = self.read_pc().ok()?;
        let elf = self.st.elf.as_ref()?;
        let p = elf::place(elf, &self.toolchain, pc)?;
        Some((p.file, p.line))
    }

    /// Walk instructions until the source line changes. A call is entered or skipped.
    /// Stops on a repeated PC so a one-line loop cannot flood ST-Link.
    fn step_line(&mut self, into: bool) -> Result<(), String> {
        let Some(start) = self.line_at() else {
            if self.st.elf.is_none() {
                self.toast(Tone::Warn, "ELF не загружен — шаг на одну инструкцию. Сначала «Собрать».");
            } else {
                self.toast(Tone::Warn, "Для этого адреса нет строки исходника — шаг на одну инструкцию.");
            }
            return self.step_insn();
        };
        let mut seen = Vec::new();
        for _ in 0..12 {
            let pc = self.read_pc()?;
            if seen.contains(&pc) {
                self.toast(Tone::Info, "Эта строка зациклена — останов здесь. Точка останова или Продолжить (F5).");
                return Ok(());
            }
            seen.push(pc);
            let text = self.insn_text(pc)?;
            if debug::is_call(&text) {
                if into {
                    return self.step_insn();
                }
                return self.run_to(pc.wrapping_add(4));
            }
            self.step_insn()?;
            if self.line_at().as_ref() != Some(&start) {
                return Ok(());
            }
        }
        self.toast(Tone::Info, "Длинная строка — останов после нескольких инструкций. Ещё раз F10 продолжит её.");
        Ok(())
    }

    /// `Ok(true)` means the core was running and this call only halted it.
    fn ensure_halted(&mut self) -> Result<bool, String> {
        self.swd_refresh_state();
        if self.st.swd_state == "halted" {
            return Ok(false);
        }
        self.ocd_eval("halt")?;
        self.swd_refresh_state();
        if self.st.swd_state != "halted" {
            return Err(format!("не удалось остановить ядро ({})", self.st.swd_state));
        }
        Ok(true)
    }

    fn swd_step(&mut self, kind: StepKind) -> Result<bool, String> {
        if self.ensure_halted()? {
            self.toast(Tone::Info, "Ядро остановлено на текущей строке. Ещё раз F10 или F11 — следующий шаг.");
            return Ok(true);
        }
        match kind {
            StepKind::Into => self.step_line(true)?,
            StepKind::Over => self.step_line(false)?,
            StepKind::Out => {
                let lr = self.reg("lr");
                let Some(ret) = debug::step_out_target(lr, &self.st.frames) else {
                    return Err("некуда выходить: адрес возврата не в программе".into());
                };
                self.run_to(ret)?;
            }
        }
        Ok(false)
    }

    fn swd_bp(&mut self, addr: u32, set: bool) {
        let addr = addr & !1;
        if matches!(self.swd, Some(Swd::Real(_))) {
            let cmd = if set { format!("bp 0x{addr:08x} 2 hw") } else { format!("rbp 0x{addr:08x}") };
            let set_res = match self.swd.as_mut() {
                Some(Swd::Real(o)) => o.eval(&cmd),
                _ => return,
            };
            if let Err(e) = set_res {
                self.toast(Tone::Err, format!("Точка останова: {e}"));
                return;
            }
            let listed = match self.swd.as_mut() {
                Some(Swd::Real(o)) => o.capture("bp"),
                _ => return,
            };
            match listed {
                Ok(s) => self.st.breaks = debug::parse_breakpoints(&s),
                Err(e) => self.toast(Tone::Warn, e),
            }
            self.toast(Tone::Ok, if set { format!("Точка 0x{addr:08X}") } else { format!("Снята 0x{addr:08X}") });
            return;
        }
        match self.swd.as_mut() {
            Some(Swd::Sim) => {
                if set {
                    if !self.st.breaks.contains(&addr) {
                        self.st.breaks.push(addr);
                    }
                    self.toast(Tone::Info, "На симуляторе точка только в списке, ядро не останавливается");
                } else {
                    self.st.breaks.retain(|a| *a != addr);
                }
            }
            None => self.toast(Tone::Warn, "SWD не подключён"),
            Some(Swd::Real(_)) => {}
        }
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
                let git = git_state(&self.project);
                self.bg = Some(BgJob { name: name.to_string(), out, done, reload_elf, started: Instant::now(), git, then_flash: None });
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
        } else if let Some(extra) = crate::core::config::cube_toolchain_path() {
            let path = std::env::var("PATH").unwrap_or_default();
            c.env("PATH", format!("{};{}", extra, path));
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
                    self.st.tasks = None;
                    self.st.tasks_prev = None;
                    self.st.tasks_missing = false;
                    let r = self.transact(vec![CMD_PING], false, false);
                    match r {
                        Some(_) => {
                            self.toast(Tone::Ok, format!("UART1 {}: плата ответила", name));
                            self.st.polling = true;
                            self.transact(vec![CMD_STATUS], false, false);
                        }
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
                self.st.tasks_missing = false;
                self.transact(vec![CMD_PING], false, false);
                self.st.polling = true;
                self.transact(vec![CMD_STATUS], false, false);
                self.toast(Tone::Info, "UART1 → виртуальная плата");
            }
            Job::ProtoClose => {
                self.st.tasks = None;
                self.st.tasks_prev = None;
                self.st.tasks_missing = false;
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
                        self.toast(Tone::Ok, format!("Лог {}: {}", boards::cur().log_uart, name));
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
            Job::Plan { label, steps } => {
                if self.proto.is_none() {
                    self.toast(Tone::Warn, "UART1 не подключён");
                } else {
                    self.transact_plan(label, &steps, true);
                }
            }
            Job::Robust(which) => {
                if self.proto.is_none() {
                    self.toast(Tone::Warn, "UART1 не подключён");
                    return;
                }
                if which.len() > 1 {
                    self.st.robust.clear();
                    self.st.robust_total = None;
                }
                self.robust_prev = None;
                self.robust_q = which.into();
                self.st.robust_left = self.robust_q.len();
            }
            Job::VerifyFlash => {
                if !matches!(self.swd, Some(Swd::Real(_))) {
                    self.toast(Tone::Warn, "Сверка с платой: нужен ST-Link");
                } else {
                    self.verify_flash();
                    match self.st.flash_check {
                        Some(c) if c.same => self.toast(Tone::Ok, format!("На плате эта сборка: {} Б совпадают", c.bytes)),
                        Some(c) => self.toast(Tone::Warn, format!("На плате другая программа: отличие с 0x{:08X}", c.at.unwrap_or(0))),
                        None => self.toast(Tone::Err, "Сверка с платой не удалась: Flash не читается"),
                    }
                }
            }
            Job::BootTrace(addrs) => {
                self.st.boot_trace.clear();
                let full = addrs.len();
                match self.boot_trace(addrs) {
                    Ok(note) => {
                        let all = self.st.boot_trace.len() == full;
                        self.toast(if all { Tone::Ok } else { Tone::Warn }, format!("Трасса старта: {note}"));
                        self.st.boot_note = note;
                    }
                    Err(e) => {
                        self.st.boot_note = format!("не выполнена: {e}");
                        self.toast(Tone::Err, format!("Трасса старта: {e}"));
                    }
                }
                self.last_halt_pc = None;
                self.swd_refresh_state();
            }
            Job::RobustStop => {
                self.robust_q.clear();
                self.st.robust_left = 0;
                self.robust_prev = None;
                self.robust_first = None;
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
                let cfg = cfg.with_target(boards::cur().ocd_target);
                match OpenOcd::start(&cfg) {
                    Ok(mut o) => {
                        self.st.swd_voltage = o.target_voltage();
                        self.st.idcode = o.read_words(0xE004_2000, 1).ok().map(|v| v[0]);
                        if let Some(other) = self.st.idcode.and_then(boards::by_idcode).filter(|b| *b != boards::cur().id) {
                            self.toast(
                                Tone::Warn,
                                format!("По SWD отвечает {} ({}), а выбрана {}. Переключите плату вверху слева.", other.profile().board, other.profile().chip, boards::cur().board),
                            );
                        }
                        self.st.ocd_console = o.log_tail(30);
                        self.swd = Some(Swd::Real(Box::new(o)));
                        self.st.swd = Some("ST-Link (OpenOCD)".into());
                        self.swd_refresh_state();
                        self.toast(Tone::Ok, "SWD подключён");
                        self.verify_flash();
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
                self.st.idcode = Some(boards::cur().idcode_sim);
                self.st.swd_state = "running".into();
            }
            Job::SwdClose => {
                self.swd = None;
                self.st.flash_check = None;
                self.st.swd = None;
                self.st.regs.clear();
                self.st.disasm.clear();
                self.st.frames.clear();
                self.st.breaks.clear();
            }
            Job::SwdCtl(c) => {
                match self.swd.as_mut() {
                    Some(Swd::Real(_)) => {}
                    Some(Swd::Sim) => {
                        if c.starts_with("reset") {
                            self.sim.xfer(&frame(&[CMD_RESET]));
                        }
                        self.toast(Tone::Info, format!("Симулятор: «{}» имитирован", c));
                        self.swd_refresh_state();
                    }
                    None => self.toast(Tone::Warn, "SWD не подключён"),
                }
                if !matches!(self.swd, Some(Swd::Real(_))) {
                    // simulator and disconnected are handled above
                } else {
                    self.swd_refresh_state();
                    let already = (c == "resume" && self.st.swd_state == "running") || (c == "halt" && self.st.swd_state == "halted");
                    if c == "resume" && self.st.swd_state == "running" {
                        self.toast(Tone::Info, "Ядро уже выполняется. Пауза — F6, шаг по строке — F10.");
                    } else if !already {
                        let err = match self.swd.as_mut() {
                            Some(Swd::Real(o)) => o.eval(c).err(),
                            _ => None,
                        };
                        if let Some(e) = err {
                            self.toast(Tone::Err, ctl_fail(c, &e));
                            if c == "resume" || c.starts_with("reset") {
                                let _ = self.ocd_eval("halt");
                            }
                        }
                    }
                    self.swd_refresh_state();
                    if self.st.swd_state == "halted" {
                        self.run(Job::SwdRegs);
                    }
                }
            }
            Job::SwdStep(kind) => {
                if !matches!(self.swd, Some(Swd::Real(_))) {
                    self.toast(Tone::Info, "Шаг по функциям работает на живом ST-Link");
                } else {
                    match self.swd_step(kind) {
                        Err(e) => {
                            self.toast(Tone::Err, e);
                            self.swd_refresh_state();
                        }
                        Ok(_) => self.swd_refresh_state(),
                    }
                    if self.st.swd_state == "halted" {
                        self.run(Job::SwdRegs);
                    }
                }
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
                self.swd_listing();
                if self.st.swd_state == "halted" && matches!(self.swd, Some(Swd::Real(_))) {
                    self.follow_editor();
                }
            }
            Job::Editor(e) => {
                self.editor = e;
                self.last_jump = None;
            }
            Job::SwdBpSet(addr) => self.swd_bp(addr, true),
            Job::SwdBpClr(addr) => self.swd_bp(addr, false),
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
            Job::Board(svd, project) => {
                self.proto = None;
                self.log = None;
                self.swd = None;
                self.svd = svd;
                self.sim = SimBoard::new();
                self.project = PathBuf::from(project);
                self.last_watch = Instant::now() - Duration::from_secs(10);
                let keep = (self.st.poll_ms, self.st.sim_fram);
                self.st = State { poll_ms: keep.0, sim_fram: keep.1, ..Default::default() };
                self.last_halt_pc = None;
                self.last_jump = None;
                for p in elf::candidates(&self.project) {
                    if p.exists() {
                        self.load_elf(&p, false);
                        break;
                    }
                }
            }
            Job::Build(cfg) => {
                let c = Self::shell(&cfg.build_cmd, &cfg.project_dir, &cfg.toolchain_dir);
                self.toolchain = cfg.toolchain_dir.clone();
                self.spawn_bg("Сборка", c, true);
            }
            Job::BuildFlash(cfg) => {
                if self.bg.is_some() {
                    self.toast(Tone::Warn, "Дождитесь окончания текущей операции");
                    return;
                }
                let c = Self::shell(&cfg.build_cmd, &cfg.project_dir, &cfg.toolchain_dir);
                self.toolchain = cfg.toolchain_dir.clone();
                self.spawn_bg("Сборка", c, true);
                if let Some(bg) = self.bg.as_mut() {
                    bg.then_flash = Some(Box::new(cfg));
                }
            }
            Job::Project(dir) => {
                self.project = PathBuf::from(dir);
                self.st.elf = None;
                self.st.elf_mtime = None;
                self.st.job_out.clear();
                self.st.job_ok = None;
                for p in elf::candidates(&self.project) {
                    if p.exists() {
                        self.load_elf(&p, false);
                        break;
                    }
                }
                self.st.git.clear();
                self.watch_project();
            }
            Job::Flash(cfg) => {
                let t1 = Instant::now();
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
                        self.verify_flash();
                    }
                    Err(e) => {
                        self.st.job_out.push(e.clone());
                        self.st.job_ok = Some(false);
                        self.toast(Tone::Err, format!("Прошивка: {}", e));
                    }
                }
                self.out.runs.push(Run {
                    t: config::now_secs(),
                    dir: self.project.to_string_lossy().into_owned(),
                    board: boards::cur().id,
                    what: "Прошивка".into(),
                    ok: self.st.job_ok == Some(true),
                    secs: t1.elapsed().as_secs_f32(),
                    git: self.st.git.clone(),
                });
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
            Job::SimStop => {
                let mut stopped = false;
                if matches!(self.proto, Some(Proto::Sim)) {
                    self.proto = None;
                    self.st.proto = None;
                    self.st.polling = false;
                    stopped = true;
                }
                if matches!(self.log, Some(LogSrc::Sim)) {
                    self.log = None;
                    self.st.log = None;
                    stopped = true;
                }
                if matches!(self.swd, Some(Swd::Sim)) {
                    self.swd = None;
                    self.st.swd = None;
                    self.st.regs.clear();
                    stopped = true;
                }
                if stopped {
                    self.toast(Tone::Info, "Симуляция остановлена");
                }
            }
            Job::SimFram(on) => {
                self.sim.fram_present = on;
                self.st.sim_fram = on;
            }
        }
    }
}

fn cmd_timed_out(e: &str) -> bool {
    let l = e.to_lowercase();
    l.contains("timed out") || l.contains("timeout") || l.contains("semaphore") || l.contains("таймаут") || l.contains("истекл")
}

fn ctl_fail(cmd: &str, err: &str) -> String {
    let what = match cmd {
        "resume" => "Не удалось продолжить",
        "halt" => "Не удалось остановить ядро",
        _ => "Команда отладки не прошла",
    };
    format!("{what}: {err}")
}

fn mtime(p: &std::path::Path) -> Option<u64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Newest modification time of the firmware sources: Core/ (.c .h .s),
/// the Makefile and linker scripts in the project root.
fn newest_source(project: &std::path::Path) -> Option<u64> {
    fn walk(d: &std::path::Path, depth: u32, best: &mut Option<u64>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 4 {
                    walk(&p, depth + 1, best);
                }
            } else if matches!(p.extension().and_then(|x| x.to_str()), Some("c" | "h" | "s" | "S")) {
                *best = (*best).max(mtime(&p));
            }
        }
    }
    let mut best = None;
    walk(&project.join("Core"), 0, &mut best);
    if let Ok(rd) = std::fs::read_dir(project) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "Makefile" || name.ends_with(".ld") {
                best = best.max(mtime(&p));
            }
        }
    }
    best
}

/// "main a1b2c3d +изм." (uncommitted changes to tracked files), empty outside git.
fn git_state(dir: &std::path::Path) -> String {
    let git = |args: &[&str]| -> Option<String> {
        let mut c = Command::new(config::find_tool(if cfg!(windows) { "git.exe" } else { "git" }));
        c.arg("-C").arg(dir).args(args).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let o = c.output().ok().filter(|o| o.status.success())?;
        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let Some(head) = git(&["log", "-1", "--format=%h"]) else { return String::new() };
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    format!("{branch} {head}{}", if dirty { " +изм." } else { "" })
}
