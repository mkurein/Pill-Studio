//! FreeRTOS monitor: tasks with stack watermarks, heap, counters, history
//! plots (from STATUS polling) and the data-flow diagram of the firmware.

use crate::app::Shared;
use crate::canvas::Canvas;
use crate::core::proto::{self, CMD_STATUS};
use crate::fmt;
use crate::theme::mix;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, Align2, RichText, Stroke};
use egui_plot::{Legend, Line, Plot, PlotPoints};

#[derive(Default)]
pub struct St {
    window_s: u32,
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    if st.window_s == 0 {
        st.window_s = 120;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(ui, &p, "FreeRTOS 10.0.1", "heap_4 · тик 1 кГц · вытесняющая многозадачность · данные — команда STATUS и g_diag по SWD");
        ui.horizontal(|ui| {
            let mut on = sh.st.polling;
            if ui.checkbox(&mut on, "опрашивать STATUS").changed() {
                sh.job(Job::Poll(on));
            }
            ui.label(format!("период {} мс", sh.st.poll_ms));
            if ui.button("Запросить").clicked() {
                sh.send(vec![CMD_STATUS]);
            }
            if sh.swd_ok() && ui.button("Прочитать g_diag по SWD").clicked() {
                sh.job(Job::ReadDiag);
            }
            ui.label("окно графиков:");
            for s in [60, 120, 600] {
                ui.selectable_value(&mut st.window_s, s, format!("{} с", s));
            }
        });
        let Some(s) = sh.st.status else {
            ui.add_space(20.0);
            card(ui, &p, "", |ui| {
                ui.label(RichText::new("Нет данных STATUS").strong());
                para(ui, &p, "Подключите UART1 (или виртуальную плату) на экране «Обзор» и включите опрос.");
            });
            flow(ui, sh);
            return;
        };
        ui.add_space(6.0);
        ui.columns(4, |c| {
            metric(&mut c[0], sh, "Аптайм", &fmt::uptime(s.uptime_ms), &format!("heartbeat {}", s.heartbeat));
            metric(&mut c[1], sh, "Загрузок", &s.boot_count.to_string(), &proto::reset_cause_ru(s.reset_cause));
            metric(&mut c[2], sh, "Кадров UART1", &s.rx_frames.to_string(), &format!("CRC-ошибок {} · UART-ошибок {} · потерь {}", s.rx_crc_err, s.uart_err, s.queue_full));
            metric(&mut c[3], sh, "FRAM-тесты", &format!("{} / {}", s.fram_ok, s.fram_fail), "успешно / неуспешно");
        });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Задачи и стеки", |ui| {
                egui::Grid::new("tasks").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                    for h in ["Задача", "Приоритет", "Стек", "Использовано (пик)"] {
                        ui.label(RichText::new(h).strong().color(p.fg2));
                    }
                    ui.end_row();
                    let free = [s.stack_free_default, s.stack_free_led, s.stack_free_uart];
                    for (i, (name, prio, size, what)) in proto::tasks().iter().enumerate() {
                        let used = size.saturating_sub(free[i] as u32);
                        let frac = used as f32 / *size as f32;
                        ui.label(RichText::new(*name).monospace().strong()).on_hover_text(*what);
                        ui.label(*prio);
                        ui.label(fmt::bytes(*size));
                        gauge(ui, &p, frac, &format!("{} Б, свободно {} Б", used, free[i]), level(&p, frac), 190.0);
                        ui.end_row();
                    }
                    for (name, prio, size, what) in [("IDLE", "0 (ядро)", 512, "простой, чистит удалённые задачи"), ("Tmr Svc", "2 (ядро)", 1024, "программные таймеры (не используются)")] {
                        ui.label(RichText::new(name).monospace().color(p.muted)).on_hover_text(what);
                        ui.label(RichText::new(prio).color(p.muted));
                        ui.label(RichText::new(fmt::bytes(size)).color(p.muted));
                        ui.label(RichText::new("не измеряется").small().color(p.muted));
                        ui.end_row();
                    }
                });
                hint(ui, &p, "Пик = размер − «высокая вода» (uxTaskGetStackHighWaterMark). < 128 Б свободно — пора увеличивать стек. Наведите на имя задачи — её назначение.");
            });
            card(&mut cols[1], &p, "Куча FreeRTOS (heap_4, 10 КБ)", |ui| {
                let heap_total = proto::heap_total();
                let used = heap_total - s.heap_free;
                let peak = heap_total - s.heap_min;
                ui.label("Занято сейчас");
                gauge(ui, &p, used as f32 / heap_total as f32, &format!("{} из {}", fmt::bytes(used), fmt::bytes(heap_total)), level(&p, used as f32 / heap_total as f32), ui.available_width());
                ui.label("Максимум за всё время (xPortGetMinimumEverFreeHeapSize)");
                gauge(ui, &p, peak as f32 / heap_total as f32, &format!("{} — свободно минимум {}", fmt::bytes(peak), fmt::bytes(s.heap_min)), level(&p, peak as f32 / heap_total as f32), ui.available_width());
                para(ui, &p, "Из кучи выделены: стеки трёх задач (3.5 КБ) и задач ядра, их TCB, очередь uartQueue (100 Б) и мьютекс лога. Во время работы память не выделяется — график должен быть ровным. Рост «занято» = утечка.");
            });
        });
        ui.add_space(8.0);
        plots(ui, sh, st);
        if let Some(d) = &sh.st.diag {
            ui.add_space(8.0);
            card(ui, &p, "g_diag (по SWD, .noinit)", |ui| {
                egui::Grid::new("gdiag").num_columns(4).spacing([14.0, 3.0]).show(ui, |ui| {
                    kv(ui, &p, "magic", format!("0x{:08X} {}", d.magic, if d.valid() { "✓" } else { "— не инициализирован" }));
                    kv(ui, &p, "последний сбой", if d.has_fault() { d.error_name().to_string() } else { "нет".into() });
                    kv(ui, &p, "rx_overflow", d.rx_overflow.to_string());
                    kv(ui, &p, "last_cmd", format!("0x{:02X}", d.last_cmd));
                });
            });
        }
        ui.add_space(8.0);
        flow(ui, sh);
    });
}

fn metric(ui: &mut egui::Ui, sh: &Shared, title: &str, value: &str, sub: &str) {
    let p = sh.pal;
    card(ui, &p, "", |ui| {
        ui.label(RichText::new(title).color(p.muted));
        ui.label(RichText::new(value).size(22.0).strong());
        ui.add(egui::Label::new(RichText::new(sub).small().color(p.fg2)).wrap());
    });
}

fn plots(ui: &mut egui::Ui, sh: &Shared, st: &St) {
    let p = sh.pal;
    let Some(last) = sh.samples.back() else { return };
    let t_end = last.t;
    let t0 = t_end - st.window_s as f64;
    let pts = |f: &dyn Fn(&proto::Status) -> f64| -> PlotPoints {
        sh.samples.iter().filter(|x| x.t >= t0).map(|x| [x.t - t_end, f(&x.st)]).collect::<Vec<_>>().into()
    };
    ui.columns(2, |cols| {
        card(&mut cols[0], &p, "Свободно в стеках задач, байт", |ui| {
            Plot::new("stk").height(190.0).legend(Legend::default()).allow_scroll(false).x_axis_label("с").show(ui, |pu| {
                pu.line(Line::new("defaultTask", pts(&|s| s.stack_free_default as f64)).color(p.accent));
                pu.line(Line::new("LedTask", pts(&|s| s.stack_free_led as f64)).color(p.sig));
                pu.line(Line::new("Uart1Task", pts(&|s| s.stack_free_uart as f64)).color(p.clk));
            });
        });
        card(&mut cols[1], &p, "Куча и обмен", |ui| {
            Plot::new("heap").height(190.0).legend(Legend::default()).allow_scroll(false).x_axis_label("с").show(ui, |pu| {
                pu.line(Line::new("куча свободно, Б", pts(&|s| s.heap_free as f64)).color(p.ok));
                pu.line(Line::new("куча минимум, Б", pts(&|s| s.heap_min as f64)).color(p.warn));
                pu.line(Line::new("кадров принято ×10", pts(&|s| s.rx_frames as f64 * 10.0)).color(p.refc));
                pu.line(Line::new("ошибок CRC ×100", pts(&|s| s.rx_crc_err as f64 * 100.0)).color(p.err));
            });
        });
    });
}

/// Data flow inside the firmware (who talks to whom).
fn flow(ui: &mut egui::Ui, sh: &Shared) {
    let p = sh.pal;
    card(ui, &p, "Как устроена прошивка", |ui| {
        let (cv, _) = Canvas::new(ui, [0.0, 0.0, 1000.0, 250.0], 290.0);
        let bx = |x: f32, y: f32, w: f32, t: &str, sub: &str, c| {
            cv.rect(x, y, w, 56.0, 7.0, mix(c, 12.0, p.panel), Stroke::new(1.4, c));
            cv.text_bold(x + w / 2.0, y + 20.0, Align2::CENTER_CENTER, t, 13.0, p.fg);
            cv.text(x + w / 2.0, y + 40.0, Align2::CENTER_CENTER, sub, 10.5, p.fg2);
        };
        let ar = |pts: &[(f32, f32)], c| {
            cv.line(pts, c, 2.0);
            let (x, y) = pts[pts.len() - 1];
            cv.line(&[(x - 8.0, y - 4.0), (x, y), (x - 8.0, y + 4.0)], c, 2.0);
        };
        bx(10.0, 20.0, 150.0, "ПК: UART1", "38400, кадры с CRC", p.clk);
        bx(220.0, 20.0, 170.0, "USART1 IRQ", "prio 5, по 1 байту", p.pwr);
        bx(450.0, 20.0, 150.0, "uartQueue", "очередь 100 байт", p.sig);
        bx(660.0, 20.0, 170.0, "Uart1Task", "пауза 20 мс = конец кадра", p.accent);
        ar(&[(160.0, 48.0), (220.0, 48.0)], p.clk);
        ar(&[(390.0, 48.0), (450.0, 48.0)], p.pwr);
        ar(&[(600.0, 48.0), (660.0, 48.0)], p.sig);
        bx(860.0, 20.0, 130.0, "FRAM (SPI1)", "команда 0x02", p.dig);
        ar(&[(830.0, 48.0), (860.0, 48.0)], p.accent);
        cv.line(&[(745.0, 76.0), (745.0, 110.0), (85.0, 110.0), (85.0, 76.0)], p.accent, 2.0);
        cv.line(&[(77.0, 84.0), (85.0, 76.0), (93.0, 84.0)], p.accent, 2.0);
        cv.text(415.0, 104.0, Align2::CENTER_BOTTOM, "ответ 01 · статус · данные · CRC", 11.0, p.accent);
        bx(220.0, 160.0, 170.0, "defaultTask", "Tick 1 с, DIAG 5 с", p.accent);
        bx(450.0, 160.0, 150.0, "log_mutex", "один писатель", p.sig);
        bx(660.0, 160.0, 170.0, &format!("{} → ПК", crate::core::boards::cur().log_uart), "115200, лог", p.refc);
        ar(&[(390.0, 188.0), (450.0, 188.0)], p.accent);
        ar(&[(600.0, 188.0), (660.0, 188.0)], p.sig);
        bx(10.0, 160.0, 150.0, "LedTask", "PC13 раз в 1 с", p.warn);
        cv.text(870.0, 175.0, Align2::LEFT_TOP, "сбой → FAULT", 11.0, p.err);
        cv.text(870.0, 192.0, Align2::LEFT_TOP, "+ g_diag (.noinit)", 11.0, p.err);
    });
}
