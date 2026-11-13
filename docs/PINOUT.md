# Pinout

Функции выводов совпадают, кроме лога и кварца. Источник истины — код
(`MX_*_Init`, `*_hal_msp.c`); `.ioc` с кодом расходится (ADR-005).

| Function | Blue Pill (F103) | Black Pill (F411) | Mode | Owner | Note |
|----------|------------------|-------------------|------|-------|------|
| SPI memory CS | PA4 | PA4 | GPIO out PP | fram.c | soft NSS, active low |
| SPI1_SCK | PA5 | PA5 (AF5) | AF PP | fram.c | |
| SPI1_MISO | PA6 | PA6 (AF5) | input / AF | fram.c | |
| SPI1_MOSI | PA7 | PA7 (AF5) | AF PP | fram.c | |
| USART1_TX | PA9 | PA9 (AF7) | AF PP | uart.c | protocol |
| USART1_RX | PA10 | PA10 (AF7) | input / AF | uart.c | protocol |
| Log TX | PB10 (USART3) | PA11 (USART6, AF8) | AF PP | uart.c | 115200 |
| Log RX | PB11 (USART3) | PA12 (USART6, AF8) | input / AF | — | не используется |
| LED | PC13 | PC13 | GPIO out PP | LedTask, cmd 0x70 | active low |
| activity | PC15 | PC15 | GPIO out PP | Uart1Task | toggle на каждый кадр |
| KEY | — | PA0 | — | — | кнопка на землю, не используется |
| SWDIO / SWCLK | PA13 / PA14 | PA13 / PA14 | SWD | debugger | не занимать |
| HSE | PD0/PD1, 8 МГц | PH0/PH1, 25 МГц | OSC | RCC | |
| LSE | PC14/PC15 (кварц на плате) | PC14/PC15 (кварц на плате) | — | — | RTC не используется |

## Blue Pill

Свободны: PA0–PA3, PA8, PA11/12 (= USB), PA15, PB0–PB9, PB12–PB15.
JTAG отключён (`__HAL_AFIO_REMAP_SWJ_NOJTAG` в msp), поэтому PA15/PB3/PB4 — обычные GPIO.
PC13–PC15 слаботочные (≤ 3 mA). Альтернативные функции — через AFIO remap.

## Black Pill

Свободны: PA1–PA3, PA8, PA15, PB0–PB10, PB12–PB15. PB11 в корпусе UFQFPN48 нет
(на его месте VCAP_1). PA11/PA12 — это ещё и USB OTG FS (разъём USB-C):
пока там лог USART6, USB использовать нельзя. Альтернативные функции — AFRL/AFRH,
номер AF на каждый вывод (DS10314 Table 9). PC13–PC15 тоже слаботочные.
