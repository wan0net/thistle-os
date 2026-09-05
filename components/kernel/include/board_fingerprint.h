#pragma once

#include <stdbool.h>
#include <stdint.h>

#define TWATCH_ULTRA_I2C_CST9217    (1u << 0)
#define TWATCH_ULTRA_I2C_XL9555     (1u << 1)
#define TWATCH_ULTRA_I2C_BHI260AP   (1u << 2)
#define TWATCH_ULTRA_I2C_AXP2101    (1u << 3)
#define TWATCH_ULTRA_I2C_PCF85063A  (1u << 4)
#define TWATCH_ULTRA_I2C_DRV2605    (1u << 5)

#define TWATCH_ULTRA_I2C_REQUIRED \
    (TWATCH_ULTRA_I2C_CST9217 | TWATCH_ULTRA_I2C_XL9555 | \
     TWATCH_ULTRA_I2C_BHI260AP | TWATCH_ULTRA_I2C_AXP2101 | \
     TWATCH_ULTRA_I2C_PCF85063A | TWATCH_ULTRA_I2C_DRV2605)

bool board_fingerprint_twatch_ultra_complete(int sda, int scl,
                                              uint8_t observed_devices);
