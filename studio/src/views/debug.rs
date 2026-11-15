//! SWD debugging: a halted-core view (disassembly, registers, call stack,
//! breakpoints), fault analysis from g_diag, symbols, the active firmware
//! project with build / flash / hil.py and their history, OpenOCD console.

use crate::app::{Act, Shared};
use crate::core::boards::BoardId;
use crate::core::config::{self, Config};
use crate::core::debug::{self, StepKind};
use crate::core::elf;
use crate::core::openocd::cfsr_ru;
use crate::core::proto;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct St {
    sym: String,
    console: String,
    bp: String,
    /// Last two register snapshots, so a changed value stays highlighted.
    seen: Vec<(String, u32)>,
    prev: Vec<(String, u32)>,
    /// addr2line is a process; keep the last answer instead of spawning it every frame.
    pc_at: Option<u32>,
    pc_txt: String,
    frame_at: Vec<u32>,
    frame_txt: Vec<String>,
    /// None: source follows PC. Some: a function or stack frame the user opened.
    browse: Option<u32>,
    fnq: String,
    src_key: Option<u32>,
    src_title: String,
    src_line: u32,
    src_lines: Vec<String>,
    src_scroll: bool,
    /// Folder typed in the project picker.
    proj_path: String,
    /// History of every project, not only the active one.
    hist_all: bool,
    /// "What is SWD" window is open.
    help: bool,
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(
            ui,
            &p,
            "SWD-отладка",
            "Шаг идёт по строкам исходника. F5 продолжить, F6 пауза, F10 следующая строка, F11 зайти в функцию, Shift+F11 выйти, F9 точка, Shift+F5 отключить.",
        );
        let label = if st.help { "✕ Скрыть описание SWD" } else { "ℹ Что такое SWD и как это работает" };
        if ui
            .button(label)
            .on_hover_text("провода, протокол, отладочные блоки ядра, цепочка Pill Studio → OpenOCD → ST-Link → чип, что делать, если плата не видна")
            .clicked()
        {
            st.help = !st.help;
        }
        if st.help {
            ui.add_space(6.0);
            card(ui, &p, "Что такое SWD и как это работает", |ui| super::swd_help::body(ui, &p));
            ui.add_space(8.0);
        }
        if st.seen != sh.st.regs {
            if !st.seen.is_empty() {
                st.prev.clone_from(&st.seen);
            }
            st.seen.clone_from(&sh.st.regs);
        }
        let pc = sh.st.regs.iter().find(|r| r.0 == "pc").map(|r| r.1);
        if st.pc_at != pc || st.frame_at != sh.st.frames {
            st.pc_at = pc;
            st.pc_txt = pc
                .and_then(|a| sh.st.elf.as_ref().map(|e| elf::where_is(e, &sh.cfg.toolchain_dir, a)))
                .unwrap_or_else(|| "ELF не загружен".into());
            st.frame_at.clone_from(&sh.st.frames);
            st.frame_txt = st
                .frame_at
                .iter()
                .map(|a| sh.st.elf.as_ref().map(|e| elf::where_is(e, &sh.cfg.toolchain_dir, *a)).unwrap_or_else(|| "—".into()))
                .collect();
        }
        let halted = sh.st.swd_state == "halted";
        let running = sh.st.swd_state == "running";
        if !sh.busy {
            let keys: Vec<(bool, bool, egui::Key)> = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((modifiers.shift, modifiers.ctrl || modifiers.command, *key)),
                        _ => None,
                    })
                    .collect()
            });
            for (shift, ctrl, key) in keys {
                if ctrl && !(shift && key == egui::Key::F5) {
                    continue;
                }
                match (shift, ctrl, key) {
                    (true, true, egui::Key::F5) if sh.swd_ok() => sh.job(Job::SwdCtl("reset run")),
                    (false, false, egui::Key::F5) if !sh.swd_ok() => {
                        sh.job(Job::SwdOpen(sh.cfg.ocd.clone()));
                    }
                    (false, false, egui::Key::F5) if halted => {
                        st.browse = None;
                        sh.job(Job::SwdCtl("resume"));
                    }
                    (false, false, egui::Key::F6) if sh.swd_ok() && !halted => {
                        st.browse = None;
                        sh.job(Job::SwdCtl("halt"));
                    }
                    (true, false, egui::Key::F5) if sh.swd_ok() => sh.job(Job::SwdClose),
                    (false, false, egui::Key::F9) if sh.swd_ok() => {
                        if let Some(a) = st.browse.or(pc) {
                            let a = a & !1;
                            if sh.st.breaks.contains(&a) {
                                sh.job(Job::SwdBpClr(a));
                            } else {
                                sh.job(Job::SwdBpSet(a));
                            }
                        }
                    }
                    (false, false, egui::Key::F10) if sh.swd_ok() => {
                        st.browse = None;
                        sh.job(Job::SwdStep(StepKind::Over));
                    }
                    (false, false, egui::Key::F11) if sh.swd_ok() => {
                        st.browse = None;
                        sh.job(Job::SwdStep(StepKind::Into));
                    }
                    (true, false, egui::Key::F11) if sh.swd_ok() => {
                        st.browse = None;
                        sh.job(Job::SwdStep(StepKind::Out));
                    }
                    _ => {}
                }
            }
        }
        project_card(ui, sh, st);
        ui.add_space(8.0);
        card(ui, &p, "", |ui| {
            ui.horizontal_wrapped(|ui| {
                if sh.swd_ok() {
                    let (label, color) = if halted {
                        ("остановлено", p.warn)
                    } else if running {
                        ("выполняется", p.ok)
                    } else {
                        (sh.st.swd_state.as_str(), p.muted)
                    };
                    badge(ui, &p, label, color);
                    if ui.button("Отключить").clicked() {
                        sh.job(Job::SwdClose);
                    }
                } else {
                    badge(ui, &p, "нет связи", p.muted);
                    if primary(ui, &p, "Подключить ST-Link").clicked() {
                        sh.job(Job::SwdOpen(sh.cfg.ocd.clone()));
                    }
                    if ui.button("Симулятор").clicked() {
                        sh.job(Job::SwdSim);
                    }
                }
                let on = sh.swd_ok() && !sh.busy;
                let pause_on = on && !halted;
                let go_on = on && halted;
                for (t, c, en, h) in [
                    ("Пауза  F6", "halt", pause_on, "остановить ядро и показать строку исходника"),
                    ("Продолжить  F5", "resume", go_on, "пустить остановленное ядро. Пока оно выполняется, кнопка серая — это нормально"),
                    ("Сброс", "reset run", on, "аппаратный сброс и запуск"),
                ] {
                    if ui.add_enabled(en, egui::Button::new(t)).on_hover_text(h).clicked() {
                        if c == "halt" || c == "resume" {
                            st.browse = None;
                        }
                        sh.job(Job::SwdCtl(c));
                    }
                }
                let step_hint = if halted {
                    "шаг по строке исходника"
                } else {
                    "ядро выполняется: первое нажатие остановит его на текущей строке, второе — шагнёт дальше"
                };
                for (t, k, h) in [
                    ("В функцию  F11", StepKind::Into, format!("{step_hint}. Если в строке вызов — зайти в подпрограмму")),
                    ("Строка  F10", StepKind::Over, format!("{step_hint}. Вызов выполняется целиком")),
                    ("Выйти  Shift+F11", StepKind::Out, format!("{step_hint}. До возврата из текущей функции")),
                ] {
                    if ui.add_enabled(on, egui::Button::new(t)).on_hover_text(h).clicked() {
                        st.browse = None;
                        sh.job(Job::SwdStep(k));
                    }
                }
            });
            if let Some(s) = &sh.st.swd {
                let id = sh.st.idcode.map(|v| format!("IDCODE 0x{v:08X}")).unwrap_or_default();
                ui.label(
                    RichText::new(format!(
                        "{s} · {} · {id}",
                        sh.st.swd_voltage.clone().unwrap_or_default()
                    ))
                    .small()
                    .color(p.fg2),
                );
                if !halted {
                    hint(ui, &p, "Ядро выполняется, поэтому «Продолжить» серое. F6, F10 или F11 остановят на текущей строке. Следующее нажатие F10/F11 шагает дальше.");
                }
            } else {
                hint(ui, &p, "Не подключено. Провода SWD, BOOT0 = 0, питание. Если ST-Link молчит — «подключение под сбросом» в Настройках (нужен провод NRST).");
            }
            if let Some(pc) = pc {
                ui.label(RichText::new(format!("PC  {pc:08X}   {}", st.pc_txt)).monospace().color(if halted { p.accent } else { p.fg }).size(15.0));
            }
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut st.bp)
                        .hint_text("адрес или символ: 0x08001234, main, prvIdleTask")
                        .desired_width(360.0)
                        .font(egui::TextStyle::Monospace),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let sym = sh.st.elf.as_ref().and_then(|e| e.sym(st.bp.trim())).map(|s| s.addr);
                let addr = debug::resolve_addr(st.bp.trim(), sym);
                if ui.add_enabled(sh.swd_ok() && addr.is_some(), egui::Button::new("Точка останова")).clicked() || (enter && addr.is_some()) {
                    if let Some(a) = addr {
                        sh.job(Job::SwdBpSet(a));
                    }
                }
            });
            if !sh.st.breaks.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for a in sh.st.breaks.clone() {
                        let name = sh.st.elf.as_ref().and_then(|e| e.func_at(a)).map(|s| s.name.as_str()).unwrap_or("");
                        let label = if name.is_empty() { format!("{a:08X}  ✕") } else { format!("{name}  ✕") };
                        if ui.small_button(label).on_hover_text(format!("снять 0x{a:08X}")).clicked() {
                            sh.job(Job::SwdBpClr(a));
                        }
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Программа", |ui| {
                let focus = st.browse.or(pc);
                if st.src_key != focus {
                    st.src_key = focus;
                    st.src_scroll = true;
                    st.src_lines.clear();
                    st.src_line = 0;
                    st.src_title.clear();
                    if let (Some(addr), Some(e)) = (focus, sh.st.elf.as_ref()) {
                        let project = std::path::PathBuf::from(&sh.cfg.project_dir);
                        if let Some(pl) = elf::place(e, &sh.cfg.toolchain_dir, addr) {
                            let short = pl.file.rsplit(['/', '\\']).next().unwrap_or(&pl.file);
                            st.src_title = format!("{}   {short}:{}", pl.func, pl.line);
                            st.src_line = pl.line;
                            match elf::source_file(&project, &pl.file) {
                                Some(path) => match std::fs::read_to_string(&path) {
                                    Ok(text) => st.src_lines = text.lines().map(|s| s.to_string()).collect(),
                                    Err(err) => st.src_title = format!("{} — {err}", st.src_title),
                                },
                                None => st.src_title = format!("{} — файл не найден", st.src_title),
                            }
                        } else if let Some(f) = e.func_at(addr) {
                            st.src_title = format!("{} +0x{:x} — в ELF нет номера строки", f.name, (addr & !1) - f.addr);
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if st.browse.is_some() {
                        ui.label(RichText::new("открыта функция, не место останова").small().color(p.warn));
                        if ui.small_button("К останову").clicked() {
                            st.browse = None;
                        }
                    } else if st.src_title.is_empty() {
                        hint(ui, &p, &format!("Остановите ядро. Нужен ELF: кнопка «Собрать» ниже кладёт build/{}.", crate::core::boards::cur().elf));
                    }
                });
                if !st.src_title.is_empty() {
                    ui.label(RichText::new(&st.src_title).monospace().color(p.accent));
                }
                egui::ScrollArea::vertical().id_salt("src").max_height(280.0).show(ui, |ui| {
                    for (i, line) in st.src_lines.iter().enumerate() {
                        let n = (i as u32) + 1;
                        let here = n == st.src_line;
                        let mark = if here { "▸" } else { " " };
                        let color = if here { p.accent } else { p.fg2 };
                        let resp = ui.label(RichText::new(format!("{mark}{n:4}  {line}")).monospace().small().color(color));
                        if here && st.src_scroll {
                            resp.scroll_to_me(Some(egui::Align::Center));
                            st.src_scroll = false;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Дизассемблер").strong().color(p.fg));
                if sh.st.disasm.is_empty() {
                    hint(
                        ui,
                        &p,
                        if running {
                            "Ядро выполняется. «Пауза» остановит его и покажет инструкции вокруг PC."
                        } else if halted {
                            "Остановлено, но в ELF нет инструкций по этому адресу. Соберите прошивку ещё раз."
                        } else {
                            "Подключите ST-Link и остановите ядро."
                        },
                    );
                }
                egui::ScrollArea::vertical().id_salt("disasm").max_height(320.0).show(ui, |ui| {
                    for line in &sh.st.disasm {
                        let here = pc.is_some_and(|pc| line.addr == (pc & !1));
                        let mark = if here { "▸" } else { " " };
                        let color = if here { p.accent } else { p.fg2 };
                        ui.label(RichText::new(format!("{mark} {:08X}  {}", line.addr, line.text)).monospace().small().color(color));
                    }
                });
            });
            card(&mut cols[1], &p, "Функции, регистры, стек", |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.fnq).hint_text("фильтр: uart, Led, main").desired_width(220.0));
                });
                let q = st.fnq.trim().to_ascii_lowercase();
                egui::ScrollArea::vertical().id_salt("fns").max_height(150.0).show(ui, |ui| {
                    let Some(e) = &sh.st.elf else {
                        hint(ui, &p, "Список функций берётся из ELF. Сначала соберите прошивку.");
                        return;
                    };
                    // unused IRQ handlers are weak aliases of Default_Handler: dozens of names, one address
                    let mut n = 0;
                    for s in e.syms.iter().filter(|s| {
                        s.func
                            && Some(s.addr) != e.default_handler
                            && (q.is_empty() || s.name.to_ascii_lowercase().contains(&q))
                    }) {
                        if n == 40 {
                            hint(ui, &p, "показаны первые 40 — уточните фильтр");
                            break;
                        }
                        n += 1;
                        ui.horizontal(|ui| {
                            if ui.selectable_label(st.browse == Some(s.addr), &s.name).clicked() {
                                st.browse = Some(s.addr);
                            }
                            if ui.add_enabled(sh.swd_ok(), egui::Button::new("●")).on_hover_text("точка останова на входе").clicked() {
                                sh.job(Job::SwdBpSet(s.addr));
                            }
                        });
                    }
                    if n == 0 {
                        hint(ui, &p, "нет такой функции");
                    }
                });
                ui.add_space(6.0);
                if sh.st.regs.is_empty() {
                    hint(ui, &p, "Появятся после паузы. Изменившиеся с прошлого останова значения подсвечиваются.");
                } else {
                    egui::Grid::new("core_regs").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
                        for (i, (n, v)) in sh.st.regs.iter().enumerate() {
                            let changed = st.prev.iter().any(|(pn, pv)| pn == n && pv != v);
                            ui.label(RichText::new(n).monospace().small().color(p.muted));
                            ui.label(RichText::new(format!("{v:08X}")).monospace().small().color(if changed { p.warn } else { p.fg }));
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Стек вызовов").strong().color(p.fg));
                if sh.st.frames.is_empty() {
                    hint(ui, &p, "PC и адрес возврата — когда ядро остановлено.");
                } else {
                    for (i, a) in sh.st.frames.iter().enumerate() {
                        let place = st.frame_txt.get(i).map(String::as_str).unwrap_or("—");
                        let label = format!("#{i}  {a:08X}  {place}");
                        if ui.selectable_label(st.browse == Some(*a), RichText::new(label).monospace().small()).clicked() {
                            st.browse = Some(*a);
                        }
                    }
                }
            });
        });
        ui.add_space(8.0);
        card(ui, &p, "Анализ сбоя (g_diag)", |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(sh.swd_ok(), egui::Button::new("Прочитать g_diag")).clicked() {
                        sh.job(Job::ReadDiag);
                    }
                    if ui.add_enabled(sh.swd_ok(), egui::Button::new("CFSR/HFSR сейчас")).clicked() {
                        sh.job(Job::ReadWords(vec![0xE000_ED28, 0xE000_ED2C, 0xE000_ED34, 0xE000_ED38]));
                    }
                });
                match &sh.st.diag {
                    None => hint(ui, &p, "Прошивка записывает причину любого сбоя (HardFault, переполнение стека, Error_Handler) в g_diag — область RAM, которая не очищается при сбросе."),
                    Some(d) if !d.valid() => {
                        ui.label(RichText::new("g_diag не инициализирован (другая прошивка или только что подано питание?)").color(p.warn));
                    }
                    Some(d) => {
                        egui::Grid::new("diag").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                            kv(ui, &p, "Загрузок", d.boot_count.to_string());
                            kv(ui, &p, "Причина сброса", proto::reset_cause_ru(d.reset_cause));
                            kv(ui, &p, "Heartbeat", d.heartbeat.to_string());
                            kv(ui, &p, "Кадров / CRC-ошибок / переполнений", format!("{} / {} / {}", d.rx_frames, d.rx_crc_err, d.rx_overflow));
                            kv(ui, &p, "Ошибок UART / потерь очереди", format!("{} / {}", d.uart_err, d.queue_full));
                            kv(ui, &p, "FRAM успешно / неуспешно", format!("{} / {}", d.fram_ok, d.fram_fail));
                        });
                        if d.has_fault() {
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Сбой: {}", d.error_name())).strong().color(p.err));
                            let el = sh.st.elf.as_ref();
                            let wh = |a: u32| el.map(|e| elf::where_is(e, &sh.cfg.toolchain_dir, a)).unwrap_or_default();
                            egui::Grid::new("fault").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                                kv(ui, &p, "PC (где упало)", format!("0x{:08X}  {}", d.fault_pc, wh(d.fault_pc)));
                                kv(ui, &p, "LR (откуда вызвано)", format!("0x{:08X}  {}", d.fault_lr, wh(d.fault_lr)));
                                kv(ui, &p, "CFSR / HFSR", format!("0x{:08X} / 0x{:08X}", d.fault_cfsr, d.fault_hfsr));
                                if d.fault_cfsr & (1 << 7) != 0 {
                                    kv(ui, &p, "MMFAR (адрес)", format!("0x{:08X}", d.fault_mmfar));
                                }
                                if d.fault_cfsr & (1 << 15) != 0 {
                                    kv(ui, &p, "BFAR (адрес)", format!("0x{:08X}", d.fault_bfar));
                                }
                                if d.error_code == 8 {
                                    kv(ui, &p, "Проверка configASSERT", format!("{}:{}", d.fault_task, d.fault_line));
                                } else if !d.fault_task.is_empty() {
                                    kv(ui, &p, "Задача", d.fault_task.clone());
                                }
                            });
                            for s in cfsr_ru(d.fault_cfsr) {
                                ui.label(RichText::new(format!("• {}", s)).color(p.fg2));
                            }
                        } else {
                            ui.label(RichText::new("Сбоев не записано ✓").color(p.ok));
                        }
                    }
                }
                if let Some(cfsr) = sh.mem(0xE000_ED28) {
                    hint(ui, &p, &format!("CFSR сейчас: 0x{:08X} {}", cfsr, cfsr_ru(cfsr).join("; ")));
                }
            });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Переменная по имени (из ELF)", |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.sym).hint_text("g_diag, uartQueue, uwTick…").desired_width(200.0));
                    let sym = sh.st.elf.as_ref().and_then(|e| e.sym(st.sym.trim())).cloned();
                    if ui.add_enabled(sh.swd_ok() && sym.is_some(), egui::Button::new("Прочитать")).clicked() {
                        if let Some(s) = &sym {
                            let n = s.size.div_ceil(4).clamp(1, 64);
                            sh.job(Job::ReadWords((0..n).map(|i| s.addr + 4 * i).collect()));
                        }
                    }
                });
                let Some(e) = &sh.st.elf else {
                    hint(ui, &p, "ELF не загружен.");
                    return;
                };
                let q = st.sym.trim();
                if let Some(s) = e.sym(q) {
                    ui.label(RichText::new(format!("{} @ 0x{:08X}, {} Б{}", s.name, s.addr, s.size, if s.func { ", функция" } else { "" })).monospace());
                    let n = s.size.div_ceil(4).clamp(1, 64);
                    let words: Vec<String> = (0..n).filter_map(|i| sh.mem(s.addr + 4 * i)).map(|w| format!("{:08X}", w)).collect();
                    if !words.is_empty() {
                        for chunk in words.chunks(8) {
                            ui.label(RichText::new(chunk.join(" ")).monospace().color(p.accent));
                        }
                    }
                } else if !q.is_empty() {
                    let hits: Vec<&str> = e.syms.iter().filter(|s| s.name.contains(q)).take(12).map(|s| s.name.as_str()).collect();
                    ui.horizontal_wrapped(|ui| {
                        for h in hits {
                            if ui.small_button(h).clicked() {
                                st.sym = h.to_string();
                            }
                        }
                    });
                }
            });
            card(&mut cols[1], &p, "История сборок, прошивок и автотестов", |ui| history(ui, sh, st));
        });
        ui.add_space(8.0);
        card(ui, &p, "Консоль OpenOCD (команды TCL)", |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut st.console).hint_text(if crate::core::boards::cur().family == crate::core::boards::Family::F4 { "mdw 0x40020800 6 · reg pc · flash info 0 · stm32f2x options_read 0" } else { "mdw 0x40011000 4 · reg pc · flash info 0 · stm32f1x options_read 0" }).desired_width(480.0).font(egui::TextStyle::Monospace));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Выполнить").clicked() || enter) && !st.console.trim().is_empty() {
                    sh.job(Job::OcdExec(st.console.trim().to_string()));
                    r.request_focus();
                }
            });
            egui::ScrollArea::vertical().id_salt("ocd").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                for l in &sh.st.ocd_console {
                    ui.label(RichText::new(l).monospace().small().color(p.fg2));
                }
            });
        });
    });
}

fn short_name(dir: &str) -> String {
    std::path::Path::new(dir).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.to_string())
}

fn board_name(b: BoardId) -> &'static str {
    b.profile().board
}

/// "только что", "12 мин", "3 ч", "2 дн".
pub fn ago(t: u64) -> String {
    if t == 0 {
        return String::new();
    }
    let d = config::now_secs().saturating_sub(t);
    match d {
        0..=59 => "только что".into(),
        60..=3599 => format!("{} мин назад", d / 60),
        3600..=86_399 => format!("{} ч назад", d / 3600),
        _ => format!("{} дн назад", d / 86_400),
    }
}

/// Active firmware project (any board, recent list, found nearby, any
/// folder), its git state, whether the ELF is older than the sources, and
/// build / flash / build+flash / autotest.
fn project_card(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let cfg: &Config = &sh.cfg;
    card(ui, &p, "Проект · сборка · прошивка · автотест", |ui| {
        let idle = sh.st.job.is_none();
        ui.horizontal_wrapped(|ui| {
            ui.label("Проект:");
            let cur = if cfg.project_dir.is_empty() { "не выбран".to_string() } else { format!("{} · {}", short_name(&cfg.project_dir), board_name(cfg.board)) };
            egui::ComboBox::from_id_salt("project_pick")
                .width(320.0)
                .selected_text(RichText::new(cur).strong())
                .show_ui(ui, |ui| {
                    ui.label(RichText::new("Недавние").small().color(p.muted));
                    for r in &cfg.recent {
                        let text = format!("{}   {} · {}", short_name(&r.dir), board_name(r.board), ago(r.used));
                        if ui.add_enabled(idle, egui::Button::selectable(r.dir == cfg.project_dir, text)).on_hover_text(&r.dir).clicked() && r.dir != cfg.project_dir {
                            sh.act(Act::Project(r.dir.clone()));
                        }
                    }
                    let found: Vec<(String, BoardId)> =
                        config::find_projects(&cfg.project_dir).into_iter().filter(|(d, _)| !cfg.recent.iter().any(|r| r.dir == *d)).collect();
                    if !found.is_empty() {
                        ui.separator();
                        ui.label(RichText::new("Найдены рядом").small().color(p.muted));
                        for (d, b) in found {
                            if ui.add_enabled(idle, egui::Button::selectable(false, format!("{}   {}", short_name(&d), board_name(b)))).on_hover_text(&d).clicked() {
                                sh.act(Act::Project(d));
                            }
                        }
                    }
                });
            ui.add(egui::TextEdit::singleline(&mut st.proj_path).hint_text("другая папка проекта…").desired_width(280.0));
            if ui.add_enabled(idle && !st.proj_path.trim().is_empty(), egui::Button::new("Открыть")).on_hover_text("папка с Core/ и линкер-скриптом платы; плата выбирается сама").clicked() {
                sh.act(Act::Project(st.proj_path.trim().to_string()));
                st.proj_path.clear();
            }
        });
        if !cfg.project_dir.is_empty() {
            ui.label(RichText::new(&cfg.project_dir).small().monospace().color(p.muted));
        }
        ui.horizontal_wrapped(|ui| {
            if !sh.st.git.is_empty() {
                let dirty = sh.st.git.ends_with("+изм.");
                ui.label(RichText::new(format!("git: {}", sh.st.git)).small().color(if dirty { p.warn } else { p.fg2 }))
                    .on_hover_text(if dirty { "есть незакоммиченные изменения: в историю запишется этот же хеш с пометкой +изм." } else { "рабочая копия совпадает с коммитом" });
            }
            match (&sh.st.elf, sh.st.elf_mtime) {
                (Some(e), Some(t)) => {
                    ui.label(RichText::new(format!("ELF {} собран {}", e.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), ago(t))).small().color(p.fg2))
                        .on_hover_text(e.path.display().to_string());
                }
                (Some(e), None) => {
                    ui.label(RichText::new(format!("ELF: {}", e.path.display())).small().color(p.fg2));
                }
                (None, _) => {
                    ui.label(RichText::new("ELF нет — соберите прошивку").small().color(p.warn));
                }
            }
        });
        if let (Some(src), Some(elf)) = (sh.st.src_mtime, sh.st.elf_mtime) {
            if src > elf {
                ui.label(
                    RichText::new(format!("Исходники изменены {} — после последней сборки. Нажмите «Собрать и прошить».", ago(src)))
                        .color(p.warn),
                );
            }
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(idle, egui::Button::new("🔨 Собрать")).on_hover_text(&cfg.build_cmd).clicked() {
                sh.job(Job::Build(cfg.clone()));
            }
            if ui.add_enabled(idle, egui::Button::new("⚡ Прошить")).on_hover_text("OpenOCD: program … verify reset — загруженный ELF").clicked() {
                sh.job(Job::Flash(cfg.clone()));
            }
            if ui.add_enabled(idle, egui::Button::new("🔨⚡ Собрать и прошить")).on_hover_text("сборка, и если она успешна — сразу прошивка свежего ELF").clicked() {
                sh.job(Job::BuildFlash(cfg.clone()));
            }
            if ui.add_enabled(idle, egui::Button::new("✔ Автотест hil.py")).on_hover_text("tools/hil.py test: сборка, прошивка и ~30 проверок на плате; порты освобождаются").clicked() {
                sh.job(Job::HilTest(cfg.clone()));
            }
            ui.label(RichText::new(format!("сборка: {}", cfg.build_cmd)).small().monospace().color(p.muted));
        });
            match (sh.st.job.as_deref(), sh.st.job_ok) {
                (Some(j), _) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(j);
                    });
                }
                (None, Some(true)) => {
                    ui.label(RichText::new("✓ успешно").color(p.ok));
                }
                (None, Some(false)) => {
                    ui.label(RichText::new("✗ ошибка").color(p.err));
                }
                _ => {}
            }
            if sh.st.job_out.is_empty() {
                return; // no output yet: no empty box
            }
            egui::Frame::new().fill(p.panel2).corner_radius(6).inner_margin(6).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("jobout").max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                    for l in &sh.st.job_out {
                        let c = if l.contains("FAIL") || l.contains("error") || l.contains("Error") {
                            p.err
                        } else if l.contains("PASS") || l.contains("[  ok ]") {
                            p.ok
                        } else {
                            p.fg2
                        };
                        ui.label(RichText::new(l).monospace().small().color(c));
                    }
                });
            });
    });
}

/// Saved runs: when, which project, what, result, how long, git state.
fn history(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let cfg = &sh.cfg;
    ui.horizontal(|ui| {
        ui.checkbox(&mut st.hist_all, "все проекты");
        if !cfg.history.is_empty() && ui.small_button("Очистить").clicked() {
            let mut c = cfg.clone();
            c.history.clear();
            sh.act(Act::SaveCfg(Box::new(c)));
        }
    });
    let rows: Vec<&config::Run> = cfg.history.iter().rev().filter(|r| st.hist_all || r.dir == cfg.project_dir).take(20).collect();
    if rows.is_empty() {
        hint(ui, &p, "Здесь появятся сборки, прошивки и автотесты: когда, какой проект, результат и коммит.");
        return;
    }
    egui::ScrollArea::vertical().id_salt("hist").max_height(240.0).show(ui, |ui| {
        egui::Grid::new("hist_grid").num_columns(5).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
            for r in rows {
                ui.label(RichText::new(ago(r.t)).small().color(p.muted));
                if st.hist_all {
                    let name = format!("{} · {}", short_name(&r.dir), board_name(r.board));
                    if r.dir != cfg.project_dir {
                        if ui.small_button(name).on_hover_text(format!("{}\nсделать активным", r.dir)).clicked() {
                            sh.act(Act::Project(r.dir.clone()));
                        }
                    } else {
                        ui.label(RichText::new(name).small());
                    }
                } else {
                    ui.label("");
                }
                ui.label(RichText::new(&r.what).small());
                ui.label(RichText::new(if r.ok { "✓" } else { "✗" }).color(if r.ok { p.ok } else { p.err }).strong());
                ui.label(RichText::new(format!("{:.1} с  {}", r.secs, r.git)).small().monospace().color(p.fg2));
                ui.end_row();
            }
        });
    });
}
