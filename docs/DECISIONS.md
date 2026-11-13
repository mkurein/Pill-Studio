# Architecture Decisions

Формат: решение → причина → следствие. Новое решение = новый ADR внизу,
старые не переписываются (только `Status: superseded by ADR-NNN`).

## ADR-001 — HAL + CubeMX init

STM32 HAL, инициализация периферии генерируется CubeMX (`primGPT.ioc`).
Reason: быстрый старт, учебный проект. LL / регистры — только точечно
(пример: `RCC->CSR` для причины сброса).

## ADR-002 — Frame end by silence

Конец кадра UART1 = пауза > `UART_TIMEOUT_MS`. Поля длины нет.
Reason: простота. Таймаут уменьшен 200 → 20 ms (быстрее реакция,
при 38400 это ~77 символов запаса).
Consequence: хост обязан выдерживать паузу ≥ 20 ms между кадрами
и не делать пауз внутри кадра.

## ADR-003 — ISR → queue → task

ISR только кладёт байт в `uartQueue` и обновляет `last_rx_time`.
Разбор, CRC, выполнение — в `Uart1Task`.
Reason: короткие ISR, вся логика под планировщиком.

## ADR-004 — Mixed CMSIS-RTOS v2 / native FreeRTOS API

Задачи: `osThreadNew` (так генерирует CubeMX). Очередь: `xQueueCreate`.
Status: accepted as is. Не переводить массово без отдельной задачи.

## ADR-005 — Do not regenerate from .ioc (constraint)

Код и `primGPT.ioc` разошлись: в .ioc включён USART3_IRQn и нет
USART1_IRQn, стек defaultTask 128 words (в коде 1024 B),
`MX_USART3_UART_Init` и USART3 MSP написаны вручную вне USER CODE.
Consequence: «Generate Code» в CubeMX сломает приём команд.
Сначала синхронизировать .ioc (TODO), потом можно генерировать.

## ADR-006 — FreeRTOS heap 10 KB

`configTOTAL_HEAP_SIZE` 14 → 10 KB (при `configMAX_PRIORITIES = 56`).
Reason: static RAM был 99.5 % (20384 of 20480 B). После — 72 %,
runtime heap `Min:5960` из 10240 B. Замеры: notes/README_RAM_RTOS.md §19.

## ADR-007 — Fail-stop hooks

`configCHECK_FOR_STACK_OVERFLOW = 2`, malloc-failed hook включён.
Hook пишет в UART3 и останавливает MCU (`__disable_irq`, цикл).
Reason: явный сигнал вместо тихой порчи памяти. Watchdog пока нет.

## ADR-008 — Software CS for FRAM

CS на PA4 как GPIO, `SPI_NSS_SOFT`.
Reason: аппаратный NSS на F1 держит линию low всё время, пока SPI включён.

## ADR-009 — Single project root = git repo

Проект = этот репозиторий (`primGPT/`, GitHub `mkurein/F103C8T`).
Родительская папка `STM32F1/` — только workspace STM32CubeIDE (`.metadata`).
Открывать в VS Code / Cursor / Claude Code / OpenCode именно `primGPT/`.

## ADR-010 — Docs for small-context LLM

Схема из «Qwen — работа над большими проектами»: короткое ядро `docs/*.md`
(≤ ~1000 tokens каждый, язык смешанный: термины EN, пояснения RU),
точка входа `AGENTS.md`, карта `docs/INDEX.md`.
Длинные руководства — `docs/guides|notes|lessons/`, в контекст LLM
не грузятся без необходимости.
Reason: окно Qwen3.8-27B 8192 tokens.

## ADR-011 — LED: manual / auto mode (T5)

`0x70 00/01` переводит PC13 в ручной режим, `LedTask` перестаёт мигать;
`0x70 02` возвращает авто-мигание. Состояние читается `0x71`.
Reason: иначе команда перетиралась `LedTask` через ≤ 1 s, проверить её было нельзя.

## ADR-012 — Fault capture in .noinit + HIL tool

`g_diag` (`diag.c`) в секции `.noinit` линкер-скрипта: счётчики и запись о
последнем сбое переживают soft reset. Fault handlers остаются в `stm32f1xx_it.c`,
перехват — `DIAG_FAULT_ENTRY()` в USER CODE. Хост `tools/hil.py` читает `g_diag`,
регистры и GPIO по SWD (OpenOCD) и гоняет полный тест по UART1/UART3.
Reason: AI-агент и человек проверяют прошивку на железе без IDE и видят причину сбоя.

## ADR-013 — Command-line build `make` → `build/`

Корневой `Makefile` (arm-none-eabi-gcc из PATH) собирает в `build/`, не трогая
`Debug/` CubeIDE. Новые `.c` в `Core/Src` подхватываются автоматически.
Reason: сборка на macOS и в CI без генерации `Debug/makefile`.

## ADR-014 — F103 Studio: нативная GUI-студия на Rust egui

`studio/` — отдельный крейт (eframe 0.33, без WebView): справочник MCU и платы,
регистры из `STM32F103.svd` с чтением по SWD (OpenOCD TCL RPC), консоль
протокола USART1, монитор FreeRTOS по STATUS, лог USART3, разбор `g_diag`,
сборка / прошивка / `hil.py test`. Виртуальная плата (`studio/src/core/sim.rs`)
повторяет протокол и лог прошивки. Константы протокола — `studio/src/core/proto.rs`.
Reason: отладка и обучение без PDF и IDE на macOS и Windows; тот же HIL-цикл, что у агента.
Rule: меняется протокол / STATUS / `diag_t` → обновить `proto.rs`, `sim.rs`,
`core/mod.rs` в том же commit.
