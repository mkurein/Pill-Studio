//! SWD debugging: connection, run control, core registers, fault analysis
//! from g_diag with source lines, variables by symbol, build / flash / hil.py
//! autotest, OpenOCD console.

use crate::app::Shared;
use crate::core::elf;
use crate::core::openocd::cfsr_ru;
use crate::core::proto;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct St {
    sym: String,
    console: String,
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(ui, &p, "SWD-отладка", "ST-Link + OpenOCD: чтение памяти и регистров без остановки программы, разбор сбоев, сборка, прошивка и автотест.");
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Подключение и ход программы", |ui| {
                ui.horizontal_wrapped(|ui| {
                    if sh.swd_ok() {
                        if ui.button("Отключить").clicked() {
                            sh.job(Job::SwdClose);
                        }
                    } else {
                        if primary(ui, &p, "Подключить ST-Link").clicked() {
                            sh.job(Job::SwdOpen(sh.cfg.ocd.clone()));
                        }
                        if ui.button("Симулятор").clicked() {
                            sh.job(Job::SwdSim);
                        }
                    }
                    let on = sh.swd_ok();
                    for (t, c, h) in [
                        ("⏸ Стоп", "halt", "остановить ядро (все задачи замрут)"),
                        ("▶ Пуск", "resume", "продолжить выполнение"),
                        ("⤼ Шаг", "step", "одна инструкция"),
                        ("↻ Сброс", "reset run", "аппаратный сброс и запуск"),
                    ] {
                        if ui.add_enabled(on, egui::Button::new(t)).on_hover_text(h).clicked() {
                            sh.job(Job::SwdCtl(c));
                        }
                    }
                    if ui.add_enabled(on, egui::Button::new("Регистры ядра")).clicked() {
                        sh.job(Job::SwdRegs);
                    }
                });
                if let Some(s) = &sh.st.swd {
                    ui.label(format!(
                        "{} · состояние: {} · {} · IDCODE {}",
                        s,
                        sh.st.swd_state,
                        sh.st.swd_voltage.clone().unwrap_or_default(),
                        sh.st.idcode.map(|v| format!("0x{:08X} (DEV_ID 0x{:03X} = medium density)", v, v & 0xFFF)).unwrap_or_default()
                    ));
                } else {
                    hint(ui, &p, "Не подключено. Если ST-Link не видит MCU — проверьте провода SWD, BOOT0 = 0, питание; включите «подключение под сбросом» в Настройках (нужен провод NRST).");
                }
                if !sh.st.regs.is_empty() {
                    egui::Grid::new("core_regs").num_columns(4).spacing([14.0, 2.0]).show(ui, |ui| {
                        for (i, (n, v)) in sh.st.regs.iter().enumerate() {
                            ui.label(RichText::new(n).monospace().color(p.muted));
                            ui.label(RichText::new(format!("0x{:08X}", v)).monospace());
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                    if let (Some(pc), Some(e)) = (sh.st.regs.iter().find(|r| r.0 == "pc"), &sh.st.elf) {
                        ui.label(RichText::new(format!("PC → {}", elf::where_is(e, &sh.cfg.toolchain_dir, pc.1))).color(p.accent));
                    }
                }
            });
            card(&mut cols[1], &p, "Анализ сбоя (g_diag)", |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(sh.swd_ok(), egui::Button::new("Прочитать g_diag")).clicked() {
                        sh.job(Job::ReadDiag);
                    }
                    if ui.add_enabled(sh.swd_ok(), egui::Button::new("CFSR/HFSR сейчас")).clicked() {
                        sh.job(Job::ReadWords(vec![0xE000_ED28, 0xE000_ED2C, 0xE000_ED34, 0xE000_ED38]));
                    }
                });
                match &sh.st.diag {
                    None => hint(ui, &p, "Прошивка записывает причину любого сбоя (HardFault, переполнение стека, Error_Handler) в g_diag — область RAM, которая не очищается при сбросе."),
                    Some(d) if !d.valid() => {
                        ui.label(RichText::new("g_diag не инициализирован (другая прошивка или только что подано питание?)").color(p.warn));
                    }
                    Some(d) => {
                        egui::Grid::new("diag").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                            kv(ui, &p, "Загрузок", d.boot_count.to_string());
                            kv(ui, &p, "Причина сброса", proto::reset_cause_ru(d.reset_cause));
                            kv(ui, &p, "Heartbeat", d.heartbeat.to_string());
                            kv(ui, &p, "Кадров / CRC-ошибок / переполнений", format!("{} / {} / {}", d.rx_frames, d.rx_crc_err, d.rx_overflow));
                            kv(ui, &p, "Ошибок UART / потерь очереди", format!("{} / {}", d.uart_err, d.queue_full));
                            kv(ui, &p, "FRAM успешно / неуспешно", format!("{} / {}", d.fram_ok, d.fram_fail));
                        });
                        if d.has_fault() {
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Сбой: {}", d.error_name())).strong().color(p.err));
                            let el = sh.st.elf.as_ref();
                            let wh = |a: u32| el.map(|e| elf::where_is(e, &sh.cfg.toolchain_dir, a)).unwrap_or_default();
                            egui::Grid::new("fault").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                                kv(ui, &p, "PC (где упало)", format!("0x{:08X}  {}", d.fault_pc, wh(d.fault_pc)));
                                kv(ui, &p, "LR (откуда вызвано)", format!("0x{:08X}  {}", d.fault_lr, wh(d.fault_lr)));
                                kv(ui, &p, "CFSR / HFSR", format!("0x{:08X} / 0x{:08X}", d.fault_cfsr, d.fault_hfsr));
                                if d.fault_cfsr & (1 << 7) != 0 {
                                    kv(ui, &p, "MMFAR (адрес)", format!("0x{:08X}", d.fault_mmfar));
                                }
                                if d.fault_cfsr & (1 << 15) != 0 {
                                    kv(ui, &p, "BFAR (адрес)", format!("0x{:08X}", d.fault_bfar));
                                }
                                if !d.fault_task.is_empty() {
                                    kv(ui, &p, "Задача", d.fault_task.clone());
                                }
                            });
                            for s in cfsr_ru(d.fault_cfsr) {
                                ui.label(RichText::new(format!("• {}", s)).color(p.fg2));
                            }
                        } else {
                            ui.label(RichText::new("Сбоев не записано ✓").color(p.ok));
                        }
                    }
                }
                if let Some(cfsr) = sh.mem(0xE000_ED28) {
                    hint(ui, &p, &format!("CFSR сейчас: 0x{:08X} {}", cfsr, cfsr_ru(cfsr).join("; ")));
                }
            });
        });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Переменная по имени (из ELF)", |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.sym).hint_text("g_diag, uartQueue, uwTick…").desired_width(200.0));
                    let sym = sh.st.elf.as_ref().and_then(|e| e.sym(st.sym.trim())).cloned();
                    if ui.add_enabled(sh.swd_ok() && sym.is_some(), egui::Button::new("Прочитать")).clicked() {
                        if let Some(s) = &sym {
                            let n = s.size.div_ceil(4).clamp(1, 64);
                            sh.job(Job::ReadWords((0..n).map(|i| s.addr + 4 * i).collect()));
                        }
                    }
                });
                let Some(e) = &sh.st.elf else {
                    hint(ui, &p, "ELF не загружен.");
                    return;
                };
                let q = st.sym.trim();
                if let Some(s) = e.sym(q) {
                    ui.label(RichText::new(format!("{} @ 0x{:08X}, {} Б{}", s.name, s.addr, s.size, if s.func { ", функция" } else { "" })).monospace());
                    let n = s.size.div_ceil(4).clamp(1, 64);
                    let words: Vec<String> = (0..n).filter_map(|i| sh.mem(s.addr + 4 * i)).map(|w| format!("{:08X}", w)).collect();
                    if !words.is_empty() {
                        for chunk in words.chunks(8) {
                            ui.label(RichText::new(chunk.join(" ")).monospace().color(p.accent));
                        }
                    }
                } else if !q.is_empty() {
                    let hits: Vec<&str> = e.syms.iter().filter(|s| s.name.contains(q)).take(12).map(|s| s.name.as_str()).collect();
                    ui.horizontal_wrapped(|ui| {
                        for h in hits {
                            if ui.small_button(h).clicked() {
                                st.sym = h.to_string();
                            }
                        }
                    });
                }
            });
            card(&mut cols[1], &p, "Сборка · прошивка · автотест", |ui| {
                ui.horizontal_wrapped(|ui| {
                    let idle = sh.st.job.is_none();
                    if ui.add_enabled(idle, egui::Button::new("🔨 Собрать")).on_hover_text(&sh.cfg.build_cmd).clicked() {
                        sh.job(Job::Build(sh.cfg.clone()));
                    }
                    if ui.add_enabled(idle, egui::Button::new("⚡ Прошить")).on_hover_text("OpenOCD: program … verify reset").clicked() {
                        sh.job(Job::Flash(sh.cfg.clone()));
                    }
                    if ui.add_enabled(idle, egui::Button::new("✔ Автотест hil.py")).on_hover_text("tools/hil.py test: сборка, прошивка и ~30 проверок на плате; порты освобождаются").clicked() {
                        sh.job(Job::HilTest(sh.cfg.clone()));
                    }
                });
                if let Some(e) = &sh.st.elf {
                    hint(ui, &p, &format!("ELF: {}", e.path.display()));
                }
                match (sh.st.job.as_deref(), sh.st.job_ok) {
                    (Some(j), _) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(j);
                        });
                    }
                    (None, Some(true)) => {
                        ui.label(RichText::new("✓ успешно").color(p.ok));
                    }
                    (None, Some(false)) => {
                        ui.label(RichText::new("✗ ошибка").color(p.err));
                    }
                    _ => {}
                }
                egui::Frame::new().fill(p.panel2).corner_radius(6).inner_margin(6).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("jobout").max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                        for l in &sh.st.job_out {
                            let c = if l.contains("FAIL") || l.contains("error") || l.contains("Error") {
                                p.err
                            } else if l.contains("PASS") || l.contains("[  ok ]") {
                                p.ok
                            } else {
                                p.fg2
                            };
                            ui.label(RichText::new(l).monospace().small().color(c));
                        }
                    });
                });
            });
        });
        ui.add_space(8.0);
        card(ui, &p, "Консоль OpenOCD (команды TCL)", |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut st.console).hint_text("mdw 0x40011000 4 · reg pc · flash info 0 · stm32f1x options_read 0").desired_width(480.0).font(egui::TextStyle::Monospace));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Выполнить").clicked() || enter) && !st.console.trim().is_empty() {
                    sh.job(Job::OcdExec(st.console.trim().to_string()));
                    r.request_focus();
                }
            });
            egui::ScrollArea::vertical().id_salt("ocd").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                for l in &sh.st.ocd_console {
                    ui.label(RichText::new(l).monospace().small().color(p.fg2));
                }
            });
        });
    });
}
