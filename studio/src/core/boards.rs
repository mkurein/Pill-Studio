//! Board profiles: everything that differs between the Blue Pill (STM32F103C8T6)
//! and the Black Pill (STM32F411CEU6). The rest of the studio reads the current
//! profile through `cur()`; switching the board in Settings calls `set()`.

use super::mcu::{self, Block, Clocks, Pin, Region, Usage};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoardId {
    #[default]
    BluePill,
    BlackPill,
}

impl BoardId {
    pub const ALL: [BoardId; 2] = [BoardId::BluePill, BoardId::BlackPill];
    pub fn key(self) -> &'static str {
        match self {
            BoardId::BluePill => "bluepill",
            BoardId::BlackPill => "blackpill",
        }
    }
    pub fn profile(self) -> &'static Profile {
        match self {
            BoardId::BluePill => &BLUE_PILL,
            BoardId::BlackPill => &BLACK_PILL,
        }
    }
}

/// GPIO register layout: F1 = CRL/CRH 4-bit nibbles, F4 = MODER/OTYPER/OSPEEDR/PUPDR/AFR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    F1,
    F4,
}

pub struct Profile {
    pub id: BoardId,
    /// "Blue Pill"
    pub board: &'static str,
    /// "STM32F103C8T6"
    pub chip: &'static str,
    /// Short chip label for the board drawing ("F103C8T6").
    pub chip_short: &'static str,
    pub package: &'static str,
    /// Subtitle of the controller screen.
    pub summary: &'static str,
    pub family: Family,
    pub core: &'static str,
    pub flash: (u32, u32),
    pub ram: (u32, u32),
    /// CMSIS-SVD of the family and the peripherals that exist in this exact part.
    pub svd: &'static str,
    pub svd_name: &'static str,
    pub present: &'static [&'static str],
    pub pins: &'static [Pin],
    pub usage: fn(&str) -> Option<Usage>,
    /// Board headers as silkscreened, USB connector on the left.
    pub header_top: &'static [&'static str],
    pub header_bottom: &'static [&'static str],
    pub header_swd: &'static [&'static str],
    pub crystal: &'static str,
    /// What is wired in this project: (who, pins, what).
    pub wiring: &'static [(&'static str, &'static str, &'static str)],
    pub safety: &'static str,
    pub blocks: &'static [Block],
    /// Block ids in the diagram: AHB column, APB2 row, APB1 row.
    pub ahb: &'static [&'static str],
    pub apb2: &'static [&'static str],
    pub apb1: &'static [&'static str],
    pub used_blocks: &'static [&'static str],
    pub block_svd: fn(&str) -> Option<&'static str>,
    pub memory: &'static [Region],
    pub clocks: Clocks,
    /// OpenOCD target script and DBGMCU DEV_ID (IDCODE & 0xFFF).
    pub ocd_target: &'static str,
    pub dev_id: u32,
    pub idcode_sim: u32,
    /// Firmware ELF name in build/ (and Debug/).
    pub elf: &'static str,
    /// A file in the firmware repo root that identifies it (linker script).
    pub marker: &'static str,
    /// Text log UART: peripheral and TX pin.
    pub log_uart: &'static str,
    pub log_tx: &'static str,
    /// FreeRTOS tasks: (name, priority, stack bytes, purpose).
    pub tasks: [(&'static str, &'static str, u32, &'static str); 3],
    pub heap_total: u32,
    /// SPI memory on PA4..PA7.
    pub spi_mem: &'static str,
    pub repo: &'static str,
}

#[cfg(not(test))]
static CUR: AtomicU8 = AtomicU8::new(0);

// Unit tests run in parallel threads: each test thread has its own board.
#[cfg(test)]
thread_local! {
    static CUR: AtomicU8 = const { AtomicU8::new(0) };
}

#[cfg(not(test))]
fn load() -> u8 {
    CUR.load(Ordering::Relaxed)
}
#[cfg(test)]
fn load() -> u8 {
    CUR.with(|c| c.load(Ordering::Relaxed))
}

pub fn cur() -> &'static Profile {
    if load() == 1 {
        &BLACK_PILL
    } else {
        &BLUE_PILL
    }
}

pub fn set(id: BoardId) {
    let v = if id == BoardId::BlackPill { 1 } else { 0 };
    #[cfg(not(test))]
    CUR.store(v, Ordering::Relaxed);
    #[cfg(test)]
    CUR.with(|c| c.store(v, Ordering::Relaxed));
}

/// Board whose chip has this DBGMCU IDCODE.
pub fn by_idcode(idcode: u32) -> Option<BoardId> {
    BoardId::ALL.into_iter().find(|b| b.profile().dev_id == idcode & 0xFFF)
}

pub static BLUE_PILL: Profile = Profile {
    id: BoardId::BluePill,
    board: "Blue Pill",
    chip: "STM32F103C8T6",
    chip_short: "F103C8T6",
    package: "LQFP48",
    summary: "Cortex-M3 72 МГц · Flash 64 КБ · SRAM 20 КБ · LQFP48 · 2.0–3.6 В · −40…+85 °C",
    family: Family::F1,
    core: "Cortex-M3",
    flash: (0x0800_0000, 64 * 1024),
    ram: (0x2000_0000, 20 * 1024),
    svd: include_str!("../../../STM32F103.svd"),
    svd_name: "STM32F103.svd",
    present: &[
        "RCC", "FLASH", "PWR", "BKP", "RTC", "IWDG", "WWDG", "GPIOA", "GPIOB", "GPIOC", "GPIOD", "AFIO", "EXTI",
        "DMA1", "TIM1", "TIM2", "TIM3", "TIM4", "USART1", "USART2", "USART3", "SPI1", "SPI2", "I2C1", "I2C2",
        "ADC1", "ADC2", "CAN", "USB", "CRC", "DBG", "DBGMCU", "NVIC", "SCB", "STK", "MPU", "SCB_ACTRL",
        "NVIC_STIR",
    ],
    pins: mcu::F103_PINS,
    usage: mcu::f103_usage,
    header_top: &[
        "GND", "GND", "3.3", "R", "B11", "B10", "B1", "B0", "A7", "A6", "A5", "A4", "A3", "A2", "A1", "A0", "C15",
        "C14", "C13", "VB",
    ],
    header_bottom: &[
        "B12", "B13", "B14", "B15", "A8", "A9", "A10", "A11", "A12", "A15", "B3", "B4", "B5", "B6", "B7", "B8", "B9",
        "5V", "GND", "3.3",
    ],
    header_swd: &["3.3", "DIO", "CLK", "GND"],
    crystal: "8 МГц",
    wiring: &[
        ("USB-UART №1", "A9 → RX, A10 ← TX, GND", "протокол команд, 38400"),
        ("USB-UART №2", "B10 → RX, GND", "текстовый лог, 115200"),
        ("ST-Link V2", "SWD: DIO, CLK, GND, 3.3", "прошивка и отладка"),
        ("FRAM 32 КБ (SPI)", "A4 CS, A5 SCK, A6 MISO, A7 MOSI", "энергонезависимая память"),
        ("Светодиод платы", "C13 (0 = горит)", "индикация, команда 0x70"),
        ("Контрольная точка", "C15", "меняется на каждый кадр"),
    ],
    safety: "Безопасность: логика 3.3 В. На выводы без отметки FT (A0–A7, B0, B1, B5, C13–C15) нельзя подавать 5 В. \
             Если ST-Link питает плату, не подключайте одновременно 5 В от USB-UART к выводу 3.3.",
    blocks: mcu::F103_BLOCKS,
    ahb: &["FLASH", "SRAM", "DMA1", "RCC", "CRC"],
    apb2: &["GPIO", "AFIO", "EXTI", "ADC", "TIM1", "SPI1", "USART1"],
    apb1: &["TIM234", "USART23", "SPI2", "I2C", "USB", "CAN", "WDG", "RTC"],
    used_blocks: &["CORE", "FLASH", "SRAM", "RCC", "GPIO", "AFIO", "TIM1", "SPI1", "USART1", "USART23"],
    block_svd: mcu::f103_block_svd,
    memory: mcu::F103_MEMORY,
    clocks: mcu::F103_CLOCKS,
    ocd_target: "target/stm32f1x.cfg",
    dev_id: 0x410,
    idcode_sim: 0x2000_0410,
    elf: "primGPT.elf",
    marker: "STM32F103C8TX_FLASH.ld",
    log_uart: "USART3",
    log_tx: "PB10",
    tasks: [
        ("defaultTask", "Normal", 1024, "лог Tick/DIAG в UART3, heartbeat"),
        ("LedTask", "Normal", 512, "мигание PC13 в авто-режиме"),
        ("Uart1Task", "AboveNormal", 2048, "приём кадров UART1, команды"),
    ],
    heap_total: 10 * 1024,
    spi_mem: "FRAM 32 КБ",
    repo: "github.com/mkurein/F103C8T",
};

pub static BLACK_PILL: Profile = Profile {
    id: BoardId::BlackPill,
    board: "Black Pill",
    chip: "STM32F411CEU6",
    chip_short: "F411CEU6",
    package: "UFQFPN48",
    summary: "Cortex-M4F 100 МГц (здесь 96) · Flash 512 КБ · SRAM 128 КБ · UFQFPN48 · 1.7–3.6 В · −40…+85 °C",
    family: Family::F4,
    core: "Cortex-M4F",
    flash: (0x0800_0000, 512 * 1024),
    ram: (0x2000_0000, 128 * 1024),
    svd: include_str!("../../../STM32F411.svd"),
    svd_name: "STM32F411.svd",
    present: &[
        "ADC_Common", "ADC1", "CRC", "DBG", "EXTI", "FLASH", "IWDG", "OTG_FS_DEVICE", "OTG_FS_GLOBAL", "OTG_FS_HOST",
        "OTG_FS_PWRCLK", "PWR", "RCC", "RTC", "SDIO", "SYSCFG", "TIM1", "TIM10", "TIM11", "TIM2", "TIM3", "TIM4",
        "TIM5", "TIM9", "USART1", "USART2", "USART6", "WWDG", "DMA2", "DMA1", "GPIOH", "GPIOC", "GPIOB", "GPIOA",
        "I2C3", "I2C2", "I2C1", "I2S2ext", "I2S3ext", "SPI1", "SPI2", "SPI3", "SPI4", "SPI5",
    ],
    pins: mcu::F411_PINS,
    usage: mcu::f411_usage,
    header_top: &[
        "VB", "C13", "C14", "C15", "R", "A0", "A1", "A2", "A3", "A4", "A5", "A6", "A7", "B0", "B1", "B2", "B10",
        "3.3", "GND", "5V",
    ],
    header_bottom: &[
        "B12", "B13", "B14", "B15", "A8", "A9", "A10", "A11", "A12", "A15", "B3", "B4", "B5", "B6", "B7", "B8", "B9",
        "5V", "GND", "3.3",
    ],
    header_swd: &["3.3", "DIO", "CLK", "GND"],
    crystal: "25 МГц",
    wiring: &[
        ("USB-UART №1", "A9 → RX, A10 ← TX, GND", "протокол команд, 38400"),
        ("USB-UART №2", "A11 → RX, GND", "лог USART6, 115200"),
        ("ST-Link V2", "SWD: DIO, CLK, GND, 3.3", "прошивка и отладка"),
        ("W25Q64 (на плате)", "A4 CS, A5 SCK, A6 MISO, A7 MOSI", "SPI NOR-флеш 8 МБ"),
        ("Светодиод платы", "C13 (0 = горит)", "индикация, команда 0x70"),
        ("Кнопка KEY", "A0 (0 = нажата)", "не используется"),
    ],
    safety: "Безопасность: логика 3.3 В. Выводы F411 — FT, кроме аналогового режима и PC14/PC15/PH0/PH1. \
             A11/A12 — линии USB: с логом на A11 USB для данных не использовать. Не питайте плату сразу от USB и от 5 В адаптера.",
    blocks: mcu::F411_BLOCKS,
    ahb: &["FLASH", "SRAM", "DMA", "GPIO", "RCC", "USB"],
    apb2: &["EXTI", "ADC", "TIM1", "TIM911", "SPI1", "SPI45", "USART1", "USART6", "SDIO"],
    apb1: &["TIM25", "USART2", "SPI23", "I2C", "WDG", "RTC"],
    used_blocks: &["CORE", "FLASH", "SRAM", "RCC", "GPIO", "TIM1", "SPI1", "USART1", "USART6"],
    block_svd: mcu::f411_block_svd,
    memory: mcu::F411_MEMORY,
    clocks: mcu::F411_CLOCKS,
    ocd_target: "target/stm32f4x.cfg",
    dev_id: 0x431,
    idcode_sim: 0x1000_6431,
    elf: "primGPT_F411.elf",
    marker: "STM32F411CEUX_FLASH.ld",
    log_uart: "USART6",
    log_tx: "PA11",
    tasks: [
        ("defaultTask", "Normal", 2048, "лог Tick/DIAG в USART6, heartbeat"),
        ("LedTask", "Normal", 512, "мигание PC13 в авто-режиме"),
        ("Uart1Task", "AboveNormal", 2048, "приём кадров UART1, команды"),
    ],
    heap_total: 10 * 1024,
    spi_mem: "W25Q64 8 МБ",
    repo: "github.com/kureinmaxim/F411CE",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_consistent() {
        for b in BoardId::ALL {
            let p = b.profile();
            assert_eq!(p.id, b);
            assert_eq!(p.pins.len(), 48, "{}", p.board);
            for (i, pin) in p.pins.iter().enumerate() {
                assert_eq!(pin.lqfp as usize, i + 1, "{} {}", p.board, pin.name);
            }
            for id in p.ahb.iter().chain(p.apb2).chain(p.apb1).chain(p.used_blocks) {
                assert!(p.blocks.iter().any(|x| x.id == *id), "{} block {}", p.board, id);
            }
            for s in p.header_top.iter().chain(p.header_bottom).chain(p.header_swd) {
                assert!(mcu::silk_to_pin_in(p, s).is_some() || mcu::silk_ru(s).is_some(), "{} {}", p.board, s);
            }
            assert_eq!(by_idcode(p.idcode_sim), Some(b));
            assert!(p.clocks.issues().is_empty(), "{:?}", p.clocks.issues());
        }
        assert_eq!(by_idcode(0x2ba0_1477), None);
    }
}
