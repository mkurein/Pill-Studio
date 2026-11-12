#ifndef UART_H
#define UART_H

#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdarg.h>   // va_list, va_start, va_end
#include <string.h>   // strlen
#include "stm32f1xx_hal.h"

extern UART_HandleTypeDef huart1;   // binary protocol, 38400
extern UART_HandleTypeDef huart3;   // text log, 115200

/* USART3 text log (thread-safe once log_init() was called) */
void log_init(void);
void log_printf(const char *format, ...);
#define printf_uart3 log_printf     // old name, kept for compatibility

/* USART1 protocol port */
void uart1_start_rx(void);
void uart1_put_ch(char ch);
void uart1_write(const uint8_t *data, uint16_t len);
void uart1_put_u16(uint16_t data);
void uart1_put_u32(uint32_t data);

#endif // UART_H
