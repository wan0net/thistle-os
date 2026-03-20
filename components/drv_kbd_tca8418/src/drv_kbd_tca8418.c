// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — TCA8418 I2C keyboard matrix driver
//
// The TCA8418 scans up to an 8x10 key matrix and buffers key events in an
// 10-deep FIFO.  We expose an interrupt-driven poll() call: if pin_int is
// wired up the ISR sets a flag so that the poll is cheap when idle.

#include "drv_kbd_tca8418.h"

#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/i2c_master.h"
#include "driver/gpio.h"
#include "esp_timer.h"

#include <string.h>
#include <stdatomic.h>

static const char *TAG = "tca8418";

/* ── TCA8418 register map ─────────────────────────────────────────────────*/
#define TCA8418_REG_CFG         0x01   /* Configuration                   */
#define TCA8418_REG_INT_STAT    0x02   /* Interrupt status                */
#define TCA8418_REG_KEY_LCK_EC  0x03   /* Key-lock + event count          */
#define TCA8418_REG_KEY_EVENT_A 0x04   /* Key event FIFO (read repeatedly)*/
#define TCA8418_REG_KEY_EVENT_B 0x05
#define TCA8418_REG_KEY_EVENT_C 0x06
#define TCA8418_REG_KEY_EVENT_D 0x07
#define TCA8418_REG_KEY_EVENT_E 0x08
#define TCA8418_REG_KEY_EVENT_F 0x09
#define TCA8418_REG_KEY_EVENT_G 0x0A
#define TCA8418_REG_KEY_EVENT_H 0x0B
#define TCA8418_REG_KEY_EVENT_I 0x0C
#define TCA8418_REG_KEY_EVENT_J 0x0D
#define TCA8418_REG_KP_LCK_TMR  0x0E
#define TCA8418_REG_UNLOCK1     0x0F
#define TCA8418_REG_UNLOCK2     0x10
#define TCA8418_REG_GPIO_INT_STAT1 0x11
#define TCA8418_REG_GPIO_INT_STAT2 0x12
#define TCA8418_REG_GPIO_INT_STAT3 0x13
#define TCA8418_REG_GPIO_DAT_STAT1 0x14
#define TCA8418_REG_GPIO_DAT_STAT2 0x15
#define TCA8418_REG_GPIO_DAT_STAT3 0x16
#define TCA8418_REG_GPIO_DAT_OUT1  0x17
#define TCA8418_REG_GPIO_DAT_OUT2  0x18
#define TCA8418_REG_GPIO_DAT_OUT3  0x19
#define TCA8418_REG_GPIO_INT_LVL1  0x1A
#define TCA8418_REG_GPIO_INT_LVL2  0x1B
#define TCA8418_REG_GPIO_INT_LVL3  0x1C
#define TCA8418_REG_DEBOUNCE_DIS1  0x1D
#define TCA8418_REG_DEBOUNCE_DIS2  0x1E
#define TCA8418_REG_DEBOUNCE_DIS3  0x1F
#define TCA8418_REG_GPIO_PULL1     0x20
#define TCA8418_REG_GPIO_PULL2     0x21
#define TCA8418_REG_GPIO_PULL3     0x22
/* KP_GPIO selects whether each row/col is keypad or GPIO */
#define TCA8418_REG_KP_GPIO1       0x1D   /* rows R0-R7 / cols C0-C7 */
#define TCA8418_REG_KP_GPIO2       0x1E   /* cols C8-C9              */
#define TCA8418_REG_KP_GPIO3       0x1F

/* CFG bits */
#define TCA8418_CFG_KE_IEN   (1 << 0)   /* Key event interrupt enable */
#define TCA8418_CFG_GPI_IEN  (1 << 1)   /* GPIO interrupt enable       */
#define TCA8418_CFG_K_LCK_IEN (1 << 2)  /* Key-lock interrupt enable   */
#define TCA8418_CFG_OVR_FLOW_IEN (1 << 3)
#define TCA8418_CFG_INT_CFG  (1 << 4)   /* Interrupt output config     */
#define TCA8418_CFG_OVR_FLOW_M  (1 << 5)
#define TCA8418_CFG_TIMEOUTM (1 << 6)
#define TCA8418_CFG_AI       (1 << 7)   /* Auto-increment              */

/* INT_STAT bits */
#define TCA8418_INT_STAT_K_INT  (1 << 0)
#define TCA8418_INT_STAT_GPI_INT (1 << 1)
#define TCA8418_INT_STAT_K_LCK_INT (1 << 2)
#define TCA8418_INT_STAT_OVR_FLOW_INT (1 << 3)
#define TCA8418_INT_STAT_CAD_INT (1 << 4)

/* Key event bits */
#define KEY_EVENT_PRESS   0x80   /* 1 = press, 0 = release */
#define KEY_EVENT_KEY_MSK 0x7F

/* ── Keymap ──────────────────────────────────────────────────────────────
 * TCA8418 encodes key position as: key_code = row*10 + col + 1  (1-based)
 * Rows R0-R7 (8 rows), Cols C0-C9 (10 cols).
 * Map to ASCII / special keycodes.  0 = unmapped.
 */
/* clang-format off */
static const uint16_t KEY_MAP[8][10] = {
    /* C0     C1     C2     C3     C4     C5     C6     C7     C8     C9  */
    {  'q',  'w',  'e',  'r',  't',  'y',  'u',  'i',  'o',  'p'  },
    {  'a',  's',  'd',  'f',  'g',  'h',  'j',  'k',  'l', '\b'  },
    {  'z',  'x',  'c',  'v',  'b',  'n',  'm',  ',',  '.', '\n'  },
    { 0x01, 0x02, 0x03, ' ',   '1',  '2',  '3',  '4',  '5',  '6'  }, /* 0x01=Fn 0x02=Sym 0x03=Shift */
    {  '7',  '8',  '9',  '0',  '-',  '=',  '[',  ']', '\\', '\''  },
    {  ';',  '/',  '`', 0x1B,    0,    0,    0,    0,    0,    0   }, /* 0x1B=Esc */
    {    0,    0,    0,    0,    0,    0,    0,    0,    0,    0   },
    {    0,    0,    0,    0,    0,    0,    0,    0,    0,    0   },
};
/* clang-format on */

/* ── Driver state ────────────────────────────────────────────────────────── */
static struct {
    i2c_master_dev_handle_t  dev;
    kbd_tca8418_config_t     cfg;
    hal_input_cb_t           cb;
    void                    *cb_data;
    volatile atomic_bool     irq_pending;
    bool                     initialised;
} s_kbd;

/* ── I2C helpers ─────────────────────────────────────────────────────────── */

static esp_err_t tca8418_write_reg(uint8_t reg, uint8_t val)
{
    uint8_t buf[2] = { reg, val };
    esp_err_t ret = i2c_master_transmit(s_kbd.dev, buf, sizeof(buf), 50);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "write reg 0x%02X failed: %s", reg, esp_err_to_name(ret));
    }
    return ret;
}

static esp_err_t tca8418_read_reg(uint8_t reg, uint8_t *val)
{
    esp_err_t ret = i2c_master_transmit_receive(s_kbd.dev, &reg, 1, val, 1, 50);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "read reg 0x%02X failed: %s", reg, esp_err_to_name(ret));
    }
    return ret;
}

/* ── ISR ─────────────────────────────────────────────────────────────────── */

static void IRAM_ATTR tca8418_isr_handler(void *arg)
{
    (void)arg;
    atomic_store(&s_kbd.irq_pending, true);
}

/* ── Init ────────────────────────────────────────────────────────────────── */

static esp_err_t tca8418_init(const void *config)
{
    if (!config) {
        ESP_LOGE(TAG, "init: NULL config");
        return ESP_ERR_INVALID_ARG;
    }
    if (s_kbd.initialised) {
        ESP_LOGW(TAG, "already initialised");
        return ESP_OK;
    }

    memcpy(&s_kbd.cfg, config, sizeof(kbd_tca8418_config_t));
    atomic_store(&s_kbd.irq_pending, false);

    /* ── Add I2C device ── */
    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address  = s_kbd.cfg.i2c_addr,
        .scl_speed_hz    = 400000,
    };
    esp_err_t ret = i2c_master_bus_add_device(s_kbd.cfg.i2c_bus, &dev_cfg, &s_kbd.dev);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2c_master_bus_add_device failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* ── Soft-reset by writing 0 to CFG then re-configure ── */
    /* Enable auto-increment and key-event interrupt */
    ret = tca8418_write_reg(TCA8418_REG_CFG,
                            TCA8418_CFG_KE_IEN | TCA8418_CFG_AI);
    if (ret != ESP_OK) goto fail;

    /* Configure R0-R7 (bits [7:0] of KP_GPIO1) as keypad rows */
    ret = tca8418_write_reg(TCA8418_REG_KP_GPIO1, 0xFF);
    if (ret != ESP_OK) goto fail;

    /* Configure C0-C7 (bits [7:0] of KP_GPIO2) and C8-C9 (bits [1:0] of KP_GPIO3) */
    ret = tca8418_write_reg(TCA8418_REG_KP_GPIO2, 0xFF);
    if (ret != ESP_OK) goto fail;

    ret = tca8418_write_reg(TCA8418_REG_KP_GPIO3, 0x03);
    if (ret != ESP_OK) goto fail;

    /* Clear any stale interrupts */
    ret = tca8418_write_reg(TCA8418_REG_INT_STAT, 0xFF);
    if (ret != ESP_OK) goto fail;

    /* ── Optional interrupt pin ── */
    if (s_kbd.cfg.pin_int != GPIO_NUM_NC) {
        gpio_config_t io = {
            .mode         = GPIO_MODE_INPUT,
            .pull_up_en   = GPIO_PULLUP_ENABLE,
            .intr_type    = GPIO_INTR_NEGEDGE,
            .pin_bit_mask = 1ULL << s_kbd.cfg.pin_int,
        };
        ret = gpio_config(&io);
        if (ret != ESP_OK) goto fail;

        gpio_install_isr_service(0);   /* idempotent */
        ret = gpio_isr_handler_add(s_kbd.cfg.pin_int, tca8418_isr_handler, NULL);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "failed to add ISR: %s", esp_err_to_name(ret));
            goto fail;
        }
        /* Mark pending so first poll drains anything already in FIFO */
        atomic_store(&s_kbd.irq_pending, true);
    }

    s_kbd.initialised = true;
    ESP_LOGI(TAG, "TCA8418 initialised, addr=0x%02X", s_kbd.cfg.i2c_addr);
    return ESP_OK;

fail:
    i2c_master_bus_rm_device(s_kbd.dev);
    s_kbd.dev = NULL;
    return ret;
}

/* ── Deinit ──────────────────────────────────────────────────────────────── */

static void tca8418_deinit(void)
{
    if (!s_kbd.initialised) return;

    if (s_kbd.cfg.pin_int != GPIO_NUM_NC) {
        gpio_isr_handler_remove(s_kbd.cfg.pin_int);
    }

    i2c_master_bus_rm_device(s_kbd.dev);
    s_kbd.dev         = NULL;
    s_kbd.cb          = NULL;
    s_kbd.cb_data     = NULL;
    s_kbd.initialised = false;
    ESP_LOGI(TAG, "deinit complete");
}

/* ── register_callback ───────────────────────────────────────────────────── */

static esp_err_t tca8418_register_callback(hal_input_cb_t cb, void *user_data)
{
    s_kbd.cb      = cb;
    s_kbd.cb_data = user_data;
    return ESP_OK;
}

/* ── poll ────────────────────────────────────────────────────────────────── */

static esp_err_t tca8418_poll(void)
{
    if (!s_kbd.initialised) return ESP_ERR_INVALID_STATE;

    /* If using interrupt pin, skip I2C traffic when no interrupt fired */
    if (s_kbd.cfg.pin_int != GPIO_NUM_NC) {
        if (!atomic_load(&s_kbd.irq_pending)) {
            return ESP_OK;
        }
    }

    /* Check interrupt status */
    uint8_t int_stat = 0;
    esp_err_t ret = tca8418_read_reg(TCA8418_REG_INT_STAT, &int_stat);
    if (ret != ESP_OK) return ret;

    if (!(int_stat & TCA8418_INT_STAT_K_INT)) {
        /* No key event; clear flag and return */
        atomic_store(&s_kbd.irq_pending, false);
        return ESP_OK;
    }

    /* Read event count */
    uint8_t ec_reg = 0;
    ret = tca8418_read_reg(TCA8418_REG_KEY_LCK_EC, &ec_reg);
    if (ret != ESP_OK) return ret;

    uint8_t event_count = ec_reg & 0x0F;
    if (event_count == 0) {
        /* Overflow?  Drain all 10 slots */
        event_count = 10;
    }

    for (uint8_t i = 0; i < event_count; i++) {
        uint8_t ev = 0;
        ret = tca8418_read_reg(TCA8418_REG_KEY_EVENT_A, &ev);
        if (ret != ESP_OK) break;

        if (ev == 0) break;   /* FIFO empty sentinel */

        bool pressed = (ev & KEY_EVENT_PRESS) != 0;
        uint8_t key_code = ev & KEY_EVENT_KEY_MSK;

        /* key_code is 1-based: key_code = row*10 + col + 1 */
        if (key_code == 0) continue;
        key_code--;   /* make 0-based */

        uint8_t row = key_code / 10;
        uint8_t col = key_code % 10;

        uint16_t keycode = 0;
        if (row < 8 && col < 10) {
            keycode = KEY_MAP[row][col];
        }

        if (keycode == 0) {
            ESP_LOGD(TAG, "unmapped key r=%u c=%u", row, col);
            continue;
        }

        if (s_kbd.cb) {
            hal_input_event_t event = {
                .type      = pressed ? HAL_INPUT_EVENT_KEY_DOWN
                                     : HAL_INPUT_EVENT_KEY_UP,
                .timestamp = (uint32_t)(esp_timer_get_time() / 1000),
                .key       = { .keycode = keycode },
            };
            s_kbd.cb(&event, s_kbd.cb_data);
        }
    }

    /* Clear the key-event interrupt bit */
    tca8418_write_reg(TCA8418_REG_INT_STAT, TCA8418_INT_STAT_K_INT);
    atomic_store(&s_kbd.irq_pending, false);
    return ret;
}

/* ── Vtable ──────────────────────────────────────────────────────────────── */

static const hal_input_driver_t tca8418_driver = {
    .init              = tca8418_init,
    .deinit            = tca8418_deinit,
    .register_callback = tca8418_register_callback,
    .poll              = tca8418_poll,
    .name              = "TCA8418",
    .is_touch          = false,
};

const hal_input_driver_t *drv_kbd_tca8418_get(void)
{
    return &tca8418_driver;
}
