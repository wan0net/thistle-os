// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — CST328 capacitive touch controller driver
//
// The CST328 uses 16-bit register addresses (big-endian) over I2C.
// Up to 5 simultaneous touch points are reported; we track a single
// primary touch for the HAL event model.

#include "drv_touch_cst328.h"

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"
#include "esp_timer.h"

#include <string.h>
#include <stdatomic.h>

static const char *TAG = "cst328";

/* ── CST328 register addresses (16-bit) ─────────────────────────────────── */
#define CST328_REG_TOUCH_INFO  0xD000   /* Number of touch points (1 byte)  */
/* Touch point 1 layout at 0xD001 (7 bytes per point):
 *  [0] x_high  (bits [11:8] in [3:0])
 *  [1] x_low   (bits [7:0])
 *  [2] y_high  (bits [11:8] in [3:0])
 *  [3] y_low   (bits [7:0])
 *  [4] touch_id and event type
 *  [5] pressure (unused on many panels)
 *  [6] area     (unused on many panels)
 */
#define CST328_REG_TOUCH_PT1   0xD001
#define CST328_REG_TOUCH_PT2   0xD008
#define CST328_REG_TOUCH_PT3   0xD00F
#define CST328_REG_TOUCH_PT4   0xD016
#define CST328_REG_TOUCH_PT5   0xD01D

#define CST328_REG_MODULE_VER  0xD100   /* Module version (2 bytes) */
#define CST328_REG_COMMAND     0xD109   /* Write 0xAB = normal mode  */

#define CST328_CMD_NORMAL_MODE 0xAB

/* Touch point raw data length (bytes per point) */
#define CST328_PT_LEN          7

/* ── Driver state ────────────────────────────────────────────────────────── */
static struct {
    i2c_master_dev_handle_t dev;
    touch_cst328_config_t   cfg;
    hal_input_cb_t          cb;
    void                   *cb_data;
    volatile atomic_bool    irq_pending;
    bool                    touching;        /* true if finger currently down */
    uint16_t                last_x, last_y;
    bool                    initialised;
} s_touch;

/* ── I2C helpers ─────────────────────────────────────────────────────────── */
/*
 * CST328 uses 16-bit register addresses transmitted MSB-first.
 */

static esp_err_t cst328_write_reg(uint16_t reg_addr, uint8_t val)
{
    uint8_t buf[3] = {
        (uint8_t)(reg_addr >> 8),
        (uint8_t)(reg_addr & 0xFF),
        val,
    };
    esp_err_t ret = i2c_master_transmit(s_touch.dev, buf, sizeof(buf), 50);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "write 0x%04X failed: %s", reg_addr, esp_err_to_name(ret));
    }
    return ret;
}

static esp_err_t cst328_read_regs(uint16_t reg_addr, uint8_t *buf, size_t len)
{
    uint8_t addr_buf[2] = {
        (uint8_t)(reg_addr >> 8),
        (uint8_t)(reg_addr & 0xFF),
    };
    esp_err_t ret = i2c_master_transmit_receive(s_touch.dev,
                                                addr_buf, sizeof(addr_buf),
                                                buf, len, 50);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "read 0x%04X failed: %s", reg_addr, esp_err_to_name(ret));
    }
    return ret;
}

/* ── ISR ─────────────────────────────────────────────────────────────────── */

static void IRAM_ATTR cst328_isr_handler(void *arg)
{
    (void)arg;
    atomic_store(&s_touch.irq_pending, true);
}

/* ── Hardware reset ──────────────────────────────────────────────────────── */

static void cst328_hw_reset(void)
{
    if (s_touch.cfg.pin_rst == GPIO_NUM_NC) return;
    gpio_set_level(s_touch.cfg.pin_rst, 0);
    vTaskDelay(pdMS_TO_TICKS(20));
    gpio_set_level(s_touch.cfg.pin_rst, 1);
    vTaskDelay(pdMS_TO_TICKS(100));
}

/* ── Init ────────────────────────────────────────────────────────────────── */

static esp_err_t cst328_init(const void *config)
{
    if (!config) {
        ESP_LOGE(TAG, "init: NULL config");
        return ESP_ERR_INVALID_ARG;
    }
    if (s_touch.initialised) {
        ESP_LOGW(TAG, "already initialised");
        return ESP_OK;
    }

    memcpy(&s_touch.cfg, config, sizeof(touch_cst328_config_t));
    atomic_store(&s_touch.irq_pending, false);
    s_touch.touching = false;

    /* ── Optional reset pin ── */
    if (s_touch.cfg.pin_rst != GPIO_NUM_NC) {
        gpio_config_t rst_cfg = {
            .mode         = GPIO_MODE_OUTPUT,
            .pull_up_en   = GPIO_PULLUP_DISABLE,
            .pull_down_en = GPIO_PULLDOWN_DISABLE,
            .intr_type    = GPIO_INTR_DISABLE,
            .pin_bit_mask = 1ULL << s_touch.cfg.pin_rst,
        };
        ESP_ERROR_CHECK(gpio_config(&rst_cfg));
        cst328_hw_reset();
    }

    /* ── Add I2C device ── */
    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address  = s_touch.cfg.i2c_addr,
        .scl_speed_hz    = 400000,
    };
    esp_err_t ret = i2c_master_bus_add_device(s_touch.cfg.i2c_bus, &dev_cfg, &s_touch.dev);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2c_master_bus_add_device failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* ── Verify chip presence by reading module version ── */
    uint8_t ver[2] = {0, 0};
    ret = cst328_read_regs(CST328_REG_MODULE_VER, ver, sizeof(ver));
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "failed to read module version — check wiring/address");
        goto fail;
    }
    ESP_LOGI(TAG, "CST328 module version: 0x%02X%02X", ver[0], ver[1]);

    /* Put controller into normal operating mode */
    ret = cst328_write_reg(CST328_REG_COMMAND, CST328_CMD_NORMAL_MODE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "failed to set normal mode");
        goto fail;
    }
    vTaskDelay(pdMS_TO_TICKS(10));

    /* ── Optional interrupt pin ── */
    if (s_touch.cfg.pin_int != GPIO_NUM_NC) {
        gpio_config_t int_cfg = {
            .mode         = GPIO_MODE_INPUT,
            .pull_up_en   = GPIO_PULLUP_ENABLE,
            .intr_type    = GPIO_INTR_NEGEDGE,
            .pin_bit_mask = 1ULL << s_touch.cfg.pin_int,
        };
        ret = gpio_config(&int_cfg);
        if (ret != ESP_OK) goto fail;

        gpio_install_isr_service(0);   /* idempotent */
        ret = gpio_isr_handler_add(s_touch.cfg.pin_int, cst328_isr_handler, NULL);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "failed to add ISR: %s", esp_err_to_name(ret));
            goto fail;
        }
    }

    s_touch.initialised = true;
    ESP_LOGI(TAG, "CST328 initialised, addr=0x%02X, max=(%u,%u)",
             s_touch.cfg.i2c_addr, s_touch.cfg.max_x, s_touch.cfg.max_y);
    return ESP_OK;

fail:
    i2c_master_bus_rm_device(s_touch.dev);
    s_touch.dev = NULL;
    return ret;
}

/* ── Deinit ──────────────────────────────────────────────────────────────── */

static void cst328_deinit(void)
{
    if (!s_touch.initialised) return;

    if (s_touch.cfg.pin_int != GPIO_NUM_NC) {
        gpio_isr_handler_remove(s_touch.cfg.pin_int);
    }

    i2c_master_bus_rm_device(s_touch.dev);
    s_touch.dev          = NULL;
    s_touch.cb           = NULL;
    s_touch.cb_data      = NULL;
    s_touch.touching     = false;
    s_touch.initialised  = false;
    ESP_LOGI(TAG, "deinit complete");
}

/* ── register_callback ───────────────────────────────────────────────────── */

static esp_err_t cst328_register_callback(hal_input_cb_t cb, void *user_data)
{
    s_touch.cb      = cb;
    s_touch.cb_data = user_data;
    return ESP_OK;
}

/* ── poll ────────────────────────────────────────────────────────────────── */

static esp_err_t cst328_poll(void)
{
    if (!s_touch.initialised) return ESP_ERR_INVALID_STATE;

    /* If using interrupt, skip when no edge seen (and not currently touching,
     * since we must still detect lift-off which may not trigger a new IRQ) */
    if (s_touch.cfg.pin_int != GPIO_NUM_NC) {
        if (!atomic_load(&s_touch.irq_pending) && !s_touch.touching) {
            return ESP_OK;
        }
    }

    /* Read number of active touch points */
    uint8_t n_touches = 0;
    esp_err_t ret = cst328_read_regs(CST328_REG_TOUCH_INFO, &n_touches, 1);
    if (ret != ESP_OK) {
        /* Don't spam errors on I2C noise */
        return ret;
    }
    n_touches &= 0x0F;   /* lower nibble is touch count on most variants */

    uint32_t now_ms = (uint32_t)(esp_timer_get_time() / 1000);

    if (n_touches > 0) {
        /* Read primary touch point data */
        uint8_t pt[CST328_PT_LEN] = {0};
        ret = cst328_read_regs(CST328_REG_TOUCH_PT1, pt, sizeof(pt));
        if (ret != ESP_OK) return ret;

        /* Extract 12-bit X and Y */
        uint16_t x = ((uint16_t)(pt[0] & 0x0F) << 8) | pt[1];
        uint16_t y = ((uint16_t)(pt[2] & 0x0F) << 8) | pt[3];

        /* Clamp to panel dimensions */
        if (x >= s_touch.cfg.max_x) x = s_touch.cfg.max_x - 1;
        if (y >= s_touch.cfg.max_y) y = s_touch.cfg.max_y - 1;

        hal_input_event_type_t ev_type;
        if (!s_touch.touching) {
            ev_type         = HAL_INPUT_EVENT_TOUCH_DOWN;
            s_touch.touching = true;
        } else {
            ev_type = HAL_INPUT_EVENT_TOUCH_MOVE;
        }
        s_touch.last_x = x;
        s_touch.last_y = y;

        if (s_touch.cb) {
            hal_input_event_t event = {
                .type      = ev_type,
                .timestamp = now_ms,
                .touch     = { .x = x, .y = y },
            };
            s_touch.cb(&event, s_touch.cb_data);
        }

    } else {
        /* No touches — emit UP if we were previously touching */
        if (s_touch.touching) {
            s_touch.touching = false;
            if (s_touch.cb) {
                hal_input_event_t event = {
                    .type      = HAL_INPUT_EVENT_TOUCH_UP,
                    .timestamp = now_ms,
                    .touch     = { .x = s_touch.last_x, .y = s_touch.last_y },
                };
                s_touch.cb(&event, s_touch.cb_data);
            }
        }
    }

    atomic_store(&s_touch.irq_pending, false);
    return ESP_OK;
}

/* ── Vtable ──────────────────────────────────────────────────────────────── */

static const hal_input_driver_t cst328_driver = {
    .init              = cst328_init,
    .deinit            = cst328_deinit,
    .register_callback = cst328_register_callback,
    .poll              = cst328_poll,
    .name              = "CST328",
    .is_touch          = true,
};

const hal_input_driver_t *drv_touch_cst328_get(void)
{
    return &cst328_driver;
}
