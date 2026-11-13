//! Settings: project folder, toolchain, OpenOCD command line, ports, polling.

use crate::app::{Act, Shared};
use crate::core::config::Config;
use crate::core::openocd::OcdConfig;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct St {
    draft: Option<Config>,
    ocd_args: String,
    elf: String,
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let mut draft = st.draft.take().unwrap_or_else(|| sh.cfg.clone());
    let d = &mut draft;
    if st.ocd_args.is_empty() {
        st.ocd_args = join_args(&d.ocd.args);
    }
    let mut reset = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(ui, &p, "Настройки", "Хранятся в папке настроек пользователя (f103-studio/config.json).");
        card(ui, &p, "Проект прошивки", |ui| {
            egui::Grid::new("set_proj").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Папка проекта");
                ui.add(egui::TextEdit::singleline(&mut d.project_dir).desired_width(520.0));
                ui.end_row();
                ui.label("Команда сборки");
                ui.add(egui::TextEdit::singleline(&mut d.build_cmd).desired_width(520.0));
                ui.end_row();
                ui.label("Папка arm-none-eabi");
                ui.add(egui::TextEdit::singleline(&mut d.toolchain_dir).hint_text("пусто = PATH; Windows: …\\STM32CubeIDE\\plugins\\…gnu-tools…\\tools\\bin").desired_width(520.0));
                ui.end_row();
            });
            hint(ui, &p, "Команда выполняется в папке проекта. Здесь прошивка собирается корневым Makefile: «make -j8» → build/primGPT.elf. Каталога Debug в репозитории нет. Если папка toolchain пустая, на Windows студия сама находит make и arm-none-eabi-gcc из STM32CubeIDE.");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.elf).hint_text("путь к .elf вручную").desired_width(420.0));
                if ui.button("Загрузить ELF").clicked() && !st.elf.trim().is_empty() {
                    sh.job(Job::LoadElf(st.elf.trim().into()));
                }
            });
        });
        ui.add_space(8.0);
        card(ui, &p, "OpenOCD (SWD)", |ui| {
            egui::Grid::new("set_ocd").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Программа");
                ui.add(egui::TextEdit::singleline(&mut d.ocd.exe).desired_width(520.0));
                ui.end_row();
                ui.label("Аргументы");
                ui.add(egui::TextEdit::multiline(&mut st.ocd_args).desired_rows(2).desired_width(520.0).font(egui::TextStyle::Monospace));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut d.ocd.under_reset, "подключение под сбросом (нужен провод NRST)");
                ui.end_row();
            });
            hint(ui, &p, "Аргументы через пробел; фразы в кавычках — одним аргументом. OpenOCD из STM32CubeIDE не видит скрипты сам: нужен -s …\\st_scripts, interface/stlink-dap.cfg и transport select dapdirect_swd. Старые аргументы stlink.cfg + hla_swd студия подменяет сама.");
            if ui.small_button("Вернуть по умолчанию").clicked() {
                d.ocd = OcdConfig::default();
                st.ocd_args = join_args(&d.ocd.args);
            }
        });
        ui.add_space(8.0);
        card(ui, &p, "Порты и опрос", |ui| {
            egui::Grid::new("set_ports").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("UART1 по умолчанию");
                ui.add(egui::TextEdit::singleline(&mut d.proto_port).desired_width(300.0));
                ui.end_row();
                ui.label("UART3 по умолчанию");
                ui.add(egui::TextEdit::singleline(&mut d.log_port).desired_width(300.0));
                ui.end_row();
                ui.label("Период STATUS, мс");
                ui.add(egui::DragValue::new(&mut d.poll_ms).range(200..=10_000));
                ui.end_row();
            });
            ui.label(RichText::new("Найденные порты:").color(p.fg2));
            for port in &sh.ports {
                ui.label(RichText::new(format!("{}   {}", port.name, port.desc)).monospace().small());
            }
            if ui.small_button("⟳ Обновить").clicked() {
                sh.act(Act::RefreshPorts);
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if primary(ui, &p, "Сохранить").clicked() {
                d.ocd.args = split_args(&st.ocd_args);
                sh.act(Act::SaveCfg(Box::new(d.clone())));
                sh.act(Act::Toast(crate::worker::Tone::Ok, "Настройки сохранены".into()));
            }
            if ui.button("Отменить изменения").clicked() {
                reset = true;
            }
        });
    });
    if reset {
        st.ocd_args.clear();
    } else {
        st.draft = Some(draft);
    }
}

fn join_args(a: &[String]) -> String {
    a.iter().map(|x| if x.contains(' ') { format!("\"{}\"", x) } else { x.clone() }).collect::<Vec<_>>().join(" ")
}

/// Split a command line, keeping "quoted phrases" together.
fn split_args(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn args() {
        let a = super::split_args("-f a.cfg -c \"transport select hla_swd\"");
        assert_eq!(super::split_args(&super::join_args(&a)), a);
        assert_eq!(super::split_args("-f a.cfg -c \"transport select hla_swd\""), ["-f", "a.cfg", "-c", "transport select hla_swd"]);
    }
}
