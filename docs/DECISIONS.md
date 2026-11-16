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

Проект = этот репозиторий (`primGPT/`, GitHub `mkurein/Pill-Studio`, до 2026-11-13 — `F103C8T`).
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

С ADR-016 — Pill Studio, для двух плат.

`studio/` — отдельный крейт (eframe 0.33, без WebView): справочник MCU и платы,
регистры из `STM32F103.svd` с чтением по SWD (OpenOCD TCL RPC), консоль
протокола USART1, монитор FreeRTOS по STATUS, лог USART3, разбор `g_diag`,
сборка / прошивка / `hil.py test`. Виртуальная плата (`studio/src/core/sim.rs`)
повторяет протокол и лог прошивки. Константы протокола — `studio/src/core/proto.rs`.
Reason: отладка и обучение без PDF и IDE на macOS и Windows; тот же HIL-цикл, что у агента.
Rule: меняется протокол / STATUS / `diag_t` → обновить `proto.rs`, `sim.rs`,
`core/mod.rs` в том же commit.

## ADR-015 — Студия и Cursor на одном OpenOCD

OpenOCD студии открывает GDB-порт 3333 (`OcdConfig.gdb_port`, 0 = закрыт) рядом
с TCL 6667. Cursor подключается к нему конфигурацией Cortex-Debug
`servertype: external` («Attach to Pill Studio»). Студия раз в 300 мс читает
состояние ядра: новый останов или новый PC → регистры, листинг и
`cursor -g файл:строка`.
Reason: один ST-Link не делят два OpenOCD; у Cursor нет API для внешней программы.
Ограничение: шаг/пуск из студии GDB не замечает, пока сам не остановит ядро.
Дальше: DAP-сервер в студии (`debugServer` в launch.json) — общий сеанс отладки.

## ADR-016 — Pill Studio: одна студия для Blue Pill и Black Pill

Студия переименована в Pill Studio. Всё, что зависит от платы (выводы и гребёнки,
SVD и список периферии, блоки и шины, карта памяти, тактирование, разбор GPIO,
цель OpenOCD, DEV_ID, имя ELF, UART лога, задачи и куча, значения симулятора),
собрано в `studio/src/core/boards.rs` (`Profile`), данные F411 — `mcu_f411.rs`.
Выбор платы — в шапке и в Настройках; папка проекта, команда сборки и порты
хранятся для каждой платы отдельно (`Config.slots`). Если по SWD отвечает чип
другой платы, студия предлагает переключиться. Прошивка F411 (репозиторий F411CE)
приведена к протоколу и `g_diag` этого проекта; `tools/hil.py` общий.
Драйвер `fram.c` узнаёт SPI NOR (W25Qxx) по JEDEC и пишет через сектор 4 КБ
(`SPI_NOR_SUPPORT`, на F1 выключен — не хватает RAM).
Reason: дома и на работе разные платы, софт и протокол один.
Rule: новая плата = новый `Profile` + данные в `mcu_<chip>.rs`; протокол общий.

## ADR-017 — Диагностика FreeRTOS: TASKS, assert в g_diag, счётчики сбросов
Новая команда TASKS (0x04) вместо расширения STATUS: формат STATUS (56 B) остаётся,
старые `hil.py` и студия продолжают работать, а студия по UNKNOWN_CMD понимает, что
прошивка старая. Время задач считает TIM1 — тот же, что даёт тик HAL.
`configASSERT` пишет файл и строку в `g_diag` и останавливается, как остальные сбои.
В `g_diag` добавлены счётчики сбросов по причинам и пик очереди; `DIAG_MAGIC` = 0xD1A60003,
поэтому запись старой прошивки после обновления обнуляется.
Reason: зависание по assert не оставляло следа; не было видно, какая задача ест процессор.
Rule: новые данные диагностики — отдельной командой или в конец `g_diag`, STATUS не трогать.

## ADR-018 — Сторожевой таймер с отметками задач, PVD, команда FAULT
IWDG сбрасывает только контролёр в `defaultTask`, и только когда отметились все задачи:
зависшая задача, сбой (обработчик стоит с выключенными прерываниями) и остановка тактирования
заканчиваются перезапуском, а не вечным зависанием. Запись о сбое теперь не стирается при
старте — иначе после перезапуска её не прочитать; её стирает команда FAULT 0.
PVD (2.9 В) только считает просадки в `g_diag`. Команда FAULT (0x7F) вызывает сбои нарочно,
чтобы `hil.py test` и студия проверяли защиту на железе.
Reason: плата без присмотра должна возвращаться в работу сама и оставлять след.
Rule: каждая задача с циклом обязана отмечаться чаще раза в секунду; долгая операция
(> 2 с) — отмечаться внутри. FAULT — тестовая команда: в изделии её убрать.

## ADR-019 — Обзор проекта и профиль прошивки

«Обзор» показывает выбранный проект, локальный ELF и результат сверки с Flash по SWD.
Профиль платы определяет MCU, SVD и цель OpenOCD. Профиль прошивки определяет,
доступны ли протокол, лог, STATUS, симулятор и автотест primGPT. Для неизвестных
проектов используется «Только SWD». Существующие репозитории primGPT распознаются
по `tools/hil.py` и командам PING/STATUS в `Core/Inc/DataFile.h`. Выбор пользователя
и явный путь ELF хранятся в настройках Studio для каждой папки.

При смене проекта закрываются соединения и сбрасываются данные отладки.
Для обычного проекта ELF ищется в корне, `build/`, `Debug/`, `Release/`.
Если найдено несколько файлов, пользователь выбирает ELF на «Обзоре».

Reason: на одной плате можно собирать и отлаживать разные программы, включая
проекты без UART и FreeRTOS.

## ADR-020 — Единый источник версии

Версия Pill Studio хранится в `config/version.json` и расставляется
`tools/update_version.py` в `studio/Cargo.toml`, `studio/Cargo.lock` и значок
`README.md`. Скрипты упаковки сверяют `Cargo.toml` с источником. История — `CHANGELOG.md`.
Прошивка Blue Pill по-прежнему берёт строку из `git describe` (метка `vX.Y.Z`).
Reason: номер жил в нескольких файлах и правился руками; так же устроен репозиторий DDS.
Rule: версию менять только через `tools/update_version.py`, затем `status` и запись в `CHANGELOG.md`.
