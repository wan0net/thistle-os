// SPDX-License-Identifier: BSD-3-Clause
// TCA8418 I2C Keyboard Driver — standalone .drv.elf
//
// This is the first ThistleOS driver compiled as a standalone loadable ELF.
// It reads its config from board.json via thistle_driver_get_config(),
// gets the I2C bus handle from the HAL, and registers as an input driver.

#include <stddef.h>
#include "thistle_driver.h"
#include "hal/input.h"

#define TAG "tca8418"

// TCA8418 registers
#define TCA8418_REG_CFG         0x01
#define TCA8418_REG_INT_STAT    0x02
#define TCA8418_REG_KEY_LCK_EC  0x03
#define TCA8418_REG_KEY_EVENT_A 0x04

// Driver state
static void *s_i2c_dev = NULL;
static int   s_int_pin = -1;
static hal_input_cb_t s_input_cb = NULL;
static void *s_input_cb_data = NULL;

// Simple JSON string extractor (matches kernel's pattern)
static int json_get_int_val(const char *json, const char *key) {
    char pattern[64];
    int i = 0;
    pattern[i++] = '"';
    for (const char *k = key; *k; k++) pattern[i++] = *k;
    pattern[i++] = '"';
    pattern[i] = '\0';

    const char *p = json;
    while (*p) {
        const char *found = p;
        int match = 1;
        for (int j = 0; pattern[j]; j++) {
            if (found[j] != pattern[j]) { match = 0; break; }
        }
        if (match) {
            p = found + i;
            while (*p == ' ' || *p == ':' || *p == '\t') p++;
            // Handle hex "0x34" or decimal
            if (p[0] == '"' && p[1] == '0' && p[2] == 'x') {
                // Hex string like "0x34"
                p += 3;
                int val = 0;
                while (*p != '"' && *p) {
                    int c = *p++;
                    if (c >= '0' && c <= '9') val = val * 16 + (c - '0');
                    else if (c >= 'a' && c <= 'f') val = val * 16 + (c - 'a' + 10);
                    else if (c >= 'A' && c <= 'F') val = val * 16 + (c - 'A' + 10);
                }
                return val;
            }
            // Decimal
            int val = 0;
            int neg = 0;
            if (*p == '"') p++;
            if (*p == '-') { neg = 1; p++; }
            while (*p >= '0' && *p <= '9') { val = val * 10 + (*p - '0'); p++; }
            return neg ? -val : val;
        }
        p++;
    }
    return -1;
}

// I2C read/write helpers
static int tca_read_reg(unsigned char reg, unsigned char *val) {
    return i2c_master_transmit_receive(s_i2c_dev, &reg, 1, val, 1, 100);
}

static int tca_write_reg(unsigned char reg, unsigned char val) {
    unsigned char buf[2] = { reg, val };
    return i2c_master_transmit(s_i2c_dev, buf, 2, 100);
}

// Key event to ASCII mapping (simplified — T-Deck Pro layout)
static unsigned short key_event_to_ascii(unsigned char event) {
    unsigned char key = event & 0x7F;
    // Simplified: map key codes 1-26 to 'a'-'z'
    if (key >= 1 && key <= 26) return 'a' + key - 1;
    if (key == 27) return ' ';
    if (key == 28) return '\n';
    if (key == 29) return '\b';
    if (key == 30) return 0x1B; // ESC
    return 0;
}

// HAL input driver callbacks
static int kbd_init(const void *config) {
    (void)config;
    if (!s_i2c_dev) return -1;

    // Configure TCA8418: enable key scan, interrupt on events
    tca_write_reg(TCA8418_REG_CFG, 0x11); // KE_IEN + AI

    thistle_log(TAG, "TCA8418 initialized on I2C");
    return 0;
}

static void kbd_deinit(void) {
    s_i2c_dev = NULL;
}

static int kbd_register_cb(hal_input_cb_t cb, void *user_data) {
    s_input_cb = cb;
    s_input_cb_data = user_data;
    return 0;
}

static int kbd_poll(void) {
    if (!s_i2c_dev || !s_input_cb) return 0;

    // Read key event count
    unsigned char key_lck_ec = 0;
    if (tca_read_reg(TCA8418_REG_KEY_LCK_EC, &key_lck_ec) != 0) return 0;

    int event_count = key_lck_ec & 0x0F;
    if (event_count == 0) return 0;

    // Drain the FIFO
    for (int i = 0; i < event_count; i++) {
        unsigned char event = 0;
        if (tca_read_reg(TCA8418_REG_KEY_EVENT_A, &event) != 0) break;
        if (event == 0) break;

        unsigned char pressed = (event & 0x80) ? 1 : 0;
        unsigned short keycode = key_event_to_ascii(event);
        if (keycode == 0) continue;

        // Create HAL input event
        hal_input_event_t evt;
        evt.type = pressed ? HAL_INPUT_EVENT_KEY_DOWN : HAL_INPUT_EVENT_KEY_UP;
        evt.timestamp = thistle_millis();
        evt.key.keycode = keycode;
        s_input_cb(&evt, s_input_cb_data);
    }

    // Clear interrupt
    tca_write_reg(TCA8418_REG_INT_STAT, 0x0F);

    return 0;
}

// HAL vtable
static const hal_input_driver_t kbd_driver = {
    .init = kbd_init,
    .deinit = kbd_deinit,
    .register_callback = kbd_register_cb,
    .poll = kbd_poll,
    .name = "TCA8418 Keyboard (loadable)",
    .is_touch = 0,
};

// ── Entry point (called by kernel's driver loader) ──────────────────

int driver_init(void)
{
    // Get config JSON from board.json
    const char *config = thistle_driver_get_config();
    thistle_log(TAG, "Config: %s");

    // Parse I2C bus index and address from config
    int i2c_bus_idx = json_get_int_val(config, "i2c_bus");
    int i2c_addr    = json_get_int_val(config, "i2c_addr");
    s_int_pin       = json_get_int_val(config, "int_pin");

    if (i2c_bus_idx < 0) i2c_bus_idx = 0;
    if (i2c_addr < 0) i2c_addr = 0x34;

    thistle_log(TAG, "I2C bus=%d addr=0x%x int=%d");

    // Get the shared I2C bus handle from HAL
    void *i2c_bus = hal_bus_get_i2c(i2c_bus_idx);
    if (!i2c_bus) {
        thistle_log(TAG, "No I2C bus available");
        return -1;
    }

    // Add this device to the I2C bus
    // The config struct for i2c_master_bus_add_device is platform-specific.
    // For now, we store the bus handle and use direct I2C transactions.
    // A proper implementation would call i2c_master_bus_add_device().
    s_i2c_dev = i2c_bus; // Simplified — use bus handle directly

    // Register with HAL
    int ret = hal_input_register(&kbd_driver, NULL);
    if (ret != 0) {
        thistle_log(TAG, "HAL registration failed");
        return ret;
    }

    thistle_log(TAG, "TCA8418 keyboard driver loaded");
    return 0;
}
