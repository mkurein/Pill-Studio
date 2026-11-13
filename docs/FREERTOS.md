# FreeRTOS

FreeRTOS v10.0.1, CMSIS-RTOS v2 wrapper, `heap_4`, tick 1 kHz, preemption on.
Config: `Core/Inc/FreeRTOSConfig.h`. Таблицы ниже — Blue Pill; отличия
Black Pill (репозиторий F411CE) — в конце, раздел «Black Pill».

## Tasks

| Task | Entry | Priority | Stack, B | Period | Purpose |
|------|-------|----------|----------|--------|---------|
| Uart1Task | `Uart1Task` | AboveNormal (32) | 2048 | blocks on queue | UART1 frames, commands |
| defaultTask | `StartDefaultTask` | Normal (24) | 1024 | 1 s / 5 s | tick log, `g_diag.heartbeat`, heap/stack diag |
| LedTask | `LedTask` | Normal (24) | 512 | 1 s | toggle PC13 (only in auto mode, ADR-011) |
| Tmr Svc | kernel | 2 | 256 words | — | `configUSE_TIMERS=1`, таймеров нет |
| IDLE | kernel | 0 | 128 words | — | — |

Все задачи и очередь создаются в `main()` через `osThreadNew` /
`xQueueCreate` (смешение CMSIS и native API — осознанно, см. ADR-004).
Размер стека в `osThreadAttr_t.stack_size` — **в байтах**.

## Queues / sync

| Object | Type | Producer → Consumer |
|--------|------|---------------------|
| `uartQueue` | queue 100 x `uint8_t` | USART1 ISR → Uart1Task |
| `log_mutex` | mutex | все задачи → USART3 (`log_printf`, `printf`) |

Конец кадра: `xQueueReceive(uartQueue, .., 20 ms)` вернул timeout при
непустом буфере. Очередь создаётся **до** `uart1_start_rx()` (RX IRQ).
Semaphores, event groups, software timers: нет.

## Interrupts (NVIC, 4 bits, 0 = highest)

| IRQ | Prio | Handler → callback | FreeRTOS API |
|-----|------|--------------------|--------------|
| USART1_IRQn | 5 | `HAL_UART_IRQHandler` → `HAL_UART_RxCpltCallback` / `HAL_UART_ErrorCallback` (uart.c) | `xQueueSendFromISR` |
| TIM1_UP_IRQn | 15 | HAL timebase (`HAL_IncTick`) | нет |
| SysTick, PendSV | 15 | FreeRTOS kernel | — |
| USART3_IRQn | off | TX polling | — |

`configLIBRARY_MAX_SYSCALL_INTERRUPT_PRIORITY = 5`: любое прерывание,
вызывающее `*FromISR`, должно иметь приоритет **5..15**.

DMA: нет. Timers: TIM1 = HAL timebase (SysTick отдан FreeRTOS).

## Memory

- `configTOTAL_HEAP_SIZE` = 10 KB (heap_4). Newlib heap 0x200, MSP 0x400.
- RAM static (data+bss) = 16 288 of 20 480 B (80 %), heap входит в bss.
- Пороги: min free heap > 1024 B ок, < 512 B риск;
  stack watermark > 256 B ок, < 128 B увеличить стек.
- Лог UART3 каждые 5 s: `HeapB`, `HeapU`, `StkB`, `StkU`
  (подробно: [notes/README_RAM_RTOS.md](notes/README_RAM_RTOS.md)).

## Hooks

`configCHECK_FOR_STACK_OVERFLOW = 2`, `configUSE_MALLOC_FAILED_HOOK = 1`.
Оба hook в `main.c` (USER CODE 4) → `diag_fault()`: запись в `g_diag` (.noinit),
строка `FAULT` в UART3 (polling), IRQ off, вечный цикл. Так же HardFault /
MemManage / BusFault / UsageFault (`DIAG_FAULT_ENTRY` в USER CODE `stm32f1xx_it.c`).

## Rules

- В задачах `vTaskDelay` / `osDelay`, **не** `HAL_Delay`.
- Большие буферы — static/global, не на стеке задачи.
- Новая задача → обновить эту таблицу и диагностику в `StartDefaultTask`.
- Задачи, очередь, хуки и приоритеты IRQ одинаковы на обеих платах: меняешь здесь —
  перенеси в F411CE и в `boards.rs` (`tasks`, `heap_total`) студии.

## Black Pill (STM32F411CE)

Та же структура: Uart1Task / defaultTask / LedTask, `uartQueue`, `log_mutex`,
хуки → `diag_fault()`, `DIAG_FAULT_ENTRY` в USER CODE `stm32f4xx_it.c`.

| | Blue Pill | Black Pill |
|---|---|---|
| FreeRTOS, порт | 10.0.1, `GCC/ARM_CM3` | 10.3.1, `GCC/ARM_CM4F` (FPU, lazy stacking) |
| defaultTask стек | 1024 B | 2048 B |
| Uart1Task / LedTask | 2048 / 512 B | 2048 / 512 B |
| `configMINIMAL_STACK_SIZE` (IDLE) | 128 слов | 400 слов |
| Tmr Svc стек | 256 слов | 800 слов |
| Heap | 10 КБ heap_4 | 10 КБ heap_4 (RAM 128 КБ — можно больше) |
| Лог | USART3 TX polling | USART6 TX polling |
| HAL timebase | TIM1_UP_IRQn, 15 | TIM1_UP_TIM10_IRQn, 15 |
| USART1_IRQn | 5 | 5 |
| RAM static | 83 % из 20 КБ | 19 % из 128 КБ |

`configLIBRARY_MAX_SYSCALL_INTERRUPT_PRIORITY = 5`, 4 бита приоритета — как на F103.
Задачи, использующие float, на F411 сохраняют регистры FPU в стеке (+~136 B на
кадр исключения): считать запас стека с этим.
