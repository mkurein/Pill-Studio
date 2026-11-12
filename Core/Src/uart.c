#include "uart.h"
#include "DataFile.h"
#include "diag.h"
#include "FreeRTOS.h"
#include "task.h"
#include "queue.h"
#include "semphr.h"

extern QueueHandle_t uartQueue;          /* created in main.c */

static volatile uint8_t uart1_rx_byte;   /* HAL receives one byte here per interrupt */
static SemaphoreHandle_t log_mutex;      /* serialises USART3 output between tasks */

/*********************************************************************
 * USART3 log (115200) — human/AI readable text
 *********************************************************************/
void log_init(void)
{
    if (log_mutex == NULL) {
        log_mutex = xSemaphoreCreateMutex();
    }
}

static void log_write(const char *buf, size_t len)
{
    bool locked = false;
    if (log_mutex != NULL &&
        xTaskGetSchedulerState() == taskSCHEDULER_RUNNING &&
        (SCB->ICSR & SCB_ICSR_VECTACTIVE_Msk) == 0) {
        locked = (xSemaphoreTake(log_mutex, pdMS_TO_TICKS(200)) == pdTRUE);
    }
    HAL_UART_Transmit(&huart3, (uint8_t *)buf, (uint16_t)len, 100);
    if (locked) {
        xSemaphoreGive(log_mutex);
    }
}

void log_printf(const char *format, ...)
{
    char buffer[128];
    va_list args;
    va_start(args, format);
    int length = vsnprintf(buffer, sizeof(buffer), format, args);
    va_end(args);

    if (length <= 0) return;
    size_t tx_len = (size_t)length;
    if (tx_len >= sizeof(buffer)) tx_len = sizeof(buffer) - 1;
    log_write(buffer, tx_len);
}

/* printf() goes to the USART3 log as well (USART1 is a binary protocol port,
 * text there would corrupt protocol frames). */
int _write(int file, char *ptr, int len)
{
    (void)file;
    if (len > 0) log_write(ptr, (size_t)len);
    return len;
}

/*********************************************************************
 * USART1 (38400) — binary protocol port
 *********************************************************************/
void uart1_put_ch(char ch)
{
    HAL_UART_Transmit(&huart1, (uint8_t *)&ch, 1, 100);
}

void uart1_write(const uint8_t *data, uint16_t len)
{
    HAL_UART_Transmit(&huart1, (uint8_t *)data, len, 100 + len);
}

void uart1_put_u16(uint16_t data)
{
    uint8_t buffer[2] = { (uint8_t)(data >> 8), (uint8_t)data };
    uart1_write(buffer, 2);
}

void uart1_put_u32(uint32_t data)
{
    uint8_t buffer[4] = { (uint8_t)(data >> 24), (uint8_t)(data >> 16),
                          (uint8_t)(data >> 8), (uint8_t)data };
    uart1_write(buffer, 4);
}

/* Call once after uartQueue is created: arms the 1-byte RX interrupt. */
void uart1_start_rx(void)
{
    HAL_UART_Receive_IT(&huart1, (uint8_t *)&uart1_rx_byte, 1);
}

/*********************************************************************
 * RX interrupt: push every byte into the FreeRTOS queue.
 * Frame boundaries are detected in Uart1Task by an inter-byte gap.
 *********************************************************************/
void HAL_UART_RxCpltCallback(UART_HandleTypeDef *huart)
{
    if (huart->Instance != USART1) return;

    uint8_t ch = uart1_rx_byte;
    BaseType_t xHigherPriorityTaskWoken = pdFALSE;

    if (uartQueue == NULL ||
        xQueueSendFromISR(uartQueue, &ch, &xHigherPriorityTaskWoken) != pdPASS) {
        g_diag.queue_full++;
    }

    HAL_UART_Receive_IT(&huart1, (uint8_t *)&uart1_rx_byte, 1);
    portYIELD_FROM_ISR(xHigherPriorityTaskWoken);
}

/* Overrun / framing / noise error: HAL aborts reception, so it must be
 * re-armed here, otherwise USART1 goes deaf until reset. */
void HAL_UART_ErrorCallback(UART_HandleTypeDef *huart)
{
    if (huart->Instance != USART1) return;

    g_diag.uart_err++;
    __HAL_UART_CLEAR_PEFLAG(huart);   /* SR then DR read clears PE/FE/NE/ORE */
    HAL_UART_Receive_IT(&huart1, (uint8_t *)&uart1_rx_byte, 1);
}
