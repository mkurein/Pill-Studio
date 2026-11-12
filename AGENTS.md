# AGENTS.md — primGPT

STM32F103C8 (Blue Pill) + FreeRTOS 10.0.1 + HAL. UART1 38400 бинарные
команды с CRC16-MODBUS, SPI FRAM 32 KB, UART3 115200 debug log.
RAM 20 KB (heap 10 KB), Flash 64 KB.

## Read order

1. `docs/PROJECT.md` — что и в каком состоянии
2. `docs/INDEX.md` — карта и наборы контекста под тип задачи
3. `docs/TODO.md` — текущая задача
4. Только файлы из набора контекста задачи

Не читать целиком: `Drivers/`, `Middlewares/`, `Debug/`, `docs/guides|notes|lessons/`.

## Build

```text
make -C Debug all -j8
```

Toolchain из STM32CubeIDE 1.17.0 должен быть в PATH
(VS Code: `Ctrl+Shift+B` делает это сам). `Debug/makefile` генерирует CubeIDE.

Без CubeIDE (macOS / Linux / CI): `make` в корне → `build/primGPT.elf` (ADR-013).

## Verify on hardware

`.venv/bin/python tools/hil.py test` — сборка, прошивка по SWD, ~30 проверок
по UART1/UART3/SWD, exit 0 = PASS, отчёт `build/hil_report.json`.
Отладка: `hil.py log 5`, `hil.py diag`, `hil.py regs`, `hil.py gpio`.
Подробно: `docs/HIL_TESTING.md`.

## Hard rules

- CubeMX-файлы (`main.c`, `stm32f1xx_hal_msp.c`, `stm32f1xx_it.c`):
  правки только внутри `USER CODE BEGIN/END`.
- Не запускать CubeMX «Generate Code» — .ioc расходится с кодом (ADR-005).
- В задачах `vTaskDelay`, не `HAL_Delay`. Большие буферы не на стеке.
- `*FromISR` только из IRQ с приоритетом 5..15.
- Меняешь протокол → сначала `docs/PROTOCOL.md`.
- Меняешь задачи / стеки / NVIC → обновить `docs/FREERTOS.md`.
- Новое архитектурное решение → ADR в `docs/DECISIONS.md`.

## Workflow

Task → Plan (анализ, без правок) → Build (минимальная правка) →
`make` → `tools/hil.py test` на плате → `git --no-pager diff` → commit →
отметить `[x]` в `docs/TODO.md`. Одна задача — один commit.
Формат задачи: `docs/TASK_TEMPLATE.md`.
