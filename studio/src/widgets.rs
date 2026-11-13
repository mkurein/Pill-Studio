//! Small widgets shared by all views.

use crate::theme::{mix, Pal};
use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Stroke, Ui};

/// Status chip in the top bar: "UART1 ● /dev/cu.usbserial-120".
pub fn chip(ui: &mut Ui, p: &Pal, label: &str, value: &str, ok: bool) -> egui::Response {
    let c = if ok { p.ok } else { p.muted };
    let text = RichText::new(format!("{}  ● {}", label, value)).color(if ok { p.fg } else { p.muted }).size(12.0);
    ui.add(
        egui::Button::new(text)
            .fill(mix(c, if ok { 16.0 } else { 6.0 }, p.bg2))
            .stroke(Stroke::new(1.0, mix(c, 55.0, p.bg2)))
            .corner_radius(CornerRadius::same(12)),
    )
}

/// Panel with a title row.
pub fn card<R>(ui: &mut Ui, p: &Pal, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            if !title.is_empty() {
                ui.label(RichText::new(title).strong().size(14.0).color(p.fg));
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner
}

pub fn h1(ui: &mut Ui, p: &Pal, title: &str, sub: &str) {
    ui.label(RichText::new(title).size(22.0).strong().color(p.fg));
    if !sub.is_empty() {
        ui.label(RichText::new(sub).color(p.fg2));
    }
    ui.add_space(8.0);
}

pub fn hint(ui: &mut Ui, p: &Pal, text: &str) {
    ui.label(RichText::new(text).small().color(p.muted));
}

pub fn kv(ui: &mut Ui, p: &Pal, k: &str, v: impl Into<String>) {
    ui.label(RichText::new(k).color(p.muted));
    ui.add(egui::Label::new(RichText::new(v.into()).color(p.fg).monospace()).wrap());
    ui.end_row();
}

pub fn badge(ui: &mut Ui, p: &Pal, text: &str, c: Color32) {
    egui::Frame::new()
        .fill(mix(c, 18.0, p.panel))
        .stroke(Stroke::new(1.0, mix(c, 60.0, p.panel)))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(c));
        });
}

/// Horizontal bar: `frac` of 1.0 filled.
pub fn gauge(ui: &mut Ui, p: &Pal, frac: f32, text: &str, c: Color32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, p.panel3);
    let mut fill = rect;
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
    painter.rect_filled(fill, 4.0, mix(c, 70.0, p.panel3));
    painter.text(rect.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.5), p.fg);
}

/// Colour for "fraction used": green → yellow → red.
pub fn level(p: &Pal, used: f32) -> Color32 {
    if used > 0.85 {
        p.err
    } else if used > 0.65 {
        p.warn
    } else {
        p.ok
    }
}

/// A primary (accent) button.
pub fn primary(ui: &mut Ui, p: &Pal, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(p.accent_fg)).fill(p.accent))
}

/// Paragraph text that wraps.
pub fn para(ui: &mut Ui, p: &Pal, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(p.fg2)).wrap());
}
