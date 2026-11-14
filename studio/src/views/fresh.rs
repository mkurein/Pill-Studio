//! One line that says whether the screen shows the program as it is now:
//! sources against the ELF, and the ELF against the Flash of the board.
//! Quiet when all is well; a button next to whatever is out of date.

use super::debug::ago;
use crate::app::Shared;
use crate::worker::Job;
use eframe::egui::{self, RichText};

pub fn bar(ui: &mut egui::Ui, sh: &Shared) {
    let p = sh.pal;
    if sh.st.elf.is_none() {
        return;
    }
    let idle = sh.st.job.is_none();
    let real = sh.swd_ok() && sh.st.swd.as_deref() != Some("виртуальная плата");
    let stale = matches!((sh.st.src_mtime, sh.st.elf_mtime), (Some(s), Some(e)) if s > e);
    let check = sh.st.flash_check.filter(|c| c.elf_mtime == sh.st.elf_mtime);
    ui.horizontal_wrapped(|ui| {
        if stale {
            ui.label(RichText::new("⚠ Исходники изменены после сборки: на экране прошлая сборка.").color(p.warn));
            if ui.add_enabled(idle, egui::Button::new("Собрать и прошить")).clicked() {
                sh.job(Job::BuildFlash(sh.cfg.clone()));
            }
        }
        match (real, check) {
            (true, Some(c)) if !c.same => {
                ui.label(RichText::new("⚠ На плате другая программа, чем на экране.").color(p.warn))
                    .on_hover_text(format!("Flash платы отличается от ELF с адреса 0x{:08X}", c.at.unwrap_or(0)));
                if ui.add_enabled(idle, egui::Button::new("Прошить")).clicked() {
                    sh.job(Job::Flash(sh.cfg.clone()));
                }
            }
            (true, None) => {
                if ui.small_button("Сверить с платой").clicked() {
                    sh.job(Job::VerifyFlash);
                }
            }
            _ => {}
        }
        if !stale {
            let built = sh.st.elf_mtime.map(ago).unwrap_or_default();
            let text = match (real, check) {
                (true, Some(c)) if c.same => format!("✓ Соответствует текущей программе: собрано {built}, на плате эта же сборка"),
                (true, _) => String::new(),
                (false, _) => format!("✓ Собрано {built}, исходники не менялись. Плата не сверена: нет ST-Link"),
            };
            if !text.is_empty() {
                let ok = real;
                ui.label(RichText::new(text).small().color(if ok { p.ok } else { p.muted }))
                    .on_hover_text("сравниваются время правки исходников со временем сборки ELF и содержимое Flash платы с кодом из ELF");
            }
        }
    });
    ui.add_space(4.0);
}
