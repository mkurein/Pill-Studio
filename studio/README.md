# Pill Studio

Нативная (Rust + egui) отладочная студия для прошивки **primGPT** + FreeRTOS на двух платах:
**Blue Pill** (STM32F103C8T6, этот репозиторий) и **Black Pill** (STM32F411CEU6,
репозиторий [F411CE](https://github.com/mkurein/F411CE)). Плата выбирается в шапке;
выводы, SVD, тактирование, цель OpenOCD, ELF и порты — свои для каждой (ADR-016).
macOS и Windows, без WebView и Node.

![Обзор](docs/screens/01_overview_dark.jpg)

## Что умеет

| Экран | Что там |
|---|---|
| **Обзор** | три канала связи (UART1 протокол, UART3 лог, SWD), STATUS платы, быстрые команды, кнопка «Виртуальная плата» |
| **Плата** | рисунок Blue Pill или Black Pill по шелкографии, функции выводов этого проекта, 5 В-толерантность, живой уровень выводов по SWD |
| **Контроллер** | блоки и шины (AHB/APB1/APB2), карта памяти с занятостью из ELF, дерево тактирования + калькулятор BRR (PLL ×9 у F103, M/N/P/Q у F411), таблица 48 выводов (DS5319 / DS10314) |
| **Регистры** | все регистры из `STM32F103.svd` или `STM32F411.svd` (только блоки, что есть в корпусе), поиск, живое чтение по SWD, сетка битов, запись в два шага |
| **UART-протокол** | каталог команд, свой кадр в hex (CRC добавить / без CRC / испортить, формат линии 8E1·8O1·7N1·другая скорость, паузы между байтами, повторы), тест надёжности из 20 сбоев кадра и линии со STATUS после каждого ([описание тестов](../UART_TESTING.md)), стресс, разбор ответов по полям, реакция платы в логе |
| **FreeRTOS** | задачи и стеки (high-water mark), куча heap_4, счётчики, графики во времени, схема потоков данных прошивки |
| **Лог** (USART3 / USART6) | раскраска BOOT/TEST/FAULT/DIAG, ритм `Tick` (подвисание задач видно сразу), фильтр, копирование |
| **SWD-отладка** | стоп/пуск/шаг/сброс, регистры ядра с PC → строка исходника, разбор сбоя из `g_diag` (CFSR/HFSR/BFAR), переменные по имени из ELF, сборка, прошивка, автотест `tools/hil.py`, консоль OpenOCD; общий OpenOCD с Cursor (GDB :3333) и переход Cursor на строку останова |
| **Справочник** | 10 коротких статей: память и запуск, тактирование, GPIO, NVIC, USART, SPI/FRAM, FreeRTOS, SWD, проверка изменений; для Black Pill — ещё «что иначе на F411» |
| **Настройки** | плата, папка проекта, команда сборки, порты и OpenOCD — отдельно для каждой платы |

Без железа всё работает против **виртуальной платы**: симулятор отвечает на те же кадры,
пишет такой же лог и отдаёт правдоподобные значения регистров.

| | |
|---|---|
| ![Плата](docs/screens/02_board_dark.jpg) | ![Контроллер](docs/screens/03_mcu_dark.jpg) |
| ![Регистры](docs/screens/04_regs_dark.jpg) | ![Протокол](docs/screens/05_proto_dark.jpg) |
| ![FreeRTOS](docs/screens/06_rtos_dark.jpg) | ![Плата, светлая тема](docs/screens/12_board_light.jpg) |
| ![Black Pill](docs/screens/21_blackpill_board_dark.jpg) | ![Контроллер F411](docs/screens/22_blackpill_mcu_dark.jpg) |

## Две платы

| | Blue Pill | Black Pill |
|---|---|---|
| Чип | STM32F103C8T6 | STM32F411CEU6 |
| Прошивка | этот репозиторий, `build/primGPT.elf` | [F411CE](https://github.com/mkurein/F411CE), `build/primGPT_F411.elf` |
| OpenOCD | `target/stm32f1x.cfg`, DEV_ID 0x410 | `target/stm32f4x.cfg`, DEV_ID 0x431 |
| Лог | USART3, PB10 | USART6, PA11 |
| SPI-память | FRAM 32 КБ | W25Q64 8 МБ |

Плата выбирается в шапке или в «Настройках». Папка проекта, команда сборки и порты
запоминаются для каждой платы. Если по SWD отвечает чип другой платы, в шапке
появляется «⚠ по SWD … — переключить». Без железа обе работают на виртуальной плате.
Новая плата = новый `Profile` в `src/core/boards.rs` + данные в `src/core/mcu_<chip>.rs` (ADR-016).

## Запуск

```bash
cd studio
cargo run --release
```

Нужен Rust ≥ 1.88 (`eframe 0.33`). Для SWD — OpenOCD в PATH (`brew install open-ocd`) или
путь к нему в «Настройках»; для «строка исходника» — `arm-none-eabi-addr2line`.

Подключение платы:

| Канал | Провода | Скорость |
|---|---|---|
| USART1 (команды) | PA9 → RX адаптера, PA10 ← TX, GND (обе платы) | 38400 8N1 |
| Лог | Blue Pill: PB10 (USART3), Black Pill: PA11 (USART6) → RX второго адаптера, GND | 115200 |
| SWD | 3.3 · DIO · CLK · GND (ST-Link V2) | — |

## Сборка дистрибутива

Команды для Windows (`setup.exe`) и macOS (`.dmg`): [BUILD.md](BUILD.md).

## Проверки

```bash
cargo test
cargo clippy --all-targets -- -D warnings
APP_SHOTS=target/shots cargo run --release   # обход всех экранов в двух темах → BMP
```

Тест надёжности UART1 на настоящей плате, без интерфейса (порт — ваш адаптер):

```bash
PILL_PORT=/dev/cu.usbserial-120 cargo test --release hardware -- --ignored --nocapture
```

Что проверяет каждый из 20 случаев и как читать результат — [UART_TESTING.md](../UART_TESTING.md).

## Устройство

```text
src/
  core/        логика без UI: boards (профиль платы), proto (кадры, CRC, STATUS), svd,
               mcu / mcu_f411 (выводы, блоки, такты),
               openocd (TCL RPC :6667), elf (символы, секции), sim (виртуальная плата),
               link (serial), config
  worker.rs    поток ввода-вывода: Job → State + потоки (обмены, лог, выборки)
  app.rs       оболочка: навигация, статус-бар, тосты, Act → Job
  views/       экраны; получают &Shared, ставят действия в очередь
  theme.rs     палитра светлой/тёмной темы
  canvas.rs    рисование схем в координатах viewBox
```

UI никогда не блокируется: порты, OpenOCD, сборка и `hil.py` работают в потоке worker.
Протокол и раскладка `STATUS` повторяют `docs/PROTOCOL.md`; при изменении протокола
обновите `src/core/proto.rs` и `src/core/sim.rs` в том же commit.
