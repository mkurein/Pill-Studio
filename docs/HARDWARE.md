# Hardware

Две платы, одна прошивка по смыслу и один протокол (ADR-016).
Этот репозиторий — Blue Pill; Black Pill — репозиторий
[F411CE](https://github.com/mkurein/F411CE).

## Boards

| | Blue Pill | Black Pill (WeAct v3.x) |
|---|---|---|
| MCU | STM32F103C8T6, LQFP48 | STM32F411CEU6, UFQFPN48 |
| Core | Cortex-M3, без FPU | Cortex-M4F, FPU fpv4-sp-d16 |
| Clock | HSE 8 МГц → PLL ×9 → 72 МГц | HSE 25 МГц → PLL M25 N192 P2 → 96 МГц |
| AHB / APB1 / APB2 | 72 / 36 / 72 МГц | 96 / 48 / 96 МГц |
| Flash wait states | 2 | 3 |
| Flash @ 0x08000000 | 64 КБ | 512 КБ |
| RAM @ 0x20000000 | 20 КБ | 128 КБ |
| Linker | `STM32F103C8TX_FLASH.ld` | `STM32F411CEUX_FLASH.ld` |
| SVD | `STM32F103.svd` | `STM32F411.svd` |
| DEV_ID / OpenOCD | 0x410 / `target/stm32f1x.cfg` | 0x431 / `target/stm32f4x.cfg` |
| GPIO | CRL/CRH (4 бита на вывод), AFIO remap | MODER/OTYPER/OSPEEDR/PUPDR, AFRL/AFRH |
| 5 В-толерантность | только выводы FT (DS5319 Table 5); PA0–PA7, PB0, PB1 — нет | все I/O FT, кроме аналогового режима; PB11 в корпусе 48 нет (VCAP_1) |
| Кнопки | RESET | RESET, BOOT0, KEY (PA0) |
| SPI-память | FRAM 32 КБ (внешняя) | W25Q64, 8 МБ NOR (распаяна на плате) |
| FreeRTOS | 10.0.1, порт ARM_CM3 | 10.3.1, порт ARM_CM4F |
| ELF | `build/primGPT.elf` | `build/primGPT_F411.elf` |

Пины: [PINOUT.md](PINOUT.md).

## Peripherals

| Periph | Blue Pill | Black Pill | Used by |
|--------|-----------|------------|---------|
| USART1 | 38400 8N1, RX IT, PA9/PA10 | то же, AF7 | command protocol |
| Log UART | USART3 115200, TX PB10 | USART6 115200, TX PA11 (AF8) | `log_printf` |
| SPI1 | master, mode 0, /16 = 4.5 МГц | master, mode 0, /128 = 750 кГц | `fram.c` |
| TIM1 | HAL timebase 1 кГц (TIM1_UP) | HAL timebase 1 кГц (TIM1_UP_TIM10) | `HAL_GetTick` |
| SysTick | FreeRTOS tick 1 кГц | FreeRTOS tick 1 кГц | kernel |

## SPI memory

`fram.c` один и тот же в обоих репозиториях: при старте читает JEDEC ID (0x9F).

- **FRAM** (Blue Pill): 32 КБ (`FRAM_MEM_SIZE` 0x8000), 16-битный адрес, запись
  без стирания. Opcodes: WREN 0x06, WRDI 0x04, RDSR 0x05, WRSR 0x01, READ 0x03,
  WRITE 0x02. Модель микросхемы в проекте не записана — см. TODO.
- **SPI NOR W25Q64** (Black Pill, JEDEC EF 40 17): 24-битный адрес, запись через
  чтение — стирание (0x20) — запись сектора 4 КБ из static-буфера. Включено
  `SPI_NOR_SUPPORT`, только на F4: на F103 нет 4 КБ свободной RAM.

Лог при старте: `FRAM probe=.. mem=FRAM|NOR jedec=..` ([PROTOCOL.md](PROTOCOL.md)).

## Debug / flash

- ST-Link V2 (или клон), SWD: PA13 SWDIO, PA14 SWCLK, GND, 3.3 В; NRST желателен.
- OpenOCD: из Homebrew / STM32CubeIDE; Pill Studio подставляет цель по плате.
- VS Code / Cursor: `.vscode/launch.json` (Cortex-Debug), «Attach to Pill Studio»
  — подключение к OpenOCD студии (ADR-015).
- Breakpoints: 6 аппаратных на F103 (FPB), 6 на F411.

## Build

- macOS / Linux / CI: `make -j8` в корне → `build/` (ADR-013), в обоих репозиториях.
- Windows / CubeIDE: `make -C Debug all -j8` (Makefile генерирует CubeIDE, `Ctrl+Shift+B`).
- Выход: `.elf`, `.bin`, `.hex`, `.map`.

Пошагово: [guides/QUICKSTART.md](guides/QUICKSTART.md).
