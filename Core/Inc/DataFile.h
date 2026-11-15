#ifndef INC_DATAFILE_H_
#define INC_DATAFILE_H_

#define UART_TIMEOUT_MS 20               // Inter-byte gap that ends a frame
#define UART_BUF_SIZE   100              // Maximum frame size (incl. CRC)
#define polynomial      0xA001

#define ID_BU   1
#define ID_BZ   2
#define ID_BF   3

/* ---- Protocol (USART1, 38400 8N1) -------------------------------------
 * Request : CMD | ARGS... | CRC16_LO | CRC16_HI        (CRC16/MODBUS)
 * Reply   : ID_BU | STATUS | PAYLOAD... | CRC16_LO | CRC16_HI
 * Frame end = no bytes for UART_TIMEOUT_MS.
 * Full description: doc/HIL_TESTING.md
 */
#define CMD_PING        0x01   // -> payload: firmware version (ASCII)
#define CMD_FRAM        0x02   // arg 0: write/read "KUREIN" @0x0150, 1: probe, 2: pattern test
#define CMD_STATUS      0x03   // -> payload: status_reply_t (little-endian)
#define CMD_STATS       0x05   // -> main stack free/size u16, then u32: task switches, SysTick, TIM1 tick, USART1 irq, PVD
#define CMD_TASKS       0x04   // -> payload: run time u32, count u8, count x 16-byte task records
#define CMD_RESET       0x0F   // reply, then NVIC_SystemReset()
#define CMD_LED         0x70   // arg 0: LED off, 1: LED on, 2: auto-blink
#define CMD_FAULT       0x7F   // test: 0 clear fault record, 1 stall this task, 2 assert, 3 HardFault, 4 fake PVD
#define CMD_LED_GET     0x71   // -> payload: [ODR PC13, mode(0 auto/1 manual)]

#define ST_OK           0x00
#define ST_FAIL         0x01   // command executed, check failed
#define ST_BAD_ARG      0x02
#define ST_BAD_CRC      0xE1
#define ST_UNKNOWN_CMD  0xE2
#define ST_OVERFLOW     0xE3

#endif /* INC_DATAFILE_H_ */
