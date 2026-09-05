#include "board_fingerprint.h"

bool board_fingerprint_twatch_ultra_complete(int sda, int scl,
                                              uint8_t observed_devices)
{
    return sda == 3 && scl == 2 &&
           (observed_devices & TWATCH_ULTRA_I2C_REQUIRED) ==
               TWATCH_ULTRA_I2C_REQUIRED;
}
