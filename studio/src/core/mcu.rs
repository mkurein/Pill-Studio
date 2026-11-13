//! Static knowledge about the two boards, in Russian.
//! STM32F103C8T6 / Blue Pill: DS5319 Table 5 "pin definitions", Figure 1 block
//! diagram, Figure 11 memory map; RM0008 §7 RCC, §9 GPIO/AFIO.
//! STM32F411CEU6 / Black Pill: see `mcu_f411.rs`.
//! Project usage comes from Core/Src/main.c and *_hal_msp.c of each firmware.
//! The functions at the top answer for the board selected in `boards::cur()`.

use super::boards::{self, Family, Profile};
pub use super::mcu_f411::{f411_block_svd, f411_usage, F411_BLOCKS, F411_CLOCKS, F411_MEMORY, F411_PINS};

/// What this firmware does with a pin: (function, colour token, explanation).
pub type Usage = (&'static str, &'static str, &'static str);

pub fn pins() -> &'static [Pin] {
    boards::cur().pins
}
pub fn pin(name: &str) -> Option<&'static Pin> {
    pins().iter().find(|p| p.name == name)
}
pub fn usage(name: &str) -> Option<Usage> {
    (boards::cur().usage)(name)
}
pub fn blocks() -> &'static [Block] {
    boards::cur().blocks
}
pub fn memory() -> &'static [Region] {
    boards::cur().memory
}
pub fn clocks() -> Clocks {
    boards::cur().clocks
}
/// Silkscreen label → MCU pin name on the current board.
pub fn silk_to_pin(s: &str) -> Option<&'static str> {
    silk_to_pin_in(boards::cur(), s)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PinKind {
    Io,
    Power,
    Ground,
    Reset,
    Boot,
}

pub struct Pin {
    /// LQFP48 pin number (0 for board-only pins such as 5V).
    pub lqfp: u8,
    pub name: &'static str,
    pub kind: PinKind,
    /// 5 V tolerant (FT in DS5319 Table 5).
    pub ft: bool,
    /// Default alternate functions.
    pub af: &'static str,
    /// Functions after AFIO remap.
    pub remap: &'static str,
    pub note: &'static str,
}

const fn io(lqfp: u8, name: &'static str, ft: bool, af: &'static str, remap: &'static str) -> Pin {
    Pin { lqfp, name, kind: PinKind::Io, ft, af, remap, note: "" }
}

pub static F103_PINS: &[Pin] = &[
    Pin { lqfp: 1, name: "VBAT", kind: PinKind::Power, ft: false, af: "", remap: "", note: "питание RTC и backup-регистров (на Blue Pill соединён с 3.3 В)" },
    Pin { lqfp: 2, name: "PC13", kind: PinKind::Io, ft: false, af: "TAMPER-RTC", remap: "", note: "слаботочный вывод (≤ 3 мА, ≤ 2 МГц); светодиод Blue Pill, активный уровень 0" },
    Pin { lqfp: 3, name: "PC14", kind: PinKind::Io, ft: false, af: "OSC32_IN", remap: "", note: "слаботочный; кварц 32.768 кГц" },
    Pin { lqfp: 4, name: "PC15", kind: PinKind::Io, ft: false, af: "OSC32_OUT", remap: "", note: "слаботочный; кварц 32.768 кГц" },
    Pin { lqfp: 5, name: "PD0", kind: PinKind::Io, ft: false, af: "OSC_IN", remap: "", note: "кварц 8 МГц (HSE)" },
    Pin { lqfp: 6, name: "PD1", kind: PinKind::Io, ft: false, af: "OSC_OUT", remap: "", note: "кварц 8 МГц (HSE)" },
    Pin { lqfp: 7, name: "NRST", kind: PinKind::Reset, ft: false, af: "", remap: "", note: "сброс, активный 0; кнопка RESET, вывод R на гребёнке" },
    Pin { lqfp: 8, name: "VSSA", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "аналоговая земля" },
    Pin { lqfp: 9, name: "VDDA", kind: PinKind::Power, ft: false, af: "", remap: "", note: "аналоговое питание (АЦП, PLL, сброс)" },
    io(10, "PA0", false, "WKUP / USART2_CTS / ADC12_IN0 / TIM2_CH1_ETR", ""),
    io(11, "PA1", false, "USART2_RTS / ADC12_IN1 / TIM2_CH2", ""),
    io(12, "PA2", false, "USART2_TX / ADC12_IN2 / TIM2_CH3", ""),
    io(13, "PA3", false, "USART2_RX / ADC12_IN3 / TIM2_CH4", ""),
    io(14, "PA4", false, "SPI1_NSS / USART2_CK / ADC12_IN4", ""),
    io(15, "PA5", false, "SPI1_SCK / ADC12_IN5", ""),
    io(16, "PA6", false, "SPI1_MISO / ADC12_IN6 / TIM3_CH1", "TIM1_BKIN"),
    io(17, "PA7", false, "SPI1_MOSI / ADC12_IN7 / TIM3_CH2", "TIM1_CH1N"),
    io(18, "PB0", false, "ADC12_IN8 / TIM3_CH3", "TIM1_CH2N"),
    io(19, "PB1", false, "ADC12_IN9 / TIM3_CH4", "TIM1_CH3N"),
    Pin { lqfp: 20, name: "PB2", kind: PinKind::Boot, ft: true, af: "BOOT1", remap: "", note: "BOOT1 — перемычка на плате; читается только при сбросе" },
    io(21, "PB10", true, "I2C2_SCL / USART3_TX", "TIM2_CH3"),
    io(22, "PB11", true, "I2C2_SDA / USART3_RX", "TIM2_CH4"),
    Pin { lqfp: 23, name: "VSS_1", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "" },
    Pin { lqfp: 24, name: "VDD_1", kind: PinKind::Power, ft: false, af: "", remap: "", note: "3.3 В" },
    io(25, "PB12", true, "SPI2_NSS / I2C2_SMBA / USART3_CK / TIM1_BKIN", ""),
    io(26, "PB13", true, "SPI2_SCK / USART3_CTS / TIM1_CH1N", ""),
    io(27, "PB14", true, "SPI2_MISO / USART3_RTS / TIM1_CH2N", ""),
    io(28, "PB15", true, "SPI2_MOSI / TIM1_CH3N", ""),
    io(29, "PA8", true, "USART1_CK / TIM1_CH1 / MCO", ""),
    io(30, "PA9", true, "USART1_TX / TIM1_CH2", ""),
    io(31, "PA10", true, "USART1_RX / TIM1_CH3", ""),
    io(32, "PA11", true, "USART1_CTS / CAN_RX / USB_DM / TIM1_CH4", ""),
    io(33, "PA12", true, "USART1_RTS / CAN_TX / USB_DP / TIM1_ETR", ""),
    io(34, "PA13", true, "JTMS / SWDIO", ""),
    Pin { lqfp: 35, name: "VSS_2", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "" },
    Pin { lqfp: 36, name: "VDD_2", kind: PinKind::Power, ft: false, af: "", remap: "", note: "3.3 В" },
    io(37, "PA14", true, "JTCK / SWCLK", ""),
    io(38, "PA15", true, "JTDI", "TIM2_CH1_ETR / SPI1_NSS"),
    io(39, "PB3", true, "JTDO / TRACESWO", "TIM2_CH2 / SPI1_SCK"),
    io(40, "PB4", true, "NJTRST", "TIM3_CH1 / SPI1_MISO"),
    io(41, "PB5", false, "I2C1_SMBA", "TIM3_CH2 / SPI1_MOSI"),
    io(42, "PB6", true, "I2C1_SCL / TIM4_CH1", "USART1_TX"),
    io(43, "PB7", true, "I2C1_SDA / TIM4_CH2", "USART1_RX"),
    Pin { lqfp: 44, name: "BOOT0", kind: PinKind::Boot, ft: false, af: "", remap: "", note: "0 — старт из Flash, 1 — системный загрузчик (UART1)" },
    io(45, "PB8", true, "TIM4_CH3", "I2C1_SCL / CAN_RX"),
    io(46, "PB9", true, "TIM4_CH4", "I2C1_SDA / CAN_TX"),
    Pin { lqfp: 47, name: "VSS_3", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "" },
    Pin { lqfp: 48, name: "VDD_3", kind: PinKind::Power, ft: false, af: "", remap: "", note: "3.3 В" },
];

pub fn f103_usage(name: &str) -> Option<Usage> {
    Some(match name {
        "PA9" => ("USART1_TX", "clk", "протокол → RX адаптера USB-UART №1, 38400 8N1"),
        "PA10" => ("USART1_RX", "clk", "протокол ← TX адаптера USB-UART №1; прерывание USART1 (prio 5)"),
        "PB10" => ("USART3_TX", "refc", "текстовый лог → RX адаптера USB-UART №2, 115200"),
        "PB11" => ("USART3_RX", "refc", "настроен, но не используется"),
        "PA4" => ("FRAM CS", "sig", "программный chip select FRAM (ADR-008), активный 0"),
        "PA5" => ("SPI1_SCK", "sig", "тактирование FRAM, 72 МГц / 16 = 4.5 МГц"),
        "PA6" => ("SPI1_MISO", "sig", "данные FRAM → MCU"),
        "PA7" => ("SPI1_MOSI", "sig", "данные MCU → FRAM"),
        "PC13" => ("LED", "warn", "светодиод платы; 0 = горит; LedTask или команда 0x70"),
        "PC15" => ("ACT", "dig", "переключается на каждый принятый кадр (точка для осциллографа)"),
        "PA13" => ("SWDIO", "pwr", "отладка ST-Link — не переназначать"),
        "PA14" => ("SWCLK", "pwr", "отладка ST-Link — не переназначать"),
        "PD0" | "PD1" => ("HSE 8 МГц", "muted", "кварц → PLL ×9 = 72 МГц"),
        _ => return None,
    })
}

pub fn silk_to_pin_in(b: &Profile, s: &str) -> Option<&'static str> {
    Some(match s {
        "R" => "NRST",
        "VB" => "VBAT",
        "DIO" => "PA13",
        "CLK" => "PA14",
        "GND" | "3.3" | "5V" => return None,
        _ => {
            let full = format!("P{}", s);
            return b.pins.iter().find(|p| p.name == full).map(|p| p.name);
        }
    })
}

pub fn silk_ru(s: &str) -> Option<&'static str> {
    Some(match s {
        "GND" => "земля",
        "3.3" => "3.3 В от стабилизатора платы (≈ 100–300 мА на всё)",
        "5V" => "5 В от USB; не подавать на выводы без FT",
        _ => return None,
    })
}

pub struct Block {
    pub id: &'static str,
    pub title: &'static str,
    pub bus: &'static str,
    pub text: &'static str,
}

/// Peripherals of STM32F103C8 (DS5319 Figure 1) with short Russian descriptions.
pub static F103_BLOCKS: &[Block] = &[
    Block { id: "CORE", title: "Cortex-M3", bus: "core", text: "32-бит ядро ARMv7-M, 72 МГц, 1.25 DMIPS/МГц, Thumb-2, аппаратное деление, 3-ступенчатый конвейер. NVIC (43 прерывания, 16 уровней приоритета, 4 бита), SysTick (у нас — тик FreeRTOS 1 кГц), MPU нет." },
    Block { id: "FLASH", title: "Flash 64 КБ", bus: "ahb", text: "Программа и константы, адрес 0x0800 0000, страницы по 1 КБ. На 72 МГц нужно 2 такта ожидания (FLASH_LATENCY_2) и буфер предвыборки." },
    Block { id: "SRAM", title: "SRAM 20 КБ", bus: "ahb", text: "0x2000 0000. В проекте: .data+.bss ≈ 16 КБ, из них куча FreeRTOS 10 КБ (стеки задач, очередь, мьютекс), стек main/прерываний 1 КБ." },
    Block { id: "DMA1", title: "DMA1 · 7 каналов", bus: "ahb", text: "Пересылки память↔периферия без ядра. В проекте не используется (UART и SPI работают по прерываниям/опросом)." },
    Block { id: "RCC", title: "RCC", bus: "ahb", text: "Тактирование и сбросы: HSI 8 МГц, HSE 4–16 МГц (8 МГц кварц), PLL, LSI 40 кГц, LSE 32.768 кГц. Включение тактирования каждой периферии (ENR-регистры). CSR хранит причину сброса." },
    Block { id: "CRC", title: "CRC", bus: "ahb", text: "Аппаратный CRC-32 (полином Ethernet). Протокол проекта использует программный CRC-16/MODBUS, поэтому блок не задействован." },
    Block { id: "GPIO", title: "GPIOA/B/C/D", bus: "apb2", text: "Порты ввода-вывода. Каждый вывод: вход (аналог/плавающий/подтяжка) или выход (push-pull/open-drain, 2/10/50 МГц), настраивается 4 битами в CRL/CRH. ODR — выход, IDR — вход, BSRR/BRR — атомарная установка/сброс." },
    Block { id: "AFIO", title: "AFIO", bus: "apb2", text: "Переназначение (remap) альтернативных функций и выбор порта для линий EXTI. Здесь же отключение JTAG с сохранением SWD (SWJ_CFG)." },
    Block { id: "EXTI", title: "EXTI", bus: "apb2", text: "19 линий внешних прерываний/событий: 16 от выводов GPIO + PVD, RTC alarm, USB wakeup. Фронт/спад настраивается." },
    Block { id: "ADC", title: "ADC1, ADC2", bus: "apb2", text: "Два 12-бит АЦП, 1 мкс, 10 внешних каналов (PA0–PA7, PB0, PB1) + датчик температуры и Vrefint. Тактирование ≤ 14 МГц (72/6 = 12 МГц)." },
    Block { id: "TIM1", title: "TIM1 (advanced)", bus: "apb2", text: "16-бит таймер с комплементарными ШИМ-выходами и мёртвым временем (моторы). В проекте — системный тик HAL (HAL_IncTick каждую 1 мс), потому что SysTick отдан FreeRTOS." },
    Block { id: "SPI1", title: "SPI1", bus: "apb2", text: "SPI до 18 Мбит/с. В проекте — FRAM: master, режим 0, 8 бит, MSB first, предделитель 16 (4.5 МГц), программный NSS на PA4." },
    Block { id: "USART1", title: "USART1", bus: "apb2", text: "UART до 4.5 Мбит/с. В проекте — бинарный протокол 38400 8N1, приём по прерыванию по 1 байту в очередь FreeRTOS. Также используется системным загрузчиком (BOOT0 = 1)." },
    Block { id: "TIM234", title: "TIM2, TIM3, TIM4", bus: "apb1", text: "16-бит таймеры общего назначения: 4 канала захвата/сравнения, ШИМ, энкодер. Тактирование 72 МГц (APB1 ×2). Свободны." },
    Block { id: "USART23", title: "USART2, USART3", bus: "apb1", text: "UART до 2.25 Мбит/с. USART3 (PB10) — текстовый лог 115200, передача опросом, защищена мьютексом. USART2 свободен." },
    Block { id: "SPI2", title: "SPI2", bus: "apb1", text: "Второй SPI (PB12–PB15), до 18 Мбит/с. Свободен." },
    Block { id: "I2C", title: "I2C1, I2C2", bus: "apb1", text: "I²C до 400 кГц, SMBus/PMBus. Свободны. Известны ошибки кремния — см. errata ES096." },
    Block { id: "USB", title: "USB FS", bus: "apb1", text: "USB 2.0 Full Speed device, 12 Мбит/с, PA11/PA12, нужен 48 МГц (72/1.5). Делит 512 Б SRAM с CAN. На Blue Pill подтяжка D+ часто 10 кОм вместо 1.5 кОм." },
    Block { id: "CAN", title: "bxCAN", bus: "apb1", text: "CAN 2.0B до 1 Мбит/с. Нельзя одновременно с USB (общая память)." },
    Block { id: "WDG", title: "IWDG, WWDG", bus: "apb1", text: "Сторожевые таймеры: IWDG от LSI 40 кГц, WWDG от APB1 с окном. В проекте не включены — после сбоя MCU остаётся в цикле до ручного сброса." },
    Block { id: "RTC", title: "RTC, BKP, PWR", bus: "apb1", text: "RTC с питанием от VBAT, 10 backup-регистров по 16 бит, режимы пониженного потребления (Sleep, Stop, Standby), детектор напряжения PVD." },
];

pub struct Region {
    pub start: u32,
    pub end: u32,
    pub name: &'static str,
    pub text: &'static str,
}

/// DS5319 Figure 11 memory map (parts that exist in C8).
pub static F103_MEMORY: &[Region] = &[
    Region { start: 0x0000_0000, end: 0x0000_FFFF, name: "Алиас загрузки", text: "Отражение Flash или системной памяти — зависит от BOOT0/BOOT1. Отсюда ядро берёт SP и вектор сброса." },
    Region { start: 0x0800_0000, end: 0x0800_FFFF, name: "Flash 64 КБ", text: "Код программы, таблица векторов, константы, начальные значения .data." },
    Region { start: 0x1FFF_F000, end: 0x1FFF_F7FF, name: "System memory 2 КБ", text: "Загрузчик ST в ПЗУ (прошивка по USART1 при BOOT0 = 1)." },
    Region { start: 0x1FFF_F800, end: 0x1FFF_F80F, name: "Option bytes", text: "Защита от чтения/записи, настройки сторожа и сброса." },
    Region { start: 0x2000_0000, end: 0x2000_4FFF, name: "SRAM 20 КБ", text: "Переменные, куча FreeRTOS, стеки. Бит-бэнд-алиас: 0x2200 0000." },
    Region { start: 0x4000_0000, end: 0x4000_7FFF, name: "APB1", text: "TIM2–4, RTC, WWDG, IWDG, SPI2, USART2/3, I2C1/2, USB, CAN, BKP, PWR. До 36 МГц." },
    Region { start: 0x4001_0000, end: 0x4001_3FFF, name: "APB2", text: "AFIO, EXTI, GPIOA–D, ADC1/2, TIM1, SPI1, USART1. До 72 МГц." },
    Region { start: 0x4002_0000, end: 0x4002_33FF, name: "AHB", text: "DMA1, RCC, интерфейс Flash, CRC." },
    Region { start: 0x4200_0000, end: 0x43FF_FFFF, name: "Бит-бэнд периферии", text: "Каждый бит регистра периферии — отдельное 32-бит слово (атомарные операции)." },
    Region { start: 0xE000_0000, end: 0xE00F_FFFF, name: "Cortex-M3 PPB", text: "NVIC, SysTick, SCB (CFSR/HFSR — причины сбоя), DWT, ITM, отладка." },
];

pub fn f103_block_svd(id: &str) -> Option<&'static str> {
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

/// Clock settings of a project (SystemClock_Config in main.c).
/// VCO = HSE / M × N, SYSCLK = VCO / P, USB = VCO / Q.
/// F1 has no M/P dividers in this sense: M = 1, N = PLLMUL, P = 1, Q = USB prescaler 1.5.
#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    pub family: Family,
    pub hse: f64,
    pub pll_m: f64,
    pub pll_n: f64,
    pub pll_p: f64,
    pub pll_q: f64,
    pub ahb_div: f64,
    pub apb1_div: f64,
    pub apb2_div: f64,
    pub adc_div: f64,
    pub flash_latency: u32,
}

pub const F103_CLOCKS: Clocks = Clocks {
    family: Family::F1,
    hse: 8e6,
    pll_m: 1.0,
    pll_n: 9.0,
    pll_p: 1.0,
    pll_q: 1.5,
    ahb_div: 1.0,
    apb1_div: 2.0,
    apb2_div: 1.0,
    adc_div: 6.0,
    flash_latency: 2,
};

/// (SYSCLK, APB1, APB2, ADC) limits of the family.
fn limits(f: Family) -> (f64, f64, f64, f64) {
    match f {
        Family::F1 => (72e6, 36e6, 72e6, 14e6),
        Family::F4 => (100e6, 50e6, 100e6, 36e6),
    }
}

impl Clocks {
    pub fn vco(&self) -> f64 {
        self.hse / self.pll_m * self.pll_n
    }
    pub fn sysclk(&self) -> f64 {
        self.vco() / self.pll_p
    }
    pub fn hclk(&self) -> f64 {
        self.sysclk() / self.ahb_div
    }
    pub fn pclk1(&self) -> f64 {
        self.hclk() / self.apb1_div
    }
    pub fn pclk2(&self) -> f64 {
        self.hclk() / self.apb2_div
    }
    /// Timers on APB with prescaler ≠ 1 run at 2× PCLK (RM0008 §7.2, RM0383 §6.2).
    pub fn tim_apb1(&self) -> f64 {
        if self.apb1_div > 1.0 {
            self.pclk1() * 2.0
        } else {
            self.pclk1()
        }
    }
    pub fn adc(&self) -> f64 {
        self.pclk2() / self.adc_div
    }
    pub fn usb(&self) -> f64 {
        self.vco() / self.pll_q
    }
    /// Datasheet limits violated by this configuration (Russian).
    pub fn issues(&self) -> Vec<String> {
        let (sys, apb1, apb2, adc) = limits(self.family);
        let mut v = vec![];
        if self.sysclk() > sys {
            v.push(format!("SYSCLK выше {} МГц", sys / 1e6));
        }
        if self.pclk1() > apb1 {
            v.push(format!("APB1 выше {} МГц", apb1 / 1e6));
        }
        if self.pclk2() > apb2 {
            v.push(format!("APB2 выше {} МГц", apb2 / 1e6));
        }
        if self.adc() > adc {
            v.push(format!("ADCCLK выше {} МГц", adc / 1e6));
        }
        if (self.usb() - 48e6).abs() > 1e3 {
            v.push("USB не 48 МГц".into());
        }
        if self.family == Family::F4 {
            let vin = self.hse / self.pll_m;
            if !(1e6..=2e6).contains(&vin) {
                v.push("вход PLL (HSE/M) вне 1–2 МГц".into());
            }
            if !(100e6..=432e6).contains(&self.vco()) {
                v.push("VCO вне 100–432 МГц".into());
            }
        }
        v
    }
    /// USART baud error (RM0008 §27.3.4, RM0383 §19.3.4, OVER8 = 0): USARTDIV =
    /// PCLK / (16·baud) with a 4-bit fraction; BRR = USARTDIV·16.
    pub fn uart_error(pclk: f64, baud: f64) -> (u32, f64) {
        let div = (pclk / (16.0 * baud) * 16.0).round() / 16.0;
        let brr = (div * 16.0).round() as u32;
        let real = pclk / (16.0 * div);
        (brr, (real - baud) / baud * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_tree_f103() {
        let c = F103_CLOCKS;
        assert_eq!(c.sysclk(), 72e6);
        assert_eq!(c.pclk1(), 36e6);
        assert_eq!(c.tim_apb1(), 72e6);
        assert_eq!(c.usb(), 48e6);
        assert!(c.issues().is_empty());
        // USART1 at 72 MHz, 38400: BRR 0x753, error ≈ 0
        let (brr, err) = Clocks::uart_error(72e6, 38400.0);
        assert_eq!(brr, 0x753);
        assert!(err.abs() < 0.01);
    }

    #[test]
    fn clock_tree_f411() {
        let c = F411_CLOCKS;
        assert_eq!(c.sysclk(), 96e6);
        assert_eq!(c.pclk1(), 48e6);
        assert_eq!(c.pclk2(), 96e6);
        assert_eq!(c.usb(), 48e6);
        assert!(c.issues().is_empty(), "{:?}", c.issues());
        // USART1 at 96 MHz, 38400: USARTDIV 156.25 → BRR 0x9C4; USART6 115200 → 0x341
        assert_eq!(Clocks::uart_error(96e6, 38400.0).0, 0x9C4);
        assert_eq!(Clocks::uart_error(96e6, 115200.0).0, 0x341);
    }
}

/// Live state of one pin decoded from the GPIO registers read over SWD.
#[derive(Clone, Debug, PartialEq)]
pub struct PinLive {
    /// Driven by the output register (output or alternate function), not an input.
    pub output: bool,
    pub mode: String,
    pub idr: u32,
    pub odr: u32,
}

/// GPIO ports of the current part: (letter, base).
pub fn gpio_ports() -> &'static [(&'static str, u32)] {
    match boards::cur().family {
        Family::F1 => &[("A", 0x4001_0800), ("B", 0x4001_0C00), ("C", 0x4001_1000)],
        Family::F4 => &[("A", 0x4002_0000), ("B", 0x4002_0400), ("C", 0x4002_0800), ("H", 0x4002_1C00)],
    }
}

/// Register offsets to read for `gpio_live`: F1 CRL CRH IDR ODR; F4 MODER OTYPER OSPEEDR PUPDR IDR ODR AFRL AFRH.
fn gpio_offsets(f: Family) -> &'static [u32] {
    match f {
        Family::F1 => &[0x00, 0x04, 0x08, 0x0C],
        Family::F4 => &[0x00, 0x04, 0x08, 0x0C, 0x10, 0x14, 0x20, 0x24],
    }
}

/// Addresses the board view polls once a second.
pub fn gpio_poll_addrs() -> Vec<u32> {
    let f = boards::cur().family;
    gpio_ports().iter().flat_map(|(_, b)| gpio_offsets(f).iter().map(move |o| b + o)).collect()
}

pub fn gpio_live(read: impl Fn(u32) -> Option<u32>, pin: &str) -> Option<PinLive> {
    let port = pin.get(1..2)?;
    let n: u32 = pin.get(2..)?.parse().ok()?;
    let base = gpio_ports().iter().find(|(p, _)| *p == port)?.1;
    match boards::cur().family {
        Family::F1 => {
            let cr = read(base + if n < 8 { 0 } else { 4 })?;
            let nib = (cr >> ((n % 8) * 4)) & 0xF;
            let (mode, cnf) = (nib & 3, nib >> 2);
            let text = format!("{} {}", super::svd::gpio_mode_ru(mode, cnf), super::svd::gpio_speed_ru(mode));
            Some(PinLive { output: mode != 0, mode: text.trim().to_string(), idr: (read(base + 8)? >> n) & 1, odr: (read(base + 12)? >> n) & 1 })
        }
        Family::F4 => {
            let f2 = |off: u32| read(base + off).map(|v| (v >> (n * 2)) & 3);
            let moder = f2(0x00)?;
            let otype = if (read(base + 0x04)? >> n) & 1 == 1 { "open-drain" } else { "push-pull" };
            let speed = super::svd::gpio_f4_ru("OSPEEDR", f2(0x08)?);
            let pupd = super::svd::gpio_f4_ru("PUPDR", f2(0x0C)?);
            let text = match moder {
                0 => format!("вход, {}", pupd),
                1 => format!("выход {}, {}", otype, speed),
                2 => {
                    let afr = read(base + if n < 8 { 0x20 } else { 0x24 })?;
                    format!("альт. функция AF{} {}, {}", (afr >> ((n % 8) * 4)) & 0xF, otype, speed)
                }
                _ => "аналоговый".to_string(),
            };
            Some(PinLive {
                output: moder == 1 || moder == 2,
                mode: text,
                idr: (read(base + 0x10)? >> n) & 1,
                odr: (read(base + 0x14)? >> n) & 1,
            })
        }
    }
}
