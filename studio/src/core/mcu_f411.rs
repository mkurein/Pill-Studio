//! STM32F411CEU6 and the WeAct Black Pill, in Russian.
//! Sources: DS10314 (datasheet) Table 8 "pin definitions" (UFQFPN48 column),
//! Table 9 "alternate function mapping", Figure 3 block diagram, Figure 15
//! memory map; RM0383 (reference manual) §6 RCC, §8 GPIO. Board: WeAct
//! "BlackPill" V3.x silkscreen (headers, KEY on PA0, LED on PC13, W25Q64 on PA4–PA7).
//! Project usage comes from F411CE Core/Src/main.c and stm32f4xx_hal_msp.c.
//! On F4 every I/O is FT except in analog mode; the 48-pin package has no PB11
//! (pin 22 is VCAP_1) and no GPIOD/E pins.

use super::boards::Family;
use super::mcu::{Block, Clocks, Pin, PinKind, Region, Usage};

const fn io(n: u8, name: &'static str, af: &'static str) -> Pin {
    Pin { lqfp: n, name, kind: PinKind::Io, ft: true, af, remap: "", note: "" }
}

pub static F411_PINS: &[Pin] = &[
    Pin { lqfp: 1, name: "VBAT", kind: PinKind::Power, ft: false, af: "", remap: "", note: "питание RTC и backup-регистров (на Black Pill соединён с 3.3 В, вывод VB)" },
    Pin { lqfp: 2, name: "PC13", kind: PinKind::Io, ft: true, af: "RTC_AF1", remap: "", note: "слаботочный вывод (≤ 3 мА); светодиод Black Pill, активный уровень 0" },
    Pin { lqfp: 3, name: "PC14", kind: PinKind::Io, ft: false, af: "OSC32_IN", remap: "", note: "слаботочный; кварц 32.768 кГц на плате" },
    Pin { lqfp: 4, name: "PC15", kind: PinKind::Io, ft: false, af: "OSC32_OUT", remap: "", note: "слаботочный; кварц 32.768 кГц на плате (в проекте — выход-метка кадров)" },
    Pin { lqfp: 5, name: "PH0", kind: PinKind::Io, ft: false, af: "OSC_IN", remap: "", note: "кварц 25 МГц (HSE)" },
    Pin { lqfp: 6, name: "PH1", kind: PinKind::Io, ft: false, af: "OSC_OUT", remap: "", note: "кварц 25 МГц (HSE)" },
    Pin { lqfp: 7, name: "NRST", kind: PinKind::Reset, ft: false, af: "", remap: "", note: "сброс, активный 0; кнопка NRST, вывод R на гребёнке" },
    Pin { lqfp: 8, name: "VSSA", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "аналоговая земля / VREF−" },
    Pin { lqfp: 9, name: "VDDA", kind: PinKind::Power, ft: false, af: "", remap: "", note: "аналоговое питание / VREF+ (АЦП, PLL, сброс)" },
    io(10, "PA0", "WKUP / TIM2_CH1_ETR / TIM5_CH1 / USART2_CTS / ADC1_IN0"),
    io(11, "PA1", "TIM2_CH2 / TIM5_CH2 / SPI4_MOSI / USART2_RTS / ADC1_IN1"),
    io(12, "PA2", "TIM2_CH3 / TIM5_CH3 / TIM9_CH1 / I2S2_CKIN / USART2_TX / ADC1_IN2"),
    io(13, "PA3", "TIM2_CH4 / TIM5_CH4 / TIM9_CH2 / I2S2_MCK / USART2_RX / ADC1_IN3"),
    io(14, "PA4", "SPI1_NSS / SPI3_NSS / I2S3_WS / USART2_CK / ADC1_IN4"),
    io(15, "PA5", "TIM2_CH1_ETR / SPI1_SCK / ADC1_IN5"),
    io(16, "PA6", "TIM1_BKIN / TIM3_CH1 / SPI1_MISO / I2S2_MCK / SDIO_CMD / ADC1_IN6"),
    io(17, "PA7", "TIM1_CH1N / TIM3_CH2 / SPI1_MOSI / ADC1_IN7"),
    io(18, "PB0", "TIM1_CH2N / TIM3_CH3 / SPI5_SCK / ADC1_IN8"),
    io(19, "PB1", "TIM1_CH3N / TIM3_CH4 / SPI5_NSS / ADC1_IN9"),
    Pin { lqfp: 20, name: "PB2", kind: PinKind::Boot, ft: true, af: "BOOT1", remap: "", note: "BOOT1 — на Black Pill через резистор на землю; читается только при сбросе" },
    io(21, "PB10", "TIM2_CH3 / I2C2_SCL / SPI2_SCK / I2S2_CK / I2S3_MCK / SDIO_D7"),
    Pin { lqfp: 22, name: "VCAP_1", kind: PinKind::Power, ft: false, af: "", remap: "", note: "выход внутреннего стабилизатора ядра 1.2 В, конденсатор 4.7 мкФ; PB11 в этом корпусе нет" },
    Pin { lqfp: 23, name: "VSS_1", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "" },
    Pin { lqfp: 24, name: "VDD_1", kind: PinKind::Power, ft: false, af: "", remap: "", note: "3.3 В" },
    io(25, "PB12", "TIM1_BKIN / SPI2_NSS / I2S2_WS / SPI4_NSS / SPI3_SCK / I2S3_CK"),
    io(26, "PB13", "TIM1_CH1N / SPI2_SCK / I2S2_CK / SPI4_SCK"),
    io(27, "PB14", "TIM1_CH2N / SPI2_MISO / I2S2ext_SD / SDIO_D6"),
    io(28, "PB15", "RTC_REFIN / TIM1_CH3N / SPI2_MOSI / I2S2_SD / SDIO_CK"),
    io(29, "PA8", "MCO_1 / TIM1_CH1 / I2C3_SCL / USART1_CK / USB_FS_SOF / SDIO_D1"),
    io(30, "PA9", "TIM1_CH2 / I2C3_SMBA / USART1_TX / USB_FS_VBUS / SDIO_D2"),
    io(31, "PA10", "TIM1_CH3 / SPI5_MOSI / USART1_RX / USB_FS_ID"),
    io(32, "PA11", "TIM1_CH4 / SPI4_MISO / USART1_CTS / USART6_TX / USB_FS_DM"),
    io(33, "PA12", "TIM1_ETR / SPI5_MISO / USART1_RTS / USART6_RX / USB_FS_DP"),
    io(34, "PA13", "JTMS / SWDIO"),
    Pin { lqfp: 35, name: "VSS_2", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "" },
    Pin { lqfp: 36, name: "VDD_2", kind: PinKind::Power, ft: false, af: "", remap: "", note: "3.3 В" },
    io(37, "PA14", "JTCK / SWCLK"),
    io(38, "PA15", "JTDI / TIM2_CH1_ETR / SPI1_NSS / SPI3_NSS / I2S3_WS / USART1_TX"),
    io(39, "PB3", "JTDO / SWO / TIM2_CH2 / SPI1_SCK / SPI3_SCK / I2S3_CK / USART1_RX / I2C2_SDA"),
    io(40, "PB4", "NJTRST / TIM3_CH1 / SPI1_MISO / SPI3_MISO / I2C3_SDA / SDIO_D0"),
    io(41, "PB5", "TIM3_CH2 / I2C1_SMBA / SPI1_MOSI / SPI3_MOSI / I2S3_SD / SDIO_D3"),
    io(42, "PB6", "TIM4_CH1 / I2C1_SCL / USART1_TX"),
    io(43, "PB7", "TIM4_CH2 / I2C1_SDA / USART1_RX / SDIO_D0"),
    Pin { lqfp: 44, name: "BOOT0", kind: PinKind::Boot, ft: false, af: "", remap: "", note: "кнопка BOOT0 на плате: держать при сбросе — системный загрузчик (USART1/USB DFU)" },
    io(45, "PB8", "TIM4_CH3 / TIM10_CH1 / I2C1_SCL / SPI5_MOSI / I2C3_SDA / SDIO_D4"),
    io(46, "PB9", "TIM4_CH4 / TIM11_CH1 / I2C1_SDA / SPI2_NSS / I2S2_WS / I2C2_SDA / SDIO_D5"),
    Pin { lqfp: 47, name: "VSS_3", kind: PinKind::Ground, ft: false, af: "", remap: "", note: "" },
    Pin { lqfp: 48, name: "VDD_3", kind: PinKind::Power, ft: false, af: "", remap: "", note: "3.3 В" },
];

pub fn f411_usage(name: &str) -> Option<Usage> {
    Some(match name {
        "PA9" => ("USART1_TX", "clk", "протокол → RX адаптера USB-UART №1, 38400 8N1 (AF7)"),
        "PA10" => ("USART1_RX", "clk", "протокол ← TX адаптера USB-UART №1; прерывание USART1 (prio 5)"),
        "PA11" => ("USART6_TX", "refc", "текстовый лог → RX адаптера USB-UART №2, 115200 (AF8); это же линия USB D−"),
        "PA12" => ("USART6_RX", "refc", "настроен, но не используется; линия USB D+"),
        "PA4" => ("SPI CS", "sig", "программный chip select памяти на SPI1, активный 0"),
        "PA5" => ("SPI1_SCK", "sig", "тактирование W25Q64, 96 МГц / 128 = 750 кГц"),
        "PA6" => ("SPI1_MISO", "sig", "данные памяти → MCU"),
        "PA7" => ("SPI1_MOSI", "sig", "данные MCU → памяти"),
        "PC13" => ("LED", "warn", "светодиод платы; 0 = горит; LedTask или команда 0x70"),
        "PC15" => ("ACT", "dig", "переключается на каждый принятый кадр (точка для осциллографа)"),
        "PA13" => ("SWDIO", "pwr", "отладка ST-Link — не переназначать"),
        "PA14" => ("SWCLK", "pwr", "отладка ST-Link — не переназначать"),
        "PA0" => ("KEY", "muted", "кнопка KEY на плате (замыкает на землю); прошивкой не используется"),
        "PH0" | "PH1" => ("HSE 25 МГц", "muted", "кварц → PLL /25 ×192 /2 = 96 МГц"),
        _ => return None,
    })
}

pub fn f411_block_svd(id: &str) -> Option<&'static str> {
    Some(match id {
        "FLASH" => "FLASH",
        "DMA" => "DMA1",
        "RCC" => "RCC",
        "GPIO" => "GPIOA",
        "USB" => "OTG_FS_GLOBAL",
        "EXTI" => "EXTI",
        "ADC" => "ADC1",
        "TIM1" => "TIM1",
        "TIM911" => "TIM9",
        "SPI1" => "SPI1",
        "SPI45" => "SPI4",
        "USART1" => "USART1",
        "USART6" => "USART6",
        "SDIO" => "SDIO",
        "TIM25" => "TIM2",
        "USART2" => "USART2",
        "SPI23" => "SPI2",
        "I2C" => "I2C1",
        "WDG" => "IWDG",
        "RTC" => "RTC",
        _ => return None,
    })
}

/// Peripherals of STM32F411CE (DS10314 Figure 3) with short Russian descriptions.
pub static F411_BLOCKS: &[Block] = &[
    Block { id: "CORE", title: "Cortex-M4F", bus: "core", text: "32-бит ядро ARMv7E-M до 100 МГц (здесь 96), FPU одинарной точности, DSP-инструкции, MPU. NVIC (62 прерывания, 16 уровней приоритета), SysTick — тик FreeRTOS 1 кГц. Порт FreeRTOS — ARM_CM4F (сохраняет регистры FPU при переключении задач)." },
    Block { id: "FLASH", title: "Flash 512 КБ", bus: "ahb", text: "0x0800 0000, сектора 4×16 + 64 + 3×128 КБ (стирание только секторами). Ускоритель ART (кэш инструкций и данных) позволяет работать на 96 МГц с 3 тактами ожидания (FLASH_LATENCY_3)." },
    Block { id: "SRAM", title: "SRAM 128 КБ", bus: "ahb", text: "0x2000 0000. В проекте занято ≈ 24 КБ: куча FreeRTOS 10 КБ, буфер сектора SPI-флеш 4 КБ, g_diag в .noinit. Запаса много — можно увеличить кучу и стеки." },
    Block { id: "DMA", title: "DMA1, DMA2", bus: "ahb", text: "Два контроллера по 8 потоков; DMA2 умеет память↔память. В проекте не используется (UART и SPI работают по прерываниям/опросом)." },
    Block { id: "GPIO", title: "GPIOA/B/C/H", bus: "ahb", text: "На F4 порты сидят прямо на AHB1 (быстрее, чем на F1). Каждый вывод: MODER (вход/выход/альт. функция/аналог), OTYPER (push-pull/open-drain), OSPEEDR (4 скорости), PUPDR (подтяжки), AFRL/AFRH — номер альт. функции AF0–AF15. ODR/IDR/BSRR как на F1." },
    Block { id: "RCC", title: "RCC, CRC", bus: "ahb", text: "Тактирование и сбросы: HSI 16 МГц, HSE 4–26 МГц (на плате 25 МГц), PLL с делителями M/N/P/Q, LSI 32 кГц, LSE 32.768 кГц. CSR хранит причину сброса. Рядом на AHB1 — аппаратный CRC-32 (протокол считает CRC-16 программно)." },
    Block { id: "USB", title: "USB OTG FS", bus: "ahb", text: "USB 2.0 Full Speed (device/host/OTG) на AHB2, PA11/PA12, нужен ровно 48 МГц (PLL Q). В проекте PA11 занят логом USART6, поэтому USB не используется." },
    Block { id: "EXTI", title: "SYSCFG, EXTI", bus: "apb2", text: "SYSCFG выбирает порт для линий EXTI и переключает отображение памяти; EXTI — 23 линии прерываний/событий (16 от выводов + PVD, RTC, USB wakeup)." },
    Block { id: "ADC", title: "ADC1", bus: "apb2", text: "Один 12-бит АЦП, до 2.4 MSPS, 10 внешних каналов (PA0–PA7, PB0, PB1) + датчик температуры, Vrefint, VBAT. Тактирование ≤ 36 МГц (APB2 / 4 = 24 МГц). Свободен." },
    Block { id: "TIM1", title: "TIM1 (advanced)", bus: "apb2", text: "16-бит таймер с комплементарными ШИМ-выходами. В проекте — системный тик HAL (HAL_IncTick каждую 1 мс, прерывание TIM1_UP_TIM10), потому что SysTick отдан FreeRTOS." },
    Block { id: "TIM911", title: "TIM9–TIM11", bus: "apb2", text: "16-бит таймеры на APB2 (96 МГц): TIM9 — 2 канала, TIM10/11 — по одному. Свободны." },
    Block { id: "SPI1", title: "SPI1", bus: "apb2", text: "SPI/I2S до 50 Мбит/с. В проекте — память на плате: master, режим 0, 8 бит, предделитель 128 (750 кГц), программный CS на PA4. На WeAct там распаяна W25Q64 (NOR-флеш 8 МБ), драйвер узнаёт её по JEDEC и пишет через стирание сектора 4 КБ." },
    Block { id: "SPI45", title: "SPI4, SPI5", bus: "apb2", text: "Ещё два SPI на APB2 (выводы PA1/PB12/PB13 и PB0/PB1/PA10/PA12/PB8). Свободны." },
    Block { id: "USART1", title: "USART1", bus: "apb2", text: "UART до 6 Мбит/с. В проекте — бинарный протокол 38400 8N1 на PA9/PA10 (AF7), приём по прерыванию по 1 байту в очередь FreeRTOS. Также используется системным загрузчиком." },
    Block { id: "USART6", title: "USART6", bus: "apb2", text: "UART на PA11/PA12 (AF8). В проекте — текстовый лог 115200, передача опросом, защищена мьютексом (на F103 эту роль играет USART3)." },
    Block { id: "SDIO", title: "SDIO", bus: "apb2", text: "Интерфейс SD-карт 4/8 бит, до 48 МГц. Свободен." },
    Block { id: "TIM25", title: "TIM2–TIM5", bus: "apb1", text: "Таймеры общего назначения: TIM2 и TIM5 32-битные, TIM3/TIM4 16-битные; 4 канала, ШИМ, энкодер. Тактирование 96 МГц (APB1 ×2). Свободны." },
    Block { id: "USART2", title: "USART2", bus: "apb1", text: "UART на PA2/PA3, до 3 Мбит/с. Свободен." },
    Block { id: "SPI23", title: "SPI2, SPI3 (I2S)", bus: "apb1", text: "SPI до 25 Мбит/с с режимом I2S для звука (I2S2ext/I2S3ext — полный дуплекс). Свободны." },
    Block { id: "I2C", title: "I2C1–I2C3", bus: "apb1", text: "I²C до 400 кГц с цифровым и аналоговым фильтром, SMBus. Свободны." },
    Block { id: "WDG", title: "IWDG, WWDG", bus: "apb1", text: "Сторожевые таймеры: IWDG от LSI 32 кГц, WWDG от APB1 с окном. В проекте не включены." },
    Block { id: "RTC", title: "RTC, PWR", bus: "apb1", text: "RTC с календарём, будильниками и 20 backup-регистрами (питание от VBAT), режимы Sleep/Stop/Standby, детектор PVD." },
];

/// DS10314 Figure 15 memory map (regions that exist in F411CE).
pub static F411_MEMORY: &[Region] = &[
    Region { start: 0x0000_0000, end: 0x0007_FFFF, name: "Алиас загрузки", text: "Отражение Flash, системной памяти или SRAM — зависит от BOOT0/BOOT1 (и SYSCFG_MEMRMP). Отсюда ядро берёт SP и вектор сброса." },
    Region { start: 0x0800_0000, end: 0x0807_FFFF, name: "Flash 512 КБ", text: "Код, таблица векторов, константы, начальные значения .data. Сектора 0–3 по 16 КБ, 4 — 64 КБ, 5–7 — по 128 КБ." },
    Region { start: 0x1FFF_0000, end: 0x1FFF_77FF, name: "System memory 30 КБ", text: "Загрузчик ST в ПЗУ: USART1/2, I2C, SPI, USB DFU при BOOT0 = 1." },
    Region { start: 0x1FFF_7800, end: 0x1FFF_7A0F, name: "OTP 528 Б", text: "Однократно программируемая область (16 блоков по 32 байта + байты блокировки)." },
    Region { start: 0x1FFF_C000, end: 0x1FFF_C00F, name: "Option bytes", text: "Защита от чтения (RDP), защита секторов от записи, BOR, настройки сторожа." },
    Region { start: 0x2000_0000, end: 0x2001_FFFF, name: "SRAM 128 КБ", text: "Переменные, куча FreeRTOS, стеки, g_diag (.noinit). Бит-бэнд-алиас: 0x2200 0000." },
    Region { start: 0x4000_0000, end: 0x4000_7FFF, name: "APB1", text: "TIM2–5, RTC, WWDG, IWDG, I2S2ext/3ext, SPI2/3, USART2, I2C1–3, PWR. До 50 МГц (здесь 48)." },
    Region { start: 0x4001_0000, end: 0x4001_57FF, name: "APB2", text: "TIM1, USART1/6, ADC1, SDIO, SPI1/4/5, SYSCFG, EXTI, TIM9–11. До 100 МГц (здесь 96)." },
    Region { start: 0x4002_0000, end: 0x4002_67FF, name: "AHB1", text: "GPIOA/B/C/H (0x4002 0000…), CRC, RCC (0x4002 3800), интерфейс Flash, DMA1/DMA2." },
    Region { start: 0x5000_0000, end: 0x5003_FFFF, name: "AHB2", text: "USB OTG FS." },
    Region { start: 0x4200_0000, end: 0x43FF_FFFF, name: "Бит-бэнд периферии", text: "Каждый бит регистра периферии — отдельное 32-бит слово (атомарные операции)." },
    Region { start: 0xE000_0000, end: 0xE00F_FFFF, name: "Cortex-M4 PPB", text: "NVIC, SysTick, SCB (CFSR/HFSR — причины сбоя), FPU (CPACR, FPCCR), MPU, DWT, ITM." },
];

/// SystemClock_Config of F411CE main.c: HSE 25 MHz, M = 25, N = 192, P = 2, Q = 4,
/// AHB /1, APB1 /2, APB2 /1, FLASH_LATENCY_3. ADC prescaler shown as /4 (ADC unused).
pub const F411_CLOCKS: Clocks = Clocks {
    family: Family::F4,
    hse: 25e6,
    pll_m: 25.0,
    pll_n: 192.0,
    pll_p: 2.0,
    pll_q: 4.0,
    ahb_div: 1.0,
    apb1_div: 2.0,
    apb2_div: 1.0,
    adc_div: 4.0,
    flash_latency: 3,
};
