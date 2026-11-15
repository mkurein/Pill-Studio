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

/* ---- main stack high-water mark ------------------------------------------
 * The last _Min_Stack_Size bytes below _estack are filled with a pattern at
 * the start of main(). MSP serves main() until the scheduler starts and every
 * interrupt after that; what is still pattern was never used. */
extern uint32_t _estack;
extern char _Min_Stack_Size[];          /* linker symbol: its address is the size */
#define MSP_PATTERN 0xA5A5A5A5u

void diag_msp_fill(void)
{
    uint32_t *lo = &_estack - (uint32_t)(uintptr_t)_Min_Stack_Size / 4u;
    uint32_t *sp = (uint32_t *)__get_MSP() - 8;      /* keep clear of the live frame */
    for (uint32_t *p = lo; p < sp; p++) *p = MSP_PATTERN;
}

uint32_t diag_msp_free(void)
{
    const uint32_t *lo = &_estack - (uint32_t)(uintptr_t)_Min_Stack_Size / 4u;
    const uint32_t *p = lo;
    while (p < &_estack && *p == MSP_PATTERN) p++;
    return (uint32_t)(p - lo) * 4u;
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
    /* RCC_CSR: 31 LPWR, 30 WWDG, 29 IWDG, 28 SFT, 27 POR, 26 PIN, 25 BOR (F4) */
    if (reset_cause & ((1u << 29) | (1u << 30)))      g_diag.rst_wdg++;
    else if (reset_cause & (1u << 28))                g_diag.rst_soft++;
    else if (reset_cause & ((1u << 27) | (1u << 25))) { if (g_diag.boot_count > 1) g_diag.rst_power++; }
    else if (reset_cause & (1u << 26))                g_diag.rst_pin++;
    g_diag.heartbeat = 0;
    /* counters are per-boot; the fault record is kept until read and cleared */
    g_diag.rx_frames = g_diag.rx_crc_err = g_diag.rx_overflow = 0;
    g_diag.uart_err = g_diag.queue_full = g_diag.last_cmd = 0;
    g_diag.fram_ok = g_diag.fram_fail = 0;
    g_diag.queue_peak = 0;
    g_diag.ctx_switches = g_diag.irq_systick = g_diag.irq_tick = g_diag.irq_usart1 = 0;
    g_diag.msp_size = (uint32_t)(uintptr_t)_Min_Stack_Size;
    g_diag.msp_free = diag_msp_free();
}

void diag_fault(uint32_t code, const uint32_t *frame, const char *task)
{
    __disable_irq();
    g_diag.fault_magic = DIAG_FAULT_MAGIC;
    g_diag.error_code  = code;
    if (code != DIAG_ERR_ASSERT) g_diag.fault_line = 0;
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

    /* Stay here: a debugger can inspect the state; once the scheduler runs,
     * the watchdog restarts the board in a few seconds and the record stays. */
    while (1) {}
}

/* configASSERT() failed: file name (without path) goes to fault_task. */
void diag_assert(const char *file, int line)
{
    const char *base = file;
    for (const char *p = file; p && *p; p++) if (*p == '/' || *p == '\\') base = p + 1;
    g_diag.fault_line = (uint32_t)line;
    diag_fault(DIAG_ERR_ASSERT, NULL, base);
}

/* Run-time counter for FreeRTOS task statistics: the HAL tick (TIM1, 1 kHz)
 * plus the timer's own 1 MHz count. Wraps after ~71 min; readers use deltas. */
uint32_t diag_runtime_us(void)
{
    extern __IO uint32_t uwTick;
    static uint32_t last;
    uint32_t v = uwTick * 1000u + TIM1->CNT;
    /* TIM1 wrapped but its interrupt has not counted the tick yet */
    if (v < last && last - v < 2000u) v = last;
    last = v;
    return v;
}

/* ---- watchdog -----------------------------------------------------------
 * IWDG runs from the LSI, so it also catches a stopped main clock. Tasks
 * check in with diag_wdg_alive(); the supervisor (defaultTask, 1 Hz) feeds
 * the watchdog only when all of them did. A fault handler spins with IRQs
 * off, so it is not fed either: the board restarts and prints PREV_FAULT. */
static volatile uint32_t wdg_alive;

void diag_wdg_start(void)
{
#if defined(DBGMCU_APB1_FZ_DBG_IWDG_STOP)
    DBGMCU->APB1FZ |= DBGMCU_APB1_FZ_DBG_IWDG_STOP;   /* breakpoints do not reset the board */
#elif defined(DBGMCU_CR_DBG_IWDG_STOP)
    DBGMCU->CR |= DBGMCU_CR_DBG_IWDG_STOP;
#endif
    IWDG->KR = 0xCCCCu;                 /* start (turns the LSI on) */
    IWDG->KR = 0x5555u;                 /* unlock PR / RLR */
    IWDG->PR = 4u;                      /* LSI / 64 */
    IWDG->RLR = 2500u;                  /* 4 s at 40 kHz (F1), 5 s at 32 kHz (F4) */
    for (uint32_t guard = 200000u; IWDG->SR && --guard;) {}
    IWDG->KR = 0xAAAAu;
}

void diag_wdg_alive(uint32_t task)
{
    uint32_t pm = __get_PRIMASK();
    __disable_irq();
    wdg_alive |= task;
    __set_PRIMASK(pm);
}

void diag_wdg_supervise(void)
{
    static uint32_t misses;
    g_diag.msp_free = diag_msp_free();
    diag_wdg_alive(DIAG_WDG_DEFAULT);
    uint32_t pm = __get_PRIMASK();
    __disable_irq();
    uint32_t seen = wdg_alive & DIAG_WDG_ALL;
    if (seen == DIAG_WDG_ALL) wdg_alive = 0;
    __set_PRIMASK(pm);
    if (seen == DIAG_WDG_ALL) {
        misses = 0;
        IWDG->KR = 0xAAAAu;
    } else if (++misses >= 2u) {
        g_diag.wdg_missing = DIAG_WDG_ALL & ~seen;   /* who to blame after the reset */
    }
}

/* ---- supply early warning ---------------------------------------------- */
void diag_pvd_start(void)
{
    RCC->APB1ENR |= RCC_APB1ENR_PWREN;
    (void)RCC->APB1ENR;
    PWR->CR = (PWR->CR & ~PWR_CR_PLS) | PWR_CR_PLS_LEV7;   /* 2.9 V */
    PWR->CR |= PWR_CR_PVDE;
    EXTI->PR = 1u << 16;
    EXTI->RTSR |= 1u << 16;             /* PVD output rises when VDD falls below the level */
    EXTI->IMR |= 1u << 16;
    NVIC_SetPriority(PVD_IRQn, 4);      /* above the FreeRTOS syscall level: uses no RTOS API */
    NVIC_EnableIRQ(PVD_IRQn);
}

void PVD_IRQHandler(void)
{
    extern __IO uint32_t uwTick;
    EXTI->PR = 1u << 16;
    g_diag.pvd_count++;
    g_diag.pvd_ms = uwTick;
}

void diag_fault_clear(void)
{
    g_diag.fault_magic = 0;
    g_diag.error_code = 0;
    g_diag.fault_line = 0;
    g_diag.wdg_missing = 0;
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
