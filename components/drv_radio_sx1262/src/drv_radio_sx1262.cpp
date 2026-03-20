// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — SX1262 LoRa driver via RadioLib (MIT)

extern "C" {
#include "drv_radio_sx1262.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "rom/ets_sys.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
}

#include <cstring>

/* Arduino constants needed by RadioLibHal base class */
#define INPUT           0x01
#define OUTPUT          0x02
#define LOW             0x00
#define HIGH            0x01
#define RISING          0x01
#define FALLING         0x02

#include <RadioLib.h>

static const char *TAG = "sx1262";

/* RadioLib needs a HAL (Hardware Abstraction Layer) for ESP-IDF.
 * RadioLib ships with EspHal for Arduino. For ESP-IDF, we use
 * the built-in EspIdfHal or create a minimal one. */

/* RadioLib ESP-IDF HAL — wraps ESP-IDF SPI/GPIO */
class ThistleHal : public RadioLibHal {
public:
    ThistleHal(spi_host_device_t spi_host, int spi_clock)
        : RadioLibHal(INPUT, OUTPUT, LOW, HIGH, RISING, FALLING),
          _spi_host(spi_host), _spi_clock(spi_clock), _spi(nullptr) {}

    void init() override {
        spiBegin();
    }

    void term() override {
        spiEnd();
    }

    void pinMode(uint32_t pin, uint32_t mode) override {
        if (pin == RADIOLIB_NC) return;
        gpio_config_t cfg = {};
        cfg.pin_bit_mask = 1ULL << pin;
        cfg.mode = (mode == OUTPUT) ? GPIO_MODE_OUTPUT : GPIO_MODE_INPUT;
        cfg.pull_up_en = GPIO_PULLUP_DISABLE;
        cfg.pull_down_en = GPIO_PULLDOWN_DISABLE;
        cfg.intr_type = GPIO_INTR_DISABLE;
        gpio_config(&cfg);
    }

    void digitalWrite(uint32_t pin, uint32_t value) override {
        if (pin == RADIOLIB_NC) return;
        gpio_set_level((gpio_num_t)pin, value);
    }

    uint32_t digitalRead(uint32_t pin) override {
        if (pin == RADIOLIB_NC) return 0;
        return gpio_get_level((gpio_num_t)pin);
    }

    void attachInterrupt(uint32_t interruptNum, void (*interruptCb)(void), uint32_t mode) override {
        if (interruptNum == RADIOLIB_NC) return;
        gpio_install_isr_service(0);
        gpio_int_type_t intr = GPIO_INTR_POSEDGE;
        if (mode == FALLING) intr = GPIO_INTR_NEGEDGE;
        else if (mode == RISING) intr = GPIO_INTR_POSEDGE;

        gpio_config_t cfg = {};
        cfg.pin_bit_mask = 1ULL << interruptNum;
        cfg.mode = GPIO_MODE_INPUT;
        cfg.intr_type = intr;
        gpio_config(&cfg);
        gpio_isr_handler_add((gpio_num_t)interruptNum, (gpio_isr_t)(void *)interruptCb, nullptr);
    }

    void detachInterrupt(uint32_t interruptNum) override {
        if (interruptNum == RADIOLIB_NC) return;
        gpio_isr_handler_remove((gpio_num_t)interruptNum);
    }

    void delay(unsigned long ms) override {
        vTaskDelay(pdMS_TO_TICKS(ms));
    }

    void delayMicroseconds(unsigned long us) override {
        ets_delay_us(us);
    }

    unsigned long millis() override {
        return (unsigned long)(esp_timer_get_time() / 1000);
    }

    unsigned long micros() override {
        return (unsigned long)esp_timer_get_time();
    }

    long pulseIn(uint32_t pin, uint32_t state, unsigned long timeout) override {
        (void)pin; (void)state; (void)timeout;
        return 0;
    }

    void spiBegin() {
        /* SPI bus already initialized by board — just add our device */
        spi_device_interface_config_t devcfg = {};
        devcfg.clock_speed_hz = _spi_clock > 0 ? _spi_clock : 8000000;
        devcfg.mode = 0;
        devcfg.spics_io_num = -1;  /* RadioLib manages CS via GPIO */
        devcfg.queue_size = 1;
        spi_bus_add_device(_spi_host, &devcfg, &_spi);
    }

    void spiEnd() {
        if (_spi) {
            spi_bus_remove_device(_spi);
            _spi = nullptr;
        }
    }

    void spiBeginTransaction() override {}
    void spiEndTransaction() override {}

    void spiTransfer(uint8_t *out, size_t len, uint8_t *in) override {
        spi_transaction_t t = {};
        t.length = len * 8;
        t.tx_buffer = out;
        t.rx_buffer = in;
        spi_device_polling_transmit(_spi, &t);
    }

    void yield() override {
        vTaskDelay(1);
    }

private:
    spi_host_device_t _spi_host;
    int _spi_clock;
    spi_device_handle_t _spi;
};

/* ── Driver state ────────────────────────────────────────────────── */

static struct {
    radio_sx1262_config_t cfg;
    ThistleHal *hal;
    SX1262 *radio;
    Module *mod;
    hal_radio_rx_cb_t rx_cb;
    void *rx_cb_data;
    bool initialized;
    bool receiving;
    TaskHandle_t irq_task;
    volatile bool irq_pending;
} s_radio;

/* ISR → task notification */
static void IRAM_ATTR dio1_isr(void) {
    s_radio.irq_pending = true;
    BaseType_t wake = pdFALSE;
    if (s_radio.irq_task) {
        vTaskNotifyGiveFromISR(s_radio.irq_task, &wake);
        portYIELD_FROM_ISR(wake);
    }
}

static void radio_irq_task(void *arg) {
    (void)arg;
    while (1) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);

        if (!s_radio.initialized || !s_radio.radio) continue;

        /* Read received data */
        uint8_t buf[256];
        size_t len = s_radio.radio->getPacketLength();
        if (len > sizeof(buf)) len = sizeof(buf);

        int state = s_radio.radio->readData(buf, len);

        if (state == RADIOLIB_ERR_NONE && len > 0 && s_radio.rx_cb) {
            float rssi = s_radio.radio->getRSSI();
            s_radio.rx_cb(buf, len, (int)rssi, s_radio.rx_cb_data);
        }

        /* Re-arm RX if in continuous mode */
        if (s_radio.receiving) {
            s_radio.radio->startReceive();
        }
    }
}

/* ── HAL vtable implementations ──────────────────────────────────── */

static esp_err_t sx1262_init(const void *config) {
    if (s_radio.initialized) return ESP_OK;
    if (!config) return ESP_ERR_INVALID_ARG;

    memcpy(&s_radio.cfg, config, sizeof(radio_sx1262_config_t));

    /* Create RadioLib HAL + Module + SX1262 */
    s_radio.hal = new ThistleHal(s_radio.cfg.spi_host,
                                  s_radio.cfg.spi_clock_hz);

    s_radio.mod = new Module(s_radio.hal,
                              s_radio.cfg.pin_cs,
                              s_radio.cfg.pin_dio1,
                              s_radio.cfg.pin_reset,
                              s_radio.cfg.pin_busy);

    s_radio.radio = new SX1262(s_radio.mod);

    /* Initialize with defaults: 915MHz, 125kHz BW, SF7, CR4/5 */
    int state = s_radio.radio->begin(915.0, 125.0, 7, 5, RADIOLIB_SX126X_SYNC_WORD_PRIVATE, 22);
    if (state != RADIOLIB_ERR_NONE) {
        ESP_LOGE(TAG, "RadioLib begin() failed: %d", state);
        delete s_radio.radio; s_radio.radio = nullptr;
        delete s_radio.mod; s_radio.mod = nullptr;
        delete s_radio.hal; s_radio.hal = nullptr;
        return ESP_FAIL;
    }

    /* Set DIO1 as interrupt */
    s_radio.radio->setDio1Action(dio1_isr);

    /* Create IRQ handler task */
    xTaskCreate(radio_irq_task, "radio_irq", 4096, nullptr, configMAX_PRIORITIES - 1, &s_radio.irq_task);

    s_radio.initialized = true;
    ESP_LOGI(TAG, "SX1262 initialized via RadioLib");
    return ESP_OK;
}

static void sx1262_deinit(void) {
    if (!s_radio.initialized) return;

    if (s_radio.irq_task) {
        vTaskDelete(s_radio.irq_task);
        s_radio.irq_task = nullptr;
    }

    s_radio.radio->standby();

    delete s_radio.radio; s_radio.radio = nullptr;
    delete s_radio.mod; s_radio.mod = nullptr;
    delete s_radio.hal; s_radio.hal = nullptr;

    s_radio.initialized = false;
    ESP_LOGI(TAG, "SX1262 deinitialized");
}

static esp_err_t sx1262_set_frequency(uint32_t freq_hz) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    float mhz = freq_hz / 1000000.0f;
    int state = s_radio.radio->setFrequency(mhz);
    return state == RADIOLIB_ERR_NONE ? ESP_OK : ESP_FAIL;
}

static esp_err_t sx1262_set_tx_power(int8_t dbm) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    int state = s_radio.radio->setOutputPower(dbm);
    return state == RADIOLIB_ERR_NONE ? ESP_OK : ESP_FAIL;
}

static esp_err_t sx1262_set_bandwidth(uint32_t bw_hz) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    float bw_khz = bw_hz / 1000.0f;
    int state = s_radio.radio->setBandwidth(bw_khz);
    return state == RADIOLIB_ERR_NONE ? ESP_OK : ESP_FAIL;
}

static esp_err_t sx1262_set_spreading_factor(uint8_t sf) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    int state = s_radio.radio->setSpreadingFactor(sf);
    return state == RADIOLIB_ERR_NONE ? ESP_OK : ESP_FAIL;
}

static esp_err_t sx1262_send(const uint8_t *data, size_t len) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    if (!data || len == 0 || len > 255) return ESP_ERR_INVALID_ARG;

    s_radio.receiving = false;
    int state = s_radio.radio->transmit((uint8_t *)data, len);

    if (state == RADIOLIB_ERR_NONE) {
        ESP_LOGD(TAG, "TX done (%zu bytes)", len);
        return ESP_OK;
    }
    ESP_LOGE(TAG, "TX failed: %d", state);
    return ESP_FAIL;
}

static esp_err_t sx1262_start_receive(hal_radio_rx_cb_t cb, void *user_data) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;

    s_radio.rx_cb = cb;
    s_radio.rx_cb_data = user_data;
    s_radio.receiving = true;

    int state = s_radio.radio->startReceive();
    if (state != RADIOLIB_ERR_NONE) {
        ESP_LOGE(TAG, "startReceive failed: %d", state);
        return ESP_FAIL;
    }

    ESP_LOGD(TAG, "Continuous RX started");
    return ESP_OK;
}

static esp_err_t sx1262_stop_receive(void) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    s_radio.receiving = false;
    s_radio.radio->standby();
    return ESP_OK;
}

static int sx1262_get_rssi(void) {
    if (!s_radio.initialized) return -128;
    return (int)s_radio.radio->getRSSI();
}

static esp_err_t sx1262_sleep(bool enter) {
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    if (enter) {
        s_radio.radio->sleep();
    } else {
        s_radio.radio->standby();
    }
    return ESP_OK;
}

/* ── Vtable ──────────────────────────────────────────────────────── */

static const hal_radio_driver_t sx1262_driver = {
    .init = sx1262_init,
    .deinit = sx1262_deinit,
    .set_frequency = sx1262_set_frequency,
    .set_tx_power = sx1262_set_tx_power,
    .set_bandwidth = sx1262_set_bandwidth,
    .set_spreading_factor = sx1262_set_spreading_factor,
    .send = sx1262_send,
    .start_receive = sx1262_start_receive,
    .stop_receive = sx1262_stop_receive,
    .get_rssi = sx1262_get_rssi,
    .sleep = sx1262_sleep,
    .name = "SX1262 (RadioLib)",
};

extern "C" const hal_radio_driver_t *drv_radio_sx1262_get(void)
{
    return &sx1262_driver;
}
