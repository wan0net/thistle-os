// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Arduino compatibility shim
//
// Provides standard Arduino APIs routed through the ThistleOS kernel and
// ESP-IDF HAL.  Intended for use by Arduino-based apps such as MeshCore.

#include "shim/arduino.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "esp_rom_sys.h"
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "driver/i2c_master.h"
#include "driver/uart.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "thistle/kernel.h"  /* kernel_uptime_ms() */

#include <string.h>

static const char *TAG = "arduino_shim";

/* Default bus parameters — match board_tdeck_pro.h */
#define SHIM_SPI_HOST       SPI2_HOST
#define SHIM_SPI_CLOCK_HZ   1000000   /* conservative default; overridden by beginTransaction */
#define SHIM_I2C_PORT       I2C_NUM_0
#define SHIM_I2C_FREQ_HZ    100000

/* ------------------------------------------------------------------ */
/* Timing                                                               */
/* ------------------------------------------------------------------ */

uint32_t millis(void)
{
    return (uint32_t)kernel_uptime_ms();
}

uint32_t micros(void)
{
    return (uint32_t)esp_timer_get_time();
}

void delay(uint32_t ms)
{
    vTaskDelay(pdMS_TO_TICKS(ms));
}

void delayMicroseconds(uint32_t us)
{
    esp_rom_delay_us(us);
}

/* ------------------------------------------------------------------ */
/* GPIO                                                                 */
/* ------------------------------------------------------------------ */

void pinMode(uint8_t pin, uint8_t mode)
{
    gpio_config_t cfg = {
        .pin_bit_mask = (1ULL << pin),
        .intr_type    = GPIO_INTR_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
    };

    switch (mode) {
    case OUTPUT:
        cfg.mode = GPIO_MODE_OUTPUT;
        break;
    case INPUT_PULLUP:
        cfg.mode       = GPIO_MODE_INPUT;
        cfg.pull_up_en = GPIO_PULLUP_ENABLE;
        break;
    case INPUT:
    default:
        cfg.mode = GPIO_MODE_INPUT;
        break;
    }

    gpio_config(&cfg);
}

void digitalWrite(uint8_t pin, uint8_t val)
{
    gpio_set_level((gpio_num_t)pin, val);
}

int digitalRead(uint8_t pin)
{
    return gpio_get_level((gpio_num_t)pin);
}

int analogRead(uint8_t pin)
{
    /* TODO: wire up ESP-IDF ADC driver */
    (void)pin;
    return 0;
}

/* ------------------------------------------------------------------ */
/* Interrupts                                                           */
/* ------------------------------------------------------------------ */

/*
 * We store one handler pointer per GPIO number (ESP32 supports up to 48).
 * The ISR wrapper looks up the right user handler via the pin number passed
 * as arg.
 */
#define SHIM_GPIO_MAX 48

static void (*s_irq_handlers[SHIM_GPIO_MAX])(void);

static void IRAM_ATTR shim_gpio_isr(void *arg)
{
    uint32_t pin = (uint32_t)(uintptr_t)arg;
    if (pin < SHIM_GPIO_MAX && s_irq_handlers[pin]) {
        s_irq_handlers[pin]();
    }
}

void attachInterrupt(uint8_t pin, void (*handler)(void), int mode)
{
    if (pin >= SHIM_GPIO_MAX) return;

    gpio_int_type_t idf_mode;
    switch (mode) {
    case RISING:  idf_mode = GPIO_INTR_POSEDGE; break;
    case FALLING: idf_mode = GPIO_INTR_NEGEDGE; break;
    case CHANGE:  idf_mode = GPIO_INTR_ANYEDGE; break;
    default:      return;
    }

    s_irq_handlers[pin] = handler;

    /* gpio_install_isr_service is idempotent when already installed */
    gpio_install_isr_service(0);
    gpio_set_intr_type((gpio_num_t)pin, idf_mode);
    gpio_isr_handler_add((gpio_num_t)pin, shim_gpio_isr,
                         (void *)(uintptr_t)pin);
    gpio_intr_enable((gpio_num_t)pin);
}

void detachInterrupt(uint8_t pin)
{
    if (pin >= SHIM_GPIO_MAX) return;

    gpio_intr_disable((gpio_num_t)pin);
    gpio_isr_handler_remove((gpio_num_t)pin);
    s_irq_handlers[pin] = NULL;
}

/* ------------------------------------------------------------------ */
/* Serial                                                               */
/* ------------------------------------------------------------------ */

static void serial_begin(unsigned long baud)
{
    /* UART0 is already initialised by IDF boot; nothing to do */
    (void)baud;
}

static size_t serial_print(const char *str)
{
    if (!str) return 0;
    ESP_LOGI(TAG, "%s", str);
    return strlen(str);
}

static size_t serial_println(const char *str)
{
    if (!str) {
        ESP_LOGI(TAG, "");
        return 1;
    }
    ESP_LOGI(TAG, "%s", str);
    return strlen(str) + 1;  /* +1 for the implicit newline */
}

static size_t serial_write(uint8_t byte)
{
    char buf[2] = { (char)byte, '\0' };
    uart_write_bytes(UART_NUM_0, buf, 1);
    return 1;
}

static size_t serial_writeBytes(const uint8_t *buf, size_t len)
{
    if (!buf || len == 0) return 0;
    int written = uart_write_bytes(UART_NUM_0, (const char *)buf, len);
    return (written < 0) ? 0 : (size_t)written;
}

static void serial_flush(void)
{
    uart_wait_tx_done(UART_NUM_0, pdMS_TO_TICKS(100));
}

static int serial_available(void)
{
    size_t len = 0;
    uart_get_buffered_data_len(UART_NUM_0, &len);
    return (int)len;
}

static int serial_read(void)
{
    uint8_t byte;
    int n = uart_read_bytes(UART_NUM_0, &byte, 1, 0);
    return (n == 1) ? (int)byte : -1;
}

static int serial_peek(void)
{
    /* ESP-IDF UART driver has no native peek; drain into a 1-byte ring.
     * This is a best-effort implementation: only reliable when called
     * immediately before read() with no interleaving tasks. */
    size_t avail = 0;
    uart_get_buffered_data_len(UART_NUM_0, &avail);
    if (avail == 0) return -1;

    uint8_t byte;
    /* Read without consuming — not supported directly; return first byte
     * and re-inject via a local flag pattern. */
    int n = uart_read_bytes(UART_NUM_0, &byte, 1, 0);
    if (n != 1) return -1;

    /*
     * NOTE: ESP-IDF does not expose a UART push-back API.  The byte is
     * consumed here and will NOT be returned again by read().  Applications
     * that rely on peek() followed by read() should avoid using peek() on
     * ThistleOS.
     */
    ESP_LOGW(TAG, "Serial.peek() consumed byte 0x%02X — pushback not supported", byte);
    return (int)byte;
}

arduino_serial_t Serial = {
    .begin      = serial_begin,
    .print      = serial_print,
    .println    = serial_println,
    .write      = serial_write,
    .writeBytes = serial_writeBytes,
    .flush      = serial_flush,
    .available  = serial_available,
    .read       = serial_read,
    .peek       = serial_peek,
};

/* ------------------------------------------------------------------ */
/* SPI                                                                  */
/* ------------------------------------------------------------------ */

static struct {
    spi_device_handle_t dev;
    bool                open;
    uint32_t            clock_hz;
    uint8_t             bit_order;
    uint8_t             data_mode;
} s_spi;

static void spi_begin(void)
{
    if (s_spi.open) return;

    spi_device_interface_config_t devcfg = {
        .clock_source   = SPI_CLK_SRC_DEFAULT,  /* required in ESP-IDF v6 */
        .clock_speed_hz = s_spi.clock_hz ? s_spi.clock_hz : SHIM_SPI_CLOCK_HZ,
        .mode           = s_spi.data_mode,
        .spics_io_num   = -1,   /* CS managed manually by Arduino apps */
        .queue_size     = 1,
        .flags          = 0,
    };

    esp_err_t err = spi_bus_add_device(SHIM_SPI_HOST, &devcfg, &s_spi.dev);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SPI.begin: spi_bus_add_device failed: %s",
                 esp_err_to_name(err));
        return;
    }
    s_spi.open = true;
    ESP_LOGI(TAG, "SPI.begin: device added to SPI%d at %lu Hz",
             SHIM_SPI_HOST, (unsigned long)devcfg.clock_speed_hz);
}

static void spi_end(void)
{
    if (!s_spi.open) return;
    spi_bus_remove_device(s_spi.dev);
    s_spi.dev  = NULL;
    s_spi.open = false;
}

static uint8_t spi_transfer(uint8_t data)
{
    if (!s_spi.open) return 0xFF;

    uint8_t rx = 0;
    spi_transaction_t t = {
        .length    = 8,
        .tx_buffer = &data,
        .rx_buffer = &rx,
    };
    esp_err_t err = spi_device_polling_transmit(s_spi.dev, &t);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SPI.transfer failed: %s", esp_err_to_name(err));
    }
    return rx;
}

static void spi_transferBytes(const uint8_t *out, uint8_t *in, uint32_t size)
{
    if (!s_spi.open || size == 0) return;

    spi_transaction_t t = {
        .length    = size * 8,
        .tx_buffer = out,
        .rx_buffer = in,
    };
    esp_err_t err = spi_device_polling_transmit(s_spi.dev, &t);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SPI.transferBytes failed: %s", esp_err_to_name(err));
    }
}

static void spi_beginTransaction(uint32_t clock, uint8_t bitOrder, uint8_t dataMode)
{
    /*
     * Arduino SPISettings are applied before a transaction.  If the device
     * handle is already open with different settings we remove and re-add it
     * so the IDF clock and mode take effect.
     */
    bool needs_reopen = s_spi.open &&
                        (s_spi.clock_hz   != clock    ||
                         s_spi.data_mode  != dataMode);

    if (needs_reopen) {
        spi_end();
    }

    s_spi.clock_hz   = clock;
    s_spi.bit_order  = bitOrder;
    s_spi.data_mode  = dataMode;

    if (!s_spi.open) spi_begin();

    /* Acquire the bus for the duration of the transaction */
    spi_device_acquire_bus(s_spi.dev, portMAX_DELAY);
}

static void spi_endTransaction(void)
{
    if (!s_spi.open) return;
    spi_device_release_bus(s_spi.dev);
}

arduino_spi_t SPI = {
    .begin            = spi_begin,
    .end              = spi_end,
    .transfer         = spi_transfer,
    .transferBytes    = spi_transferBytes,
    .beginTransaction = spi_beginTransaction,
    .endTransaction   = spi_endTransaction,
};

/* ------------------------------------------------------------------ */
/* Wire (I2C)                                                           */
/* ------------------------------------------------------------------ */

#define WIRE_TX_BUF_SIZE 128
#define WIRE_RX_BUF_SIZE 128

static struct {
    i2c_master_bus_handle_t bus;
    i2c_master_dev_handle_t dev;
    uint8_t  tx_addr;
    uint8_t  tx_buf[WIRE_TX_BUF_SIZE];
    uint16_t tx_len;
    uint8_t  rx_buf[WIRE_RX_BUF_SIZE];
    uint16_t rx_len;
    uint16_t rx_pos;
    bool     bus_open;
    bool     dev_open;
} s_wire;

static void wire_ensure_bus(void)
{
    if (s_wire.bus_open) return;

    esp_err_t err = i2c_master_get_bus_handle(SHIM_I2C_PORT, &s_wire.bus);
    if (err == ESP_OK) {
        s_wire.bus_open = true;
        ESP_LOGI(TAG, "Wire.begin: acquired I2C%d bus handle", SHIM_I2C_PORT);
    } else {
        ESP_LOGE(TAG, "Wire.begin: i2c_master_get_bus_handle failed: %s",
                 esp_err_to_name(err));
    }
}

static void wire_open_device(uint8_t addr)
{
    if (s_wire.dev_open && s_wire.tx_addr == addr) return;

    /* Remove any previously opened device */
    if (s_wire.dev_open) {
        i2c_master_bus_rm_device(s_wire.dev);
        s_wire.dev      = NULL;
        s_wire.dev_open = false;
    }

    wire_ensure_bus();
    if (!s_wire.bus_open) return;

    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address  = addr,
        .scl_speed_hz    = SHIM_I2C_FREQ_HZ,
    };
    esp_err_t err = i2c_master_bus_add_device(s_wire.bus, &dev_cfg, &s_wire.dev);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Wire: i2c_master_bus_add_device(0x%02X) failed: %s",
                 addr, esp_err_to_name(err));
        return;
    }
    s_wire.dev_open = true;
    s_wire.tx_addr  = addr;
}

static void wire_begin(void)
{
    wire_ensure_bus();
}

static void wire_beginTransmission(uint8_t addr)
{
    s_wire.tx_addr = addr;
    s_wire.tx_len  = 0;
}

static uint8_t wire_endTransmission(void)
{
    wire_open_device(s_wire.tx_addr);
    if (!s_wire.dev_open) return 4;  /* Arduino: 4 = other error */

    esp_err_t err = i2c_master_transmit(s_wire.dev,
                                        s_wire.tx_buf, s_wire.tx_len,
                                        pdMS_TO_TICKS(100));
    s_wire.tx_len = 0;
    if (err == ESP_OK)             return 0;
    if (err == ESP_ERR_TIMEOUT)    return 3;  /* Arduino: 3 = NACK on data */
    return 4;
}

static size_t wire_write(uint8_t data)
{
    if (s_wire.tx_len >= WIRE_TX_BUF_SIZE) return 0;
    s_wire.tx_buf[s_wire.tx_len++] = data;
    return 1;
}

static size_t wire_writeBytes(const uint8_t *data, size_t len)
{
    size_t n = 0;
    while (n < len && s_wire.tx_len < WIRE_TX_BUF_SIZE) {
        s_wire.tx_buf[s_wire.tx_len++] = data[n++];
    }
    return n;
}

static uint8_t wire_requestFrom(uint8_t addr, uint8_t quantity)
{
    if (quantity == 0 || quantity > WIRE_RX_BUF_SIZE) return 0;

    wire_open_device(addr);
    if (!s_wire.dev_open) return 0;

    esp_err_t err = i2c_master_receive(s_wire.dev,
                                       s_wire.rx_buf, quantity,
                                       pdMS_TO_TICKS(100));
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "Wire.requestFrom(0x%02X, %u) failed: %s",
                 addr, quantity, esp_err_to_name(err));
        s_wire.rx_len = 0;
        s_wire.rx_pos = 0;
        return 0;
    }

    s_wire.rx_len = quantity;
    s_wire.rx_pos = 0;
    return quantity;
}

static int wire_read(void)
{
    if (s_wire.rx_pos >= s_wire.rx_len) return -1;
    return (int)s_wire.rx_buf[s_wire.rx_pos++];
}

static int wire_available(void)
{
    return (int)(s_wire.rx_len - s_wire.rx_pos);
}

arduino_wire_t Wire = {
    .begin             = wire_begin,
    .beginTransmission = wire_beginTransmission,
    .endTransmission   = wire_endTransmission,
    .write             = wire_write,
    .writeBytes        = wire_writeBytes,
    .requestFrom       = wire_requestFrom,
    .read              = wire_read,
    .available         = wire_available,
};

/* ------------------------------------------------------------------ */
/* Math                                                                 */
/* ------------------------------------------------------------------ */

long map(long x, long in_min, long in_max, long out_min, long out_max)
{
    return (x - in_min) * (out_max - out_min) / (in_max - in_min) + out_min;
}

/* ------------------------------------------------------------------ */
/* Random                                                               */
/* ------------------------------------------------------------------ */

void randomSeed(unsigned long seed)
{
    srand((unsigned int)seed);
}

long arduino_random(long maxval)
{
    if (maxval <= 0) return 0;
    return (long)(rand() % (unsigned long)maxval);
}

long arduino_random_range(long minval, long maxval)
{
    if (maxval <= minval) return minval;
    return minval + (long)(rand() % (unsigned long)(maxval - minval));
}

/* ------------------------------------------------------------------ */
/* Lifecycle                                                            */
/* ------------------------------------------------------------------ */

esp_err_t arduino_shim_init(void)
{
    /* Zero out interrupt handler table */
    memset(s_irq_handlers, 0, sizeof(s_irq_handlers));

    /* Initialise SPI state */
    memset(&s_spi, 0, sizeof(s_spi));

    /* Initialise Wire state */
    memset(&s_wire, 0, sizeof(s_wire));

    ESP_LOGI(TAG, "Arduino shim initialized");
    return ESP_OK;
}

esp_err_t arduino_shim_run(arduino_setup_fn setup, arduino_loop_fn loop)
{
    if (!setup || !loop) {
        return ESP_ERR_INVALID_ARG;
    }

    setup();

    while (1) {
        loop();
        vTaskDelay(pdMS_TO_TICKS(1));
    }

    return ESP_OK;  /* unreachable, but satisfies the compiler */
}
