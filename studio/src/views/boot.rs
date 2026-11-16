//! Start and reset: the last reset and its cause, the path from the reset
//! vector to running tasks (with a live trace over SWD), what survives a
//! reset, how the core enters an interrupt, and what a power failure does.

use crate::app::{Act, Shared, View};
use crate::core::boot::{self, Stage, Step};
use crate::core::elf;
use crate::core::proto::{CMD_FAULT, CMD_RESET, CMD_STATUS};
use crate::fmt;
use crate::theme::Pal;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct St {
    /// ELF, its build time and the time of the last source edit.
    key: Option<(PathBuf, Option<u64>, Option<u64>)>,
    /// Steps of main() were read from the project's main.c.
    from_src: bool,
    steps: Vec<Step>,
    /// "main.c:322" per step, from addr2line.
    places: Vec<String>,
    /// A reset asked from this screen: (what, boot_count before, when to ask the board again).
    asked: Option<(&'static str, Option<u32>, Instant)>,
    asked_done: bool,
    /// The pending answer comes after a watchdog reset (seconds, not milliseconds).
    asked_wdg: bool,
}

fn stage_color(p: &Pal, s: Stage) -> Color32 {
    match s {
        Stage::Core => p.muted,
        Stage::Startup => p.refc,
        Stage::Main => p.accent,
        Stage::Rtos => p.clk,
        Stage::Task => p.ok,
    }
}

fn reload(sh: &Shared, st: &mut St) {
    let Some(e) = &sh.st.elf else {
        st.key = None;
        st.steps.clear();
        return;
    };
    let key = (e.path.clone(), sh.st.elf_mtime, sh.st.src_mtime);
    if st.key.as_ref() == Some(&key) {
        return;
    }
    let main_c = std::fs::read_to_string(std::path::Path::new(&sh.cfg.project_dir).join("Core/Src/main.c")).ok();
    st.from_src = main_c.is_some();
    st.steps = boot::steps(e, main_c.as_deref());
    st.places = st
        .steps
        .iter()
        .map(|s| {
            s.addr
                .and_then(|a| elf::place(e, &sh.cfg.toolchain_dir, a))
                .map(|pl| format!("{}:{}", pl.file.rsplit(['/', '\\']).next().unwrap_or(&pl.file), pl.line))
                .unwrap_or_default()
        })
        .collect();
    st.key = Some(key);
}

/// boot_count and RCC_CSR of the last boot: STATUS over UART1, else g_diag over SWD.
fn last_boot(sh: &Shared) -> Option<(u32, u32, &'static str)> {
    if let Some(s) = sh.st.status {
        return Some((s.boot_count, s.reset_cause, "STATUS по UART1"));
    }
    sh.st.diag.as_ref().filter(|d| d.valid()).map(|d| (d.boot_count, d.reset_cause, "g_diag по SWD"))
}

fn ask_board(sh: &Shared) {
    if sh.uart_ok() {
        sh.send(vec![CMD_STATUS]);
    }
    if sh.swd_ok() {
        sh.job(Job::ReadDiag);
    }
}

fn table(ui: &mut egui::Ui, p: &Pal, id: &str, widths: &[f32], rows: &[&[&str]]) {
    egui::Grid::new(id).num_columns(widths.len()).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for (i, row) in rows.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                let t = RichText::new(*cell).small();
                let t = if i == 0 {
                    t.strong().color(p.fg2)
                } else if j == 0 {
                    t.color(p.accent)
                } else {
                    t.color(p.fg)
                };
                ui.vertical(|ui| {
                    ui.set_max_width(widths[j]);
                    ui.add(egui::Label::new(t).wrap());
                });
            }
            ui.end_row();
        }
    });
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    reload(sh, st);
    // a reset was asked here: read the board again once it has booted
    if let Some((_, _, at)) = st.asked {
        if !st.asked_done {
            if Instant::now() >= at {
                ask_board(sh);
                st.asked_done = true;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(
            ui,
            &p,
            "Старт и сброс",
            "Что происходит от сброса до работающих задач, почему плата перезапустилась в последний раз и что при этом сохранилось.",
        );
        super::fresh::bar(ui, sh);
        if sh.primgpt() {
            reset_card(ui, sh, st);
            ui.add_space(8.0);
            guard_card(ui, sh, st);
            ui.add_space(8.0);
        }
        path_card(ui, sh, st);
        if !sh.primgpt() {
            return;
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Справка: что переживает сброс, вход в прерывание, сбой питания").id_salt("boot_ref").show(ui, |ui| {
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Что переживает сброс", |ui| {
                table(
                    ui,
                    &p,
                    "boot_survive",
                    &[150.0, 150.0, 150.0],
                    &[
                        &["Что", "Сброс без снятия питания", "Пропадание питания"],
                        &[".data — переменные с начальным значением", "заново копируются из Flash", "заново копируются из Flash"],
                        &[".bss — остальные переменные, куча FreeRTOS", "обнуляются", "обнуляются"],
                        &["g_diag (.noinit): boot_count, запись о сбое, счётчики", "сохраняются", "теряются: метки DIAG_MAGIC нет — запись обнуляется, boot_count = 1"],
                        &["Стеки задач, очередь uartQueue", "создаются заново", "создаются заново"],
                        &["Регистры периферии, тактирование", "значения по сбросу, такт HSI", "значения по сбросу, такт HSI"],
                        &["Флаги причины в RCC_CSR", "копятся, пока прошивка не сотрёт (RMVF) — она стирает при старте", "ставится POR/PDR"],
                        &["Flash (программа), внешняя SPI-память", "сохраняются", "сохраняются"],
                        &["Точки останова, отладочные регистры ядра", "сохраняются при сбросе командой или по SWD", "теряются"],
                    ],
                );
            });
            card(&mut cols[1], &p, "Вход в прерывание и переключение задач", |ui| {
                para(ui, &p, "Что ядро делает само, когда приходит прерывание (например, байт USART1):");
                for (n, t) in [
                    ("1", "Дожидается конца текущей инструкции и сравнивает приоритеты: прерывание срочнее текущего кода — вход, иначе остаётся ожидать (pending)."),
                    ("2", "Кладёт на текущий стек 8 регистров: R0–R3, R12, LR, PC, xPSR (на F4 с FPU — ещё 18 слов, если задача считала с плавающей точкой). Задача при этом на своём стеке PSP."),
                    ("3", "Берёт адрес обработчика из таблицы векторов: запись 16 + номер IRQ."),
                    ("4", "Переходит в режим Handler, стек — главный MSP; в LR записывает EXC_RETURN (0xFFFFFFFD — вернуться в задачу на PSP)."),
                    ("5", "Обработчик — обычная функция C. Возврат по «bx lr» с EXC_RETURN: ядро снимает 8 регистров со стека, прерванный код продолжается."),
                ] {
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new(n).strong().color(p.accent));
                        ui.add(egui::Label::new(RichText::new(t).small().color(p.fg)).wrap());
                    });
                }
                ui.add_space(6.0);
                para(
                    ui,
                    &p,
                    "Переключение задач FreeRTOS — два прерывания с самым низким приоритетом (15). SysTick раз в 1 мс считает тик и, если пора сменить задачу, \
                     взводит PendSV. PendSV выполняется, когда все остальные прерывания закончились: дописывает на стек старой задачи R4–R11, \
                     сохраняет её PSP в блок управления, выбирает новую задачу и восстанавливает её регистры.",
                );
                hint(ui, &p, "Сбой в обработчике (HardFault и другие) идёт тем же путём — поэтому из стека можно достать PC места сбоя; прошивка пишет его в g_diag.");
                if ui.button("Таблица векторов →").clicked() {
                    sh.act(Act::Go(View::Memory));
                }
            });
        });
        ui.add_space(8.0);
        card(ui, &p, "Сбой питания", |ui| {
            table(
                ui,
                &p,
                "boot_power",
                &[230.0, 620.0],
                &[
                    &["Ситуация", "Что происходит сейчас"],
                    &["Питание пропало и вернулось", "Сброс POR/PDR: RAM теряется, g_diag начинается с нуля (boot_count = 1), запись о прошлом сбое пропадает. Прошивка стартует обычным путём."],
                    &["Короткая просадка, RAM не успела потеряться", "Сброс PDR (на F4 может быть BOR). g_diag обычно цел: метка DIAG_MAGIC на месте, boot_count растёт дальше — так просадку можно отличить от полного выключения."],
                    &["Питание падает ниже 2.9 В", "Срабатывает детектор PVD: прерывание увеличивает счётчик pvd_count в g_diag и запоминает время. Если RAM уцелела, после сброса видно, что была просадка."],
                    &["Задача зависла или сбой остановил программу", "Сторожевой таймер IWDG (4–5 с): задачи отмечаются раз в секунду, без отметки любой из них таймер не сбрасывается и перезапускает плату. Виновная задача и запись о сбое остаются в g_diag."],
                ],
            );
            ui.add_space(4.0);
            hint(
                ui,
                &p,
                "Проверить вручную: запомните boot_count, снимите и верните питание платы, нажмите «Обновить» вверху — должно быть boot_count = 1 и причина «включение питания». \
                 Сторож и PVD проверяются кнопками в карточке «Проверка защиты» (нужна прошивка с командой FAULT).",
            );
        });
        });
    });
}

fn reset_card(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    card(ui, &p, "Последний сброс", |ui| {
        let now = last_boot(sh);
        ui.horizontal_wrapped(|ui| {
            if ui.button("⟳ Обновить").on_hover_text("STATUS по UART1 и g_diag по SWD — что подключено").clicked() {
                ask_board(sh);
            }
            let mut go = |ui: &mut egui::Ui, label: &str, tip: &str, enabled: bool, kind: &'static str| {
                if ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(tip).clicked() {
                    st.asked = Some((kind, now.map(|n| n.0), Instant::now() + Duration::from_millis(900)));
                    st.asked_done = false;
                    st.asked_wdg = false;
                    true
                } else {
                    false
                }
            };
            if go(ui, "Сброс командой (UART1)", "команда RESET: плата отвечает и вызывает NVIC_SystemReset", sh.uart_ok(), "команда RESET") {
                sh.send(vec![CMD_RESET]);
            }
            if go(ui, "Сброс по SWD", "OpenOCD «reset run»: запрос сброса через отладочный порт", sh.swd_ok(), "сброс по SWD") {
                sh.job(Job::SwdCtl("reset run"));
            }
        });
        let Some((boots, csr, src)) = now else {
            hint(ui, &p, "Данных нет: подключите UART1 или ST-Link на Обзоре и нажмите «Обновить».");
            return;
        };
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Причина: {}", boot::main_cause(csr))).strong().size(15.0));
            ui.label(RichText::new(format!("boot_count = {boots} · RCC_CSR = 0x{csr:08X} · источник: {src}")).monospace().small().color(p.fg2));
            if let Some(s) = sh.st.status {
                ui.label(RichText::new(format!("работает {}", fmt::uptime(s.uptime_ms))).small().color(p.fg2));
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (bit, name, what) in boot::RESET_FLAGS {
                let on = csr & (1 << bit) != 0;
                let r = ui.label(RichText::new(format!("{} {name}", if on { "●" } else { "○" })).color(if on { p.warn } else { p.muted }));
                r.on_hover_text(format!("бит {bit}: {what}"));
            }
        });
        if let (Some((kind, before, _)), true) = (st.asked, st.asked_done) {
            match before {
                Some(b) if boots == b + 1 => {
                    ui.label(RichText::new(format!("✓ {kind}: boot_count {b} → {boots}, g_diag пережил сброс")).color(p.ok));
                }
                Some(b) if boots == b => {
                    ui.label(RichText::new(format!("{kind}: boot_count всё ещё {b} — плата ещё не ответила после сброса, нажмите «Обновить»")).color(p.warn));
                }
                Some(b) => {
                    ui.label(RichText::new(format!("{kind}: boot_count {b} → {boots} — не +1: g_diag обнулился или было несколько сбросов")).color(p.err));
                }
                None => {}
            }
        }
        if let Some(d) = sh.st.diag.as_ref().filter(|d| d.has_counters()) {
            ui.label(
                RichText::new(format!(
                    "С подачи питания: программных сбросов {} · по NRST {} · просадок питания {} · сторожевых {}",
                    d.rst_soft, d.rst_pin, d.rst_power, d.rst_wdg
                ))
                .small()
                .color(p.fg2),
            )
            .on_hover_text("счётчики из g_diag по SWD; обнуляются при потере питания");
        }
        if let Some(d) = sh.st.diag.as_ref().filter(|d| d.has_counters() && d.pvd_count > 0) {
            ui.label(RichText::new(format!("Просадок питания ниже 2.9 В (PVD): {}, последняя на {} мс работы", d.pvd_count, d.pvd_ms)).small().color(p.warn));
        }
        if let Some(l) = sh.lines.iter().rev().find(|l| l.text.starts_with("BOOT")) {
            ui.label(RichText::new(format!("в логе: {}", l.text)).monospace().small().color(p.fg2));
        }
    });
}

fn path_card(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    card(ui, &p, "Путь от сброса до задач", |ui| {
        if st.steps.is_empty() {
            hint(ui, &p, "ELF не найден. Соберите прошивку: SWD-отладка → «Собрать».");
            return;
        }
        let real = sh.swd_ok() && sh.st.swd.as_deref() != Some("виртуальная плата");
        let trace = &sh.st.boot_trace;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(real, egui::Button::new("▶ Пройти старт на плате"))
                .on_hover_text("сброс с остановкой, затем от шага к шагу по точкам останова; плата перезапустится и в конце продолжит работу")
                .clicked()
            {
                sh.job(Job::BootTrace(st.steps.iter().filter_map(|s| s.addr).collect()));
            }
            if !real {
                hint(ui, &p, "нужен ST-Link (SWD-отладка → «Подключить ST-Link»)");
            }
            if !sh.st.boot_note.is_empty() {
                let all = trace.len() == st.steps.iter().filter(|s| s.addr.is_some()).count();
                ui.label(RichText::new(&sh.st.boot_note).color(if all { p.ok } else { p.warn }));
            }
        });
        hint(
            ui,
            &p,
            if st.from_src {
                "Вызовы main() и задачи — из Core/Src/main.c проекта, адреса и размеры — из ELF. «Такты» — счётчик ядра от сброса (до SystemClock_Config такт HSI, после — PLL)."
            } else {
                "main.c проекта не найден: показан обычный порядок CubeMX. Адреса и размеры — из ELF."
            },
        );
        ui.add_space(4.0);
        egui::Grid::new("boot_steps").num_columns(6).striped(true).spacing([12.0, 5.0]).show(ui, |ui| {
            for h in ["", "этап", "шаг", "адрес · строка", "трасса", "что происходит"] {
                ui.label(RichText::new(h).small().strong().color(p.fg2));
            }
            ui.end_row();
            let hit_of = |s: &Step| s.addr.and_then(|a| trace.iter().find(|h| h.addr == a));
            for (i, s) in st.steps.iter().enumerate() {
                let c = stage_color(&p, s.stage);
                ui.label(RichText::new(format!("{i}")).monospace().small().color(p.muted));
                badge(ui, &p, s.stage.ru(), c);
                ui.label(RichText::new(&s.title).strong().color(if s.addr.is_some() || s.sym.is_empty() { p.fg } else { p.muted }));
                match s.addr {
                    Some(a) => {
                        ui.label(RichText::new(format!("{a:08X}  {}", st.places.get(i).map(String::as_str).unwrap_or(""))).monospace().small().color(p.fg2));
                    }
                    None if s.sym.is_empty() => {
                        ui.label(RichText::new("аппаратно").small().color(p.muted));
                    }
                    None => {
                        ui.label(RichText::new("в ELF нет: встроено компилятором").small().color(p.muted));
                    }
                }
                match hit_of(s) {
                    Some(h) => {
                        // against the step reached just before this one, whatever its row
                        let before = h.order.checked_sub(1).and_then(|o| trace.iter().find(|x| x.order == o)).map_or(0, |x| x.cycles);
                        let step = h.cycles.saturating_sub(before);
                        ui.label(RichText::new(format!("✓ №{} · {} такт. (+{})", h.order + 1, h.cycles, step)).monospace().small().color(p.ok))
                            .on_hover_text("порядок достижения · тактов ядра от сброса · прирост от шага, достигнутого перед этим");
                    }
                    None if !trace.is_empty() && s.addr.is_some() => {
                        ui.label(RichText::new("не вызывался").small().color(p.warn));
                    }
                    None => {
                        ui.label("");
                    }
                }
                ui.vertical(|ui| {
                    ui.set_max_width(520.0);
                    ui.add(egui::Label::new(RichText::new(&s.what).small().color(p.fg)).wrap());
                });
                ui.end_row();
            }
        });
    });
}

/// Fault injection: the firmware breaks itself on command, the watchdog must
/// bring it back and g_diag must say what happened.
fn guard_card(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    card(ui, &p, "Проверка защиты: сторожевой таймер, assert, сбой, просадка питания", |ui| {
        let now = last_boot(sh).map(|n| n.0);
        let on = sh.uart_ok();
        ui.horizontal_wrapped(|ui| {
            let mut inject = |ui: &mut egui::Ui, label: &str, tip: &str, what: u8, kind: &'static str| {
                if ui.add_enabled(on, egui::Button::new(label)).on_hover_text(tip).clicked() {
                    sh.send(vec![CMD_FAULT, what]);
                    // the watchdog needs 4-5 s (up to 9 s on a slow LSI), then the board boots
                    st.asked = Some((kind, now, Instant::now() + Duration::from_millis(7000)));
                    st.asked_done = false;
                    st.asked_wdg = true;
                }
            };
            inject(ui, "Остановить задачу UART", "задача перестаёт отмечаться; сторож перезапустит плату через 4–6 с и запишет виновника", 1, "остановка задачи");
            inject(ui, "configASSERT", "проверка ядра FreeRTOS не проходит: файл и строка в g_diag, затем сторож", 2, "assert");
            inject(ui, "HardFault", "запись по несуществующему адресу: PC сбоя в g_diag, затем сторож", 3, "HardFault");
            if ui.add_enabled(on, egui::Button::new("Просадка питания (PVD)")).on_hover_text("программно вызывает прерывание PVD, как при падении ниже 2.9 В; плата не сбрасывается").clicked() {
                sh.send(vec![CMD_FAULT, 4]);
                st.asked = Some(("PVD", now, Instant::now() + Duration::from_millis(300)));
                st.asked_done = false;
                st.asked_wdg = false;
            }
            if ui.add_enabled(on, egui::Button::new("Стереть запись о сбое")).on_hover_text("после разбора: иначе PREV_FAULT печатается при каждом старте").clicked() {
                sh.send(vec![CMD_FAULT, 0]);
                ask_board(sh);
            }
        });
        if !on {
            hint(ui, &p, "Нужен UART1. Виновника и место сбоя студия читает из g_diag по SWD — подключите и ST-Link.");
            return;
        }
        if let (Some((kind, _, at)), false, true) = (st.asked, st.asked_done, st.asked_wdg) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{kind}: ждём сторожевой таймер, ещё {:.0} с", at.saturating_duration_since(Instant::now()).as_secs_f32().ceil()));
            });
        }
        let Some(d) = sh.st.diag.as_ref().filter(|d| d.has_counters()) else {
            hint(ui, &p, "Результат (виновная задача, файл и строка, счётчик PVD) появится здесь, когда подключён ST-Link и прошивка знает команду FAULT.");
            return;
        };
        egui::Grid::new("guard_kv").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
            kv(ui, &p, "Сбросов сторожем с подачи питания", d.rst_wdg.to_string());
            if d.rst_wdg > 0 {
                kv(ui, &p, "Перестала отмечаться", d.wdg_missing_ru());
            }
            if d.has_fault() {
                let place = if d.error_code == 8 { format!(" — {}:{}", d.fault_task, d.fault_line) } else { format!(" — PC 0x{:08X}", d.fault_pc) };
                kv(ui, &p, "Запись о сбое", format!("{}{place}", d.error_name()));
            }
            kv(ui, &p, "Просадок питания (PVD)", d.pvd_count.to_string());
        });
        hint(ui, &p, "Это настоящие сбои на плате: она перезапустится. Подробный разбор сбоя — «SWD-отладка» → «Анализ сбоя».");
    });
}
