//! Board drawing (Blue Pill or Black Pill): headers as on the silkscreen, project usage, live GPIO
//! state over SWD, pin details with alternate functions.

use crate::app::{Act, Shared};
use crate::canvas::Canvas;
use crate::core::mcu::{self, PinKind};
use crate::core::boards::{self, BoardId};
use crate::theme::{mix, Pal};
use crate::widgets::*;
use eframe::egui::{self, Align2, Color32, RichText, Stroke};

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Filter {
    #[default]
    Project,
    Uart,
    Spi,
    I2c,
    Adc,
    Tim,
    Ft,
}

#[derive(Default)]
pub struct St {
    selected: Option<String>,
    filter: Filter,
    last_read: f64,
}

/// Live mode and level of `pin` from the SWD cache.
fn live(sh: &Shared, pin: &str) -> Option<mcu::PinLive> {
    mcu::gpio_live(|a| sh.mem(a), pin)
}

fn matches(f: Filter, pin: &mcu::Pin) -> bool {
    let af = format!("{} {}", pin.af, pin.remap);
    match f {
        Filter::Project => mcu::usage(pin.name).is_some(),
        Filter::Uart => af.contains("USART"),
        Filter::Spi => af.contains("SPI"),
        Filter::I2c => af.contains("I2C"),
        Filter::Adc => af.contains("ADC"),
        Filter::Tim => af.contains("TIM"),
        Filter::Ft => pin.ft,
    }
}

fn pin_color(p: &Pal, sh: &Shared, f: Filter, silk: &str) -> (Color32, bool) {
    match silk {
        "GND" => return (p.muted, false),
        "3.3" | "5V" => return (p.pwr, false),
        _ => {}
    }
    let Some(name) = mcu::silk_to_pin(silk) else { return (p.muted, false) };
    let Some(pin) = mcu::pin(name) else { return (p.muted, false) };
    if !matches(f, pin) {
        return (mix(p.fg2, 45.0, p.panel), false);
    }
    if f == Filter::Project {
        let tone = mcu::usage(name).map(|u| u.1).unwrap_or("fg");
        let c = match tone {
            "refc" => p.refc,
            other => p.tone(other),
        };
        return (c, true);
    }
    let _ = sh;
    (p.accent, true)
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    // live GPIO refresh once a second while SWD is connected
    let now = ui.ctx().input(|i| i.time);
    if sh.swd_ok() && now - st.last_read > 1.0 {
        st.last_read = now;
        sh.job(crate::worker::Job::ReadWords(mcu::gpio_poll_addrs()));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
    }

    let b = boards::cur();
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(
            ui,
            &p,
            &format!("Плата {} · {}", b.board, b.chip),
            "Выводы подписаны как на шелкографии. Наведите — краткая справка, нажмите — подробности. \
             При подключённом SWD цвет точки показывает живой уровень вывода.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("Подсветить:");
            for (f, t) in [
                (Filter::Project, "используется в проекте"),
                (Filter::Uart, "USART"),
                (Filter::Spi, "SPI"),
                (Filter::I2c, "I²C"),
                (Filter::Adc, "АЦП"),
                (Filter::Tim, "таймеры"),
                (Filter::Ft, "5 В-толерантные (FT)"),
            ] {
                ui.selectable_value(&mut st.filter, f, t);
            }
        });
        ui.add_space(4.0);
        draw_board(ui, sh, st);
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            details(&mut cols[0], sh, st);
            card(&mut cols[1], &p, "Что подключено в этом проекте", |ui| {
                egui::Grid::new("wiring").num_columns(3).spacing([12.0, 5.0]).striped(true).show(ui, |ui| {
                    for &(who, pins, what) in b.wiring {
                        ui.label(RichText::new(who).color(p.fg));
                        ui.label(RichText::new(pins).monospace().color(p.accent));
                        ui.label(RichText::new(what).color(p.fg2));
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                hint(ui, &p, b.safety);
            });
        });
    });
}

fn draw_board(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = boards::cur();
    let black = b.id == BoardId::BlackPill;
    let (cv, _) = Canvas::new(ui, [0.0, 0.0, 1000.0, 372.0], 434.0);
    // PCB: blue for the Blue Pill, near-black for the WeAct Black Pill
    let pcb = match (black, p.dark) {
        (false, true) => Color32::from_rgb(0x12, 0x2a, 0x5c),
        (false, false) => Color32::from_rgb(0x1d, 0x4e, 0xa8),
        (true, true) => Color32::from_rgb(0x16, 0x18, 0x1c),
        (true, false) => Color32::from_rgb(0x26, 0x29, 0x30),
    };
    cv.rect(24.0, 26.0, 916.0, 308.0, 14.0, pcb, Stroke::new(1.5, mix(pcb, 60.0, Color32::BLACK)));
    let silk = Color32::from_rgb(0xee, 0xf2, 0xff);
    // USB connector
    cv.rect(12.0, 140.0, 70.0, 80.0, 6.0, Color32::from_rgb(0xb8, 0xbe, 0xc8), Stroke::new(1.0, Color32::from_rgb(0x70, 0x76, 0x80)));
    cv.text(56.0, 228.0, Align2::CENTER_TOP, if black { "USB-C" } else { "micro-USB" }, 11.0, silk);
    cv.text(56.0, 243.0, Align2::CENTER_TOP, "(A11/A12)", 10.0, mix(silk, 70.0, pcb));
    // HSE crystal
    cv.rect(150.0, 150.0, 60.0, 26.0, 10.0, Color32::from_rgb(0xc9, 0xcd, 0xd4), Stroke::new(1.0, Color32::GRAY));
    cv.text(180.0, 180.0, Align2::CENTER_TOP, b.crystal, 10.0, silk);
    // buttons: Blue Pill has RESET only; Black Pill has BOOT0, NRST and KEY (PA0)
    let buttons: &[(&str, f32, f32)] = if black { &[("BOOT0", 240.0, 110.0), ("NRST", 240.0, 200.0), ("KEY · A0", 640.0, 200.0)] } else { &[("RESET", 240.0, 200.0)] };
    for &(name, bx, by) in buttons {
        cv.rect(bx, by, 44.0, 44.0, 4.0, Color32::from_rgb(0xdd, 0xdd, 0xdd), Stroke::new(1.0, Color32::GRAY));
        cv.circle(bx + 22.0, by + 22.0, 12.0, Color32::from_rgb(0x30, 0x30, 0x30), Stroke::NONE);
        cv.text(bx + 22.0, by + 48.0, Align2::CENTER_TOP, name, 10.0, silk);
    }
    if black {
        // on-board SPI NOR flash (WeAct footprint U3) on PA4..PA7
        cv.rect(330.0, 238.0, 56.0, 34.0, 3.0, Color32::from_rgb(0x1b, 0x1d, 0x22), Stroke::new(1.0, Color32::from_rgb(0x55, 0x58, 0x60)));
        cv.text(358.0, 255.0, Align2::CENTER_CENTER, "W25Q64", 9.0, silk);
        cv.hit(ui, "spi-flash", 330.0, 238.0, 56.0, 34.0).on_hover_text("SPI NOR-флеш 8 МБ на SPI1 (A4 CS, A5–A7). Прошивка узнаёт её по JEDEC EF 40 17.");
    }
    // MCU (rotated 45° on the real board)
    let (mx, my, r) = (470.0, 180.0, 82.0);
    let diamond: Vec<egui::Pos2> = [(mx, my - r), (mx + r, my), (mx, my + r), (mx - r, my)].iter().map(|(x, y)| cv.p(*x, *y)).collect();
    cv.painter.add(egui::Shape::convex_polygon(diamond, Color32::from_rgb(0x1b, 0x1d, 0x22), Stroke::new(1.0, Color32::from_rgb(0x55, 0x58, 0x60))));
    cv.text_bold(mx, my - 14.0, Align2::CENTER_CENTER, "STM32", 15.0, silk);
    cv.text(mx, my + 6.0, Align2::CENTER_CENTER, b.chip_short, 12.0, silk);
    cv.text(mx, my + 22.0, Align2::CENTER_CENTER, b.package, 10.0, mix(silk, 60.0, Color32::BLACK));
    let mcu_hit = cv.hit(ui, "mcu", mx - 60.0, my - 60.0, 120.0, 120.0).on_hover_text("Контроллер — схема блоков на экране «Контроллер»");
    if mcu_hit.clicked() {
        sh.act(Act::Go(crate::app::View::Mcu));
    }
    // BOOT jumpers (Blue Pill only; the Black Pill has a BOOT0 button)
    let jumpers: &[(&str, &str)] = if black { &[] } else { &[("BOOT0", "0"), ("BOOT1", "0")] };
    for (i, (name, val)) in jumpers.iter().enumerate() {
        let x = 640.0 + i as f32 * 60.0;
        cv.rect(x, 150.0, 44.0, 22.0, 3.0, Color32::from_rgb(0x20, 0x20, 0x20), Stroke::NONE);
        cv.rect(x + 2.0, 152.0, 20.0, 18.0, 2.0, Color32::from_rgb(0xe8, 0xc5, 0x3a), Stroke::NONE);
        cv.text(x + 22.0, 178.0, Align2::CENTER_TOP, format!("{} = {}", name, val), 10.0, silk);
    }
    // Both lamps sit at the SWD end, opposite the USB jack (Black Pill: blue LED).
    lamp(&cv, 800.0, 150.0, true, Color32::from_rgb(0xef, 0x44, 0x44), "PWR", "питание");
    let (pc13_on, pc13_state) = pc13_lamp(ui, sh);
    let led_c = if black { Color32::from_rgb(0x3b, 0x82, 0xf6) } else { Color32::from_rgb(0x3d, 0xd6, 0x6a) };
    lamp(&cv, 800.0, 228.0, pc13_on, led_c, "LED", pc13_state);
    cv.hit(ui, "led-pc13", 782.0, 210.0, 36.0, 56.0).on_hover_text(
        "Светодиод платы на PC13, активный 0. LedTask мигает раз в секунду; команда 0x70 включает, гасит или возвращает мигание.",
    );

    // headers
    let step = 36.0;
    let x0 = 150.0;
    let mut clicked: Option<String> = None;
    let mut draw_pin = |label: &str, x: f32, y: f32, label_dy: f32, stagger: bool| {
        let (c, active) = pin_color(&p, sh, st.filter, label);
        let name = mcu::silk_to_pin(label);
        let sel = name.is_some() && st.selected.as_deref() == name;
        let lvl = name.and_then(|n| live(sh, n));
        cv.rect(x - 13.0, y - 13.0, 26.0, 26.0, 3.0, mix(Color32::BLACK, 70.0, pcb), Stroke::NONE);
        let fill = match lvl {
            Some(l) => {
                let bit = if l.output { l.odr } else { l.idr };
                if bit == 1 {
                    c
                } else {
                    mix(c, 30.0, Color32::BLACK)
                }
            }
            None => c,
        };
        cv.circle(x, y, if active { 8.5 } else { 7.0 }, fill, Stroke::new(if sel { 3.0 } else { 1.0 }, if sel { p.fg } else { Color32::from_rgb(0xd4, 0xaf, 0x37) }));
        let usage = name.and_then(mcu::usage).filter(|_| st.filter == Filter::Project);
        if label_dy == 0.0 {
            // side header: silkscreen to the left (the function is in the tooltip)
            cv.text(x - 17.0, y, Align2::RIGHT_CENTER, label, 10.0, silk);
        } else {
            let al = if label_dy < 0.0 { Align2::CENTER_BOTTOM } else { Align2::CENTER_TOP };
            cv.text(x, y + label_dy, al, label, 11.0, silk);
            if let Some(u) = usage {
                // neighbouring pins alternate between two rows so names never overlap
                // just outside the PCB edge, two rows
                let off = if stagger { 52.0 } else { 40.0 };
                cv.text(x, y + label_dy.signum() * off, al, u.0, 9.0, c);
            }
        }
        let r = cv.hit(ui, ("pin", label, x as i32), x - 14.0, y - 14.0, 28.0, 28.0);
        let r = r.on_hover_ui(|ui| pin_summary(ui, &p, label));
        if r.clicked() {
            clicked = Some(label.to_string());
        }
    };
    for (i, s) in b.header_top.iter().enumerate() {
        draw_pin(s, x0 + i as f32 * step, 64.0, -16.0, i % 2 == 1);
    }
    for (i, s) in b.header_bottom.iter().enumerate() {
        draw_pin(s, x0 + i as f32 * step, 296.0, 16.0, i % 2 == 1);
    }
    for (i, s) in b.header_swd.iter().enumerate() {
        draw_pin(s, 905.0, 128.0 + i as f32 * 36.0, 0.0, false);
    }
    cv.text(905.0, 100.0, Align2::CENTER_BOTTOM, "SWD", 11.0, silk);

    if let Some(c) = clicked {
        st.selected = mcu::silk_to_pin(&c).map(String::from);
    }
}

/// PC13 is active-low. In auto mode LedTask toggles it every second; the
/// picture blinks on that rhythm so the lamp is visible between STATUS polls.
fn pc13_lamp(ui: &egui::Ui, sh: &Shared) -> (bool, &'static str) {
    if sh.swd_ok() {
        if let Some(l) = live(sh, "PC13") {
            let on = l.odr == 0;
            return (on, if on { "горит" } else { "погашен" });
        }
    }
    if let Some(s) = sh.st.status.filter(|s| s.led_manual == 1) {
        let on = s.led_odr == 0;
        return (on, if on { "горит" } else { "погашен" });
    }
    let on = (ui.input(|i| i.time) as i64).rem_euclid(2) == 0;
    (on, "мигает")
}

fn lamp(cv: &Canvas, x: f32, y: f32, on: bool, color: Color32, name: &str, state: &str) {
    let silk = Color32::from_rgb(0xee, 0xf2, 0xff);
    let lead = Color32::from_rgb(0xc8, 0xcc, 0xd4);
    if on {
        cv.circle(x, y, 18.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90), Stroke::NONE);
    }
    let body = if on { color } else { mix(color, 22.0, Color32::BLACK) };
    cv.circle(x, y, 10.0, body, Stroke::new(1.5, Color32::from_rgb(0x11, 0x11, 0x11)));
    cv.line(&[(x - 5.0, y + 10.0), (x - 7.0, y + 18.0)], lead, 1.6);
    cv.line(&[(x + 5.0, y + 10.0), (x + 7.0, y + 18.0)], lead, 1.6);
    cv.text(x, y + 22.0, Align2::CENTER_TOP, name, 11.0, silk);
    cv.text(x, y + 36.0, Align2::CENTER_TOP, state, 10.0, silk);
}

fn pin_summary(ui: &mut egui::Ui, p: &Pal, silk: &str) {
    if let Some(t) = mcu::silk_ru(silk) {
        ui.label(RichText::new(silk).strong());
        ui.label(t);
        return;
    }
    let Some(pin) = mcu::silk_to_pin(silk).and_then(mcu::pin) else { return };
    ui.label(RichText::new(format!("{}  (вывод {} {}){}", pin.name, pin.lqfp, boards::cur().package, if pin.ft { "  · FT 5 В" } else { "" })).strong());
    if !pin.af.is_empty() {
        ui.label(format!("функции: {}", pin.af));
    }
    if let Some(u) = mcu::usage(pin.name) {
        ui.label(RichText::new(format!("в проекте: {} — {}", u.0, u.2)).color(p.accent));
    }
}

fn details(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let title = st.selected.as_deref().map(|s| format!("Вывод {}", s)).unwrap_or_else(|| "Вывод".into());
    card(ui, &p, &title, |ui| {
        let Some(pin) = st.selected.as_deref().and_then(mcu::pin) else {
            hint(ui, &p, "Нажмите на вывод на рисунке.");
            return;
        };
        egui::Grid::new("pin_det").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
            kv(ui, &p, &format!("Номер в корпусе {}", boards::cur().package), pin.lqfp.to_string());
            kv(
                ui,
                &p,
                "Тип",
                match pin.kind {
                    PinKind::Io => "ввод-вывод",
                    PinKind::Power => "питание",
                    PinKind::Ground => "земля",
                    PinKind::Reset => "сброс",
                    PinKind::Boot => "режим загрузки",
                },
            );
            kv(ui, &p, "5 В-толерантный", if pin.ft { "да (FT)" } else { "нет — только 3.3 В" });
            if !pin.af.is_empty() {
                kv(ui, &p, "Альтернативные функции", pin.af);
            }
            if !pin.remap.is_empty() {
                kv(ui, &p, "После remap (AFIO_MAPR)", pin.remap);
            }
            if let Some(u) = mcu::usage(pin.name) {
                kv(ui, &p, "В этом проекте", format!("{} — {}", u.0, u.2));
            }
            if let Some(l) = live(sh, pin.name) {
                kv(ui, &p, "Режим (по SWD)", l.mode);
                kv(ui, &p, "Уровень IDR / ODR", format!("{} / {}", l.idr, l.odr));
            }
        });
        if !pin.note.is_empty() {
            para(ui, &p, pin.note);
        }
        if pin.name.starts_with('P') && pin.kind == PinKind::Io {
            let port = format!("GPIO{}", &pin.name[1..2]);
            if ui.button(format!("Регистры {} →", port)).clicked() {
                sh.act(Act::OpenPeriph(port));
            }
        }
        if !sh.swd_ok() {
            hint(ui, &p, "Подключите SWD (или симулятор), чтобы видеть режим и уровень вывода вживую.");
        }
    });
}
