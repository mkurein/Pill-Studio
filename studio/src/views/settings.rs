//! Settings: project folder, toolchain, OpenOCD command line, ports, polling.

use crate::app::{Act, Shared};
use crate::core::boards::{self, BoardId};
use crate::core::config::Config;
use crate::core::openocd::OcdConfig;
use crate::widgets::*;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct St {
    draft: Option<Config>,
    ocd_args: String,
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
        h1(ui, &p, "Настройки", "Хранятся в папке настроек пользователя (pill-studio/config.json).");
        card(ui, &p, "Плата", |ui| {
            ui.horizontal(|ui| {
                for id in BoardId::ALL {
                    let q = id.profile();
                    if ui.selectable_label(sh.cfg.board == id, format!("{} · {}", q.board, q.chip)).clicked() && sh.cfg.board != id {
                        sh.act(Act::Board(id));
                    }
                }
            });
            let b = boards::cur();
            hint(
                ui,
                &p,
                &format!(
                    "У каждой платы свои проект и настройки. Сейчас: OpenOCD {}, DEV_ID 0x{:03X}. \
                     Если по SWD ответит чип другой платы, вверху появится кнопка переключения.",
                    b.ocd_target, b.dev_id
                ),
            );
        });
        card(ui, &p, "Проект прошивки", |ui| {
            egui::Grid::new("set_proj").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Папка проекта");
                ui.label(RichText::new(&d.project_dir).monospace());
                ui.end_row();
                ui.label("Команда сборки");
                ui.add(egui::TextEdit::singleline(&mut d.build_cmd).desired_width(520.0));
                ui.end_row();
                ui.label("Папка arm-none-eabi");
                ui.add(egui::TextEdit::singleline(&mut d.toolchain_dir).hint_text("пусто = PATH; Windows: …\\STM32CubeIDE\\plugins\\…gnu-tools…\\tools\\bin").desired_width(520.0));
                ui.end_row();
            });
            hint(ui, &p, "Команда выполняется в папке проекта. Выбор папки и ELF находится в «Обзоре».");
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
        card(ui, &p, "Отладка вместе с Cursor", |ui| {
            egui::Grid::new("set_cursor").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("GDB-порт OpenOCD");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut d.ocd.gdb_port).range(0..=65535));
                    ui.label(RichText::new("0 — закрыт").color(p.muted));
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut d.editor_follow, "при останове открывать строку исходника в редакторе");
                ui.end_row();
                ui.label("Редактор");
                ui.add(egui::TextEdit::singleline(&mut d.editor_cmd).hint_text("пусто = Cursor; можно code или полный путь").desired_width(520.0));
                ui.end_row();
            });
            let found = if d.editor_cmd.trim().is_empty() { crate::core::config::find_cursor() } else { Some(d.editor_cmd.trim().to_string()) };
            match found {
                Some(f) => hint(ui, &p, &format!("Команда: {f} -g файл:строка")),
                None => hint(ui, &p, "Cursor не найден: укажите путь к его команде cursor (в Cursor: Shell Command → Install 'cursor' command)."),
            }
            hint(
                ui,
                &p,
                "Один OpenOCD на двоих: студия говорит с ним по TCL (6667), Cursor — по GDB (этот порт). \
                 Сначала подключите SWD в студии, потом в Cursor — конфигурация «Attach to Pill Studio (OpenOCD :3333)». \
                 Если в Cursor ядро запущено (Continue), «Пауза» в студии сразу останавливает и Cursor. \
                 Шаги и остановы в Cursor студия замечает за 0.3 с. Шаги и «Пуск» из студии Cursor не знает — \
                 он только переходит на строку; нажмите в Cursor паузу или шаг, чтобы обновить его стек. \
                 Новый порт действует после переподключения SWD.",
            );
        });
        ui.add_space(8.0);
        if sh.primgpt() {
            card(ui, &p, "Порты и опрос primGPT", |ui| {
                egui::Grid::new("set_ports").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("UART1 по умолчанию");
                    ui.add(egui::TextEdit::singleline(&mut d.proto_port).desired_width(300.0));
                    ui.end_row();
                    ui.label(format!("{} по умолчанию", boards::cur().log_uart));
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
        }
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
