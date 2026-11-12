# Command-line build for macOS/Linux (arm-none-eabi-gcc from PATH).
# STM32CubeIDE keeps using its own Debug/ makefiles; this one writes to build/.
#
#   make            - build build/primGPT.elf/.hex/.bin
#   make flash      - program via ST-Link (OpenOCD) and reset
#   make clean

TARGET   := primGPT
BUILD    := build
PREFIX   ?= arm-none-eabi-
CC       := $(PREFIX)gcc
OBJCOPY  := $(PREFIX)objcopy
SIZE     := $(PREFIX)size

FW_VERSION ?= $(shell git describe --always --dirty 2>/dev/null || echo unknown)

C_SOURCES := \
  $(wildcard Core/Src/*.c) \
  $(wildcard Drivers/STM32F1xx_HAL_Driver/Src/*.c) \
  Middlewares/Third_Party/FreeRTOS/Source/croutine.c \
  Middlewares/Third_Party/FreeRTOS/Source/event_groups.c \
  Middlewares/Third_Party/FreeRTOS/Source/list.c \
  Middlewares/Third_Party/FreeRTOS/Source/queue.c \
  Middlewares/Third_Party/FreeRTOS/Source/stream_buffer.c \
  Middlewares/Third_Party/FreeRTOS/Source/tasks.c \
  Middlewares/Third_Party/FreeRTOS/Source/timers.c \
  Middlewares/Third_Party/FreeRTOS/Source/CMSIS_RTOS_V2/cmsis_os2.c \
  Middlewares/Third_Party/FreeRTOS/Source/portable/MemMang/heap_4.c \
  Middlewares/Third_Party/FreeRTOS/Source/portable/GCC/ARM_CM3/port.c

ASM_SOURCES := Core/Startup/startup_stm32f103c8tx.s

INCLUDES := \
  -ICore/Inc \
  -IDrivers/STM32F1xx_HAL_Driver/Inc \
  -IDrivers/STM32F1xx_HAL_Driver/Inc/Legacy \
  -IDrivers/CMSIS/Device/ST/STM32F1xx/Include \
  -IDrivers/CMSIS/Include \
  -IMiddlewares/Third_Party/FreeRTOS/Source/include \
  -IMiddlewares/Third_Party/FreeRTOS/Source/CMSIS_RTOS_V2 \
  -IMiddlewares/Third_Party/FreeRTOS/Source/portable/GCC/ARM_CM3

MCU     := -mcpu=cortex-m3 -mthumb
DEFS    := -DUSE_HAL_DRIVER -DSTM32F103xB -DDEBUG -DFW_VERSION=\"$(FW_VERSION)\"
CFLAGS  := $(MCU) $(DEFS) $(INCLUDES) -std=gnu11 -g3 -Og -Wall \
           -ffunction-sections -fdata-sections -MMD -MP
ASFLAGS := $(MCU) -x assembler-with-cpp -g3
LDFLAGS := $(MCU) -TSTM32F103C8TX_FLASH.ld --specs=nano.specs --specs=nosys.specs \
           -Wl,--gc-sections -Wl,-Map=$(BUILD)/$(TARGET).map -Wl,--print-memory-usage \
           -Wl,--no-warn-rwx-segments -lc -lm

OBJECTS := $(addprefix $(BUILD)/,$(C_SOURCES:.c=.o)) $(addprefix $(BUILD)/,$(ASM_SOURCES:.s=.o))

all: $(BUILD)/$(TARGET).elf $(BUILD)/$(TARGET).hex $(BUILD)/$(TARGET).bin

$(BUILD)/%.o: %.c Makefile
	@mkdir -p $(dir $@)
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD)/%.o: %.s Makefile
	@mkdir -p $(dir $@)
	$(CC) -c $(ASFLAGS) $< -o $@

$(BUILD)/$(TARGET).elf: $(OBJECTS)
	$(CC) $(OBJECTS) $(LDFLAGS) -o $@
	$(SIZE) $@

$(BUILD)/%.hex: $(BUILD)/%.elf
	$(OBJCOPY) -O ihex $< $@

$(BUILD)/%.bin: $(BUILD)/%.elf
	$(OBJCOPY) -O binary -S $< $@

# version.o must pick up a new git hash / build time on every build
$(BUILD)/Core/Src/version.o: FORCE
FORCE:

flash: all
	.venv/bin/python tools/hil.py flash --no-build

clean:
	rm -rf $(BUILD)

.PHONY: all flash clean FORCE
-include $(OBJECTS:.o=.d)
