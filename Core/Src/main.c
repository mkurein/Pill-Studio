/* USER CODE BEGIN Header */
/**
  ******************************************************************************
  * @file           : main.c
  * @brief          : Main program body
  ******************************************************************************
  * @attention
  *
  * 16.02.25 by Kurein M.N.
  *
  * v.
  *
  ******************************************************************************
  */
/* USER CODE END Header */
/* Includes ------------------------------------------------------------------*/
#include "main.h"
#include "cmsis_os.h"

/* Private includes ----------------------------------------------------------*/
/* USER CODE BEGIN Includes */
#include "FreeRTOS.h"
#include "queue.h"
#include "task.h"
#include "stm32f1xx_hal.h"
#include "DataFile.h"
#include "uart.h"
#include "crc16.h"
#include "fram.h"
#include "diag.h"
#include "version.h"

/* USER CODE END Includes */

/* Private typedef -----------------------------------------------------------*/
/* USER CODE BEGIN PTD */

/* USER CODE END PTD */

/* Private define ------------------------------------------------------------*/
/* USER CODE BEGIN PD */
fram_t fram;
fram_cfg_t cfgFRAM;
QueueHandle_t uartQueue;

osThreadId_t LedTaskHandle, Uart1TaskHandle;

/* USER CODE END PD */

/* Private macro -------------------------------------------------------------*/
/* USER CODE BEGIN PM */

/* USER CODE END PM */

/* Private variables ---------------------------------------------------------*/
SPI_HandleTypeDef hspi1;

UART_HandleTypeDef huart1;
UART_HandleTypeDef huart3;

/* Definitions for defaultTask */
osThreadId_t defaultTaskHandle;

const osThreadAttr_t defaultTask_attributes = {
  .name = "defaultTask",
  .stack_size = 1024,
  .priority = (osPriority_t) osPriorityNormal,
};

/* USER CODE BEGIN PV */
const osThreadAttr_t LedTask_attributes = {
  .name = "LedTask",
  .stack_size = 512,
  .priority = (osPriority_t) osPriorityNormal,
};

const osThreadAttr_t Uart1Task_attributes = {
  .name = "Uart1Task",
  .stack_size = 2048,
  .priority = (osPriority_t) osPriorityAboveNormal,
};

/* USER CODE END PV */

/* Private function prototypes -----------------------------------------------*/
void SystemClock_Config(void);
static void MX_GPIO_Init(void);
static void MX_USART1_UART_Init(void);
static void MX_USART3_UART_Init(void);
static void MX_SPI1_Init(void);
void StartDefaultTask(void *argument);

/* USER CODE BEGIN PFP */
void LedTask(void *argument);
void Uart1Task(void *argument);
/* USER CODE END PFP */

/* Private user code ---------------------------------------------------------*/
/* USER CODE BEGIN 0 */
static volatile uint8_t led_manual = 0;   // 1: LED driven by CMD_LED, 0: LedTask blinks

void LedTask(void *pvParameters) {
    (void)pvParameters;
    while (1) {
        diag_wdg_alive(DIAG_WDG_LED);
        if (!led_manual) {
            HAL_GPIO_TogglePin(GPIOC, GPIO_PIN_13);
        }
        vTaskDelay(pdMS_TO_TICKS(1000));
    }
}

/* Reply: ID_BU | status | payload | CRC16 (LSB first) */
static void send_reply(uint8_t status, const uint8_t *payload, uint16_t len)
{
    uint8_t out[UART_BUF_SIZE];
    if (len > sizeof(out) - 4) len = sizeof(out) - 4;
    out[0] = ID_BU;
    out[1] = status;
    if (len) memcpy(&out[2], payload, len);
    uint16_t crc = crc16(out, (uint16_t)(len + 2));
    out[len + 2] = (uint8_t)(crc & 0xFF);
    out[len + 3] = (uint8_t)(crc >> 8);
    uart1_write(out, (uint16_t)(len + 4));
}

typedef struct __attribute__((packed)) {
    uint32_t uptime_ms;
    uint32_t reset_cause;
    uint32_t boot_count;
    uint32_t heartbeat;
    uint32_t heap_free;
    uint32_t heap_min;
    uint16_t stack_free_default;   // bytes
    uint16_t stack_free_led;
    uint16_t stack_free_uart;
    uint32_t rx_frames;
    uint32_t rx_crc_err;
    uint32_t uart_err;
    uint32_t queue_full;
    uint32_t fram_ok;
    uint32_t fram_fail;
    uint8_t  led_odr;
    uint8_t  led_manual;
} status_reply_t;

static uint16_t stack_free_bytes(osThreadId_t h)
{
    return h ? (uint16_t)(uxTaskGetStackHighWaterMark((TaskHandle_t)h) * sizeof(StackType_t)) : 0;
}

static void fill_status(status_reply_t *st)
{
    st->uptime_ms   = HAL_GetTick();
    st->reset_cause = g_diag.reset_cause;
    st->boot_count  = g_diag.boot_count;
    st->heartbeat   = g_diag.heartbeat;
    st->heap_free   = xPortGetFreeHeapSize();
    st->heap_min    = xPortGetMinimumEverFreeHeapSize();
    st->stack_free_default = stack_free_bytes(defaultTaskHandle);
    st->stack_free_led     = stack_free_bytes(LedTaskHandle);
    st->stack_free_uart    = stack_free_bytes(Uart1TaskHandle);
    st->rx_frames  = g_diag.rx_frames;
    st->rx_crc_err = g_diag.rx_crc_err;
    st->uart_err   = g_diag.uart_err;
    st->queue_full = g_diag.queue_full;
    st->fram_ok    = g_diag.fram_ok;
    st->fram_fail  = g_diag.fram_fail;
    st->led_odr    = (GPIOC->ODR & GPIO_PIN_13) ? 1 : 0;
    st->led_manual = led_manual;
}

/* FRAM write/read-back of a pseudo-random pattern; original content is restored.
 * payload: [mismatches, first_bad_index, expected, got] */
#define FRAM_TEST_ADDR 0x7F00u
#define FRAM_TEST_LEN  64u
static uint8_t fram_pattern_test(uint8_t *payload)
{
    static uint8_t saved[FRAM_TEST_LEN], pat[FRAM_TEST_LEN], rd[FRAM_TEST_LEN];
    uint8_t seed = (uint8_t)HAL_GetTick();
    uint8_t bad = 0, first = 0xFF, exp = 0, got = 0;

    fram_read(&fram, FRAM_TEST_ADDR, saved, FRAM_TEST_LEN);
    for (uint32_t i = 0; i < FRAM_TEST_LEN; i++) pat[i] = (uint8_t)(i * 37u + seed);
    fram_write(&fram, FRAM_TEST_ADDR, pat, FRAM_TEST_LEN);
    fram_read(&fram, FRAM_TEST_ADDR, rd, FRAM_TEST_LEN);
    for (uint32_t i = 0; i < FRAM_TEST_LEN; i++) {
        if (rd[i] != pat[i]) {
            if (bad == 0) { first = (uint8_t)i; exp = pat[i]; got = rd[i]; }
            if (bad < 255) bad++;
        }
    }
    fram_write(&fram, FRAM_TEST_ADDR, saved, FRAM_TEST_LEN);

    payload[0] = bad; payload[1] = first; payload[2] = exp; payload[3] = got;
    return bad ? ST_FAIL : ST_OK;
}

static void handle_frame(const uint8_t *cmd, uint16_t len)
{
    uint8_t payload[UART_BUF_SIZE - 4];
    uint16_t plen = 0;
    uint8_t status = ST_OK;
    uint16_t nargs = (uint16_t)(len - 3);     // without CMD and CRC16

    g_diag.last_cmd = cmd[0];

    switch (cmd[0]) {
    case CMD_PING:
        plen = (uint16_t)strlen(fw_version);
        memcpy(payload, fw_version, plen);
        break;

    case CMD_LED:
        if (nargs < 1 || cmd[1] > 2) { status = ST_BAD_ARG; break; }
        if (cmd[1] == 2) {
            led_manual = 0;
        } else {
            led_manual = 1;
            /* Blue Pill LED on PC13 is active-low */
            HAL_GPIO_WritePin(GPIOC, GPIO_PIN_13, cmd[1] ? GPIO_PIN_RESET : GPIO_PIN_SET);
        }
        break;

    case CMD_LED_GET:
        payload[0] = (GPIOC->ODR & GPIO_PIN_13) ? 1 : 0;
        payload[1] = led_manual;
        plen = 2;
        break;

    case CMD_FRAM: {
        uint8_t sub = nargs ? cmd[1] : 0;
        if (sub == 0) {
            static const char wr_data[7] = "KUREIN";
            char rd_data[7] = {0};
            fram_write(&fram, 0x0150, (uint8_t *)wr_data, sizeof(wr_data));
            fram_read(&fram, 0x0150, (uint8_t *)rd_data, sizeof(rd_data));
            status = memcmp(wr_data, rd_data, sizeof(wr_data)) ? ST_FAIL : ST_OK;
            memcpy(payload, rd_data, sizeof(rd_data));
            plen = sizeof(rd_data);
        } else if (sub == 1) {
            status = fram_probe(&fram, payload) == 0 ? ST_OK : ST_FAIL;
            plen = 2;
        } else if (sub == 2) {
            status = fram_pattern_test(payload);
            plen = 4;
        } else {
            status = ST_BAD_ARG;
            break;
        }
        if (status == ST_OK) g_diag.fram_ok++; else g_diag.fram_fail++;
        log_printf("TEST fram sub=%u result=%s\r\n", sub, status == ST_OK ? "OK" : "FAIL");
        break;
    }

    case CMD_STATUS: {
        status_reply_t st;
        fill_status(&st);
        memcpy(payload, &st, sizeof(st));
        plen = sizeof(st);
        break;
    }

    case CMD_TASKS: {
        /* run_time_us u32, count u8, then per task: name[8], state, priority,
         * stack_free u16 (bytes), run_time_us u32. At most 5 tasks fit a frame. */
        static TaskStatus_t ts[8];
        uint32_t total = 0;
        UBaseType_t n = uxTaskGetSystemState(ts, 8, &total);
        if (n > 5) n = 5;
        memcpy(payload, &total, 4);
        payload[4] = (uint8_t)n;
        plen = 5;
        for (UBaseType_t i = 0; i < n; i++) {
            uint8_t *r = payload + plen;
            size_t nl = strlen(ts[i].pcTaskName);
            uint16_t free_b = (uint16_t)(ts[i].usStackHighWaterMark * sizeof(StackType_t));
            memset(r, 0, 16);
            memcpy(r, ts[i].pcTaskName, nl > 8 ? 8 : nl);
            r[8] = (uint8_t)ts[i].eCurrentState;
            r[9] = (uint8_t)ts[i].uxCurrentPriority;
            memcpy(r + 10, &free_b, 2);
            memcpy(r + 12, &ts[i].ulRunTimeCounter, 4);
            plen += 16;
        }
        break;
    }

    case CMD_RESET:
        send_reply(ST_OK, NULL, 0);
        log_printf("RESET by command\r\n");
        vTaskDelay(pdMS_TO_TICKS(20));
        NVIC_SystemReset();
        break;

    case CMD_FAULT: {
        /* Fault injection for tests: the reply goes out first. */
        uint8_t what = nargs ? cmd[1] : 0xFF;
        if (what > 4) { status = ST_BAD_ARG; break; }
        if (what == 0) { diag_fault_clear(); break; }
        if (what == 4) { EXTI->SWIER |= 1u << 16; break; }   /* as if VDD dipped */
        send_reply(ST_OK, NULL, 0);
        log_printf("TEST fault=%u\r\n", what);
        vTaskDelay(pdMS_TO_TICKS(20));
        if (what == 1) vTaskSuspend(NULL);          /* stops checking in: watchdog resets */
        if (what == 2) configASSERT(what == 0);
        if (what == 3) *(volatile uint32_t *)0xFFFFFFF0u = 0;
        return;
    }

    default:
        status = ST_UNKNOWN_CMD;
        break;
    }
    send_reply(status, payload, plen);
}

void Uart1Task(void *pvParameters) {
    (void)pvParameters;
    uint8_t command[UART_BUF_SIZE];
    uint16_t index = 0;
    bool overflow = false;
    uint8_t ch;

    log_printf("Uart1Task started\r\n");
    while (1) {
        /* Idle: block until a byte arrives. Inside a frame: wait at most
         * UART_TIMEOUT_MS for the next byte; a longer gap ends the frame.
         * The whole queue is drained without extra delays, so long frames
         * are not split (old code took 1 byte per 3 ms and cut frames > ~7 bytes). */
        TickType_t wait = index ? pdMS_TO_TICKS(UART_TIMEOUT_MS) : pdMS_TO_TICKS(500);
        diag_wdg_alive(DIAG_WDG_UART);     /* at least twice a second, also with a quiet line */
        if (xQueueReceive(uartQueue, &ch, wait) == pdPASS) {
            uint32_t waiting = (uint32_t)uxQueueMessagesWaiting(uartQueue) + 1u;
            if (waiting > g_diag.queue_peak) g_diag.queue_peak = waiting;
            if (index < UART_BUF_SIZE) command[index++] = ch;
            else overflow = true;
            continue;
        }
        if (index == 0) continue;

        HAL_GPIO_TogglePin(GPIOC, GPIO_PIN_15);   // frame activity (scope probe)
        if (overflow) {
            g_diag.rx_overflow++;
            send_reply(ST_OVERFLOW, NULL, 0);
        } else if (index < 3 || !process_crc(command, index, true)) {
            g_diag.rx_crc_err++;
            send_reply(ST_BAD_CRC, NULL, 0);
        } else {
            g_diag.rx_frames++;
            handle_frame(command, index);
        }
        index = 0;
        overflow = false;
    }
}
/* USER CODE END 0 */

/**
  * @brief  The application entry point.
  * @retval int
  */
int main(void)
{
  /* USER CODE BEGIN 1 */
  uint32_t reset_cause = RCC->CSR;
  RCC->CSR |= RCC_CSR_RMVF;          // Сброс флагов сброса
  diag_boot(reset_cause);

  /* USER CODE END 1 */

  /* MCU Configuration--------------------------------------------------------*/

  /* Reset of all peripherals, Initializes the Flash interface and the Systick. */
  HAL_Init();

  /* USER CODE BEGIN Init */

  /* USER CODE END Init */

  /* Configure the system clock */
  SystemClock_Config();

  /* USER CODE BEGIN SysInit */

  /* USER CODE END SysInit */

  /* Initialize all configured peripherals */
  MX_GPIO_Init();
  MX_USART1_UART_Init();
  MX_USART3_UART_Init();
  MX_SPI1_Init();

  /* USER CODE BEGIN 2 */
  log_printf("\r\nBOOT fw=%s rst=0x%08lX boot=%lu\r\n",
             fw_version, reset_cause, g_diag.boot_count);
  if (g_diag.fault_magic == DIAG_FAULT_MAGIC) {
    log_printf("PREV_FAULT code=%lu pc=0x%08lX lr=0x%08lX cfsr=0x%08lX task=%s\r\n",
               g_diag.error_code, g_diag.fault_pc, g_diag.fault_lr,
               g_diag.fault_cfsr, (const char *)g_diag.fault_task);
    /* kept until CMD_FAULT 0: the watchdog restarts the board, the record must outlive that */
  }

  fram_cfg_setup(&cfgFRAM);
  fram_init(&fram,&cfgFRAM);
  {
    uint8_t sr[2];
    int ok = fram_probe(&fram, sr);
    log_printf("FRAM probe=%s sr=%02X/%02X mem=%s jedec=%06lX\r\n", ok == 0 ? "OK" : "FAIL",
               sr[0], sr[1], fram_kind(&fram), fram.jedec);
  }
  //log_printf("FRAM setup done\r\n");

  //fram_erase_all(&fram);
  //HAL_Delay(1000);
  //log_printf("---- Application Init ----\r\n");
  /* USER CODE END 2 */

  /* Init scheduler */
  osKernelInitialize();

  /* USER CODE BEGIN RTOS_MUTEX */
  /* add mutexes, ... */
  /* USER CODE END RTOS_MUTEX */

  /* USER CODE BEGIN RTOS_SEMAPHORES */
  /* add semaphores, ... */
  /* USER CODE END RTOS_SEMAPHORES */

  /* USER CODE BEGIN RTOS_TIMERS */
  /* start timers, add new ones, ... */
  /* USER CODE END RTOS_TIMERS */

  /* USER CODE BEGIN RTOS_QUEUES */
  /* add queues, ... */
  uartQueue = xQueueCreate(UART_BUF_SIZE, sizeof(uint8_t));
  if (uartQueue == NULL) {
       Error_Handler();// Ошибка создания очереди
  }
  log_init();
  uart1_start_rx();   // RX interrupt only after the queue exists
  diag_pvd_start();
  diag_wdg_start();   // from here the tasks must keep checking in
  /* USER CODE END RTOS_QUEUES */

  /* Create the thread(s) */
  /* creation of defaultTask */
  defaultTaskHandle = osThreadNew(StartDefaultTask, NULL, &defaultTask_attributes);

  /* USER CODE BEGIN RTOS_THREADS */
  /* add threads, ... */
  LedTaskHandle = osThreadNew(LedTask, NULL, &LedTask_attributes);

  Uart1TaskHandle = osThreadNew(Uart1Task, NULL, &Uart1Task_attributes);
  if (LedTaskHandle == NULL || Uart1TaskHandle == NULL) {
    Error_Handler();
  }
  /* USER CODE END RTOS_THREADS */

  /* USER CODE BEGIN RTOS_EVENTS */
  /* add events, ... */
  /* USER CODE END RTOS_EVENTS */

  /* Start scheduler */
  osKernelStart();

  /* We should never get here as control is now taken by the scheduler */

  /* Infinite loop */
  /* USER CODE BEGIN WHILE */

  while (1)
  {
    /* USER CODE END WHILE */

    /* USER CODE BEGIN 3 */
  }
  /* USER CODE END 3 */
}

/**
  * @brief System Clock Configuration
  * @retval None
  */
void SystemClock_Config(void)
{
  RCC_OscInitTypeDef RCC_OscInitStruct = {0};
  RCC_ClkInitTypeDef RCC_ClkInitStruct = {0};

  /** Initializes the RCC Oscillators according to the specified parameters
  * in the RCC_OscInitTypeDef structure.
  */
  RCC_OscInitStruct.OscillatorType = RCC_OSCILLATORTYPE_HSE;
  RCC_OscInitStruct.HSEState = RCC_HSE_ON;
  RCC_OscInitStruct.HSEPredivValue = RCC_HSE_PREDIV_DIV1;
  RCC_OscInitStruct.HSIState = RCC_HSI_ON;
  RCC_OscInitStruct.PLL.PLLState = RCC_PLL_ON;
  RCC_OscInitStruct.PLL.PLLSource = RCC_PLLSOURCE_HSE;
  RCC_OscInitStruct.PLL.PLLMUL = RCC_PLL_MUL9;
  if (HAL_RCC_OscConfig(&RCC_OscInitStruct) != HAL_OK)
  {
	printf("ERROR 1 DETECTED! MCU WILL RESET!\r\n");
	HAL_Delay(1000);
    Error_Handler();
  }

  /** Initializes the CPU, AHB and APB buses clocks
  */
  RCC_ClkInitStruct.ClockType = RCC_CLOCKTYPE_HCLK|RCC_CLOCKTYPE_SYSCLK
                              |RCC_CLOCKTYPE_PCLK1|RCC_CLOCKTYPE_PCLK2;
  RCC_ClkInitStruct.SYSCLKSource = RCC_SYSCLKSOURCE_PLLCLK;
  RCC_ClkInitStruct.AHBCLKDivider = RCC_SYSCLK_DIV1;
  RCC_ClkInitStruct.APB1CLKDivider = RCC_HCLK_DIV2;
  RCC_ClkInitStruct.APB2CLKDivider = RCC_HCLK_DIV1;

  if (HAL_RCC_ClockConfig(&RCC_ClkInitStruct, FLASH_LATENCY_2) != HAL_OK)
  {
	printf("ERROR 2 DETECTED! MCU WILL RESET!\r\n");
	HAL_Delay(1000);
    Error_Handler();
  }
}

/**
  * @brief SPI1 Initialization Function
  * @param None
  * @retval None
  */
static void MX_SPI1_Init(void)
{

  /* USER CODE BEGIN SPI1_Init 0 */

  /* USER CODE END SPI1_Init 0 */

  /* USER CODE BEGIN SPI1_Init 1 */

  /* USER CODE END SPI1_Init 1 */
  /* SPI1 parameter configuration*/
  hspi1.Instance = SPI1;
  hspi1.Init.Mode = SPI_MODE_MASTER;
  hspi1.Init.Direction = SPI_DIRECTION_2LINES;
  hspi1.Init.DataSize = SPI_DATASIZE_8BIT;
  hspi1.Init.CLKPolarity = SPI_POLARITY_LOW;
  hspi1.Init.CLKPhase = SPI_PHASE_1EDGE;
  hspi1.Init.NSS = SPI_NSS_SOFT;
  hspi1.Init.BaudRatePrescaler = SPI_BAUDRATEPRESCALER_16;
  hspi1.Init.FirstBit = SPI_FIRSTBIT_MSB;
  hspi1.Init.TIMode = SPI_TIMODE_DISABLE;
  hspi1.Init.CRCCalculation = SPI_CRCCALCULATION_DISABLE;
  hspi1.Init.CRCPolynomial = 10;
  if (HAL_SPI_Init(&hspi1) != HAL_OK)
  {
    printf("ERROR SPI DETECTED! MCU WILL RESET!\r\n");
	HAL_Delay(1000);
    Error_Handler();
  }
  /* USER CODE BEGIN SPI1_Init 2 */

  /* USER CODE END SPI1_Init 2 */

}

/**
  * @brief USART1 Initialization Function
  * @param None
  * @retval None
  */
static void MX_USART1_UART_Init(void)
{

  /* USER CODE BEGIN USART1_Init 0 */

  /* USER CODE END USART1_Init 0 */

  /* USER CODE BEGIN USART1_Init 1 */

  /* USER CODE END USART1_Init 1 */
  huart1.Instance = USART1;
  huart1.Init.BaudRate = 38400;
  huart1.Init.WordLength = UART_WORDLENGTH_8B;
  huart1.Init.StopBits = UART_STOPBITS_1;
  huart1.Init.Parity = UART_PARITY_NONE;
  huart1.Init.Mode = UART_MODE_TX_RX;
  huart1.Init.HwFlowCtl = UART_HWCONTROL_NONE;
  huart1.Init.OverSampling = UART_OVERSAMPLING_16;
  if (HAL_UART_Init(&huart1) != HAL_OK)
  {
	printf("ERROR USART1 DETECTED! MCU WILL RESET!\r\n");
	HAL_Delay(1000);
    Error_Handler();
  }
  /* USER CODE BEGIN USART1_Init 2 */
  // RX interrupt is armed later by uart1_start_rx(), after uartQueue is created
  /* USER CODE END USART1_Init 2 */

}
static void MX_USART3_UART_Init(void)
{
  huart3.Instance = USART3;
  huart3.Init.BaudRate = 115200;
  huart3.Init.WordLength = UART_WORDLENGTH_8B;
  huart3.Init.StopBits = UART_STOPBITS_1;
  huart3.Init.Parity = UART_PARITY_NONE;
  huart3.Init.Mode = UART_MODE_TX_RX;
  huart3.Init.HwFlowCtl = UART_HWCONTROL_NONE;
  huart3.Init.OverSampling = UART_OVERSAMPLING_16;

  if (HAL_UART_Init(&huart3) != HAL_OK)
  {
    printf("ERROR USART3 DETECTED! MCU WILL RESET!\r\n");
    HAL_Delay(1000);
    Error_Handler();
  }
}
/**
  * @brief GPIO Initialization Function
  * @param None
  * @retval None
  */
static void MX_GPIO_Init(void)
{
  GPIO_InitTypeDef GPIO_InitStruct = {0};
/* USER CODE BEGIN MX_GPIO_Init_1 */
/* USER CODE END MX_GPIO_Init_1 */

  /* GPIO Ports Clock Enable */
  __HAL_RCC_GPIOC_CLK_ENABLE();
  __HAL_RCC_GPIOD_CLK_ENABLE();
  __HAL_RCC_GPIOA_CLK_ENABLE();

  /*Configure GPIO pin Output Level */
  HAL_GPIO_WritePin(GPIOC, GPIO_PIN_13|GPIO_PIN_15, GPIO_PIN_RESET);

  /*Configure GPIO pin Output Level */
  HAL_GPIO_WritePin(GPIOA, GPIO_PIN_4, GPIO_PIN_RESET);

  /*Configure GPIO pins : PC13 PC15 */
  GPIO_InitStruct.Pin = GPIO_PIN_13|GPIO_PIN_15;
  GPIO_InitStruct.Mode = GPIO_MODE_OUTPUT_PP;
  GPIO_InitStruct.Pull = GPIO_NOPULL;
  GPIO_InitStruct.Speed = GPIO_SPEED_FREQ_LOW;
  HAL_GPIO_Init(GPIOC, &GPIO_InitStruct);

  /*Configure GPIO pin : PA4 */
  GPIO_InitStruct.Pin = GPIO_PIN_4;
  GPIO_InitStruct.Mode = GPIO_MODE_OUTPUT_PP;
  GPIO_InitStruct.Pull = GPIO_NOPULL;
  GPIO_InitStruct.Speed = GPIO_SPEED_FREQ_LOW;
  HAL_GPIO_Init(GPIOA, &GPIO_InitStruct);

/* USER CODE BEGIN MX_GPIO_Init_2 */
/* USER CODE END MX_GPIO_Init_2 */
}

/* USER CODE BEGIN 4 */
void vApplicationStackOverflowHook(TaskHandle_t xTask, char *pcTaskName)
{
  (void)xTask;
  diag_fault(DIAG_ERR_STACK_OVF, NULL, pcTaskName);
}

void vApplicationMallocFailedHook(void)
{
  diag_fault(DIAG_ERR_MALLOC, NULL, NULL);
}
/* USER CODE END 4 */

/* USER CODE BEGIN Header_StartDefaultTask */
/**
  * @brief  Function implementing the defaultTask thread.
  * @param  argument: Not used
  * @retval None
  */
/* USER CODE END Header_StartDefaultTask */
void StartDefaultTask(void *argument)
{
  /* USER CODE BEGIN 5 */
  /* Infinite loop */
  log_printf("Default Task running...\r\n");
  uint32_t diag_divider = 0;
  for (;;) {
	vTaskDelay(pdMS_TO_TICKS(1000));
	g_diag.heartbeat++;
	diag_wdg_supervise();
	log_printf("Tick:%lu\r\n", HAL_GetTick());

	/* Lightweight runtime diagnostics every 5 seconds (one parsable line). */
	if (++diag_divider >= 5) {
		diag_divider = 0;
		size_t total_heap = (size_t)configTOTAL_HEAP_SIZE;
		size_t free_heap = xPortGetFreeHeapSize();
		size_t min_heap = xPortGetMinimumEverFreeHeapSize();
		log_printf("DIAG heap_free=%u heap_min=%u heap_total=%u stk_free D=%u L=%u U=%u rx=%lu crc_err=%lu uart_err=%lu\r\n",
		           (unsigned)free_heap, (unsigned)min_heap, (unsigned)total_heap,
		           stack_free_bytes(defaultTaskHandle), stack_free_bytes(LedTaskHandle),
		           stack_free_bytes(Uart1TaskHandle),
		           g_diag.rx_frames, g_diag.rx_crc_err, g_diag.uart_err);
	}
  }
  /* USER CODE END 5 */
}

/**
  * @brief  Period elapsed callback in non blocking mode
  * @note   This function is called  when TIM1 interrupt took place, inside
  * HAL_TIM_IRQHandler(). It makes a direct call to HAL_IncTick() to increment
  * a global variable "uwTick" used as application time base.
  * @param  htim : TIM handle
  * @retval None
  */
void HAL_TIM_PeriodElapsedCallback(TIM_HandleTypeDef *htim)
{
  /* USER CODE BEGIN Callback 0 */

  /* USER CODE END Callback 0 */
  if (htim->Instance == TIM1) {
    HAL_IncTick();
  }
  /* USER CODE BEGIN Callback 1 */

  /* USER CODE END Callback 1 */
}

/**
  * @brief  This function is executed in case of error occurrence.
  * @retval None
  */
void Error_Handler(void)
{
  /* USER CODE BEGIN Error_Handler_Debug */
  /* User can add his own implementation to report the HAL error return state */
  /* HAL_Delay() with IRQs disabled never returns, so the old message+delay
   * hung forever. Record the error (readable over SWD) and stop here. */
  diag_fault(DIAG_ERR_HANDLER, NULL, NULL);
  /* USER CODE END Error_Handler_Debug */
}

#ifdef  USE_FULL_ASSERT
/**
  * @brief  Reports the name of the source file and the source line number
  *         where the assert_param error has occurred.
  * @param  file: pointer to the source file name
  * @param  line: assert_param error line source number
  * @retval None
  */
void assert_failed(uint8_t *file, uint32_t line)
{
  /* USER CODE BEGIN 6 */
  /* User can add his own implementation to report the file name and line number,
     ex: printf("Wrong parameters value: file %s on line %d\r\n", file, line) */
  /* USER CODE END 6 */
}
#endif /* USE_FULL_ASSERT */
