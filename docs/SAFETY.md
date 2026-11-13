# Safety / failure handling

Проект учебный: исполнительных механизмов нет (только LED и FRAM).
Здесь — что происходит при сбоях, чтобы новые правки это не ломали.
Таблица верна для обеих плат: `diag.c` и коды сбоев одинаковы, на Black Pill лог
идёт в USART6. На F411 к CFSR добавляются сбои FPU (lazy stacking: MLSPERR,
UsageFault `NOCP`, если FPU не включён до первой float-операции).

Логика обеих плат — 3.3 В. Не подавать 5 В на выводы без FT (Blue Pill PA0–PA7, PB0, PB1; Black Pill —
выводы в аналоговом режиме). На Black Pill стирание W25Q64 идёт секторами 4 КБ
(~100 000 циклов): не гонять FRAM-тест в бесконечном цикле.

## Failure states

| Event | Detection | Reaction | Visible as |
|-------|-----------|----------|------------|
| HAL init error | `!= HAL_OK` в `MX_*_Init`, clock | `Error_Handler` → `diag_fault(1)`, halt | `FAULT code=1` в UART3 |
| HardFault / MemManage / BusFault / UsageFault | CPU exception | `diag_fault(2..5)`: PC, LR, CFSR, HFSR, BFAR | `FAULT code=..` в UART3, `g_diag` |
| Stack overflow | FreeRTOS method 2 | `diag_fault(6, task)`, halt | `FAULT code=6 .. task=<name>` |
| Heap exhausted | malloc-failed hook | `diag_fault(7)`, halt | `FAULT code=7` |
| Queue / task create fail | NULL handle | `Error_Handler` | `FAULT code=1` |
| UART1 ORE/FE/NE | `HAL_UART_ErrorCallback` | `uart_err++`, RX перезапущен | STATUS, `DIAG uart_err=` |
| Queue full | `xQueueSendFromISR` fail | байт потерян, `queue_full++` | STATUS |
| Bad CRC / overflow / unknown | `Uart1Task` | команда не выполняется | ответ `E1` / `E3` / `E2` |
| Reset | `RCC->CSR` at boot | лог, флаги сброшены, `boot_count++` | `BOOT .. rst=0x..` |

`g_diag` лежит в `.noinit`: после soft reset (кнопка, SWD, `0x0F`) запись о
сбое остаётся и печатается при старте как `PREV_FAULT`. После power-on — обнуляется.
Читать по SWD: `tools/hil.py diag` (PC → файл:строка).

## Not implemented

- Watchdog (IWDG / WWDG) — после halt MCU висит до ручного сброса.
- Mutex на SPI1/FRAM (используется только из `Uart1Task`).

## Rules for changes

- Не блокировать `Uart1Task` надолго: байты копятся в очереди (100 B, дальше
  `queue_full`).
- Любой вызов `*FromISR` — только из IRQ с приоритетом 5..15.
- Новые аварийные пути — лог в UART3, не в UART1.
