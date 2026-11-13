#include "fram.h"
#include <string.h>
#include "FreeRTOS.h"
#include "task.h"

extern SPI_HandleTypeDef hspi1;

void fram_cfg_setup(fram_cfg_t *cfg) {
    cfg->sck_port = GPIOA;
    cfg->sck = GPIO_PIN_5;
    cfg->miso_port = GPIOA;
    cfg->miso = GPIO_PIN_6;
    cfg->mosi_port = GPIOA;
    cfg->mosi = GPIO_PIN_7;
    cfg->cs_port = GPIOA;  // for soft CS
    cfg->cs = GPIO_PIN_4;
}

int fram_init(fram_t *fram, fram_cfg_t *cfg) {
	if (!fram || !cfg) return -1; // Проверка указателей
    fram->hspi = &hspi1;          // <-- если SPI1
    fram->cs_port = cfg->cs_port;
    fram->cs_pin = cfg->cs;
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);

    uint8_t cmd = FRAM_RDID, id[3] = {0};
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, &cmd, 1, HAL_MAX_DELAY);
    HAL_SPI_Receive(fram->hspi, id, 3, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
    fram->jedec = ((uint32_t)id[0] << 16) | ((uint32_t)id[1] << 8) | id[2];
    /* NOR: real manufacturer code (not 00/FF, not the 7F continuation used by
     * Cypress FRAM, not Fujitsu 04) and a capacity code of 128 KB..32 MB */
    fram->is_nor = SPI_NOR_SUPPORT && id[0] != 0x00 && id[0] != 0xFF && id[0] != 0x7F &&
                   id[0] != 0x04 && id[2] >= 0x11 && id[2] <= 0x19;
    return 0;
}

const char *fram_kind(const fram_t *fram)
{
    if (!fram->is_nor) return "FRAM";
    if ((fram->jedec >> 8) == 0xEF40) {
        switch (fram->jedec & 0xFF) {
        case 0x15: return "W25Q16";
        case 0x16: return "W25Q32";
        case 0x17: return "W25Q64";
        case 0x18: return "W25Q128";
        default: break;
        }
    }
    return "NOR";
}

/* command + 16-bit (FRAM) or 24-bit (NOR) address, CS stays low */
static void cmd_addr(fram_t *fram, uint8_t op, uint32_t address)
{
    uint8_t cmd[4];
    uint16_t n = 0;
    cmd[n++] = op;
    if (fram->is_nor) cmd[n++] = (uint8_t)(address >> 16);
    cmd[n++] = (uint8_t)(address >> 8);
    cmd[n++] = (uint8_t)address;
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, cmd, n, HAL_MAX_DELAY);
}

void fram_write_enable(fram_t *fram) {
    uint8_t cmd = FRAM_WREN;
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, &cmd, 1, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
}

void fram_write_disable(fram_t *fram) {
    uint8_t cmd = FRAM_WRDI;
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, &cmd, 1, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
}

uint8_t fram_read_status(fram_t *fram) {
    uint8_t cmd = FRAM_RDSR;
    uint8_t status;
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, &cmd, 1, HAL_MAX_DELAY);
    HAL_SPI_Receive(fram->hspi, &status, 1, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
    return status;
}

void fram_write_status(fram_t *fram, uint8_t value) {
    uint8_t cmd[2] = { FRAM_WRSR, value };
    fram_write_enable(fram);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, cmd, 2, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
    fram_write_disable(fram);
}

void fram_read(fram_t *fram, uint16_t address, uint8_t *buffer, uint16_t count) {
    cmd_addr(fram, FRAM_READ, address);
    HAL_SPI_Receive(fram->hspi, buffer, count, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
}

#if SPI_NOR_SUPPORT
/* SR bit 0 (BUSY) after erase/program; sector erase ~45 ms, max 400 ms */
static void nor_wait(fram_t *fram, uint32_t timeout_ms)
{
    uint32_t t0 = HAL_GetTick();
    while ((fram_read_status(fram) & 0x01) && HAL_GetTick() - t0 < timeout_ms) {
        if (xTaskGetSchedulerState() == taskSCHEDULER_RUNNING) vTaskDelay(1);
    }
}

/* Read-modify-write of every 4 KB sector the range touches: contents outside
 * [address, address+count) are preserved. */
static void nor_write(fram_t *fram, uint32_t address, const uint8_t *buffer, uint16_t count)
{
    static uint8_t sec[NOR_SECTOR_SIZE];
    while (count) {
        uint32_t base = address & ~(NOR_SECTOR_SIZE - 1u);
        uint32_t off = address - base;
        uint32_t n = NOR_SECTOR_SIZE - off;
        if (n > count) n = count;

        cmd_addr(fram, FRAM_READ, base);
        HAL_SPI_Receive(fram->hspi, sec, NOR_SECTOR_SIZE, HAL_MAX_DELAY);
        HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
        memcpy(&sec[off], buffer, n);

        fram_write_enable(fram);
        cmd_addr(fram, NOR_SECTOR_ERASE, base);
        HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
        nor_wait(fram, 500);

        for (uint32_t p = 0; p < NOR_SECTOR_SIZE; p += NOR_PAGE_SIZE) {
            fram_write_enable(fram);
            cmd_addr(fram, FRAM_WRITE, base + p);
            HAL_SPI_Transmit(fram->hspi, &sec[p], NOR_PAGE_SIZE, HAL_MAX_DELAY);
            HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
            nor_wait(fram, 10);
        }
        address += n; buffer += n; count = (uint16_t)(count - n);
    }
}
#endif

void fram_write(fram_t *fram, uint16_t address, uint8_t *buffer, uint16_t count) {
#if SPI_NOR_SUPPORT
    if (fram->is_nor) { nor_write(fram, address, buffer, count); return; }
#endif
    fram_write_enable(fram);
    cmd_addr(fram, FRAM_WRITE, address);
    HAL_SPI_Transmit(fram->hspi, buffer, count, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
    fram_write_disable(fram);
}

/* Was: uint8_t empty_data[FRAM_MEM_SIZE] on the stack (32 KB > 20 KB RAM) ->
 * guaranteed stack overflow. Now writes zeros in small chunks. */
void fram_erase_all(fram_t *fram) {
    uint8_t zeros[64] = {0};
    for (uint32_t addr = 0; addr < FRAM_MEM_SIZE; addr += sizeof(zeros)) {
        fram_write(fram, (uint16_t)addr, zeros, sizeof(zeros));
    }
}

/* Presence check without touching memory contents: WREN must set the WEL bit
 * (SR bit 1) and WRDI must clear it. With no chip / broken SPI wiring MISO
 * reads constant 0x00 or 0xFF and this fails. sr_out gets {SR after WREN, SR after WRDI}. */
int fram_probe(fram_t *fram, uint8_t sr_out[2]) {
    fram_write_enable(fram);
    uint8_t sr1 = fram_read_status(fram);
    fram_write_disable(fram);
    uint8_t sr2 = fram_read_status(fram);
    if (sr_out) { sr_out[0] = sr1; sr_out[1] = sr2; }
    return ((sr1 & 0x02) && !(sr2 & 0x02)) ? 0 : -1;
}


