# Protocol — UART1

Physical: USART1, PA9 TX / PA10 RX, 38400 8N1, no flow control.
Если меняешь протокол — сначала этот документ, потом код
(`Core/Inc/DataFile.h`) и константы хоста (`tools/hil.py`, `studio/`).

## Request frame

```text
CMD | ARG | ... | CRC_L | CRC_H
```

- Нет header, length, version, request_id.
- Конец кадра = тишина > `UART_TIMEOUT_MS` (20 ms, `DataFile.h`).
- Max длина `UART_BUF_SIZE` = 100 B; длиннее → ответ `OVERFLOW`.

## CRC

CRC-16/MODBUS: poly 0xA001 (reflected 0x8005), init 0xFFFF, no final XOR.
Передаётся LSB first. Проверка: CRC всего кадра вместе с CRC == 0
(`process_crc(data, len, true)` → 1). Эталон: `"123456789"` → 0x4B37.

## Response

Ответ на **каждый** кадр:

```text
ID_BU (0x01) | STATUS | PAYLOAD... | CRC_L | CRC_H
```

| STATUS | Name | Meaning |
|--------|------|---------|
| 0x00 | OK | выполнено |
| 0x01 | FAIL | выполнено, проверка не прошла (FRAM) |
| 0x02 | BAD_ARG | неверный аргумент |
| 0xE1 | BAD_CRC | CRC кадра неверен (или кадр < 3 B) |
| 0xE2 | UNKNOWN_CMD | нет такой команды |
| 0xE3 | OVERFLOW | кадр > 100 B |

## Commands

| CMD | ARG | Action | PAYLOAD |
|-----|-----|--------|---------|
| 0x01 PING | — | — | версия прошивки, ASCII |
| 0x02 FRAM | 0x00 | write `"KUREIN\0"` @ 0x0150, read back, compare | 7 B прочитанных |
| | 0x01 | probe: WREN → SR.WEL=1, WRDI → WEL=0 | SR после WREN, SR после WRDI |
| | 0x02 | 64 B pattern @ 0x7F00, verify, restore | mismatches, first idx, expected, got |
| 0x03 STATUS | — | — | 56 B `status_reply_t` (LE), см. ниже |
| 0x0F RESET | — | ответ, затем `NVIC_SystemReset()` | — |
| 0x70 LED | 0x00 | PC13 = SET (LED off, active-low), ручной режим | — |
| | 0x01 | PC13 = RESET (LED on), ручной режим | — |
| | 0x02 | авто-мигание `LedTask` (ADR-011) | — |
| 0x71 LED_GET | — | — | ODR PC13 (0/1), mode (1 = ручной) |

`status_reply_t`: `uptime_ms, reset_cause(RCC_CSR), boot_count, heartbeat,
heap_free, heap_min` (u32) · `stack_free default/led/uart` (u16, байты) ·
`rx_frames, rx_crc_err, uart_err, queue_full, fram_ok, fram_fail` (u32) ·
`led_odr, led_manual` (u8).

## Examples (hex)

| Frame | Meaning | Reply |
|-------|---------|-------|
| `70 01 E5 B0` | LED on | `01 00 00 20` |
| `70 00 24 70` | LED off | `01 00 00 20` |
| `02 00 00 D0` | FRAM test | `01 00 4B 55 52 45 49 4E 00 ..` |
| `70 01 E5 B1` | bad CRC | `01 E1 ..` |

## Log UART (115200, TX only): USART3 PB10 на Blue Pill, USART6 PA11 на Black Pill

`BOOT fw=<ver> rst=0x<CSR> boot=<n>` → `[PREV_FAULT ...]` →
`FRAM probe=OK|FAIL sr=<WEL set>/<WEL clear> mem=FRAM|W25Q64|NOR jedec=<id>` →
`Default Task running...` → `Uart1Task started` → `Tick:<ms>` каждую 1 s,
`DIAG heap_free=.. heap_min=.. stk_free D=.. L=.. U=.. rx=.. crc_err=.. uart_err=..` каждые 5 s,
`TEST fram sub=<n> result=OK|FAIL` на cmd 0x02.
Аварии: `FAULT code=.. pc=.. lr=.. cfsr=.. [task=..]` (SAFETY.md).

На UART1 больше нет текста: `printf` → UART3, `'S'` при старте убран.

Теория таймаута кадра: [notes/README_UART.md](notes/README_UART.md).
