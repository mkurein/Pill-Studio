//! Register browser built from the SVD: every peripheral, register and field
//! with addresses and reset values; live values over SWD, bit grid, field
//! decoding and (two-step confirmed) writes.

use crate::app::Shared;
use crate::core::svd::{self, Peripheral, Register};
use crate::fmt;
use crate::theme::mix;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, Color32, RichText, Sense, Stroke};

pub struct St {
    periph: String,
    reg: usize,
    filter: String,
    only_c8: bool,
    auto: bool,
    last_read: f64,
    draft: Option<u32>,
    armed_until: f64,
}

impl Default for St {
    fn default() -> Self {
        St {
            periph: "GPIOC".into(),
            reg: 3,
            filter: String::new(),
            only_c8: true,
            auto: true,
            last_read: 0.0,
            draft: None,
            armed_until: 0.0,
        }
    }
}

impl St {
    pub fn select(&mut self, name: &str) {
        if self.periph != name {
            self.periph = name.to_string();
            self.reg = 0;
            self.draft = None;
        }
    }
}

/// Short Russian title for peripheral groups.
fn group_ru(g: &str) -> &str {
    match g {
        "GPIO" => "порты ввода-вывода",
        "USART" => "UART/USART",
        "SPI" => "SPI",
        "I2C" => "I²C",
        "TIM" => "таймеры",
        "ADC" => "АЦП",
        "RCC" => "тактирование и сброс",
        "DMA" => "DMA",
        "RTC" => "часы реального времени",
        "FLASH" => "интерфейс Flash",
        "PWR" => "питание",
        "IWDG" | "WWDG" => "сторожевой таймер",
        "EXTI" => "внешние прерывания",
        "AFIO" => "переназначение выводов",
        "NVIC" => "контроллер прерываний",
        "CAN" => "CAN",
        "USB" => "USB",
        "CRC" => "CRC",
        "BKP" => "backup-регистры",
        _ => g,
    }
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let Some(per) = sh.periph(&st.periph).or_else(|| sh.svd.first()) else {
        ui.label("SVD не загружен");
        return;
    };
    if st.reg >= per.registers.len() {
        st.reg = 0;
    }
    // live refresh
    let now = ui.ctx().input(|i| i.time);
    if sh.swd_ok() && st.auto && now - st.last_read > 1.0 {
        st.last_read = now;
        read_periph(sh, per);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
    }

    h1(ui, &p, "Регистры", "Полный справочник из STM32F103.svd (CMSIS-SVD, тексты описаний — из файла ST, на английском). Значения — живые, по SWD.");
    egui::SidePanel::left("periph_list").resizable(true).default_width(220.0).frame(egui::Frame::new()).show_inside(ui, |ui| {
        ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text("поиск: USART, BRR, ODR…"));
        ui.checkbox(&mut st.only_c8, "только то, что есть в C8");
        egui::ScrollArea::vertical().id_salt("plist").show(ui, |ui| {
            let f = st.filter.to_uppercase();
            for q in sh.svd.iter() {
                if st.only_c8 && !q.in_c8 {
                    continue;
                }
                if !f.is_empty() && !q.name.contains(&f) && !q.registers.iter().any(|r| r.name.contains(&f)) {
                    continue;
                }
                let sel = q.name == per.name;
                let r = ui.selectable_label(sel, RichText::new(&q.name).monospace().color(if q.in_c8 { p.fg } else { p.muted }));
                let r = r.on_hover_text(format!("{} · 0x{:08X} · {}", q.desc, q.base, group_ru(&q.group)));
                if r.clicked() {
                    st.select(&q.name.clone());
                }
            }
        });
    });

    egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, ..Default::default() })).show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&per.name).size(18.0).strong());
            ui.label(RichText::new(format!("0x{:08X}", per.base)).monospace().color(p.accent));
            ui.label(RichText::new(&per.desc).color(p.fg2));
            if !per.in_c8 {
                badge(ui, &p, "нет в STM32F103C8", p.warn);
            }
        });
        ui.horizontal(|ui| {
            if sh.swd_ok() {
                ui.checkbox(&mut st.auto, "обновлять каждую секунду");
                if ui.button("Прочитать сейчас").clicked() {
                    read_periph(sh, per);
                }
                if sh.st.mem_t > 0.0 {
                    hint(ui, &p, "регистры с побочным эффектом чтения (DR, SR у UART/SPI/I2C) автоматически не читаются");
                }
            } else {
                hint(ui, &p, "Подключите SWD (Обзор) — появятся текущие значения регистров.");
            }
        });
        if !per.irqs.is_empty() {
            let irqs: Vec<String> = per.irqs.iter().map(|(n, v)| format!("{} #{}", n, v)).collect();
            hint(ui, &p, &format!("Прерывания: {}", irqs.join(", ")));
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("regs").max_height(ui.available_height() * 0.42).show(ui, |ui| {
            egui::Grid::new("reglist").num_columns(6).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
                for h in ["Регистр", "Смещение", "Адрес", "Сброс", "Сейчас", "Описание"] {
                    ui.label(RichText::new(h).strong().color(p.fg2));
                }
                ui.end_row();
                for (i, r) in per.registers.iter().enumerate() {
                    let sel = i == st.reg;
                    if ui.selectable_label(sel, RichText::new(&r.name).monospace().strong()).clicked() {
                        st.reg = i;
                        st.draft = None;
                    }
                    ui.label(RichText::new(format!("+0x{:02X}", r.offset)).monospace().color(p.muted));
                    ui.label(RichText::new(format!("0x{:08X}", per.addr(r))).monospace().color(p.fg2));
                    ui.label(RichText::new(format!("0x{:08X}", r.reset)).monospace().color(p.muted));
                    match sh.mem(per.addr(r)) {
                        Some(v) => {
                            let c = if v != r.reset { p.warn } else { p.fg };
                            ui.label(RichText::new(format!("0x{:08X}", v)).monospace().color(c));
                        }
                        None if svd::read_has_side_effect(&per.name, &r.name) => {
                            ui.label(RichText::new("не читается авто").small().color(p.muted));
                        }
                        None => {
                            ui.label("—");
                        }
                    }
                    ui.add(egui::Label::new(RichText::new(&r.desc).color(p.fg2)).truncate());
                    ui.end_row();
                }
            });
        });
        ui.separator();
        if let Some(r) = per.registers.get(st.reg) {
            egui::ScrollArea::vertical().id_salt("fields").show(ui, |ui| register_detail(ui, sh, st, per, r, now));
        }
    });
}

fn read_periph(sh: &Shared, per: &Peripheral) {
    let addrs: Vec<u32> = per
        .registers
        .iter()
        .filter(|r| !svd::read_has_side_effect(&per.name, &r.name))
        .map(|r| per.addr(r))
        .collect();
    if !addrs.is_empty() {
        sh.job(Job::ReadWords(addrs));
    }
}

fn register_detail(ui: &mut egui::Ui, sh: &Shared, st: &mut St, per: &Peripheral, r: &Register, now: f64) {
    let p = sh.pal;
    let addr = per.addr(r);
    let live = sh.mem(addr);
    let shown = st.draft.or(live).unwrap_or(r.reset);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}_{}", per.name, r.name)).size(16.0).strong());
        ui.label(RichText::new(format!("0x{:08X}", addr)).monospace().color(p.accent));
        badge(ui, &p, if r.access.is_empty() { "read-write" } else { &r.access }, p.clk);
        badge(ui, &p, &format!("{} бит", r.size), p.muted);
        if svd::read_has_side_effect(&per.name, &r.name) {
            badge(ui, &p, "чтение меняет состояние", p.warn);
            if sh.swd_ok() && ui.small_button("прочитать всё равно").clicked() {
                sh.job(Job::ReadWords(vec![addr]));
            }
        }
    });
    para(ui, &p, &r.desc);
    ui.label(
        RichText::new(format!(
            "{} = 0x{:08X}  ·  {}{}",
            if st.draft.is_some() { "черновик" } else if live.is_some() { "сейчас" } else { "сброс" },
            shown,
            fmt::bin_groups(shown, 32),
            if live.is_none() && st.draft.is_none() { "  (значение после сброса)" } else { "" }
        ))
        .monospace()
        .color(p.fg),
    );
    bit_grid(ui, sh, st, r, shown);
    ui.add_space(6.0);
    egui::Grid::new("fields").num_columns(5).striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
        for h in ["Поле", "Биты", "Значение", "Доступ", "Описание"] {
            ui.label(RichText::new(h).strong().color(p.fg2));
        }
        ui.end_row();
        for f in &r.fields {
            ui.label(RichText::new(&f.name).monospace().strong());
            let bits = if f.width == 1 { format!("{}", f.offset) } else { format!("{}:{}", f.offset + f.width - 1, f.offset) };
            ui.label(RichText::new(bits).monospace().color(p.muted));
            let v = f.get(shown);
            let mut extra = String::new();
            if per.group == "GPIO" && (f.name.starts_with("MODE") || f.name.starts_with("CNF")) {
                let pin = f.name.trim_start_matches(|c: char| c.is_ascii_alphabetic());
                if let Ok(n) = pin.parse::<u32>() {
                    let sh_ = (n % 8) * 4;
                    let nib = (shown >> sh_) & 0xF;
                    extra = format!("  P{}{}: {} {}", &per.name[4..5], n + if r.name == "CRH" { 8 } else { 0 }, svd::gpio_mode_ru(nib & 3, nib >> 2), svd::gpio_speed_ru(nib & 3));
                }
            }
            ui.label(RichText::new(format!("{} (0x{:X}){}", v, v, extra)).monospace().color(if v != f.get(r.reset) { p.warn } else { p.fg }));
            ui.label(RichText::new(&f.access).small().color(p.muted));
            ui.add(egui::Label::new(RichText::new(&f.desc).color(p.fg2)).wrap());
            ui.end_row();
        }
    });
    // write
    if sh.swd_ok() && !r.access.contains("read-only") {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if let Some(d) = st.draft {
                ui.label(RichText::new(format!("Записать 0x{:08X} в {}?", d, r.name)).color(p.warn));
                let armed = now < st.armed_until;
                if ui.button(if armed { "Точно записать" } else { "Записать в MCU" }).clicked() {
                    if armed {
                        sh.job(Job::WriteWord(addr, d));
                        st.draft = None;
                        st.armed_until = 0.0;
                    } else {
                        st.armed_until = now + 4.0;
                    }
                }
                if ui.button("Отменить").clicked() {
                    st.draft = None;
                }
            } else {
                hint(ui, &p, "Нажмите на бит, чтобы подготовить запись. Запись меняет работу прошивки — используйте осознанно.");
            }
        });
    }
}

fn bit_grid(ui: &mut egui::Ui, sh: &Shared, st: &mut St, r: &Register, v: u32) {
    let p = sh.pal;
    let w = (ui.available_width() / 32.0).clamp(18.0, 34.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w * 32.0, 66.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let palette = [p.accent, p.sig, p.clk, p.dig, p.refc, p.pwr];
    for bit in 0..32u32 {
        let i = 31 - bit;
        let x = rect.left() + i as f32 * w;
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 16.0), egui::vec2(w - 1.0, 22.0));
        let field = r.fields.iter().position(|f| bit >= f.offset && bit < f.offset + f.width);
        let base = field.map(|k| palette[k % palette.len()]).unwrap_or(p.line);
        let on = v >> bit & 1 == 1;
        let fill = if field.is_none() { p.panel2 } else if on { mix(base, 75.0, p.panel) } else { mix(base, 14.0, p.panel) };
        painter.rect(cell, 2.0, fill, Stroke::new(1.0, mix(base, 50.0, p.panel)), egui::StrokeKind::Inside);
        painter.text(cell.center(), egui::Align2::CENTER_CENTER, if on { "1" } else { "0" }, egui::FontId::monospace(11.0), if on { Color32::WHITE } else { p.fg2 });
        if bit % 4 == 0 || bit == 31 {
            painter.text(egui::pos2(cell.center().x, rect.top() + 7.0), egui::Align2::CENTER_CENTER, bit.to_string(), egui::FontId::monospace(9.0), p.muted);
        }
        let id = ui.id().with(("bit", bit));
        let resp = ui.interact(cell, id, Sense::click());
        let resp = match field {
            Some(k) => {
                let f = &r.fields[k];
                resp.on_hover_text(format!("бит {} · {} — {}", bit, f.name, f.desc))
            }
            None => resp.on_hover_text(format!("бит {} — резерв", bit)),
        };
        if resp.clicked() && field.is_some() && sh.swd_ok() {
            st.draft = Some(v ^ (1 << bit));
        }
    }
    // field names under the grid
    for (k, f) in r.fields.iter().enumerate() {
        let x0 = rect.left() + (31 - (f.offset + f.width - 1)) as f32 * w;
        let width = f.width as f32 * w;
        // narrow fields (one bit, long name) alternate between two rows
        let est = f.name.chars().count() as f32 * 6.4;
        let dy = if est > width - 2.0 && k % 2 == 1 { 12.0 } else { 0.0 };
        if width >= 18.0 {
            painter.text(
                egui::pos2(x0 + width / 2.0, rect.top() + 47.0 + dy),
                egui::Align2::CENTER_CENTER,
                &f.name,
                egui::FontId::proportional(9.5),
                palette[k % palette.len()],
            );
        }
    }
}
