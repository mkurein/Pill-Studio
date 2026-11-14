//! Shell: navigation, top bar with link status, toasts, frame loop.
//! Views get `&Shared` and queue `Act`s, which run after the frame.

use crate::core::boards::{self, BoardId};
use crate::core::config::Config;
use crate::core::link::{self, PortInfo};
use crate::core::svd::{self, Peripheral};
use crate::theme::{self, Pal};
use crate::views;
use crate::widgets::chip;
use crate::worker::{Job, LogLine, Sample, State, Tone, Txn, Worker};
use eframe::egui::{self, RichText};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Overview,
    Board,
    Mcu,
    Regs,
    Proto,
    Rtos,
    Log,
    Debug,
    Learn,
    Settings,
}

/// (view, icon, title, hint)
pub const NAV: &[(View, &str, &str, &str)] = &[
    (View::Overview, "◉", "Обзор", "подключения и главное о плате"),
    (View::Board, "▦", "Плата", "выводы, функции, что подключено"),
    (View::Mcu, "◫", "Контроллер", "структура, память, тактирование"),
    (View::Regs, "≣", "Регистры", "все регистры из SVD, живое чтение по SWD"),
    (View::Proto, "⇄", "UART-протокол", "команды, кадры, CRC, реакция платы"),
    (View::Rtos, "⟳", "FreeRTOS", "задачи, стеки, куча, счётчики"),
    (View::Log, "☰", "Лог", "текстовый лог прошивки"),
    (View::Debug, "⚙", "SWD-отладка", "ядро, сбои, сборка, прошивка, автотест"),
    (View::Learn, "✎", "Справочник", "как устроен контроллер и этот проект"),
    (View::Settings, "⚒", "Настройки", "порты, OpenOCD, пути"),
];

pub enum Act {
    Job(Box<Job>),
    Go(View),
    Toast(Tone, String),
    SaveCfg(Box<Config>),
    /// Switch the studio to another board (profile, SVD, project, ports).
    Board(BoardId),
    /// Make this firmware folder the active project; switches the board when
    /// the folder belongs to the other one.
    Project(String),
    RefreshPorts,
    Theme(bool),
    OpenPeriph(String),
    ClearTxns,
    ClearLog,
}

pub struct Shared {
    pub st: State,
    pub svd: Arc<Vec<Peripheral>>,
    pub txns: VecDeque<Txn>,
    pub lines: VecDeque<LogLine>,
    pub samples: VecDeque<Sample>,
    pub cfg: Config,
    pub ports: Vec<PortInfo>,
    pub pal: Pal,
    pub view: View,
    pub busy: bool,
    acts: RefCell<Vec<Act>>,
}

impl Shared {
    pub fn act(&self, a: Act) {
        self.acts.borrow_mut().push(a);
    }
    pub fn job(&self, j: Job) {
        self.act(Act::Job(Box::new(j)));
    }
    pub fn send(&self, body: Vec<u8>) {
        self.job(Job::Send { body, raw: false });
    }
    pub fn periph(&self, name: &str) -> Option<&Peripheral> {
        self.svd.iter().find(|p| p.name == name)
    }
    /// Value read over SWD for this address, if any.
    pub fn mem(&self, addr: u32) -> Option<u32> {
        self.st.mem.get(&addr).copied()
    }
    pub fn uart_ok(&self) -> bool {
        self.st.proto.is_some()
    }
    pub fn swd_ok(&self) -> bool {
        self.st.swd.is_some()
    }
}

#[derive(Default)]
pub struct ViewState {
    pub overview: views::overview::St,
    pub board: views::board::St,
    pub mcu: views::mcu::St,
    pub regs: views::regs::St,
    pub proto: views::proto_view::St,
    pub rtos: views::rtos::St,
    pub log: views::log::St,
    pub debug: views::debug::St,
    pub learn: views::learn::St,
    pub settings: views::settings::St,
}

struct ToastMsg {
    tone: Tone,
    text: String,
    until: Instant,
}

pub struct App {
    sh: Shared,
    vs: ViewState,
    worker: Worker,
    toasts: Vec<ToastMsg>,
    busy_since: Option<Instant>,
    shots: Option<crate::shots::Shots>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> App {
        let mut cfg = Config::load();
        // screenshot tour of another board: APP_BOARD=blackpill (not saved)
        if let Some(id) = std::env::var("APP_BOARD").ok().and_then(|s| BoardId::ALL.into_iter().find(|b| b.key() == s)) {
            cfg.switch_board(id);
        }
        theme::install_fonts(&cc.egui_ctx);
        let pal = Pal::get(cfg.dark);
        theme::apply(&cc.egui_ctx, &pal);
        let svd = Arc::new(svd::parse_board(boards::cur()));
        let worker = Worker::spawn(cc.egui_ctx.clone(), svd.clone(), &cfg);
        let shots = crate::shots::Shots::from_env();
        if shots.is_some() {
            // demo data for screenshots: the virtual board on every link
            worker.send(Job::ProtoSim);
            worker.send(Job::LogSim);
            worker.send(Job::SwdSim);
            worker.send(Job::Poll(true));
        }
        App {
            sh: Shared {
                st: State::default(),
                svd,
                txns: VecDeque::new(),
                lines: VecDeque::new(),
                samples: VecDeque::new(),
                ports: link::list_ports(),
                cfg,
                pal,
                view: View::Overview,
                busy: false,
                acts: RefCell::new(vec![]),
            },
            vs: ViewState::default(),
            worker,
            toasts: vec![],
            busy_since: None,
            shots,
        }
    }

    fn refresh(&mut self) {
        let u = self.worker.take();
        if let Some(s) = u.state {
            self.sh.st = s;
        }
        self.sh.txns.extend(u.txns);
        while self.sh.txns.len() > 3000 {
            self.sh.txns.pop_front();
        }
        self.sh.lines.extend(u.lines);
        while self.sh.lines.len() > 8000 {
            self.sh.lines.pop_front();
        }
        self.sh.samples.extend(u.samples);
        while self.sh.samples.len() > 3600 {
            self.sh.samples.pop_front();
        }
        for (tone, text) in u.toasts {
            self.toast(tone, text);
        }
        if !u.runs.is_empty() {
            for r in u.runs {
                self.sh.cfg.add_run(r);
            }
            self.sh.cfg.save();
        }
        let busy = self.worker.busy() || self.sh.st.job.is_some();
        self.busy_since = match (busy, self.busy_since) {
            (true, None) => Some(Instant::now()),
            (true, s) => s,
            (false, _) => None,
        };
        self.sh.busy = self.busy_since.is_some_and(|t| t.elapsed() > Duration::from_millis(150));
    }

    fn toast(&mut self, tone: Tone, text: String) {
        let secs = if tone == Tone::Err { 8 } else { 4 };
        self.toasts.push(ToastMsg { tone, text, until: Instant::now() + Duration::from_secs(secs) });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    /// The board changed in the config: its SVD, links closed, views reset.
    fn board_changed(&mut self) {
        let svd = Arc::new(svd::parse_board(boards::cur()));
        self.sh.svd = svd.clone();
        self.worker.send(Job::Board(svd, self.sh.cfg.project_dir.clone()));
        self.vs = ViewState::default();
        self.sh.txns.clear();
        self.sh.lines.clear();
        self.sh.samples.clear();
        self.sh.st = State::default();
        let b = boards::cur();
        let proj = if self.sh.cfg.project_dir.is_empty() { "не найден — укажите в Настройках".to_string() } else { self.sh.cfg.project_dir.clone() };
        self.toast(Tone::Info, format!("Плата: {} ({}). Проект: {}", b.board, b.chip, proj));
    }

    fn run(&mut self, ctx: &egui::Context, a: Act) {
        match a {
            Act::Job(j) => self.worker.send(*j),
            Act::Go(v) => self.sh.view = v,
            Act::Toast(t, s) => self.toast(t, s),
            Act::SaveCfg(c) => {
                if c.poll_ms != self.sh.cfg.poll_ms {
                    self.worker.send(Job::PollMs(c.poll_ms));
                }
                if c.editor() != self.sh.cfg.editor() {
                    self.worker.send(Job::Editor(c.editor()));
                }
                let project = c.project_dir != self.sh.cfg.project_dir;
                self.sh.cfg = *c;
                if project {
                    self.sh.cfg.touch_project();
                    self.worker.send(Job::Project(self.sh.cfg.project_dir.clone()));
                }
                self.sh.cfg.save();
            }
            Act::Project(dir) => match self.sh.cfg.use_project(&dir) {
                Err(e) => self.toast(Tone::Err, e),
                Ok(true) => {
                    self.sh.cfg.save();
                    self.board_changed();
                }
                Ok(false) => {
                    self.sh.cfg.save();
                    self.worker.send(Job::Project(self.sh.cfg.project_dir.clone()));
                    self.toast(Tone::Info, format!("Проект: {}", self.sh.cfg.project_dir));
                }
            },
            Act::Board(id) => {
                if id == self.sh.cfg.board {
                    return;
                }
                self.sh.cfg.touch_project();
                self.sh.cfg.switch_board(id);
                self.sh.cfg.touch_project();
                self.sh.cfg.save();
                self.board_changed();
            }
            Act::RefreshPorts => self.sh.ports = link::list_ports(),
            Act::Theme(dark) => {
                self.sh.cfg.dark = dark;
                self.sh.cfg.save();
                self.sh.pal = Pal::get(dark);
                theme::apply(ctx, &self.sh.pal);
            }
            Act::OpenPeriph(name) => {
                self.vs.regs.select(&name);
                self.sh.view = View::Regs;
            }
            Act::ClearTxns => self.sh.txns.clear(),
            Act::ClearLog => self.sh.lines.clear(),
        }
    }

    fn top_bar(&self, ctx: &egui::Context) {
        let sh = &self.sh;
        let p = sh.pal;
        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::new().fill(p.bg2).inner_margin(egui::Margin::symmetric(14, 8)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Pill Studio").size(17.0).strong().color(p.fg));
                    let b = boards::cur();
                    egui::ComboBox::from_id_salt("board")
                        .selected_text(format!("{} · {}", b.board, b.chip))
                        .show_ui(ui, |ui| {
                            for id in BoardId::ALL {
                                let q = id.profile();
                                if ui.selectable_label(id == b.id, format!("{} · {} · {}", q.board, q.chip, q.core)).clicked() {
                                    sh.act(Act::Board(id));
                                }
                            }
                        })
                        .response
                        .on_hover_text("Плата: выводы, SVD, тактирование, цель OpenOCD, ELF и порты — свои для каждой");
                    // the chip on the SWD link is not the selected board
                    if let Some(other) = sh.st.idcode.and_then(boards::by_idcode).filter(|o| *o != b.id) {
                        let t = format!("⚠ по SWD {} — переключить", other.profile().board);
                        if ui.button(RichText::new(t).color(p.warn)).clicked() {
                            sh.act(Act::Board(other));
                        }
                    }
                    ui.add_space(12.0);
                    let uart = sh.st.proto.as_deref();
                    if chip(ui, &p, "UART1", uart.unwrap_or("нет"), uart.is_some()).clicked() {
                        sh.act(Act::Go(View::Overview));
                    }
                    let log = sh.st.log.as_deref();
                    let log_name = boards::cur().log_uart.replace("USART", "UART");
                    if chip(ui, &p, &log_name, log.unwrap_or("нет"), log.is_some()).clicked() {
                        sh.act(Act::Go(View::Log));
                    }
                    let swd = sh.st.swd.as_ref().map(|s| format!("{} · {}", s, sh.st.swd_state));
                    if chip(ui, &p, "SWD", swd.as_deref().unwrap_or("нет"), swd.is_some()).clicked() {
                        sh.act(Act::Go(View::Debug));
                    }
                    if sh.st.sim_running()
                        && ui.button(RichText::new("Остановить симуляцию").color(p.err)).clicked()
                    {
                        sh.job(Job::SimStop);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dark = sh.cfg.dark;
                        if ui.button(if dark { "☀ Светлая" } else { "☾ Тёмная" }).clicked() {
                            sh.act(Act::Theme(!dark));
                        }
                        if sh.busy {
                            ui.spinner();
                            if let Some(j) = &sh.st.job {
                                ui.label(RichText::new(j).color(p.warn));
                            }
                        }
                        if let Some(fw) = &sh.st.fw {
                            ui.label(RichText::new(format!("прошивка: {}", fw)).color(p.fg2).small());
                        }
                    });
                });
            });
    }

    fn sidebar(&self, ctx: &egui::Context) {
        let sh = &self.sh;
        let p = sh.pal;
        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(212.0)
            .frame(egui::Frame::new().fill(p.bg2).inner_margin(egui::Margin::symmetric(8, 10)))
            .show(ctx, |ui| {
                let b = boards::cur();
                for (v, icon, title, hint) in NAV {
                    let sel = sh.view == *v;
                    let title = match v {
                        View::Board => format!("{} {}", title, b.board),
                        View::Log => format!("{} {}", title, b.log_uart.replace("USART", "UART")),
                        _ => title.to_string(),
                    };
                    let text = RichText::new(format!("{}  {}", icon, title)).size(14.0).color(if sel { p.accent } else { p.fg });
                    let r = ui
                        .add_sized([196.0, 30.0], egui::Button::selectable(sel, text))
                        .on_hover_text(*hint);
                    if r.clicked() {
                        sh.act(Act::Go(*v));
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.hyperlink_to(RichText::new(b.repo).small(), format!("https://{}", b.repo));
                    ui.label(RichText::new("Developer Kurein MN").small().color(p.muted));
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).small().color(p.muted));
                    if let Some(s) = sh.st.status {
                        ui.label(
                            RichText::new(format!("аптайм {}", crate::fmt::uptime(s.uptime_ms)))
                                .small()
                                .color(p.muted),
                        );
                    }
                });
            });
    }

    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        let p = self.sh.pal;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                for t in &self.toasts {
                    let c = match t.tone {
                        Tone::Ok => p.ok,
                        Tone::Info => p.accent,
                        Tone::Warn => p.warn,
                        Tone::Err => p.err,
                    };
                    egui::Frame::new()
                        .fill(p.panel)
                        .stroke(egui::Stroke::new(1.0, c))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&t.text).color(p.fg));
                        });
                    ui.add_space(6.0);
                }
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some((view, dark)) = self.shots.as_ref().and_then(|s| s.current()) {
            self.sh.view = view;
            if self.sh.pal.dark != dark {
                self.sh.pal = Pal::get(dark);
                theme::apply(ctx, &self.sh.pal);
            }
        }
        self.refresh();
        self.top_bar(ctx);
        self.sidebar(ctx);
        let p = self.sh.pal;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(18, 14)))
            .show(ctx, |ui| {
                let sh = &self.sh;
                let vs = &mut self.vs;
                match sh.view {
                    View::Overview => views::overview::show(ui, sh, &mut vs.overview),
                    View::Board => views::board::show(ui, sh, &mut vs.board),
                    View::Mcu => views::mcu::show(ui, sh, &mut vs.mcu),
                    View::Regs => views::regs::show(ui, sh, &mut vs.regs),
                    View::Proto => views::proto_view::show(ui, sh, &mut vs.proto),
                    View::Rtos => views::rtos::show(ui, sh, &mut vs.rtos),
                    View::Log => views::log::show(ui, sh, &mut vs.log),
                    View::Debug => views::debug::show(ui, sh, &mut vs.debug),
                    View::Learn => views::learn::show(ui, sh, &mut vs.learn),
                    View::Settings => views::settings::show(ui, sh, &mut vs.settings),
                }
            });
        self.show_toasts(ctx);
        let acts: Vec<Act> = self.sh.acts.borrow_mut().drain(..).collect();
        for a in acts {
            self.run(ctx, a);
        }
        // live views refresh without input
        if self.sh.st.polling || self.sh.st.log.is_some() || self.sh.st.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(s) = &mut self.shots {
            s.tick(ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.sh.cfg.save();
    }
}
