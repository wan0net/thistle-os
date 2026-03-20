#include "shim/arduino.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "esp_rom_sys.h"
#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "thistle/kernel.h"  /* kernel_uptime_ms() */

static const char *TAG = "arduino_shim";

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
        cfg.mode        = GPIO_MODE_INPUT;
        cfg.pull_up_en  = GPIO_PULLUP_ENABLE;
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
/* Serial                                                               */
/* ------------------------------------------------------------------ */

static void serial_begin(unsigned long baud)
{
    /* UART already initialised by IDF; nothing to do */
    (void)baud;
}

static size_t serial_print(const char *str)
{
    ESP_LOGI(TAG, "%s", str);
    return str ? strlen(str) : 0;
}

static size_t serial_println(const char *str)
{
    ESP_LOGI(TAG, "%s", str);
    return str ? strlen(str) + 1 : 1;  /* +1 for the implicit newline */
}

static int serial_available(void)
{
    return 0;
}

static int serial_read(void)
{
    return -1;
}

arduino_serial_t Serial = {
    .begin     = serial_begin,
    .print     = serial_print,
    .println   = serial_println,
    .available = serial_available,
    .read      = serial_read,
};

/* ------------------------------------------------------------------ */
/* Lifecycle                                                            */
/* ------------------------------------------------------------------ */

esp_err_t arduino_shim_init(void)
{
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
