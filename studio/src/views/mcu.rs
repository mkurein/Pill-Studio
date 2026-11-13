//! MCU structure: block diagram (buses), memory map with the project's real
//! usage from the ELF, clock tree, full pin table.

use crate::app::{Act, Shared};
use crate::canvas::Canvas;
use crate::core::boards::{self, Family};
use crate::core::mcu::{self, Clocks};
use crate::fmt;
use crate::theme::{mix, Pal};
use crate::widgets::*;
use eframe::egui::{self, Align2, Color32, RichText, Stroke};

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Tab {
    #[default]
    Blocks,
    Memory,
    Clocks,
    Pins,
}

pub struct St {
    tab: Tab,
    block: &'static str,
    pin_filter: String,
    baud: String,
    apb1: bool,
}

impl Default for St {
    fn default() -> Self {
        St { tab: Tab::Blocks, block: "CORE", pin_filter: String::new(), baud: "38400".into(), apb1: false }
    }
}

/// Blocks used by this firmware.
fn used(id: &str) -> bool {
    boards::cur().used_blocks.contains(&id)
}

fn svd_name(id: &str) -> Option<&'static str> {
    (boards::cur().block_svd)(id)
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = boards::cur();
    h1(ui, &p, &format!("Контроллер {}", b.chip), b.summary);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.tab, Tab::Blocks, "Структура и шины");
        ui.selectable_value(&mut st.tab, Tab::Memory, "Карта памяти");
        ui.selectable_value(&mut st.tab, Tab::Clocks, "Тактирование");
        ui.selectable_value(&mut st.tab, Tab::Pins, "Все выводы");
    });
    ui.add_space(6.0);
    egui::ScrollArea::vertical().show(ui, |ui| match st.tab {
        Tab::Blocks => blocks(ui, sh, st),
        Tab::Memory => memory(ui, sh),
        Tab::Clocks => clocks(ui, sh, st),
        Tab::Pins => pins(ui, sh, st),
    });
}

#[allow(clippy::too_many_arguments)]
fn block_box(cv: &Canvas, ui: &mut egui::Ui, p: &Pal, st: &mut St, id: &'static str, x: f32, y: f32, w: f32, h: f32) {
    let b = mcu::blocks().iter().find(|b| b.id == id).expect("block id");
    let u = used(id);
    let sel = st.block == id;
    let c = if u { p.accent } else { p.line2 };
    let fill = if sel { mix(p.accent, 28.0, p.panel) } else if u { mix(p.accent, 10.0, p.panel) } else { p.panel2 };
    cv.rect(x, y, w, h, 6.0, fill, Stroke::new(if sel { 2.5 } else { 1.2 }, c));
    let tc = if u { p.fg } else { p.fg2 };
    // long titles ("USART2, USART3", "TIM1 (advanced)") go on two lines
    let split = if b.title.chars().count() as f32 * 7.0 > w - 8.0 { b.title.find(", ").map(|i| (i, 2)).or_else(|| b.title.find(' ').map(|i| (i, 1))) } else { None };
    match split {
        Some((i, skip)) => {
            let (a, rest) = b.title.split_at(i);
            let a = if skip == 2 { format!("{},", a) } else { a.to_string() };
            cv.text(x + w / 2.0, y + h / 2.0 - 8.0, Align2::CENTER_CENTER, a, 12.0, tc);
            cv.text(x + w / 2.0, y + h / 2.0 + 8.0, Align2::CENTER_CENTER, &rest[skip..], 12.0, tc);
        }
        None => cv.text(x + w / 2.0, y + h / 2.0, Align2::CENTER_CENTER, b.title, 12.5, tc),
    }
    if cv.hit(ui, id, x, y, w, h).on_hover_text(b.text).clicked() {
        st.block = id;
    }
}

fn blocks(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = boards::cur();
    let c = b.clocks;
    let mhz = |f: f64| format!("{} МГц", (f / 1e6).round());
    let (cv, _) = Canvas::new(ui, [0.0, 0.0, 1000.0, 470.0], 520.0);
    // AHB bus matrix
    cv.rect(250.0, 20.0, 26.0, 430.0, 4.0, mix(p.clk, 25.0, p.panel), Stroke::new(1.0, p.clk));
    cv.text(263.0, 460.0, Align2::CENTER_TOP, format!("AHB {}", mhz(c.hclk())), 11.0, p.clk);
    // core + AHB masters/slaves
    block_box(&cv, ui, &p, st, "CORE", 20.0, 20.0, 200.0, 80.0);
    let core_note = if b.family == Family::F4 { "NVIC · SysTick · FPU · MPU" } else { "NVIC · SysTick · MPU нет" };
    cv.text(120.0, 86.0, Align2::CENTER_CENTER, core_note, 10.0, p.muted);
    let dy = 330.0 / b.ahb.len() as f32;
    for (i, id) in b.ahb.iter().enumerate() {
        let y = 120.0 + i as f32 * dy;
        block_box(&cv, ui, &p, st, id, 40.0, y, 180.0, (dy - 16.0).min(50.0));
        cv.line(&[(220.0, y + 22.0), (250.0, y + 22.0)], p.clk, 2.0);
    }
    cv.line(&[(220.0, 60.0), (250.0, 60.0)], p.clk, 3.0);
    // APB2 bridge and bus
    let (y2, y1) = (150.0, 330.0);
    cv.line(&[(276.0, y2), (980.0, y2)], p.sig, 5.0);
    cv.text(290.0, y2 - 8.0, Align2::LEFT_BOTTOM, format!("мост → APB2 · {}", mhz(c.pclk2())), 11.0, p.sig);
    let dx2 = 680.0 / b.apb2.len() as f32;
    for (i, id) in b.apb2.iter().enumerate() {
        let x = 300.0 + i as f32 * dx2;
        block_box(&cv, ui, &p, st, id, x, 40.0, dx2 - 9.0, 52.0);
        cv.line(&[(x + (dx2 - 9.0) / 2.0, 92.0), (x + (dx2 - 9.0) / 2.0, y2)], p.sig, 2.0);
    }
    // APB1
    cv.line(&[(276.0, y1), (980.0, y1)], p.refc, 5.0);
    cv.text(290.0, y1 - 8.0, Align2::LEFT_BOTTOM, format!("мост → APB1 · {} (таймеры ×2 = {})", mhz(c.pclk1()), mhz(c.tim_apb1())), 11.0, p.refc);
    let dx1 = (680.0 / b.apb1.len() as f32).min(110.0);
    for (i, id) in b.apb1.iter().enumerate() {
        let x = 296.0 + i as f32 * dx1;
        block_box(&cv, ui, &p, st, id, x, 360.0, dx1 - 7.0, 56.0);
        cv.line(&[(x + (dx1 - 7.0) / 2.0, y1), (x + (dx1 - 7.0) / 2.0, 360.0)], p.refc, 2.0);
    }
    if b.family == Family::F4 {
        cv.text(300.0, 280.0, Align2::LEFT_CENTER, "На F4 порты GPIO и USB OTG висят на AHB, а не на APB", 11.0, p.muted);
    }
    // legend
    cv.rect(300.0, 200.0, 16.0, 12.0, 3.0, mix(p.accent, 10.0, p.panel), Stroke::new(1.2, p.accent));
    cv.text(324.0, 206.0, Align2::LEFT_CENTER, "используется прошивкой", 11.0, p.fg2);
    cv.rect(300.0, 222.0, 16.0, 12.0, 3.0, p.panel2, Stroke::new(1.2, p.line2));
    cv.text(324.0, 228.0, Align2::LEFT_CENTER, "свободно", 11.0, p.fg2);
    cv.text(300.0, 256.0, Align2::LEFT_CENTER, "Нажмите на блок — описание ниже", 11.0, p.muted);

    ui.add_space(8.0);
    let all = mcu::blocks();
    let b = all.iter().find(|b| b.id == st.block).unwrap_or(&all[0]);
    card(ui, &p, b.title, |ui| {
        ui.horizontal(|ui| {
            let bus = match b.bus {
                "core" => "ядро".to_string(),
                "ahb" => "шина AHB".to_string(),
                "apb1" => format!("шина APB1 ({})", mhz(c.pclk1())),
                _ => format!("шина APB2 ({})", mhz(c.pclk2())),
            };
            badge(ui, &p, &bus, p.clk);
            if used(b.id) {
                badge(ui, &p, "используется в проекте", p.accent);
            }
        });
        para(ui, &p, b.text);
        if let Some(name) = svd_name(b.id) {
            if let Some(per) = sh.periph(name) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} @ 0x{:08X} · {} регистров", per.name, per.base, per.registers.len())).monospace().color(p.fg2));
                    if ui.button("Регистры →").clicked() {
                        sh.act(Act::OpenPeriph(name.to_string()));
                    }
                });
                if !per.irqs.is_empty() {
                    let irqs: Vec<String> = per.irqs.iter().map(|(n, v)| format!("{} ({})", n, v)).collect();
                    hint(ui, &p, &format!("Прерывания: {}", irqs.join(", ")));
                }
            }
        }
    });
}

fn memory(ui: &mut egui::Ui, sh: &Shared) {
    let p = sh.pal;
    ui.columns(2, |cols| {
        card(&mut cols[0], &p, "Адресное пространство (4 ГБ, используемые области)", |ui| {
            egui::Grid::new("memmap").num_columns(3).striped(true).spacing([12.0, 5.0]).show(ui, |ui| {
                for r in mcu::memory() {
                    ui.label(RichText::new(format!("0x{:08X}–0x{:08X}", r.start, r.end)).monospace().color(p.accent));
                    ui.label(RichText::new(r.name).strong());
                    ui.add(egui::Label::new(RichText::new(r.text).color(p.fg2)).wrap());
                    ui.end_row();
                }
            });
        });
        card(&mut cols[1], &p, "Заполнение памяти этой прошивкой (из ELF)", |ui| {
            let Some(elf) = &sh.st.elf else {
                hint(ui, &p, "ELF не найден. Соберите прошивку (SWD-отладка → Собрать) или укажите проект в Настройках.");
                return;
            };
            hint(ui, &p, &elf.path.display().to_string());
            let b = boards::cur();
            let (fb, fs) = b.flash;
            let (rb, rs) = b.ram;
            let flash = elf.used(fb, fb + fs);
            let ram = elf.used(rb, rb + rs);
            let fr = flash as f32 / fs as f32;
            let rr = ram as f32 / rs as f32;
            ui.label("Flash");
            gauge(ui, &p, fr, &format!("{} из {} КБ ({:.0} %)", fmt::bytes(flash), fs / 1024, fr * 100.0), level(&p, fr), ui.available_width());
            ui.label("SRAM (статически: .data + .bss + .noinit + запас стека/кучи newlib)");
            gauge(ui, &p, rr, &format!("{} из {} КБ ({:.0} %)", fmt::bytes(ram), rs / 1024, rr * 100.0), level(&p, rr), ui.available_width());
            ui.add_space(6.0);
            egui::Grid::new("sections").num_columns(3).striped(true).show(ui, |ui| {
                for s in &elf.sections {
                    if s.addr >= fb && s.addr < rb + rs && !s.name.starts_with(".debug") {
                        ui.label(RichText::new(&s.name).monospace());
                        ui.label(RichText::new(format!("0x{:08X}", s.addr)).monospace().color(p.fg2));
                        ui.label(fmt::bytes(s.size));
                        ui.end_row();
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Крупнейшие переменные в RAM").strong());
            let mut big: Vec<_> = elf.syms.iter().filter(|s| !s.func && s.addr >= rb && s.addr < rb + rs).collect();
            big.sort_by_key(|s| std::cmp::Reverse(s.size));
            egui::Grid::new("bigsyms").num_columns(3).show(ui, |ui| {
                for s in big.iter().take(10) {
                    ui.label(RichText::new(&s.name).monospace().color(p.accent));
                    ui.label(fmt::bytes(s.size));
                    ui.label(
                        RichText::new(match s.name.as_str() {
                            "ucHeap" => "куча FreeRTOS (configTOTAL_HEAP_SIZE): стеки задач, очередь, мьютекс",
                            "g_diag" => "диагностика, .noinit — переживает сброс",
                            "huart1" | "huart3" | "huart6" | "hspi1" | "htim1" => "дескриптор HAL",
                            "sec.0" | "sec" => "буфер сектора SPI-флеш 4 КБ (чтение-стирание-запись)",
                            _ => "",
                        })
                        .small()
                        .color(p.muted),
                    );
                    ui.end_row();
                }
            });
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn clk_box(cv: &Canvas, p: &Pal, x: f32, y: f32, w: f32, title: &str, val: &str, c: Color32) {
    cv.rect(x, y, w, 48.0, 6.0, mix(c, 12.0, p.panel), Stroke::new(1.3, c));
    cv.text(x + w / 2.0, y + 15.0, Align2::CENTER_CENTER, title, 11.0, p.fg2);
    cv.text_bold(x + w / 2.0, y + 33.0, Align2::CENTER_CENTER, val, 13.0, p.fg);
}

fn arrow(cv: &Canvas, c: Color32, pts: &[(f32, f32)]) {
    cv.line(pts, c, 2.0);
    let (x, y) = pts[pts.len() - 1];
    let (px, py) = pts[pts.len() - 2];
    let (dx, dy) = ((x - px).signum(), (y - py).signum());
    let a = [(x - dx * 8.0 - dy * 4.0, y - dy * 8.0 - dx * 4.0), (x, y), (x - dx * 8.0 + dy * 4.0, y - dy * 8.0 + dx * 4.0)];
    cv.line(&a, c, 2.0);
}

fn clocks(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    let b = boards::cur();
    let c = mcu::clocks();
    let f4 = b.family == Family::F4;
    let (cv, _) = Canvas::new(ui, [0.0, 0.0, 1000.0, 400.0], 440.0);
    clk_box(&cv, &p, 20.0, 40.0, 130.0, "HSE кварц", &fmt::hz(c.hse), p.refc);
    clk_box(&cv, &p, 20.0, 120.0, 130.0, "HSI RC", if f4 { "16 МГц (резерв)" } else { "8 МГц (резерв)" }, p.muted);
    clk_box(&cv, &p, 20.0, 260.0, 130.0, "LSI RC", if f4 { "32 кГц → IWDG" } else { "40 кГц → IWDG" }, p.muted);
    clk_box(&cv, &p, 20.0, 330.0, 130.0, "LSE кварц", "32.768 кГц → RTC", p.muted);
    let pll = if f4 { format!("PLL /{} ×{} /{}", c.pll_m, c.pll_n, c.pll_p) } else { format!("PLL ×{}", c.pll_n) };
    clk_box(&cv, &p, 200.0, 40.0, 120.0, &pll, &fmt::hz(c.sysclk()), p.clk);
    arrow(&cv, p.refc, &[(150.0, 64.0), (200.0, 64.0)]);
    clk_box(&cv, &p, 370.0, 40.0, 130.0, "SYSCLK", &fmt::hz(c.sysclk()), p.clk);
    arrow(&cv, p.clk, &[(320.0, 64.0), (370.0, 64.0)]);
    clk_box(&cv, &p, 550.0, 40.0, 130.0, &format!("AHB /{} → HCLK", c.ahb_div), &fmt::hz(c.hclk()), p.clk);
    arrow(&cv, p.clk, &[(500.0, 64.0), (550.0, 64.0)]);
    let core = if f4 { "ядро, SRAM, DMA, GPIO, SysTick" } else { "ядро, SRAM, DMA, SysTick (FreeRTOS 1 кГц)" };
    clk_box(&cv, &p, 730.0, 20.0, 250.0, core, &fmt::hz(c.hclk()), p.ok);
    arrow(&cv, p.clk, &[(680.0, 56.0), (730.0, 46.0)]);
    let apb2 = if f4 { format!("APB2 /{}: USART1/6, SPI1, TIM1", c.apb2_div) } else { format!("APB2 /{}: GPIO, USART1, SPI1, TIM1", c.apb2_div) };
    clk_box(&cv, &p, 730.0, 120.0, 250.0, &apb2, &fmt::hz(c.pclk2()), p.sig);
    arrow(&cv, p.clk, &[(615.0, 88.0), (615.0, 144.0), (730.0, 144.0)]);
    let adc_max = if f4 { "36" } else { "14" };
    clk_box(&cv, &p, 730.0, 200.0, 250.0, &format!("ADC /{} (≤ {} МГц)", c.adc_div, adc_max), &fmt::hz(c.adc()), p.sig);
    arrow(&cv, p.sig, &[(855.0, 168.0), (855.0, 200.0)]);
    let apb1 = if f4 { format!("APB1 /{}: USART2, SPI2/3, I2C", c.apb1_div) } else { format!("APB1 /{}: USART2/3, SPI2, I2C", c.apb1_div) };
    clk_box(&cv, &p, 480.0, 200.0, 220.0, &apb1, &fmt::hz(c.pclk1()), p.refc);
    arrow(&cv, p.clk, &[(580.0, 88.0), (580.0, 200.0)]);
    clk_box(&cv, &p, 480.0, 280.0, 220.0, if f4 { "TIM2–5 (APB1 ×2)" } else { "TIM2–4 (APB1 ×2)" }, &fmt::hz(c.tim_apb1()), p.refc);
    arrow(&cv, p.refc, &[(590.0, 248.0), (590.0, 280.0)]);
    let usb = if f4 { format!("PLL Q /{} → USB OTG", c.pll_q) } else { "USB /1.5".to_string() };
    clk_box(&cv, &p, 200.0, 200.0, 220.0, &usb, &fmt::hz(c.usb()), p.warn);
    arrow(&cv, p.clk, &[(260.0, 88.0), (260.0, 200.0)]);
    cv.text(200.0, 120.0, Align2::LEFT_TOP, format!("Flash: {} такта ожидания (FLASH_LATENCY_{})", c.flash_latency, c.flash_latency), 11.0, p.fg2);
    let src = if f4 { format!("VCO = {} · SystemClock_Config() в main.c", fmt::hz(c.vco())) } else { "SystemClock_Config() в main.c".to_string() };
    cv.text(200.0, 140.0, Align2::LEFT_TOP, src, 11.0, p.muted);
    for (i, s) in c.issues().iter().enumerate() {
        cv.text(200.0, 280.0 + i as f32 * 16.0, Align2::LEFT_TOP, format!("⚠ {}", s), 11.0, p.err);
    }

    ui.add_space(8.0);
    let rm = if f4 { "RM0383 §19.3.4" } else { "RM0008 §27.3.4" };
    card(ui, &p, &format!("Калькулятор скорости USART ({})", rm), |ui| {
        ui.horizontal(|ui| {
            ui.label("Скорость, бод:");
            ui.add(egui::TextEdit::singleline(&mut st.baud).desired_width(90.0));
            let (a2, a1) = if f4 { ("USART1/6", "USART2") } else { ("USART1", "USART2/3") };
            ui.selectable_value(&mut st.apb1, false, format!("{} (APB2 {})", a2, fmt::hz(c.pclk2())));
            ui.selectable_value(&mut st.apb1, true, format!("{} (APB1 {})", a1, fmt::hz(c.pclk1())));
        });
        match st.baud.trim().parse::<f64>() {
            Ok(b) if b > 0.0 => {
                let pclk = if st.apb1 { c.pclk1() } else { c.pclk2() };
                let (brr, err) = Clocks::uart_error(pclk, b);
                let col = if err.abs() < 2.0 { p.ok } else { p.err };
                ui.label(RichText::new(format!("BRR = 0x{:04X} (мантисса {}, дробь {}/16) · погрешность {:+.3} %", brr, brr >> 4, brr & 15, err)).monospace().color(col));
                hint(ui, &p, "Приёмник терпит ошибку примерно до ±2 % суммарно на обеих сторонах.");
            }
            _ => hint(ui, &p, "Введите число."),
        }
    });
}

fn pins(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    ui.horizontal(|ui| {
        ui.label("Поиск:");
        ui.add(egui::TextEdit::singleline(&mut st.pin_filter).hint_text("PA9, USART, ADC, SPI1…").desired_width(240.0));
    });
    let f = st.pin_filter.to_uppercase();
    egui::Grid::new("pins").num_columns(6).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
        let remap = if boards::cur().family == Family::F1 { "После remap" } else { "" };
        for h in ["№", "Вывод", "FT", "Альтернативные функции", remap, "В проекте"] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for pin in mcu::pins() {
            let u = mcu::usage(pin.name);
            let hay = format!("{} {} {} {}", pin.name, pin.af, pin.remap, u.map(|u| u.0).unwrap_or("")).to_uppercase();
            if !f.is_empty() && !hay.contains(&f) {
                continue;
            }
            ui.label(pin.lqfp.to_string());
            ui.label(RichText::new(pin.name).monospace().strong());
            ui.label(if pin.ft { "5 В" } else { "" });
            ui.label(RichText::new(if pin.af.is_empty() { pin.note } else { pin.af }).color(p.fg2));
            ui.label(RichText::new(pin.remap).color(p.muted));
            ui.label(RichText::new(u.map(|u| u.0).unwrap_or("")).color(p.accent));
            ui.end_row();
        }
    });
    let _ = mix(p.fg, 1.0, p.bg);
}
