#include "fram.h"

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
    return 0;
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
    uint8_t cmd[3] = { FRAM_READ, (uint8_t)(address >> 8), (uint8_t)(address & 0xFF) };
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, cmd, 3, HAL_MAX_DELAY);
    HAL_SPI_Receive(fram->hspi, buffer, count, HAL_MAX_DELAY);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_SET);
}

void fram_write(fram_t *fram, uint16_t address, uint8_t *buffer, uint16_t count) {
    uint8_t cmd[3] = { FRAM_WRITE, (uint8_t)(address >> 8), (uint8_t)(address & 0xFF) };
    fram_write_enable(fram);
    HAL_GPIO_WritePin(fram->cs_port, fram->cs_pin, GPIO_PIN_RESET);
    HAL_SPI_Transmit(fram->hspi, cmd, 3, HAL_MAX_DELAY);
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


