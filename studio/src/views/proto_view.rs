//! UART1 protocol console: command catalogue, frame builder with live CRC,
//! fault injection, stress test, transaction log with byte-level decoding and
//! the board's reaction (UART3 lines right after the command).

use crate::app::{Act, Shared};
use crate::core::proto::{self, *};
use crate::fmt;
use crate::theme::Pal;
use crate::widgets::*;
use crate::worker::{Job, Txn};
use eframe::egui::{self, Color32, RichText};

pub struct St {
    hex: String,
    show_polls: bool,
    selected: Option<u64>,
    stress_n: u32,
    follow: bool,
}

impl Default for St {
    fn default() -> Self {
        St { hex: "70 01".into(), show_polls: false, selected: None, stress_n: 50, follow: true }
    }
}

struct CmdDef {
    title: &'static str,
    body: &'static [u8],
    what: &'static str,
}

const CMDS: &[CmdDef] = &[
    CmdDef { title: "PING", body: &[CMD_PING], what: "Плата присылает версию прошивки. Проверка связи." },
    CmdDef { title: "STATUS", body: &[CMD_STATUS], what: "56 байт: аптайм, куча, стеки задач, счётчики. Разбор — на экране FreeRTOS." },
    CmdDef { title: "LED вкл", body: &[CMD_LED, 1], what: "PC13 = 0 → светодиод горит; LedTask перестаёт мигать." },
    CmdDef { title: "LED выкл", body: &[CMD_LED, 0], what: "PC13 = 1 → светодиод погашен (ручной режим)." },
    CmdDef { title: "LED авто", body: &[CMD_LED, 2], what: "Возврат мигания раз в секунду." },
    CmdDef { title: "LED_GET", body: &[CMD_LED_GET], what: "Ответ: уровень ODR PC13 и режим." },
    CmdDef { title: "FRAM KUREIN", body: &[CMD_FRAM, 0], what: "Запись \"KUREIN\" по адресу 0x0150 и чтение обратно. В логе — TEST fram." },
    CmdDef { title: "FRAM probe", body: &[CMD_FRAM, 1], what: "Есть ли микросхема: бит WEL ставится и снимается." },
    CmdDef { title: "FRAM 64 Б", body: &[CMD_FRAM, 2], what: "Тест 64 байт по 0x7F00 с восстановлением содержимого." },
    CmdDef { title: "RESET", body: &[CMD_RESET], what: "Ответ, затем программный сброс: в логе новый BOOT, boot_count +1." },
];

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    h1(
        ui,
        &p,
        "UART-протокол (USART1, 38400 8N1)",
        "Кадр: CMD · аргументы · CRC16/MODBUS (LSB первым). Ответ: 01 · статус · данные · CRC. Конец кадра — пауза 20 мс.",
    );
    if !sh.uart_ok() {
        ui.horizontal(|ui| {
            ui.label(RichText::new("UART1 не подключён.").color(p.warn));
            if ui.button("Подключить на Обзоре").clicked() {
                sh.act(Act::Go(crate::app::View::Overview));
            }
            if ui.button("Виртуальная плата").clicked() {
                sh.job(Job::ProtoSim);
                sh.job(Job::LogSim);
            }
        });
    }
    egui::SidePanel::left("proto_left").resizable(true).default_width(380.0).frame(egui::Frame::new()).show_inside(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("proto_l").show(ui, |ui| {
            card(ui, &p, "Команды", |ui| {
                egui::Grid::new("cmds").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    for c in CMDS {
                        if ui.add_sized([110.0, 24.0], egui::Button::new(c.title)).on_hover_text(proto::hex(&frame(c.body))).clicked() {
                            sh.send(c.body.to_vec());
                        }
                        ui.add(egui::Label::new(RichText::new(c.what).small().color(p.fg2)).wrap());
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            card(ui, &p, "Конструктор кадра", |ui| {
                hint(ui, &p, "Введите CMD и аргументы в hex, CRC посчитается сама.");
                ui.add(egui::TextEdit::singleline(&mut st.hex).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                match parse_hex(&st.hex) {
                    Ok(b) if !b.is_empty() => {
                        let f = frame(&b);
                        ui.label(RichText::new(format!("на провод: {}", proto::hex(&f))).monospace().color(p.accent));
                        ui.label(RichText::new(describe_request(&b)).small().color(p.fg2));
                        ui.horizontal(|ui| {
                            if ui.button("Отправить с CRC").clicked() {
                                sh.send(b.clone());
                            }
                            if ui.button("Отправить как есть").on_hover_text("без добавления CRC — для своих экспериментов").clicked() {
                                sh.job(Job::Send { body: b, raw: true });
                            }
                        });
                    }
                    Ok(_) => hint(ui, &p, "пусто"),
                    Err(e) => {
                        ui.label(RichText::new(e).color(p.err));
                    }
                }
            });
            ui.add_space(8.0);
            card(ui, &p, "Проверка обработки ошибок", |ui| {
                hint(ui, &p, "Как прошивка реагирует на испорченные кадры — должна отвечать кодом ошибки и не зависать.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Испорченная CRC").clicked() {
                        let mut f = frame(&[CMD_PING]);
                        f[2] ^= 0x55;
                        sh.job(Job::Send { body: f, raw: true });
                    }
                    if ui.button("Кадр 150 байт").clicked() {
                        sh.job(Job::Send { body: (0..150u8).collect(), raw: true });
                    }
                    if ui.button("Неизвестная команда").clicked() {
                        sh.send(vec![0x55]);
                    }
                    if ui.button("Неверный аргумент").clicked() {
                        sh.send(vec![CMD_LED, 9]);
                    }
                    if ui.button("Один байт").clicked() {
                        sh.job(Job::Send { body: vec![0x70], raw: true });
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Стресс:");
                    ui.add(egui::DragValue::new(&mut st.stress_n).range(1..=1000).suffix(" PING"));
                    if ui.button("Запустить").clicked() {
                        sh.job(Job::Stress(st.stress_n));
                    }
                });
            });
        });
    });
    egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, ..Default::default() })).show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Журнал обменов").strong().size(15.0));
            ui.checkbox(&mut st.show_polls, "показывать автоопрос и стресс");
            ui.checkbox(&mut st.follow, "следовать за последним");
            if ui.button("Очистить").clicked() {
                sh.act(Act::ClearTxns);
                st.selected = None;
            }
        });
        let list: Vec<&Txn> = sh.txns.iter().filter(|t| st.show_polls || !t.poll).collect();
        if st.follow {
            st.selected = list.last().map(|t| t.id);
        }
        let h = (ui.available_height() * 0.5).max(160.0);
        egui::ScrollArea::vertical().id_salt("txns").max_height(h).stick_to_bottom(st.follow).show(ui, |ui| {
            egui::Grid::new("txn_grid").num_columns(5).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
                for hdr in ["время", "команда", "TX → RX", "результат", "мс"] {
                    ui.label(RichText::new(hdr).strong().color(p.fg2));
                }
                ui.end_row();
                let start = list.len().saturating_sub(400);
                for t in &list[start..] {
                    let sel = st.selected == Some(t.id);
                    if ui.selectable_label(sel, RichText::new(fmt::clock(t.t)).monospace().small()).clicked() {
                        st.selected = Some(t.id);
                        st.follow = false;
                    }
                    ui.label(&t.label);
                    ui.label(RichText::new(format!("{} → {}", short(&t.tx), if t.rx.is_empty() { "—".into() } else { short(&t.rx) })).monospace().small().color(p.fg2));
                    ui.label(RichText::new(&t.verdict).color(if t.ok { p.ok } else { p.err }));
                    ui.label(RichText::new(format!("{:.0}", t.ms)).monospace().small());
                    ui.end_row();
                }
            });
        });
        ui.separator();
        if let Some(t) = st.selected.and_then(|id| sh.txns.iter().find(|t| t.id == id)) {
            egui::ScrollArea::vertical().id_salt("txn_det").show(ui, |ui| detail(ui, sh, t));
        } else {
            hint(ui, &p, "Выберите обмен — здесь будет разбор байтов и реакция платы.");
        }
    });
}

fn short(b: &[u8]) -> String {
    if b.len() <= 12 {
        proto::hex(b)
    } else {
        format!("{} … ({} B)", proto::hex(&b[..8]), b.len())
    }
}

fn bytes_row(ui: &mut egui::Ui, p: &Pal, parts: &[(&str, &[u8], Color32)]) {
    ui.horizontal_wrapped(|ui| {
        for (name, b, c) in parts {
            if b.is_empty() {
                continue;
            }
            egui::Frame::new().fill(crate::theme::mix(*c, 14.0, p.panel)).corner_radius(4).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(*name).small().color(*c));
                    let shown = if b.len() > 24 { format!("{} …", proto::hex(&b[..24])) } else { proto::hex(b) };
                    ui.label(RichText::new(shown).monospace());
                });
            });
        }
    });
}

fn detail(ui: &mut egui::Ui, sh: &Shared, t: &Txn) {
    let p = sh.pal;
    ui.label(RichText::new(format!("{} · {}", t.label, t.verdict)).strong().color(if t.ok { p.ok } else { p.err }));
    ui.label(RichText::new("Запрос").color(p.fg2));
    let n = t.tx.len();
    if n >= 3 {
        bytes_row(ui, &p, &[("CMD", &t.tx[..1], p.accent), ("аргументы", &t.tx[1..n - 2], p.sig), ("CRC lo hi", &t.tx[n - 2..], p.muted)]);
        let ok = crc16(&t.tx) == 0;
        hint(ui, &p, if ok { "CRC запроса верна" } else { "CRC запроса неверна (так и задумано при проверке ошибок)" });
    } else {
        bytes_row(ui, &p, &[("байты", &t.tx, p.sig)]);
    }
    ui.label(RichText::new("Ответ").color(p.fg2));
    let m = t.rx.len();
    if m >= 4 {
        bytes_row(ui, &p, &[("ID", &t.rx[..1], p.clk), ("статус", &t.rx[1..2], if t.rx[1] == 0 { p.ok } else { p.err }), ("данные", &t.rx[2..m - 2], p.sig), ("CRC", &t.rx[m - 2..], p.muted)]);
        let payload = &t.rx[2..m - 2];
        let cmd = t.tx.first().copied();
        if cmd == Some(CMD_PING) && !payload.is_empty() {
            ui.label(format!("Версия: {}", String::from_utf8_lossy(payload)));
        }
        if cmd == Some(CMD_FRAM) && !payload.is_empty() {
            ui.label(format!("Данные FRAM: {}  «{}»", proto::hex(payload), String::from_utf8_lossy(payload).replace('\0', "·")));
        }
        if cmd == Some(CMD_LED_GET) && payload.len() == 2 {
            ui.label(format!("PC13 ODR = {} ({}), режим: {}", payload[0], if payload[0] == 0 { "горит" } else { "погашен" }, if payload[1] == 1 { "ручной" } else { "мигание" }));
        }
        if let (Some(CMD_STATUS), Some(s)) = (cmd, Status::decode(payload)) {
            egui::Grid::new("st_dec").num_columns(4).spacing([12.0, 2.0]).show(ui, |ui| {
                let rows = [
                    ("uptime_ms", s.uptime_ms.to_string()),
                    ("reset_cause", format!("0x{:08X}", s.reset_cause)),
                    ("boot_count", s.boot_count.to_string()),
                    ("heartbeat", s.heartbeat.to_string()),
                    ("heap_free", s.heap_free.to_string()),
                    ("heap_min", s.heap_min.to_string()),
                    ("stack D/L/U", format!("{}/{}/{}", s.stack_free_default, s.stack_free_led, s.stack_free_uart)),
                    ("rx / crc_err", format!("{}/{}", s.rx_frames, s.rx_crc_err)),
                    ("uart_err / queue_full", format!("{}/{}", s.uart_err, s.queue_full)),
                    ("fram ok/fail", format!("{}/{}", s.fram_ok, s.fram_fail)),
                ];
                for (i, (k, v)) in rows.iter().enumerate() {
                    ui.label(RichText::new(*k).monospace().color(p.muted));
                    ui.label(RichText::new(v).monospace());
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        }
    } else if m == 0 {
        ui.label(RichText::new("ответа нет").color(p.err));
    } else {
        bytes_row(ui, &p, &[("байты", &t.rx, p.err)]);
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Реакция платы в логе UART3 (1.5 с после команды)").color(p.fg2));
    let lines: Vec<_> = sh.lines.iter().filter(|l| l.t >= t.t - 0.05 && l.t <= t.t + 1.5).collect();
    if sh.st.log.is_none() {
        hint(ui, &p, "Лог UART3 не подключён.");
    } else if lines.is_empty() {
        hint(ui, &p, "Новых строк нет (для LED/PING это нормально; FRAM и RESET пишут в лог).");
    }
    for l in lines {
        ui.label(RichText::new(format!("{}  {}", fmt::clock(l.t), l.text)).monospace().small());
    }
}
