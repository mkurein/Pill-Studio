//! "What is SWD" help card on the SWD debug screen: the interface, the
//! protocol, the core's debug blocks, the chain Pill Studio → OpenOCD →
//! ST-Link → chip, and what to do when the board does not answer.

use crate::core::boards;
use crate::core::openocd::{GDB_PORT, TCL_PORT};
use crate::theme::Pal;
use crate::widgets::*;
use eframe::egui::{self, RichText};

fn section(ui: &mut egui::Ui, p: &Pal, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(15.0).color(p.fg));
    ui.add_space(2.0);
}

/// Two- or three-column table; the first row is the header.
fn table(ui: &mut egui::Ui, p: &Pal, id: &str, rows: &[&[&str]]) {
    let n = rows.first().map_or(0, |r| r.len());
    egui::Grid::new(id).num_columns(n).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
        for (i, row) in rows.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                let t = RichText::new(*cell);
                let t = if i == 0 {
                    t.strong().color(p.fg2)
                } else if j == 0 {
                    t.color(p.accent)
                } else {
                    t.color(p.fg)
                };
                // names stay on one line, text columns wrap at a fixed width
                if j == 0 {
                    ui.add(egui::Label::new(t).extend());
                } else {
                    let w = if j + 1 == n { 520.0 } else { 300.0 };
                    ui.vertical(|ui| {
                        ui.set_max_width(w);
                        ui.add(egui::Label::new(t).wrap());
                    });
                }
            }
            ui.end_row();
        }
    });
}

/// Help text, shown in a card on the SWD screen.
pub fn body(ui: &mut egui::Ui, p: &Pal) {
    let b = boards::cur();
    para(
        ui,
        p,
        "SWD (Serial Wire Debug) — отладочный порт, встроенный в каждое ядро ARM Cortex-M. Это такая же часть \
         кристалла, как UART или SPI, только подключён он к самому ядру и к шине памяти. Поэтому через него \
         компьютер видит микроконтроллер «изнутри», и программе на плате для этого ничего делать не нужно — \
         работает, даже когда прошивка зависла или упала в HardFault.",
    );

    section(ui, p, "Провода");
    table(
        ui,
        p,
        "swd_wires",
        &[
            &["Провод", "Вывод STM32", "Назначение"],
            &["SWDIO", "PA13", "данные, в обе стороны"],
            &["SWCLK", "PA14", "такт от отладчика"],
            &["GND", "—", "общий провод"],
            &["3.3V", "—", "ST-Link по нему видит, что плата запитана (напряжение в строке SWD)"],
            &["NRST", "сброс", "необязательно: для «подключения под сбросом»"],
        ],
    );
    hint(ui, p, &format!("На {} это разъём на торце платы: {}. Старый JTAG требовал 4–5 сигнальных проводов, SWD — два.", b.board, b.header_swd.join(" · ")));

    section(ui, p, "Протокол");
    para(
        ui,
        p,
        "Отладчик шлёт короткие пакеты: 8 бит запроса (что читать или писать), 3 бита ответа ACK (OK, WAIT, FAULT) \
         и 32 бита данных с битом чётности. Каждый пакет — одно чтение или запись 32-битного регистра \
         отладочного модуля ядра (DAP):",
    );
    table(
        ui,
        p,
        "swd_dap",
        &[
            &["Часть DAP", "Что это"],
            &["DP (Debug Port)", "сам порт: связь, питание отладочного домена, IDCODE ядра"],
            &["MEM-AP", "«окно» в шину памяти: записал адрес — читаешь или пишешь данные. Так читается любая ячейка: RAM, Flash, регистры периферии"],
        ],
    );
    hint(
        ui,
        p,
        &format!(
            "Через MEM-AP студия читает DBGMCU_IDCODE (0xE0042000): DEV_ID 0x{:03X} — это {}. По нему видно, какая плата подключена на самом деле.",
            b.dev_id, b.chip
        ),
    );

    section(ui, p, "Отладочные блоки ядра");
    table(
        ui,
        p,
        "swd_blocks",
        &[
            &["Блок", "Что умеет", "Где здесь"],
            &["DHCSR", "остановить ядро, продолжить, шаг на одну инструкцию", "Пауза F6, Продолжить F5, Строка F10, В функцию F11"],
            &["DCRSR / DCRDR", "читать и писать R0–R15, PC, SP, xPSR, пока ядро стоит", "«Функции, регистры, стек», PC → строка исходника"],
            &["FPB", "аппаратные точки останова (6 на Cortex-M3/M4): ядро встаёт, когда PC доходит до адреса", "«Точка останова», F9"],
            &["DWT", "точки наблюдения за данными, счётчик тактов", "пока не используется"],
            &["Контроллер Flash", "стирание и запись Flash через MEM-AP", "«⚡ Прошить»: program … verify reset"],
            &["SCB: CFSR, HFSR, BFAR", "причина HardFault, адрес ошибки шины", "«CFSR/HFSR сейчас», «Анализ сбоя»"],
        ],
    );
    hint(
        ui,
        p,
        "Чтение памяти не останавливает ядро: MEM-AP делит шину с процессором. Поэтому переменные, g_diag и регистры периферии читаются прямо во время работы прошивки.",
    );

    section(ui, p, "Цепочка от Pill Studio до кристалла");
    ui.label(
        RichText::new(format!(
            "Pill Studio ── TCL :{TCL_PORT} ──▶ OpenOCD ── USB ──▶ ST-Link ── SWD ──▶ DAP ядра {}\nCursor      ── GDB :{GDB_PORT} ──┘",
            b.chip_short
        ))
        .monospace()
        .color(p.accent),
    );
    ui.add_space(4.0);
    table(
        ui,
        p,
        "swd_chain",
        &[
            &["Звено", "Роль"],
            &["ST-Link", "USB-адаптер: превращает команды компьютера в сигналы SWDIO / SWCLK"],
            &["OpenOCD", "знает DAP, ядра Cortex-M и Flash STM32. Студия шлёт ему TCL-команды: halt, mdw, reg pc, bp, program — их же можно вводить в «Консоли OpenOCD»"],
            &["ELF", "переводит адреса в имена: 0x08001234 → main → main.c:142 (arm-none-eabi-addr2line)"],
            &["Cursor", "подключается к тому же OpenOCD по GDB: остановили ядро в Cursor — студия видит это и открывает строку"],
        ],
    );
    hint(ui, p, &format!("Цель OpenOCD для этой платы: {}.", b.ocd_target));

    section(ui, p, "Зачем это нужно");
    para(ui, p, "• Плата зависла или упала — UART молчит, а SWD работает: остановить ядро, посмотреть PC, стек вызовов, CFSR и запись g_diag.");
    para(ui, p, "• Проверить железо без кода: что в регистре GPIO, включено ли тактирование, какая скорость у USART (BRR).");
    para(ui, p, "• Прошивать без загрузчика и перемычки BOOT0.");

    section(ui, p, "Когда SWD «не видит» плату");
    table(
        ui,
        p,
        "swd_trouble",
        &[
            &["Причина", "Что делать"],
            &["прошивка перенастроила PA13/PA14 как GPIO или ушла в глубокий сон", "«подключение под сбросом» в Настройках (нужен провод NRST) или держать RESET в момент подключения"],
            &["нет питания или общего GND", "напряжение в строке SWD должно быть около 3.3 V"],
            &["перепутаны DIO и CLK", "сверить с разъёмом на торце платы"],
            &["ST-Link занят другой программой", "закрыть второй OpenOCD, STM32CubeIDE или STM32CubeProgrammer"],
        ],
    );
    ui.add_space(8.0);
    hint(ui, p, "Подробнее о выводах обеих плат — docs/HARDWARE.md и docs/PINOUT.md в репозитории Pill-Studio.");
}
