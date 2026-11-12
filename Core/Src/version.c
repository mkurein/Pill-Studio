#include "version.h"

/* FW_VERSION is passed by the Makefile (git describe); STM32CubeIDE builds use "cube". */
#ifndef FW_VERSION
#define FW_VERSION "cube"
#endif

const char fw_version[] = FW_VERSION " " __DATE__ " " __TIME__;
