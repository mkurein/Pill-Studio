//! MCU structure: block diagram (buses), memory map with the project's real
//! usage from the ELF, clock tree, full pin table.

use crate::app::{Act, Shared};
use crate::canvas::Canvas;
use crate::core::mcu::{self, Clocks, BLOCKS, MEMORY, PINS, PROJECT_CLOCKS};
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
    matches!(id, "CORE" | "FLASH" | "SRAM" | "RCC" | "GPIO" | "AFIO" | "TIM1" | "SPI1" | "USART1" | "USART23")
}

fn svd_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "CORE" => "SCB",
        "FLASH" => "FLASH",
        "DMA1" => "DMA1",
        "RCC" => "RCC",
        "CRC" => "CRC",
        "GPIO" => "GPIOA",
        "AFIO" => "AFIO",
        "EXTI" => "EXTI",
        "ADC" => "ADC1",
        "TIM1" => "TIM1",
        "SPI1" => "SPI1",
        "USART1" => "USART1",
        "TIM234" => "TIM2",
        "USART23" => "USART3",
        "SPI2" => "SPI2",
        "I2C" => "I2C1",
        "USB" => "USB",
        "CAN" => "CAN",
        "WDG" => "IWDG",
        "RTC" => "RTC",
        _ => return None,
    })
}

pub fn show(ui: &mut egui::Ui, sh: &Shared, st: &mut St) {
    let p = sh.pal;
    h1(ui, &p, "Контроллер STM32F103C8T6", "Cortex-M3 72 МГц · Flash 64 КБ · SRAM 20 КБ · LQFP48 · 2.0–3.6 В · −40…+85 °C");
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
    let b = BLOCKS.iter().find(|b| b.id == id).expect("block id");
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
    let (cv, _) = Canvas::new(ui, [0.0, 0.0, 1000.0, 470.0], 520.0);
    // AHB bus matrix
    cv.rect(250.0, 20.0, 26.0, 430.0, 4.0, mix(p.clk, 25.0, p.panel), Stroke::new(1.0, p.clk));
    cv.text(263.0, 460.0, Align2::CENTER_TOP, "AHB 72 МГц", 11.0, p.clk);
    // core + AHB masters/slaves
    block_box(&cv, ui, &p, st, "CORE", 20.0, 20.0, 200.0, 80.0);
    cv.text(120.0, 86.0, Align2::CENTER_CENTER, "NVIC · SysTick · MPU нет", 10.0, p.muted);
    for (i, id) in ["FLASH", "SRAM", "DMA1", "RCC", "CRC"].iter().enumerate() {
        let y = 120.0 + i as f32 * 66.0;
        block_box(&cv, ui, &p, st, id, 40.0, y, 180.0, 50.0);
        cv.line(&[(220.0, y + 25.0), (250.0, y + 25.0)], p.clk, 2.0);
    }
    cv.line(&[(220.0, 60.0), (250.0, 60.0)], p.clk, 3.0);
    // APB2 bridge and bus
    let (y2, y1) = (150.0, 330.0);
    cv.line(&[(276.0, y2), (980.0, y2)], p.sig, 5.0);
    cv.text(290.0, y2 - 8.0, Align2::LEFT_BOTTOM, "мост → APB2 · 72 МГц", 11.0, p.sig);
    let apb2 = ["GPIO", "AFIO", "EXTI", "ADC", "TIM1", "SPI1", "USART1"];
    for (i, id) in apb2.iter().enumerate() {
        let x = 300.0 + i as f32 * 97.0;
        block_box(&cv, ui, &p, st, id, x, 40.0, 88.0, 52.0);
        cv.line(&[(x + 44.0, 92.0), (x + 44.0, y2)], p.sig, 2.0);
    }
    // APB1
    cv.line(&[(276.0, y1), (980.0, y1)], p.refc, 5.0);
    cv.text(290.0, y1 - 8.0, Align2::LEFT_BOTTOM, "мост → APB1 · 36 МГц (таймеры ×2 = 72)", 11.0, p.refc);
    let apb1 = ["TIM234", "USART23", "SPI2", "I2C", "USB", "CAN", "WDG", "RTC"];
    for (i, id) in apb1.iter().enumerate() {
        let x = 296.0 + i as f32 * 85.0;
        block_box(&cv, ui, &p, st, id, x, 360.0, 78.0, 56.0);
        cv.line(&[(x + 39.0, y1), (x + 39.0, 360.0)], p.refc, 2.0);
    }
    // legend
    cv.rect(300.0, 200.0, 16.0, 12.0, 3.0, mix(p.accent, 10.0, p.panel), Stroke::new(1.2, p.accent));
    cv.text(324.0, 206.0, Align2::LEFT_CENTER, "используется прошивкой", 11.0, p.fg2);
    cv.rect(300.0, 222.0, 16.0, 12.0, 3.0, p.panel2, Stroke::new(1.2, p.line2));
    cv.text(324.0, 228.0, Align2::LEFT_CENTER, "свободно", 11.0, p.fg2);
    cv.text(300.0, 256.0, Align2::LEFT_CENTER, "Нажмите на блок — описание ниже", 11.0, p.muted);

    ui.add_space(8.0);
    let b = BLOCKS.iter().find(|b| b.id == st.block).unwrap_or(&BLOCKS[0]);
    card(ui, &p, b.title, |ui| {
        ui.horizontal(|ui| {
            let bus = match b.bus {
                "core" => "ядро",
                "ahb" => "шина AHB",
                "apb1" => "шина APB1 (36 МГц)",
                _ => "шина APB2 (72 МГц)",
            };
            badge(ui, &p, bus, p.clk);
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
                for r in MEMORY {
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
            let flash = elf.used(0x0800_0000, 0x0801_0000);
            let ram = elf.used(0x2000_0000, 0x2000_5000);
            ui.label("Flash");
            gauge(ui, &p, flash as f32 / 65536.0, &format!("{} из 64 КБ ({:.0} %)", fmt::bytes(flash), flash as f32 / 655.36), level(&p, flash as f32 / 65536.0), ui.available_width());
            ui.label("SRAM (статически: .data + .bss + .noinit + запас стека/кучи newlib)");
            gauge(ui, &p, ram as f32 / 20480.0, &format!("{} из 20 КБ ({:.0} %)", fmt::bytes(ram), ram as f32 / 204.8), level(&p, ram as f32 / 20480.0), ui.available_width());
            ui.add_space(6.0);
            egui::Grid::new("sections").num_columns(3).striped(true).show(ui, |ui| {
                for s in &elf.sections {
                    if s.addr >= 0x0800_0000 && s.addr < 0x2000_5000 && !s.name.starts_with(".debug") {
                        ui.label(RichText::new(&s.name).monospace());
                        ui.label(RichText::new(format!("0x{:08X}", s.addr)).monospace().color(p.fg2));
                        ui.label(fmt::bytes(s.size));
                        ui.end_row();
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Крупнейшие переменные в RAM").strong());
            let mut big: Vec<_> = elf.syms.iter().filter(|s| !s.func && s.addr >= 0x2000_0000 && s.addr < 0x2000_5000).collect();
            big.sort_by_key(|s| std::cmp::Reverse(s.size));
            egui::Grid::new("bigsyms").num_columns(3).show(ui, |ui| {
                for s in big.iter().take(10) {
                    ui.label(RichText::new(&s.name).monospace().color(p.accent));
                    ui.label(fmt::bytes(s.size));
                    ui.label(
                        RichText::new(match s.name.as_str() {
                            "ucHeap" => "куча FreeRTOS (configTOTAL_HEAP_SIZE): стеки задач, очередь, мьютекс",
                            "g_diag" => "диагностика, .noinit — переживает сброс",
                            "huart1" | "huart3" | "hspi1" | "htim1" => "дескриптор HAL",
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
    let c = PROJECT_CLOCKS;
    let (cv, _) = Canvas::new(ui, [0.0, 0.0, 1000.0, 400.0], 440.0);
    clk_box(&cv, &p, 20.0, 40.0, 130.0, "HSE кварц", "8 МГц", p.refc);
    clk_box(&cv, &p, 20.0, 120.0, 130.0, "HSI RC", "8 МГц (резерв)", p.muted);
    clk_box(&cv, &p, 20.0, 260.0, 130.0, "LSI RC", "40 кГц → IWDG", p.muted);
    clk_box(&cv, &p, 20.0, 330.0, 130.0, "LSE кварц", "32.768 кГц → RTC", p.muted);
    clk_box(&cv, &p, 200.0, 40.0, 120.0, "PLL ×9", &fmt::hz(c.sysclk()), p.clk);
    arrow(&cv, p.refc, &[(150.0, 64.0), (200.0, 64.0)]);
    clk_box(&cv, &p, 370.0, 40.0, 130.0, "SYSCLK", &fmt::hz(c.sysclk()), p.clk);
    arrow(&cv, p.clk, &[(320.0, 64.0), (370.0, 64.0)]);
    clk_box(&cv, &p, 550.0, 40.0, 130.0, "AHB /1 → HCLK", &fmt::hz(c.hclk()), p.clk);
    arrow(&cv, p.clk, &[(500.0, 64.0), (550.0, 64.0)]);
    clk_box(&cv, &p, 730.0, 20.0, 250.0, "ядро, SRAM, DMA, SysTick (FreeRTOS 1 кГц)", &fmt::hz(c.hclk()), p.ok);
    arrow(&cv, p.clk, &[(680.0, 56.0), (730.0, 46.0)]);
    clk_box(&cv, &p, 730.0, 120.0, 250.0, "APB2 /1: GPIO, USART1, SPI1, TIM1", &fmt::hz(c.pclk2()), p.sig);
    arrow(&cv, p.clk, &[(615.0, 88.0), (615.0, 144.0), (730.0, 144.0)]);
    clk_box(&cv, &p, 730.0, 200.0, 250.0, "ADC /6 (≤ 14 МГц)", &fmt::hz(c.adc()), p.sig);
    arrow(&cv, p.sig, &[(855.0, 168.0), (855.0, 200.0)]);
    clk_box(&cv, &p, 480.0, 200.0, 220.0, "APB1 /2: USART2/3, SPI2, I2C", &fmt::hz(c.pclk1()), p.refc);
    arrow(&cv, p.clk, &[(580.0, 88.0), (580.0, 200.0)]);
    clk_box(&cv, &p, 480.0, 280.0, 220.0, "TIM2–4 (APB1 ×2)", &fmt::hz(c.tim_apb1()), p.refc);
    arrow(&cv, p.refc, &[(590.0, 248.0), (590.0, 280.0)]);
    clk_box(&cv, &p, 200.0, 200.0, 220.0, "USB /1.5", &fmt::hz(c.usb()), p.warn);
    arrow(&cv, p.clk, &[(260.0, 88.0), (260.0, 200.0)]);
    cv.text(200.0, 120.0, Align2::LEFT_TOP, "Flash: 2 такта ожидания (FLASH_LATENCY_2)", 11.0, p.fg2);
    cv.text(200.0, 140.0, Align2::LEFT_TOP, "SystemClock_Config() в main.c", 11.0, p.muted);
    for (i, s) in c.issues().iter().enumerate() {
        cv.text(200.0, 280.0 + i as f32 * 16.0, Align2::LEFT_TOP, format!("⚠ {}", s), 11.0, p.err);
    }

    ui.add_space(8.0);
    card(ui, &p, "Калькулятор скорости USART (RM0008 §27.3.4)", |ui| {
        ui.horizontal(|ui| {
            ui.label("Скорость, бод:");
            ui.add(egui::TextEdit::singleline(&mut st.baud).desired_width(90.0));
            ui.selectable_value(&mut st.apb1, false, "USART1 (APB2 72 МГц)");
            ui.selectable_value(&mut st.apb1, true, "USART2/3 (APB1 36 МГц)");
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
        for h in ["№", "Вывод", "FT", "Функции по умолчанию", "После remap", "В проекте"] {
            ui.label(RichText::new(h).strong());
        }
        ui.end_row();
        for pin in PINS {
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
