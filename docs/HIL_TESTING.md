# Автотест на железе (HIL) и протокол USART1

## Подключение

| Сигнал | Пин STM32 | Куда |
|---|---|---|
| SWDIO / SWCLK / GND / 3.3V | PA13 / PA14 | ST-Link |
| NRST (желательно) | NRST | ST-Link RST — нужен для `--under-reset` |
| USART1 TX / RX | PA9 / PA10 | USB-UART RX / TX (38400 8N1) — протокол |
| USART3 TX | PB10 | второй USB-UART RX (115200) — текстовый лог |
| FRAM SPI1 | PA5 SCK, PA6 MISO, PA7 MOSI, PA4 CS | микросхема FRAM (32 КБ) |

BOOT0 = 0. Для полного автотеста нужны оба UART; с одним адаптером
тесты другого канала помечаются SKIP.

## Две платы

`tools/hil.py` один и тот же в этом репозитории и в F411CE (Black Pill).
Плата выбирается по линкер-скрипту в корне (`STM32F411CEUX_FLASH.ld` → F411) или
`HIL_BOARD=f103|f411`: от неё зависят ELF (`primGPT.elf` / `primGPT_F411.elf`),
цель OpenOCD (`stm32f1x.cfg` / `stm32f4x.cfg`), разбор GPIO (CRL/CRH или MODER)
и проверка `swd: chip is …` по DEV_ID (0x410 / 0x431). Лог на F411 — USART6, TX PA11.
На WeAct Black Pill вместо FRAM распаяна SPI NOR W25Q64: те же FRAM-тесты проходят,
драйвер пишет через стирание сектора 4 КБ.

## Запуск

```bash
python3 -m venv .venv && .venv/bin/pip install pyserial   # один раз
.venv/bin/python tools/hil.py detect     # найти порты -> tools/hil.json
.venv/bin/python tools/hil.py test       # сборка + прошивка + проверки
```

Windows: `py -m venv .venv`, `.venv\Scripts\python tools\hil.py ...`; порты
задать в `tools/hil.json` (`{"proto_port": "COM3", "log_port": "COM5"}`).

## Что проверяет `test`

Сборка без warnings → прошивка + verify → строка `BOOT` и `FRAM probe` в UART3 →
PING → LED on/off (ответ, `0x71` и **регистр GPIOC_ODR по SWD**) → ошибки
(BAD_ARG, BAD_CRC, UNKNOWN_CMD, OVERFLOW) → кадр 90 B → FRAM probe / "KUREIN" /
64 B pattern → 50 PING подряд → STATUS (heap, стеки, ошибки UART, heartbeat идёт) →
RESET (`boot_count+1`) → `g_diag` по SWD без записи о сбое.

Протокол: [PROTOCOL.md](PROTOCOL.md). Сбои: [SAFETY.md](SAFETY.md).

## Команды

| Команда | Канал | Что показывает |
|---|---|---|
| `ping`, `status`, `led on/off/auto/get`, `fram 0/1/2`, `reset`, `raw <hex>` | UART1 | ответ, декодированный |
| `log [сек]` | UART3 | текстовый лог |
| `diag` | SWD | `g_diag`: счётчики + сбой (PC → файл:строка) |
| `regs` | SWD | halt, PC/LR/SP/CFSR/HFSR, resume |
| `gpio` | SWD | реальные биты PC13/PC15 |
| `read <symbol или 0xaddr> [слов]` | SWD | любая переменная из ELF |
