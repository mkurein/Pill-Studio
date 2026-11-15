# Project Index

Сначала карта, потом только нужные файлы. `tok` — токены Qwen3.8-27B
(замер `/tokenize`, 2026-09-22). Бюджет входа ~6000 из 8192.

## Core docs (read first)

| File | tok | What |
|------|------|------|
| [PROJECT.md](PROJECT.md) | 700 | что за проект, две платы, статус |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 740 | слои, модули, поток RX, ownership |
| [FREERTOS.md](FREERTOS.md) | 880 | задачи, очередь, NVIC, память |
| [PROTOCOL.md](PROTOCOL.md) | 750 | кадр UART1, CRC, команды, ответы |
| [HARDWARE.md](HARDWARE.md) · [PINOUT.md](PINOUT.md) | 1100 · 700 | Blue Pill и Black Pill: MCU, периферия, память, пины |
| [API.md](API.md) | 810 | сигнатуры функций модулей |
| [SAFETY.md](SAFETY.md) | 470 | аварийные состояния |
| [DECISIONS.md](DECISIONS.md) | 1900 | ADR-001..018 |
| [TODO.md](TODO.md) | 1000 | задачи T1..T11 |
| [TASK_TEMPLATE.md](TASK_TEMPLATE.md) | 320 | формат задачи для Qwen |
| [HIL_TESTING.md](HIL_TESTING.md) | ~900 | автотест на плате: `tools/hil.py`, подключение обеих плат |
| [../UART_TESTING.md](../UART_TESTING.md) | ~3500 | тесты UART1 в Pill Studio: свой кадр, 20 сбоев кадра и линии, как проводить |

## Source

| Area | Files | tok |
|------|-------|------|
| Tasks, init, commands | `Core/Src/main.c` (USER CODE 0, 2, 4, 5) | 5300 целиком — давать функциями |
| UART | `Core/Src/uart.c`, `Core/Inc/uart.h` | ~1500 |
| Diagnostics / faults | `Core/Src/diag.c`, `Core/Inc/diag.h` | ~1300 |
| FRAM | `Core/Src/fram.c`, `Core/Inc/fram.h` | 1500 |
| CRC | `Core/Src/crc16.c`, `Core/Inc/crc16.h` | 570 |
| Constants | `Core/Inc/DataFile.h` | 85 |
| RTOS config | `Core/Inc/FreeRTOSConfig.h` (нужны строки 55–157) | 1560 |
| Pin mux, NVIC | `Core/Src/stm32f1xx_hal_msp.c` | 1840 |
| IRQ handlers | `Core/Src/stm32f1xx_it.c` | 1190 |
| CubeMX | `primGPT.ioc` | 2250 |

Не грузить: `Drivers/`, `Middlewares/`, `Debug/`, `STM32F103.svd`, `STM32F411.svd`, `.metadata`.

Black Pill (`../F411CE`): те же модули, отличаются `main.c` (тактирование, USART6),
`stm32f4xx_hal_msp.c`, `stm32f4xx_it.c`, `FreeRTOSConfig.h`, линкер `STM32F411CEUX_FLASH.ld`.

| Studio (`studio/src/`) | Files |
|------|-------|
| профиль платы (всё, что зависит от платы) | `core/boards.rs` |
| выводы, блоки, память, такты | `core/mcu.rs` (F103), `core/mcu_f411.rs` (F411) |
| протокол, STATUS, виртуальная плата | `core/proto.rs`, `core/sim.rs` |
| SWD, ELF, SVD | `core/openocd.rs`, `core/elf.rs`, `core/svd.rs` |

## Context sets (§10 Context on Demand)

| Task | Load |
|------|------|
| команда UART1 | PROTOCOL, ARCHITECTURE, `Uart1Task` из main.c, DataFile.h, crc16.h |
| приём / ISR | ARCHITECTURE, FREERTOS, uart.c, `Uart1Task` |
| FRAM | HARDWARE, API (fram), fram.c, fram.h |
| задачи / стек / heap | FREERTOS, `main()` + task bodies, FreeRTOSConfig.h (фрагмент) |
| пины / периферия | PINOUT, HARDWARE, msp.c (фрагмент), `MX_*_Init` |
| другая плата / перенос на F411 | HARDWARE, PINOUT, HIL_TESTING (§ Две платы), ADR-016 |
| Pill Studio | `studio/README.md`, ADR-014..016, `core/boards.rs` |
| CubeMX regen | DECISIONS (ADR-005), primGPT.ioc, msp.c |

Поиск: `rg "Uart1Task" Core/`, `rg "UART_TIMEOUT_MS" .`

## Human docs (RU, long — not for LLM context)

От 1400 до 10000 tok каждый (DEBUG_GUIDE ≈ 10000 > всего окна).

| Folder | Content |
|--------|---------|
| [guides/](guides/) | QUICKSTART, VS Code (macOS / Windows), Cursor + OpenOCD, DEBUG_GUIDE, RTOS views |
| [notes/](notes/) | BOOT_ALGORITHM, README_UART (таймаут), README_RAM_RTOS (память), README_INTERRUPTS |
| [lessons/](lessons/) | мини-уроки 01–06, PROJECT_FIRMWARE_LESSON |
| [`tools/RUN.md`](../tools/RUN.md) | хост `uart_check.py` / `.sh` из PowerShell |
| [`studio/`](../studio/README.md) | Pill Studio — GUI (Rust egui) для Blue Pill и Black Pill: плата, MCU, регистры по SWD, UART-протокол, RTOS-монитор, разбор сбоев, сборка/прошивка/автотест; ADR-014, ADR-016 |
| [`studio/BUILD.md`](../studio/BUILD.md) | команды сборки студии: Windows (`setup.exe`) и macOS (`.dmg`) |

Если модели нужен кусок оттуда — давать раздел, не файл целиком.
