#include "board_fingerprint.h"

#include <assert.h>
#include <stdio.h>

int main(void)
{
    assert(board_fingerprint_twatch_ultra_complete(
        3, 2, TWATCH_ULTRA_I2C_REQUIRED));
    assert(!board_fingerprint_twatch_ultra_complete(
        3, 2, TWATCH_ULTRA_I2C_REQUIRED & ~TWATCH_ULTRA_I2C_AXP2101));
    assert(!board_fingerprint_twatch_ultra_complete(
        13, 14, TWATCH_ULTRA_I2C_REQUIRED));
    assert(board_fingerprint_twatch_ultra_complete(
        3, 2, TWATCH_ULTRA_I2C_REQUIRED | (1u << 7)));

    puts("board fingerprint tests passed");
    return 0;
}
