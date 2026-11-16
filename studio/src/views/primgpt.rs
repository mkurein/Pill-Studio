//! UART and simulator tools for the primGPT firmware.

use crate::app::{Act, Shared, View};
use crate::core::proto::{self, *};
use crate::fmt;
use crate::widgets::*;
use crate::worker::Job;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct St {
    proto_port: String,
    log_port: String,
}

fn port_combo(ui: &mut egui::Ui, sh: &Shared, id: &str, sel: &mut String) {
    egui::ComboBox::from_id_salt(id)
        .width((ui.available_width() - 36.0).clamp(80.0, 300.0))
        .selected_text(if sel.is_empty() { "— выберите порт —".to_string() } else { sel.clone() })
        .show_ui(ui, |ui| {
            for p in &sh.ports {
                let text = if p.desc.is_empty() { p.name.clone() } else { format!("{}   {}", p.name, p.desc) };
                ui.selectable_value(sel, p.name.clone(), text);
            }
            if sh.ports.is_empty() {
                ui.label("последовательных портов не найдено");
            }
        });
    if ui.button("⟳").on_hover_text("Обновить список портов").clicked() {
        sh.act(Act::RefreshPorts);
    }
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    if st.proto_port.is_empty() {
        st.proto_port = sh.cfg.proto_port.clone();
    }
    if st.log_port.is_empty() {
        st.log_port = sh.cfg.log_port.clone();
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        h1(
            ui,
            &p,
            "primGPT",
            "Команды, лог и диагностика прошивки. Виртуальная плата работает без ST-Link.",
        );
        ui.horizontal_wrapped(|ui| {
            if primary(ui, &p, "▶ Виртуальная плата на всех каналах").clicked() {
                sh.job(Job::ProtoSim);
                sh.job(Job::LogSim);
                sh.job(Job::SwdSim);
                sh.job(Job::Poll(true));
            }
            if sh.st.sim_running() && ui.button(RichText::new("Остановить симуляцию").color(p.err)).clicked() {
                sh.job(Job::SimStop);
            }
            if std::path::Path::new(&sh.cfg.project_dir).join("tools/hil.py").is_file()
                && ui.add_enabled(!sh.busy, egui::Button::new("Автотест hil.py")).clicked()
            {
                sh.job(Job::HilTest(sh.cfg.clone()));
                sh.act(Act::Go(View::Overview));
            }
        });
        ui.add_space(8.0);

        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "UART1 — протокол команд", |ui| {
                hint(ui, &p, "PA9 (TX) → RX адаптера, PA10 (RX) ← TX адаптера, GND общий. 38400 8N1.");
                ui.horizontal(|ui| port_combo(ui, sh, "proto_port", &mut st.proto_port));
                ui.horizontal(|ui| {
                    if sh.uart_ok() {
                        if ui.button("Отключить").clicked() {
                            sh.job(Job::ProtoClose);
                        }
                        if ui.button("PING").clicked() {
                            sh.send(vec![CMD_PING]);
                        }
                    } else if ui.add_enabled(!st.proto_port.is_empty(), egui::Button::new("Подключить")).clicked() {
                        let mut c = sh.cfg.clone();
                        c.proto_port = st.proto_port.clone();
                        sh.act(Act::SaveCfg(Box::new(c)));
                        sh.job(Job::ProtoOpen(st.proto_port.clone()));
                    }
                });
                match (&sh.st.proto, &sh.st.fw) {
                    (Some(port), Some(fw)) => ui.label(RichText::new(format!("✓ {} · прошивка {}", port, fw)).color(p.ok)),
                    (Some(port), None) => ui.label(RichText::new(format!("{} открыт, плата пока не ответила", port)).color(p.warn)),
                    _ => ui.label(RichText::new("не подключено").color(p.muted)),
                };
            });
            let b = crate::core::boards::cur();
            card(&mut cols[1], &p, &format!("{} — текстовый лог", b.log_uart.replace("USART", "UART")), |ui| {
                hint(ui, &p, &format!("{} (TX) → RX второго адаптера, GND общий. 115200. Только передача.", b.log_tx));
                ui.horizontal(|ui| port_combo(ui, sh, "log_port", &mut st.log_port));
                ui.horizontal(|ui| {
                    if sh.st.log.is_some() {
                        if ui.button("Отключить").clicked() {
                            sh.job(Job::LogClose);
                        }
                        if ui.button("Открыть лог").clicked() {
                            sh.act(Act::Go(View::Log));
                        }
                    } else if ui.add_enabled(!st.log_port.is_empty(), egui::Button::new("Подключить")).clicked() {
                        let mut c = sh.cfg.clone();
                        c.log_port = st.log_port.clone();
                        sh.act(Act::SaveCfg(Box::new(c)));
                        sh.job(Job::LogOpen(st.log_port.clone()));
                    }
                });
                let last = sh.lines.back().map(|l| l.text.clone());
                match (&sh.st.log, last) {
                    (Some(_), Some(l)) => ui.label(RichText::new(format!("последняя строка: {}", l)).color(p.fg2).monospace()),
                    (Some(port), None) => ui.label(RichText::new(format!("{} открыт, строк пока нет", port)).color(p.warn)),
                    _ => ui.label(RichText::new("не подключено").color(p.muted)),
                };
            });
        });

        ui.add_space(10.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], &p, "Состояние платы (команда STATUS)", |ui| {
                ui.horizontal(|ui| {
                    let mut on = sh.st.polling;
                    if ui.checkbox(&mut on, "опрашивать автоматически").changed() {
                        sh.job(Job::Poll(on));
                    }
                    if ui.add_enabled(sh.uart_ok(), egui::Button::new("Запросить сейчас")).clicked() {
                        sh.send(vec![CMD_STATUS]);
                    }
                });
                match sh.st.status {
                    None => hint(
                        ui,
                        &p,
                        if sh.uart_ok() {
                            "UART1 открыт, но STATUS ещё не разобран. Нажмите «Запросить сейчас» — ошибка появится здесь и всплывающим сообщением."
                        } else {
                            "Нет данных: подключите UART1 и запросите STATUS."
                        },
                    ),
                    Some(s) => {
                        egui::Grid::new("ov_status").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                            kv(ui, &p, "Аптайм", fmt::uptime(s.uptime_ms));
                            kv(ui, &p, "Загрузок с подачи питания", s.boot_count.to_string());
                            kv(ui, &p, "Причина сброса", proto::reset_cause_ru(s.reset_cause));
                            kv(ui, &p, "Куча FreeRTOS свободно / минимум", format!("{} / {}", fmt::bytes(s.heap_free), fmt::bytes(s.heap_min)));
                            kv(ui, &p, "Кадров принято / ошибок CRC", format!("{} / {}", s.rx_frames, s.rx_crc_err));
                            kv(ui, &p, "Ошибок UART / потерь очереди", format!("{} / {}", s.uart_err, s.queue_full));
                            kv(
                                ui,
                                &p,
                                "Светодиод PC13",
                                format!(
                                    "{} ({})",
                                    if s.led_odr == 0 { "горит" } else { "погашен" },
                                    if s.led_manual == 1 { "ручной режим" } else { "мигает" }
                                ),
                            );
                        });
                        hint(ui, &p, &format!("обновлено {}", fmt::ago(sh.st.status_t, sh.st.status_t)));
                    }
                }
            });
            card(&mut cols[1], &p, "Быстрые действия", |ui| {
                ui.add_enabled_ui(sh.uart_ok(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                            let manual = "Ручной режим: LedTask перестаёт мигать, светодиод держит это состояние. Остальные задачи работают. Вернуть мигание — «LED мигает».";
                            if ui.button("💡 LED вкл").on_hover_text(manual).clicked() {
                                sh.send(vec![CMD_LED, 1]);
                            }
                            if ui.button("LED выкл").on_hover_text(manual).clicked() {
                                sh.send(vec![CMD_LED, 0]);
                            }
                            if ui.button("LED мигает").on_hover_text("Авто: LedTask снова переключает светодиод раз в секунду.").clicked() {
                                sh.send(vec![CMD_LED, 2]);
                            }
                            if ui.button("FRAM тест").on_hover_text("Запись 64 байт во внешнюю SPI-память, чтение и сравнение, потом восстановление. На W25Q64 стирание сектора занимает десятки мс.").clicked() {
                                sh.send(vec![CMD_FRAM, 2]);
                            }
                            if ui.button("Сброс платы").on_hover_text("Плата отвечает и перезапускается (NVIC_SystemReset): в логе BOOT, светодиод снова мигает.").clicked() {
                                sh.send(vec![CMD_RESET]);
                            }
                    });
                });
                if let Some(t) = sh.txns.iter().rev().find(|t| !t.poll) {
                    let color = if t.ok { p.ok } else { p.err };
                    ui.label(RichText::new(format!("{} — {}", t.label, t.verdict)).small().color(color));
                } else if !sh.uart_ok() {
                    hint(ui, &p, "Сначала подключите UART1. Кнопки шлют команды на плату и пишут результат этой строкой.");
                }
                if sh.st.status.as_ref().is_some_and(|s| s.led_manual != 0) {
                    ui.label(
                        RichText::new("Светодиод в ручном режиме: мигание выключено, программа и задачи работают дальше. «LED мигает» — вернуть мигание.")
                            .small()
                            .color(p.warn),
                    );
                } else {
                    hint(ui, &p, "«LED вкл» и «LED выкл» включают ручной режим: светодиод перестаёт мигать, но программа не останавливается.");
                }
                if sh.st.proto.as_deref() == Some("виртуальная плата") {
                    let mut fram = sh.st.sim_fram;
                    if ui.checkbox(&mut fram, format!("виртуальная память ({}) установлена", crate::core::boards::cur().spi_mem)).on_hover_text("снимите — и посмотрите, как прошивка сообщает об отказе микросхемы").changed() {
                        sh.job(Job::SimFram(fram));
                    }
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Плата →").clicked() {
                        sh.act(Act::Go(View::Board));
                    }
                    if ui.button("Протокол →").clicked() {
                        sh.act(Act::Go(View::Proto));
                    }
                    if ui.button("FreeRTOS →").clicked() {
                        sh.act(Act::Go(View::Rtos));
                    }
                });
            });
        });
    });
}
