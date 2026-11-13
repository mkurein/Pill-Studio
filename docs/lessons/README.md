# Мини-уроки по технологиям проекта primGPT

> **Две платы.** Документ написан для Blue Pill (STM32F103C8T6, этот репозиторий). Для Black Pill (STM32F411CEU6, репозиторий [F411CE](https://github.com/mkurein/F411CE)) всё то же, кроме: лог USART6 на PA11 вместо USART3 на PB10, SPI NOR W25Q64 на плате вместо FRAM, OpenOCD `target/stm32f4x.cfg`, `STM32F411.svd`, `build/primGPT_F411.elf`, 96 МГц, Flash 512 КБ, RAM 128 КБ, FreeRTOS 10.3.1 (порт ARM_CM4F). Сводка отличий — [HARDWARE.md](../HARDWARE.md), выводы — [PINOUT.md](../PINOUT.md).

Этот набор — короткие уроки "от простого к сложному" по технологиям, которые реально используются в проекте.

## Порядок прохождения

1. `01_STM32_HAL_HISTORY_AND_PRACTICE.md`
2. `02_UART_PROTOCOL_AND_CRC.md`
3. `03_INTERRUPTS_AND_ISR_TO_TASK_FLOW.md`
4. `04_FREERTOS_IN_THIS_PROJECT.md`
5. `05_SPI_FRAM_DRIVER.md`
6. `06_DEBUG_IN_CURSOR_OR_ANTIGRAVITY.md`

Большой разбор всей прошивки: `PROJECT_FIRMWARE_LESSON.md`.
Карта документации проекта: [../INDEX.md](../INDEX.md).

## Как использовать уроки

- Читайте по порядку.
- После каждого урока открывайте указанные файлы (`main.c`, `uart.c`, `fram.c`, `crc16.c`) и находите блоки, о которых идет речь.
- Проверяйте понимание на реальном железе: UART, OpenOCD, RTOS-логи.

