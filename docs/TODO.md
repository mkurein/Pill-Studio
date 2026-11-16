# TODO

Одна задача = один commit. Перед задачей — анализ (Plan), потом правка (Build),
потом `make -C Debug all` и проверка на плате.

## Current

- [ ] **T10** Прогнать `tools/hil.py test` на плате (SWD + оба UART), исправить найденное.
  2026-11-13: на ST-Link (V2J45S7, 3.25 В) отвечает **STM32F411CEU6**, не F103:
  Cortex-M4, DEV_ID 0x431, Flash 512 КБ, CPUTAPID 0x2ba01477 (stm32f1x.cfg его
  отвергает). Для T10 нужна плата Blue Pill со STM32F103C8.

- [x] **T11** Pill Studio для двух плат (ADR-016): профили Blue Pill / Black Pill,
  прошивка F411CE с протоколом и `g_diag`, общий `hil.py`. На Black Pill
  `hil.py test` — PASS 28/28 (2026-11-13). Blue Pill на железе после правки
  `fram.c` не проверялась (нет платы) — входит в T10.

- [x] **T12** Обзор проекта прошивки (ADR-019): выбор проекта, локальный ELF и сверка
  по SWD. UART, STATUS и симулятор находятся в профиле primGPT. Сборка Windows и
  сценарий с ELF без UART проверены. Для аппаратной проверки нужен доступный ST-Link.

## Done in branch hil-studio (2026-11)

T1–T7 закрыты: см. ADR-011..013, PROTOCOL.md.
Pill Studio (`studio/`, ADR-014): GUI на egui, виртуальная плата, `.app`/`.dmg`.

## Previously current

- [x] **T1** `Uart1Task`: заменить `HAL_Delay(1000)` в cmd 0x02 на `vTaskDelay`
  (или убрать: FRAM не требует паузы после записи). Files: `main.c`.

## Next

- [x] **T2** `fram_cfg_setup`: `sck_port` присваивается 3 раза, `miso_port` / `mosi_port`
  не заполняются. Files: `fram.c`.
- [x] **T3** `fram_erase_all`: `uint8_t empty_data[0x8000]` на стеке (32 KB > вся RAM).
  Писать чанками из static буфера 32–64 B. Files: `fram.c`.
- [x] **T4** `_write` (printf) идёт в UART1 = командный канал. Перенаправить в UART3.
  Files: `uart.c`. Проверить `Error_Handler`, `SystemClock_Config`.
- [x] **T5** Cmd 0x70 перетирается `LedTask` через ≤ 1 s. Решить: LedTask мигает
  только в «авто»-режиме или LED команды → другой пин. Files: `main.c`. Сначала ADR.
- [x] **T6** Ответ всегда `01 00` даже при ошибке CRC / неизвестной команде.
  Ввести коды ошибок. Сначала `PROTOCOL.md`.
- [x] **T7** Результат FRAM-теста (cmd 0x02) не возвращается хосту.
  Сравнить `rd_data` с `wr_data`, вернуть статус. Сначала `PROTOCOL.md`.
- [ ] **T8** Синхронизировать `primGPT.ioc` с кодом (ADR-005): USART1_IRQn prio 5,
  USART3 без IRQ, defaultTask 1024 B, USART3 init внутри USER CODE.
- [ ] **T9** Перенести `LedTask` / `Uart1Task` / команды из `main.c` в свои модули
  (`app_led.c`, `app_uart1.c`, `commands.c`).

## Later

- [x] Хост-скрипт: `tools/hil.py` (команды, CRC, лог, SWD, полный тест).
- [ ] Unit-тест `crc16.c` на ПК (gcc host build).
- [ ] IWDG watchdog + причина сброса в лог (ADR-007 → расширить).
- [ ] Кадр с `LEN` и `REQUEST_ID` (см. guide §28), версия протокола.
- [x] Mutex на UART3 (`log_mutex`).
- [ ] Записать модель FRAM-микросхемы в `HARDWARE.md`.
- [ ] Новый замер RAM (строка v0.4 в notes/README_RAM_RTOS.md §19): bss вырос
  14644 → 16180 B, причина не выяснена.
- [x] Убрать мёртвый код: `uart_read`, `usart_recv_byte`, ring buffer.

## Done

- [x] Hooks stack overflow / malloc failed (ADR-007)
- [x] Heap 14 → 10 KB (ADR-006)
- [x] `UART_TIMEOUT_MS` 200 → 20 ms (ADR-002)
- [x] `calculate_crc_for_2_bytes` реентерабельна (буфер вызывающего)
- [x] 2026-09-22 Документация → схема `docs/` (ADR-009, ADR-010)
