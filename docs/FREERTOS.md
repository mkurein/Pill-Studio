# FreeRTOS

FreeRTOS v10.0.1, CMSIS-RTOS v2 wrapper, `heap_4`, tick 1 kHz, preemption on.
Config: `Core/Inc/FreeRTOSConfig.h`.

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
