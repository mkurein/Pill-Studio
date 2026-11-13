//! USART3 text log: colouring by line kind, filters, search, Tick timing
//! analysis (detects stalled tasks: a step between Tick lines > 1100 ms).

use crate::app::{Act, Shared};
use crate::core::proto::LogKind;
use crate::fmt;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, RichText};
use egui_plot::{Bar, BarChart, Plot};

pub struct St {
    hide_ticks: bool,
    search: String,
    follow: bool,
}

impl Default for St {
    fn default() -> Self {
        St { hide_ticks: false, search: String::new(), follow: true }
    }
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = crate::core::boards::cur();
    h1(ui, &p, &format!("Лог {} ({}, 115200)", b.log_uart.replace("USART", "UART"), b.log_tx), "BOOT — старт, Tick — раз в секунду, DIAG — каждые 5 с, TEST — результат команды, FAULT — авария.");
    if let Some(port) = &sh.st.log {
        ui.horizontal(|ui| {
            if ui.button("← Обзор").on_hover_text("вернуться на Обзор; лог продолжает читаться").clicked() {
                sh.act(Act::Go(crate::app::View::Overview));
            }
            if ui.button("✕ Закрыть лог").on_hover_text("освободить порт лога и вернуться на Обзор").clicked() {
                sh.job(Job::LogClose);
                sh.act(Act::Go(crate::app::View::Overview));
            }
            ui.label(RichText::new(format!("читается {port}")).small().color(p.muted));
        });
        ui.add_space(6.0);
    } else {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Лог не подключён.").color(p.warn));
            if ui.button("Подключить на Обзоре").clicked() {
                sh.act(Act::Go(crate::app::View::Overview));
            }
            if ui.button("Виртуальная плата").clicked() {
                sh.job(Job::LogSim);
            }
        });
    }
    // Tick analysis
    let ticks: Vec<(f64, u32)> = sh
        .lines
        .iter()
        .filter_map(|l| if let LogKind::Tick(ms) = l.kind { Some((l.t, ms)) } else { None })
        .collect();
    let steps: Vec<i64> = ticks.windows(2).map(|w| w[1].1 as i64 - w[0].1 as i64).filter(|d| *d > 0 && *d < 60_000).collect();
    ui.columns(2, |cols| {
        card(&mut cols[0], &p, "Ритм задач (шаг между строками Tick)", |ui| {
            if steps.is_empty() {
                hint(ui, &p, "Нужно хотя бы две строки Tick.");
                return;
            }
            let max = *steps.iter().max().unwrap_or(&0);
            let avg = steps.iter().sum::<i64>() as f64 / steps.len() as f64;
            let stalled = max > 1100;
            ui.label(
                RichText::new(format!(
                    "среднее {:.0} мс · максимум {} мс · {}",
                    avg,
                    max,
                    if stalled { "ЗАДАЧИ ПОДВИСАЛИ (> 1100 мс)" } else { "задачи не подвисают" }
                ))
                .color(if stalled { p.err } else { p.ok }),
            );
            let n = steps.len();
            let bars: Vec<Bar> = steps
                .iter()
                .skip(n.saturating_sub(60))
                .enumerate()
                .map(|(i, d)| Bar::new(i as f64, *d as f64).fill(if *d > 1100 { p.err } else { p.accent }))
                .collect();
            Plot::new("tick_steps").height(110.0).allow_scroll(false).allow_zoom(false).show(ui, |pu| {
                pu.bar_chart(BarChart::new("шаг Tick, мс", bars));
            });
        });
        card(&mut cols[1], &p, "Последние DIAG", |ui| {
            match sh.lines.iter().rev().find(|l| l.kind == LogKind::Diag) {
                Some(l) => {
                    egui::Grid::new("diag_kv").num_columns(2).show(ui, |ui| {
                        for (k, v) in crate::core::proto::log_kv(&l.text) {
                            kv(ui, &p, &k, v);
                        }
                    });
                    hint(ui, &p, &format!("в {}", fmt::clock(l.t)));
                }
                None => hint(ui, &p, "DIAG ещё не приходил (раз в 5 с)."),
            }
        });
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut st.hide_ticks, "скрыть Tick");
        ui.checkbox(&mut st.follow, "следовать");
        ui.add(egui::TextEdit::singleline(&mut st.search).hint_text("фильтр…").desired_width(220.0));
        if ui.button("Копировать всё").clicked() {
            let txt: Vec<String> = sh.lines.iter().map(|l| format!("{} {}", fmt::clock(l.t), l.text)).collect();
            ui.ctx().copy_text(txt.join("\n"));
            sh.act(Act::Toast(crate::worker::Tone::Ok, "Лог скопирован в буфер обмена".into()));
        }
        if ui.button("Очистить").clicked() {
            sh.act(Act::ClearLog);
        }
        ui.label(RichText::new(format!("{} строк", sh.lines.len())).color(p.muted));
    });
    let f = st.search.to_lowercase();
    egui::Frame::new().fill(p.panel2).corner_radius(8).inner_margin(8).show(ui, |ui| {
        egui::ScrollArea::vertical().stick_to_bottom(st.follow).auto_shrink([false, false]).show(ui, |ui| {
            for l in &sh.lines {
                if st.hide_ticks && matches!(l.kind, LogKind::Tick(_)) {
                    continue;
                }
                if !f.is_empty() && !l.text.to_lowercase().contains(&f) {
                    continue;
                }
                let c = match l.kind {
                    LogKind::Boot => p.accent,
                    LogKind::Tick(_) => p.muted,
                    LogKind::Diag => p.clk,
                    LogKind::Fault | LogKind::PrevFault => p.err,
                    LogKind::Test => p.ok,
                    LogKind::Fram => p.dig,
                    LogKind::Other => p.fg,
                };
                ui.label(RichText::new(format!("{}  {}", fmt::clock(l.t), l.text)).monospace().color(c));
            }
        });
    });
}
