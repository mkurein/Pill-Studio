//! Active firmware project, local build, and the physical board.

use crate::app::{Act, Shared, View};
use crate::core::boards::BoardId;
use crate::core::config::{self, FirmwareProfile};
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct St {
    project_path: String,
    elf_path: String,
    hist_all: bool,
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(ui, &p, "Обзор", "");
        card(ui, &p, "Проект прошивки", |ui| {
            ui.set_min_width(ui.available_width());
            project(ui, sh, st);
        });
        ui.add_space(10.0);
        if ui.available_width() >= 680.0 {
            ui.columns(2, |cols| {
                build(&mut cols[0], sh, st);
                board(&mut cols[1], sh);
            });
        } else {
            build(ui, sh, st);
            ui.add_space(10.0);
            board(ui, sh);
        }
        if !sh.st.job_out.is_empty() {
            ui.add_space(10.0);
            card(ui, &p, "Вывод операции", |ui| {
                egui::ScrollArea::vertical().id_salt("jobout").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                    for line in &sh.st.job_out {
                        ui.label(RichText::new(line).monospace().small().color(p.fg2));
                    }
                });
            });
        }
        if !sh.cfg.history.is_empty() {
            ui.add_space(8.0);
            ui.collapsing("История операций", |ui| history(ui, sh, st));
        }
    });
}

fn project(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let cfg = &sh.cfg;
    let idle = !sh.busy;
    ui.horizontal_wrapped(|ui| {
        let title = if cfg.project_dir.is_empty() { "Проект не выбран".into() } else { short_name(&cfg.project_dir) };
        ui.label(RichText::new(title).size(20.0).strong().color(p.fg));
        ui.label(RichText::new(cfg.board.profile().chip).color(p.muted));
    });
    if !cfg.project_dir.is_empty() {
        let dir = config::display_path(&cfg.project_dir);
        ui.label(RichText::new(dir.as_ref()).monospace().small().color(p.fg2)).on_hover_text(dir.as_ref());
        if !sh.st.git.is_empty() {
            hint(ui, &p, &format!("Git · {}", sh.st.git));
        }
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(idle, |ui| {
            egui::ComboBox::from_id_salt("project_pick").selected_text("Недавние и соседние").show_ui(ui, |ui| {
                for r in &cfg.recent {
                    let label = format!("{} · {}", short_name(&r.dir), board_name(r.board));
                    if ui.selectable_label(r.dir == cfg.project_dir, label).on_hover_text(&r.dir).clicked() && r.dir != cfg.project_dir {
                        sh.act(Act::Project(r.dir.clone()));
                    }
                }
                for (dir, id) in config::find_projects(&cfg.project_dir) {
                    if !cfg.recent.iter().any(|r| r.dir == dir)
                        && ui.selectable_label(false, format!("{} · {}", short_name(&dir), board_name(id))).on_hover_text(&dir).clicked()
                    {
                        sh.act(Act::Project(dir));
                    }
                }
            });
            let width = (ui.available_width() - 78.0).clamp(180.0, 340.0);
            let input = ui.add(egui::TextEdit::singleline(&mut st.project_path).hint_text("Путь к папке проекта").desired_width(width));
            let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Открыть").clicked() || enter) && !st.project_path.trim().is_empty() {
                sh.act(Act::Project(st.project_path.trim().trim_matches('"').to_string()));
            }
        });
    });
    ui.add_space(4.0);
    if !cfg.project_dir.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Профиль прошивки").color(p.fg2));
            ui.add_enabled_ui(idle, |ui| {
                egui::ComboBox::from_id_salt("firmware_profile").selected_text(sh.profile.label()).show_ui(ui, |ui| {
                    for profile in [FirmwareProfile::Swd, FirmwareProfile::PrimGpt] {
                        if ui.selectable_label(sh.profile == profile, profile.label()).clicked() && sh.profile != profile {
                            sh.act(Act::FirmwareProfile(profile));
                        }
                    }
                });
            });
            if sh.primgpt() && ui.button("Инструменты primGPT").clicked() {
                sh.act(Act::Go(View::PrimGpt));
            }
        });
    }
}

fn build(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let selected = !sh.cfg.project_dir.is_empty();
    let idle = !sh.busy;
    let ready = selected && idle && !sh.cfg.build_cmd.trim().is_empty();
    card(ui, &p, "Локальная сборка", |ui| {
        if let Some(e) = &sh.st.elf {
            let path = e.path.to_string_lossy();
            ui.label(RichText::new(short_name(&path)).monospace().strong()).on_hover_text(path.as_ref());
            if let Some(t) = sh.st.elf_mtime {
                hint(ui, &p, &format!("Собрано {}", ago(t)));
            }
            if super::fresh::sources_newer(sh) {
                ui.label(RichText::new("Исходники новее ELF. Соберите проект.").color(p.warn));
            }
        } else if let Some(error) = &sh.st.elf_error {
            ui.label(RichText::new(error).color(p.warn));
        } else if selected {
            hint(ui, &p, "ELF ещё не найден. Соберите проект или укажите файл.");
        } else {
            hint(ui, &p, "Откройте папку проекта.");
        }
        if selected {
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled_ui(ready, |ui| primary(ui, &p, "Собрать")).inner.clicked() {
                    sh.job(Job::Build(sh.cfg.clone()));
                }
                if ui.add_enabled(ready, egui::Button::new("Собрать и прошить")).clicked() {
                    sh.job(Job::BuildFlash(sh.cfg.clone()));
                }
                if ui.small_button("Команда сборки").clicked() {
                    sh.act(Act::Go(View::Settings));
                }
            });
            hint(ui, &p, &sh.cfg.build_cmd);
            if let Some(job) = &sh.st.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(job);
                });
            } else if let Some(ok) = sh.st.job_ok {
                ui.label(RichText::new(if ok { "Операция завершена" } else { "Ошибка операции" }).color(if ok { p.ok } else { p.err }));
            }
            ui.add_space(4.0);
            ui.collapsing("Путь к ELF", |ui| {
                if st.elf_path.is_empty() {
                    if let Some(path) = sh.cfg.projects.get(&sh.cfg.project_dir) {
                        st.elf_path.clone_from(&path.elf);
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    let width = (ui.available_width() - 92.0).clamp(110.0, 420.0);
                    ui.add_enabled(idle, egui::TextEdit::singleline(&mut st.elf_path).hint_text("Например Release/firmware.elf").desired_width(width));
                    if ui.add_enabled(idle && !st.elf_path.trim().is_empty(), egui::Button::new("Загрузить")).clicked() {
                        sh.act(Act::ProjectElf(st.elf_path.trim().trim_matches('"').into()));
                    }
                    if ui.add_enabled(idle, egui::Button::new("Определять автоматически")).clicked() {
                        st.elf_path.clear();
                        sh.act(Act::ProjectElf(String::new()));
                    }
                });
            });
        }
    });
}

fn board(ui: &mut egui::Ui, sh: &Shared) {
    let p = sh.pal;
    let idle = !sh.busy;
    let real = super::fresh::real_swd(sh);
    card(ui, &p, "Плата по SWD", |ui| {
        if real {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("ST-Link подключён").color(p.ok));
                if let Some(voltage) = &sh.st.swd_voltage {
                    hint(ui, &p, voltage);
                }
                if let Some(id) = sh.st.idcode {
                    hint(ui, &p, &format!("IDCODE 0x{id:08X}"));
                }
            });
            super::fresh::flash_status(ui, sh);
        } else {
            hint(ui, &p, "Подключите ST-Link для сверки с Flash.");
        }
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            if real {
                if ui.add_enabled(idle && sh.st.elf.is_some(), egui::Button::new("Сверить с платой")).clicked() {
                    sh.job(Job::VerifyFlash);
                }
                if ui.add_enabled(idle, egui::Button::new("Отключить")).clicked() {
                    sh.job(Job::SwdClose);
                }
            } else if ui.add_enabled(idle, egui::Button::new("Подключить ST-Link")).clicked() {
                sh.job(Job::SwdOpen(sh.cfg.ocd.clone()));
            }
            if ui.add_enabled(idle && sh.st.elf.is_some(), egui::Button::new("Прошить")).clicked() {
                sh.job(Job::Flash(sh.cfg.clone()));
            }
            if ui.button("SWD-отладка").clicked() {
                sh.act(Act::Go(View::Debug));
            }
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
pub(super) fn ago(t: u64) -> String {
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
