// SPDX-License-Identifier: BSD-3-Clause
// epaper_spi_shim.c — GCC-compiled GPIO/SPI/delay helpers for the Rust e-paper driver.
//
// WHY THIS FILE EXISTS
// ====================
// The LLVM Xtensa backend (used to compile Rust for esp32s3) does not always
// reserve the mandatory 16-byte "overflow save area" at the top of each stack
// frame.  GCC for Xtensa always does.
//
// On Xtensa LX7, when 8+ CALL8 frames are live simultaneously,
// WindowOverflow8 fires and saves the oldest frame's registers to
// [SP_oldest - 16] .. [SP_oldest - 4].  This save area overlaps with the
// TOP 16 bytes of the SECOND-OLDEST frame's stack allocation.  If that frame
// is a Rust/LLVM function that didn't reserve those bytes, the overflow
// handler writes into live locals, and WindowUnderflow8 later restores
// corrupted register values → crash (StoreProhibited, InstrFetchProhibited).
//
// The init call chain from FreeRTOS to gdeq031t10_init is already 7 frames
// deep.  Adding 3–4 more for IDF SPI → ROM pushes the total to 10–11, which
// fires WindowOverflow8 multiple times through Rust frames that lack the
// reservation.  Placing all IDF calls (GPIO, SPI, delay) in this
// GCC-compiled shim gives every callee frame a correct 16-byte reserve, so
// overflow always saves to properly reserved space.

#include <stdint.h>
#include <stddef.h>

#include "driver/spi_master.h"
#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_err.h"

// ── GPIO helpers ─────────────────────────────────────────────────────────────

// Configure a set of output GPIOs (CS, DC, and optionally RST).
// pin_rst = -1 means RST is not connected.
int epaper_gpio_config_outputs(int pin_cs, int pin_dc, int pin_rst)
{
    uint64_t mask = (1ULL << pin_cs) | (1ULL << pin_dc);
    if (pin_rst >= 0) mask |= (1ULL << pin_rst);
    gpio_config_t io = {
        .pin_bit_mask = mask,
        .mode         = GPIO_MODE_OUTPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
    };
    return gpio_config(&io);
}

// Configure the BUSY input GPIO with pull-up.
int epaper_gpio_config_busy(int pin_busy)
{
    gpio_config_t io = {
        .pin_bit_mask = (1ULL << pin_busy),
        .mode         = GPIO_MODE_INPUT,
        .pull_up_en   = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
    };
    return gpio_config(&io);
}

// Set a GPIO output level (pin ≥ 0 guard).
void epaper_gpio_set(int pin, uint32_t level)
{
    if (pin >= 0) gpio_set_level((gpio_num_t)pin, level);
}

// Read a GPIO level (returns 0 if pin < 0).
int epaper_gpio_get(int pin)
{
    return (pin >= 0) ? gpio_get_level((gpio_num_t)pin) : 0;
}

// FreeRTOS delay (milliseconds → ticks, 1 ms / tick at 1000 Hz).
void epaper_delay_ms(uint32_t ms)
{
    vTaskDelay(pdMS_TO_TICKS(ms));
}

// ── SPI helpers ───────────────────────────────────────────────────────────────

// Send a single command byte via SPI (DC=0, CS toggled around the transfer).
// `spi_handle` is an opaque spi_device_handle_t cast to void*.
int epaper_spi_cmd(void *spi_handle, int pin_cs, int pin_dc, uint8_t cmd)
{
    gpio_set_level((gpio_num_t)pin_cs, 0);
    gpio_set_level((gpio_num_t)pin_dc, 0);
    spi_transaction_t t = {
        .length    = 8,
        .tx_buffer = &cmd,
        .rx_buffer = NULL,
    };
    int ret = spi_device_polling_transmit((spi_device_handle_t)spi_handle, &t);
    gpio_set_level((gpio_num_t)pin_cs, 1);
    return ret;
}

// Send data bytes via SPI (DC=1, CS toggled around the transfer).
// Sends in 4096-byte chunks to stay within ESP-IDF SPI limits.
int epaper_spi_data(void *spi_handle, int pin_cs, int pin_dc,
                    const uint8_t *data, size_t len)
{
    if (len == 0) return ESP_OK;
    gpio_set_level((gpio_num_t)pin_cs, 0);
    gpio_set_level((gpio_num_t)pin_dc, 1);
    int ret = ESP_OK;
    size_t sent = 0;
    while (sent < len && ret == ESP_OK) {
        size_t chunk = len - sent;
        if (chunk > 4096) chunk = 4096;
        spi_transaction_t t = {
            .length    = chunk * 8,
            .tx_buffer = data + sent,
            .rx_buffer = NULL,
        };
        ret = spi_device_polling_transmit((spi_device_handle_t)spi_handle, &t);
        sent += chunk;
    }
    gpio_set_level((gpio_num_t)pin_cs, 1);
    return ret;
}
