#include "diag.h"
#include "stm32f1xx.h"
#include <string.h>

/* .noinit: kept across soft resets (see STM32F103C8TX_FLASH.ld) */
__attribute__((section(".noinit"))) volatile diag_t g_diag;

/* Polling output on USART3 directly via registers: works with IRQs disabled,
 * inside fault handlers and before/after the scheduler. */
void diag_puts_raw(const char *s)
{
    if (!(USART3->CR1 & USART_CR1_UE)) return;      /* USART3 not initialised yet */
    while (*s) {
        uint32_t guard = 100000u;
        while (!(USART3->SR & USART_SR_TXE) && --guard) {}
        USART3->DR = (uint8_t)*s++;
    }
    uint32_t guard = 100000u;
    while (!(USART3->SR & USART_SR_TC) && --guard) {}
}

static void put_hex(const char *name, uint32_t v)
{
    char buf[24];
    static const char hex[] = "0123456789ABCDEF";
    size_t n = 0;
    while (*name && n < 10) buf[n++] = *name++;
    buf[n++] = '=';
    buf[n++] = '0';
    buf[n++] = 'x';
    for (int i = 7; i >= 0; i--) buf[n++] = hex[(v >> (i * 4)) & 0xF];
    buf[n++] = ' ';
    buf[n] = 0;
    diag_puts_raw(buf);
}

void diag_boot(uint32_t reset_cause)
{
    if (g_diag.magic != DIAG_MAGIC) {
        /* first boot after power-on (RAM content is random) */
        memset((void *)&g_diag, 0, sizeof(g_diag));
        g_diag.magic = DIAG_MAGIC;
    }
    g_diag.boot_count++;
    g_diag.reset_cause = reset_cause;
    g_diag.heartbeat = 0;
    /* counters are per-boot; the fault record is kept until read and cleared */
    g_diag.rx_frames = g_diag.rx_crc_err = g_diag.rx_overflow = 0;
    g_diag.uart_err = g_diag.queue_full = g_diag.last_cmd = 0;
    g_diag.fram_ok = g_diag.fram_fail = 0;
}

void diag_fault(uint32_t code, const uint32_t *frame, const char *task)
{
    __disable_irq();
    g_diag.fault_magic = DIAG_FAULT_MAGIC;
    g_diag.error_code  = code;
    g_diag.fault_pc    = frame ? frame[6] : 0;
    g_diag.fault_lr    = frame ? frame[5] : 0;
    g_diag.fault_cfsr  = SCB->CFSR;
    g_diag.fault_hfsr  = SCB->HFSR;
    g_diag.fault_bfar  = SCB->BFAR;
    g_diag.fault_mmfar = SCB->MMFAR;
    memset((void *)g_diag.fault_task, 0, sizeof(g_diag.fault_task));
    if (task) {
        for (size_t i = 0; i < sizeof(g_diag.fault_task) - 1 && task[i]; i++)
            g_diag.fault_task[i] = task[i];
    }

    diag_puts_raw("\r\nFAULT ");
    put_hex("code", code);
    put_hex("pc", g_diag.fault_pc);
    put_hex("lr", g_diag.fault_lr);
    put_hex("cfsr", g_diag.fault_cfsr);
    put_hex("hfsr", g_diag.fault_hfsr);
    put_hex("bfar", g_diag.fault_bfar);
    if (task) { diag_puts_raw("task="); diag_puts_raw(task); }
    diag_puts_raw("\r\n");

    /* Stay here so a debugger (tools/hil.py regs/diag) can inspect the state. */
    while (1) {}
}

void diag_fault_entry(uint32_t code, const uint32_t *sp, uint32_t from_psp)
{
    if (!from_psp) {
        /* skip up to 4 words pushed by the handler prologue: a valid exception
         * frame has the Thumb bit in xPSR and a PC inside flash */
        for (int i = 0; i <= 4; i++) {
            const uint32_t *f = sp + i;
            if ((f[7] & (1u << 24)) && f[6] >= FLASH_BASE && f[6] < FLASH_BASE + 0x20000u) {
                sp = f;
                break;
            }
        }
    }
    diag_fault(code, sp, NULL);
}

/* Fault handlers stay in stm32f1xx_it.c (CubeMX file); their USER CODE
 * sections call DIAG_FAULT_ENTRY() from diag.h. */
