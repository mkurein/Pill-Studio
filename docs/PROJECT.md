# primGPT + Pill Studio — две платы STM32

## Goal

Учебно-пробный firmware: принимать бинарные команды по USART1 (CRC16),
управлять GPIO и внешней SPI-памятью, писать диагностику RTOS в лог-UART,
записывать сбои так, чтобы агент сам находил их на железе.
Полигон для приёмов, которые потом пойдут в большие проекты
(теплица на H743ZI2 и др.). Pill Studio — GUI для отладки и обучения.

## Hardware

| | Blue Pill (этот репозиторий) | Black Pill ([F411CE](https://github.com/mkurein/F411CE)) |
|---|---|---|
| MCU | STM32F103C8T6, Cortex-M3, 72 МГц | STM32F411CEU6, Cortex-M4F, 96 МГц |
| Flash / RAM | 64 КБ / 20 КБ | 512 КБ / 128 КБ |
| SPI-память | FRAM 32 КБ | W25Q64 (SPI NOR 8 МБ) |
| Лог | USART3, PB10 | USART6, PA11 |

Debug: ST-Link V2, SWD, OpenOCD. Подробно: [HARDWARE.md](HARDWARE.md), [PINOUT.md](PINOUT.md).

## Software

- STM32 HAL (CubeMX-generated init), C11
- FreeRTOS 10.0.1 (F103) / 10.3.1 (F411), CMSIS-RTOS v2 wrapper, heap_4 10 КБ
- Toolchain: arm-none-eabi-gcc 13.3 (CubeIDE 1.17.0 или Homebrew), newlib-nano
- `make` в корне → `build/` (ADR-013); `tools/hil.py` — автотест на плате (общий)
- Pill Studio: `studio/`, Rust + egui, macOS и Windows (ADR-014, ADR-016)
- Editors: Cursor / VS Code (Cortex-Debug), Claude Code, OpenCode + Qwen3.8-27B

## Communication

| Link  | Baud   | Role                              |
|-------|--------|-----------------------------------|
| USART1 | 38400  | command protocol, CRC16-MODBUS (обе платы) |
| USART3 / USART6 | 115200 | debug log, TX only (Blue / Black) |

Протокол один на обе платы: [PROTOCOL.md](PROTOCOL.md).

## Status (2026-11-13)

| | Blue Pill | Black Pill |
|---|---|---|
| Flash (text+data) | 26 560 B из 64 КБ (41 %) | 26 180 B из 512 КБ (5 %) |
| RAM static (data+bss) | 16 980 B из 20 КБ (83 %) | 24 328 B из 128 КБ (19 %) |
| `hil.py test` на железе | не прогнан после ADR-016 (нет платы, T10) | PASS 28/28 |

- Работает: протокол с кодами ошибок, STATUS, FRAM / NOR-тесты, LED-режимы,
  `g_diag` в `.noinit`, лог с префиксами `BOOT` / `FAULT` / `DIAG` / `TEST`.
- Pill Studio: обе платы, виртуальная плата, общий OpenOCD с Cursor (ADR-015).
- Известные дефекты и план: [TODO.md](TODO.md).

## Next task

См. раздел `Current` в [TODO.md](TODO.md).
