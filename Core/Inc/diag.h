#ifndef DIAG_H
#define DIAG_H

/*
 * Runtime diagnostics for automated (hardware-in-the-loop) testing.
 *
 * g_diag lives in .noinit RAM: it is NOT cleared by a soft reset, so the
 * fault record of a crash is still readable after the MCU restarts.
 * tools/hil.py reads it over SWD (OpenOCD) by symbol address from the ELF,
 * and Uart1Task returns most fields via the STATUS command (0x03).
 *
 * Keep the layout in sync with DIAG_FIELDS in tools/hil.py.
 */

#include <stdint.h>

#define DIAG_MAGIC        0xD1A60002u
#define DIAG_FAULT_MAGIC  0xFA017EDu

/* Error codes stored in g_diag.error_code */
#define DIAG_ERR_NONE        0u
#define DIAG_ERR_HANDLER     1u   /* Error_Handler() called           */
#define DIAG_ERR_HARDFAULT   2u
#define DIAG_ERR_MEMMANAGE   3u
#define DIAG_ERR_BUSFAULT    4u
#define DIAG_ERR_USAGEFAULT  5u
#define DIAG_ERR_STACK_OVF   6u
#define DIAG_ERR_MALLOC      7u

typedef struct {
    uint32_t magic;          /* DIAG_MAGIC once initialised                  */
    uint32_t boot_count;     /* number of boots since power-on               */
    uint32_t reset_cause;    /* RCC->CSR captured at boot                    */
    uint32_t heartbeat;      /* incremented every second by defaultTask      */

    /* Last fatal error (survives soft reset) */
    uint32_t fault_magic;    /* DIAG_FAULT_MAGIC if a fault was recorded     */
    uint32_t error_code;     /* DIAG_ERR_*                                   */
    uint32_t fault_pc;
    uint32_t fault_lr;
    uint32_t fault_cfsr;     /* SCB->CFSR                                    */
    uint32_t fault_hfsr;     /* SCB->HFSR                                    */
    uint32_t fault_bfar;     /* SCB->BFAR                                    */
    uint32_t fault_mmfar;    /* SCB->MMFAR                                   */
    char     fault_task[16]; /* task name (stack overflow) or ""             */

    /* Protocol counters (UART1) */
    uint32_t rx_frames;      /* frames with valid CRC                        */
    uint32_t rx_crc_err;     /* frames with bad CRC                          */
    uint32_t rx_overflow;    /* frames longer than UART_BUF_SIZE             */
    uint32_t uart_err;       /* HAL UART errors (ORE/FE/NE/PE)               */
    uint32_t queue_full;     /* bytes dropped because uartQueue was full     */
    uint32_t last_cmd;       /* last valid command code                      */

    /* FRAM self-test */
    uint32_t fram_ok;
    uint32_t fram_fail;
} diag_t;

extern volatile diag_t g_diag;

void diag_boot(uint32_t reset_cause);
void diag_fault(uint32_t code, const uint32_t *stacked_frame, const char *task);
void diag_fault_entry(uint32_t code, const uint32_t *sp, uint32_t from_psp);
void diag_puts_raw(const char *s);   /* polling USART3 output, safe in faults */

/* First statement of a fault handler (USER CODE ..._IRQn 0 in stm32f1xx_it.c).
 * Picks MSP/PSP from EXC_RETURN (still in LR: the handler makes no calls before
 * this) and tail-jumps to diag_fault(code, stacked_frame, NULL).
 * Faults in tasks use PSP: the frame is exact. In handler mode (MSP) an -O0
 * prologue ("push {r7}") may shift SP; diag_fault_entry() searches for the frame. */
#define DIAG_FAULT_ENTRY(code)                         \
    __asm volatile(                                    \
        "tst lr, #4      \n"                           \
        "ite eq          \n"                           \
        "mrseq r1, msp   \n"                           \
        "mrsne r1, psp   \n"                           \
        "ubfx r2, lr, #2, #1 \n"                       \
        "movs r0, %0     \n"                           \
        "b diag_fault_entry \n"                        \
        :: "i"(code))

#endif /* DIAG_H */
