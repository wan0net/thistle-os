// SPDX-License-Identifier: BSD-3-Clause

#include <assert.h>
#include <stdint.h>
#include <string.h>

#include "../src/packed_blit.h"

#define WIDTH 240
#define HEIGHT 10
#define ROW_BYTES (WIDTH / 8)

int main(void)
{
    uint8_t framebuffer[ROW_BYTES * HEIGHT] = {0};

    const uint8_t three_by_two[] = {0xA0, 0x40};
    epaper_blit_packed_rect(framebuffer, WIDTH, 1, 2, 3, 3, three_by_two);
    assert(framebuffer[2 * ROW_BYTES] == 0x50);
    assert(framebuffer[3 * ROW_BYTES] == 0x20);

    memset(framebuffer, 0, sizeof(framebuffer));
    const uint8_t nine_by_two[] = {0xAA, 0x80, 0x55, 0x00};
    epaper_blit_packed_rect(framebuffer, WIDTH, 8, 4, 16, 5, nine_by_two);
    assert(framebuffer[4 * ROW_BYTES + 1] == 0xAA);
    assert(framebuffer[4 * ROW_BYTES + 2] == 0x80);
    assert(framebuffer[5 * ROW_BYTES + 1] == 0x55);
    assert(framebuffer[5 * ROW_BYTES + 2] == 0x00);

    memset(framebuffer, 0, sizeof(framebuffer));
    uint8_t full_width[ROW_BYTES * 2];
    memset(full_width, 0xA5, ROW_BYTES);
    memset(full_width + ROW_BYTES, 0x5A, ROW_BYTES);
    epaper_blit_packed_rect(framebuffer, WIDTH, 0, 7, WIDTH - 1, 8, full_width);
    assert(memcmp(framebuffer + 7 * ROW_BYTES, full_width, sizeof(full_width)) == 0);

    return 0;
}
