# API — firmware modules

Сигнатуры публичных функций, чтобы не грузить в контекст целые файлы.

## uart.h / uart.c

| Function | Notes |
|----------|-------|
| `void log_init(void)` | создать `log_mutex` (до старта шедулера) |
| `void log_printf(const char *fmt, ...)` | UART3, buf 128 B, mutex если шедулер запущен и не ISR |
| `printf_uart3` | макрос = `log_printf` |
| `int _write(int, char *, int)` | newlib hook: `printf` → **UART3** |
| `void uart1_start_rx(void)` | взвести RX IRQ (после `xQueueCreate`) |
| `void uart1_put_ch(char)` / `uart1_write(const uint8_t *, uint16_t)` | UART1, timeout 100 ms |
| `void uart1_put_u16(uint16_t)` / `uart1_put_u32(uint32_t)` | MSB first |
| `HAL_UART_RxCpltCallback` | ISR → `uartQueue`, `queue_full++` при переполнении |
| `HAL_UART_ErrorCallback` | `uart_err++`, перезапуск RX |

## diag.h / diag.c

| Function | Notes |
|----------|-------|
| `volatile diag_t g_diag` | `.noinit`; поля — см. diag.h и `DIAG_FIELDS` в tools/hil.py |
| `void diag_boot(uint32_t csr)` | в начале `main()` |
| `void diag_fault(code, frame, task)` | записать сбой, `FAULT` в UART3, halt |
| `DIAG_FAULT_ENTRY(code)` | первой строкой fault handler (USER CODE) |
| `void diag_puts_raw(const char *)` | polling USART3, работает при IRQ off |

## crc16.h / crc16.c

| Function | Notes |
|----------|-------|
| `uint16_t crc16(const uint8_t *data, uint16_t len)` | CRC-16/MODBUS |
| `int process_crc(uint8_t *data, uint16_t len, bool check)` | check=true → 1 если CRC ок; false → дописать CRC LSB first, вернуть len+2 (буфер ≥ len+2) |
| `void calculate_crc_for_2_bytes(const uint8_t *in, uint8_t *out)` | out[4] = in[0..1] + CRC; реентерабельна |

## fram.h / fram.c

| Function | Notes |
|----------|-------|
| `void fram_cfg_setup(fram_cfg_t *)` | пины SPI1 + CS PA4 |
| `int fram_init(fram_t *, fram_cfg_t *)` | привязка к `hspi1`, CS high; 0 = ok |
| `void fram_read(fram_t *, uint16_t addr, uint8_t *buf, uint16_t n)` | |
| `void fram_write(fram_t *, uint16_t addr, uint8_t *buf, uint16_t n)` | WREN → WRITE → WRDI |
| `uint8_t fram_read_status(fram_t *)` / `void fram_write_status(fram_t *, uint8_t)` | |
| `void fram_write_enable/disable(fram_t *)` | |
| `void fram_erase_all(fram_t *)` | нули блоками по 64 B |
| `int fram_probe(fram_t *, uint8_t sr[2])` | 0 = чип отвечает (WEL set/clear) |

Все вызовы blocking (`HAL_MAX_DELAY`), без mutex.

## DataFile.h

`UART_TIMEOUT_MS 20`, `UART_BUF_SIZE 100`, `polynomial 0xA001`,
`ID_BU 1`, `ID_BZ 2`, `ID_BF 3`, `CMD_*`, `ST_*` (PROTOCOL.md).

## main.c (tasks)

`LedTask`, `Uart1Task` → `handle_frame()` / `send_reply()`, `StartDefaultTask`,
`vApplicationStackOverflowHook`, `vApplicationMallocFailedHook`, `Error_Handler`.
Globals: `fram`, `cfgFRAM`, `uartQueue`, `led_manual`, `*TaskHandle`, `*_attributes`.
`version.c`: `fw_version[]` (git describe из Makefile, иначе `"cube"`).
