// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — SX1262 LoRa radio driver (real implementation)
#include "drv_radio_sx1262.h"
#include "esp_log.h"
#include "esp_err.h"
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "rom/ets_sys.h"
#include <string.h>
#include <math.h>

static const char *TAG = "sx1262";

// ---------------------------------------------------------------------------
// SX1262 command opcodes
// ---------------------------------------------------------------------------

#define CMD_SET_SLEEP               0x84
#define CMD_SET_STANDBY             0x80
#define CMD_SET_RX                  0x82
#define CMD_SET_TX                  0x83
#define CMD_SET_CAD                 0x84
#define CMD_SET_RF_FREQUENCY        0x86
#define CMD_SET_PACKET_TYPE         0x8A
#define CMD_SET_MODULATION_PARAMS   0x8B
#define CMD_SET_PACKET_PARAMS       0x8C
#define CMD_SET_TX_PARAMS           0x8E
#define CMD_SET_BUFFER_BASE_ADDR    0x8F
#define CMD_SET_PA_CONFIG           0x95
#define CMD_SET_DIO_IRQ_PARAMS      0x08
#define CMD_GET_STATUS              0xC0
#define CMD_GET_IRQ_STATUS          0x12
#define CMD_CLEAR_IRQ_STATUS        0x02
#define CMD_GET_RX_BUFFER_STATUS    0x13
#define CMD_GET_RSSI_INST           0x15
#define CMD_WRITE_REGISTER          0x0D
#define CMD_READ_REGISTER           0x1D
#define CMD_WRITE_BUFFER            0x0E
#define CMD_READ_BUFFER             0x1E

// ---------------------------------------------------------------------------
// IRQ bit masks
// ---------------------------------------------------------------------------

#define IRQ_TX_DONE     (1u << 0)
#define IRQ_RX_DONE     (1u << 1)
#define IRQ_CRC_ERR     (1u << 5)
#define IRQ_TIMEOUT     (1u << 9)
#define IRQ_ALL         0x03FF

// ---------------------------------------------------------------------------
// SX1262 bandwidth register values
// ---------------------------------------------------------------------------

#define BW_7800HZ     0x00
#define BW_10400HZ    0x08
#define BW_15600HZ    0x01
#define BW_20800HZ    0x09
#define BW_31250HZ    0x02
#define BW_41700HZ    0x0A
#define BW_62500HZ    0x03
#define BW_125000HZ   0x04
#define BW_250000HZ   0x05
#define BW_500000HZ   0x06

// ---------------------------------------------------------------------------
// Ramp time
// ---------------------------------------------------------------------------

#define RAMP_200US    0x04

// ---------------------------------------------------------------------------
// Driver state
// ---------------------------------------------------------------------------

static struct {
    spi_device_handle_t spi;
    radio_sx1262_config_t cfg;
    hal_radio_rx_cb_t rx_cb;
    void               *rx_cb_data;
    uint32_t            frequency_hz;
    int8_t              tx_power_dbm;
    uint8_t             spreading_factor;
    uint32_t            bandwidth_hz;
    bool                initialized;
    bool                receiving;
    TaskHandle_t        irq_task;
} s_radio;

// ---------------------------------------------------------------------------
// Forward declarations
// ---------------------------------------------------------------------------

static esp_err_t sx1262_apply_modulation_params(void);
static esp_err_t sx1262_apply_rf_frequency(uint32_t freq_hz);

// ---------------------------------------------------------------------------
// Low-level SPI helpers
// ---------------------------------------------------------------------------

static void sx1262_wait_busy(uint32_t timeout_ms)
{
    uint32_t deadline_ms = timeout_ms * 1000; /* convert to loops */
    uint32_t elapsed     = 0;
    while (gpio_get_level(s_radio.cfg.pin_busy)) {
        ets_delay_us(100);
        elapsed += 100;
        if (elapsed >= deadline_ms * 1000) {
            ESP_LOGW(TAG, "BUSY timeout after %lu ms", (unsigned long)timeout_ms);
            break;
        }
    }
}

static esp_err_t sx1262_write_command(uint8_t cmd, const uint8_t *data, size_t len)
{
    sx1262_wait_busy(100);

    /* Build tx buffer: [cmd] [data...] */
    uint8_t tx[1 + len];
    tx[0] = cmd;
    if (data && len) {
        memcpy(tx + 1, data, len);
    }

    spi_transaction_t t = {
        .length    = (1 + len) * 8,
        .tx_buffer = tx,
        .rx_buffer = NULL,
    };
    return spi_device_polling_transmit(s_radio.spi, &t);
}

static esp_err_t sx1262_read_command(uint8_t cmd, uint8_t *out, size_t len)
{
    sx1262_wait_busy(100);

    /* Protocol: send [cmd] [NOP], then read len bytes. */
    size_t total = 2 + len;
    uint8_t tx[total];
    uint8_t rx[total];
    memset(tx, 0x00, total);
    tx[0] = cmd;

    spi_transaction_t t = {
        .length    = total * 8,
        .tx_buffer = tx,
        .rx_buffer = rx,
    };
    esp_err_t err = spi_device_polling_transmit(s_radio.spi, &t);
    if (err == ESP_OK && out) {
        memcpy(out, rx + 2, len);
    }
    return err;
}

static esp_err_t sx1262_write_registers(uint16_t addr, const uint8_t *data, size_t len)
{
    sx1262_wait_busy(100);

    size_t total = 3 + len;
    uint8_t tx[total];
    tx[0] = CMD_WRITE_REGISTER;
    tx[1] = (addr >> 8) & 0xFF;
    tx[2] = addr & 0xFF;
    memcpy(tx + 3, data, len);

    spi_transaction_t t = {
        .length    = total * 8,
        .tx_buffer = tx,
        .rx_buffer = NULL,
    };
    return spi_device_polling_transmit(s_radio.spi, &t);
}

static esp_err_t sx1262_read_registers(uint16_t addr, uint8_t *out, size_t len)
{
    sx1262_wait_busy(100);

    /* [cmd] [addr_hi] [addr_lo] [NOP] [data...] */
    size_t total = 4 + len;
    uint8_t tx[total];
    uint8_t rx[total];
    memset(tx, 0x00, total);
    tx[0] = CMD_READ_REGISTER;
    tx[1] = (addr >> 8) & 0xFF;
    tx[2] = addr & 0xFF;

    spi_transaction_t t = {
        .length    = total * 8,
        .tx_buffer = tx,
        .rx_buffer = rx,
    };
    esp_err_t err = spi_device_polling_transmit(s_radio.spi, &t);
    if (err == ESP_OK && out) {
        memcpy(out, rx + 4, len);
    }
    return err;
}

static esp_err_t sx1262_write_buffer(uint8_t offset, const uint8_t *data, size_t len)
{
    sx1262_wait_busy(100);

    size_t total = 2 + len;
    uint8_t tx[total];
    tx[0] = CMD_WRITE_BUFFER;
    tx[1] = offset;
    memcpy(tx + 2, data, len);

    spi_transaction_t t = {
        .length    = total * 8,
        .tx_buffer = tx,
        .rx_buffer = NULL,
    };
    return spi_device_polling_transmit(s_radio.spi, &t);
}

static esp_err_t sx1262_read_buffer(uint8_t offset, uint8_t *out, size_t len)
{
    sx1262_wait_busy(100);

    /* [cmd] [offset] [NOP] [data...] */
    size_t total = 3 + len;
    uint8_t tx[total];
    uint8_t rx[total];
    memset(tx, 0x00, total);
    tx[0] = CMD_READ_BUFFER;
    tx[1] = offset;

    spi_transaction_t t = {
        .length    = total * 8,
        .tx_buffer = tx,
        .rx_buffer = rx,
    };
    esp_err_t err = spi_device_polling_transmit(s_radio.spi, &t);
    if (err == ESP_OK && out) {
        memcpy(out, rx + 3, len);
    }
    return err;
}

// ---------------------------------------------------------------------------
// Mid-level command wrappers
// ---------------------------------------------------------------------------

static esp_err_t sx1262_set_standby(void)
{
    uint8_t arg = 0x00; /* STDBY_RC */
    return sx1262_write_command(CMD_SET_STANDBY, &arg, 1);
}

static esp_err_t sx1262_set_packet_type_lora(void)
{
    uint8_t arg = 0x01; /* LoRa */
    return sx1262_write_command(CMD_SET_PACKET_TYPE, &arg, 1);
}

static esp_err_t sx1262_apply_rf_frequency(uint32_t freq_hz)
{
    uint32_t freq_reg = (uint32_t)((double)freq_hz / 32000000.0 * (double)(1u << 25));
    uint8_t buf[4];
    buf[0] = (freq_reg >> 24) & 0xFF;
    buf[1] = (freq_reg >> 16) & 0xFF;
    buf[2] = (freq_reg >>  8) & 0xFF;
    buf[3] =  freq_reg        & 0xFF;
    return sx1262_write_command(CMD_SET_RF_FREQUENCY, buf, 4);
}

static esp_err_t sx1262_set_pa_config_sx1262(void)
{
    /* SX1262 HP PA: paDutyCycle=0x04, hpMax=0x07, deviceSel=0x00, paLut=0x01 */
    uint8_t buf[4] = {0x04, 0x07, 0x00, 0x01};
    return sx1262_write_command(CMD_SET_PA_CONFIG, buf, 4);
}

static esp_err_t sx1262_apply_tx_params(int8_t dbm)
{
    if (dbm > 22) dbm = 22;
    if (dbm < -9) dbm = -9;
    uint8_t buf[2] = {(uint8_t)dbm, RAMP_200US};
    return sx1262_write_command(CMD_SET_TX_PARAMS, buf, 2);
}

static uint8_t sx1262_bw_reg(uint32_t bw_hz)
{
    if (bw_hz <= 7800)   return BW_7800HZ;
    if (bw_hz <= 10400)  return BW_10400HZ;
    if (bw_hz <= 15600)  return BW_15600HZ;
    if (bw_hz <= 20800)  return BW_20800HZ;
    if (bw_hz <= 31250)  return BW_31250HZ;
    if (bw_hz <= 41700)  return BW_41700HZ;
    if (bw_hz <= 62500)  return BW_62500HZ;
    if (bw_hz <= 125000) return BW_125000HZ;
    if (bw_hz <= 250000) return BW_250000HZ;
    return BW_500000HZ;
}

static esp_err_t sx1262_apply_modulation_params(void)
{
    uint8_t sf    = s_radio.spreading_factor;
    uint8_t bw    = sx1262_bw_reg(s_radio.bandwidth_hz);
    uint8_t cr    = 0x01; /* CR 4/5 */
    /* LDRO: required when symbol duration > 16 ms.
     * Symbol duration (ms) = 2^SF / BW_hz * 1000.
     * Enable when SF >= 11 with BW <= 125 kHz, or SF == 12 with BW <= 250 kHz. */
    bool ldro = false;
    if ((sf >= 11 && s_radio.bandwidth_hz <= 125000) ||
        (sf == 12 && s_radio.bandwidth_hz <= 250000)) {
        ldro = true;
    }
    uint8_t buf[4] = {sf, bw, cr, ldro ? 0x01 : 0x00};
    return sx1262_write_command(CMD_SET_MODULATION_PARAMS, buf, 4);
}

static esp_err_t sx1262_apply_packet_params(uint8_t payload_len)
{
    /* preamble=8, explicit header, payloadLen, CRC on, standard IQ */
    uint8_t buf[6] = {0x00, 0x08, 0x00, payload_len, 0x01, 0x00};
    return sx1262_write_command(CMD_SET_PACKET_PARAMS, buf, 6);
}

static esp_err_t sx1262_set_buffer_base_address(uint8_t tx_base, uint8_t rx_base)
{
    uint8_t buf[2] = {tx_base, rx_base};
    return sx1262_write_command(CMD_SET_BUFFER_BASE_ADDR, buf, 2);
}

static esp_err_t sx1262_set_dio_irq_params(uint16_t irq_mask,
                                            uint16_t dio1_mask,
                                            uint16_t dio2_mask,
                                            uint16_t dio3_mask)
{
    uint8_t buf[8] = {
        (irq_mask  >> 8) & 0xFF, irq_mask  & 0xFF,
        (dio1_mask >> 8) & 0xFF, dio1_mask & 0xFF,
        (dio2_mask >> 8) & 0xFF, dio2_mask & 0xFF,
        (dio3_mask >> 8) & 0xFF, dio3_mask & 0xFF,
    };
    return sx1262_write_command(CMD_SET_DIO_IRQ_PARAMS, buf, 8);
}

static uint16_t sx1262_get_irq_status(void)
{
    uint8_t buf[2];
    sx1262_read_command(CMD_GET_IRQ_STATUS, buf, 2);
    return ((uint16_t)buf[0] << 8) | buf[1];
}

static void sx1262_clear_irq(uint16_t mask)
{
    uint8_t buf[2] = {(mask >> 8) & 0xFF, mask & 0xFF};
    sx1262_write_command(CMD_CLEAR_IRQ_STATUS, buf, 2);
}

static void sx1262_get_rx_buffer_status(uint8_t *out_len, uint8_t *out_offset)
{
    uint8_t buf[2];
    sx1262_read_command(CMD_GET_RX_BUFFER_STATUS, buf, 2);
    if (out_len)    *out_len    = buf[0];
    if (out_offset) *out_offset = buf[1];
}

static void sx1262_set_rx(uint32_t timeout)
{
    uint8_t buf[3] = {
        (timeout >> 16) & 0xFF,
        (timeout >>  8) & 0xFF,
         timeout        & 0xFF,
    };
    sx1262_write_command(CMD_SET_RX, buf, 3);
}

static void sx1262_set_tx(uint32_t timeout)
{
    uint8_t buf[3] = {
        (timeout >> 16) & 0xFF,
        (timeout >>  8) & 0xFF,
         timeout        & 0xFF,
    };
    sx1262_write_command(CMD_SET_TX, buf, 3);
}

// ---------------------------------------------------------------------------
// DIO1 ISR + IRQ handler task
// ---------------------------------------------------------------------------

static void IRAM_ATTR dio1_isr(void *arg)
{
    BaseType_t wake = pdFALSE;
    vTaskNotifyGiveFromISR(s_radio.irq_task, &wake);
    portYIELD_FROM_ISR(wake);
}

static void radio_irq_task(void *arg)
{
    while (1) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);

        uint16_t irq = sx1262_get_irq_status();
        sx1262_clear_irq(irq);

        if (irq & IRQ_TX_DONE) {
            ESP_LOGD(TAG, "TX done");
        }

        if (irq & IRQ_TIMEOUT) {
            ESP_LOGD(TAG, "radio timeout (irq=0x%04x)", irq);
            if (s_radio.receiving) {
                /* Re-arm continuous RX after a timeout */
                sx1262_set_rx(0xFFFFFF);
            }
        }

        if (irq & IRQ_CRC_ERR) {
            ESP_LOGW(TAG, "RX CRC error");
            if (s_radio.receiving) {
                sx1262_set_rx(0xFFFFFF);
            }
        }

        if (irq & IRQ_RX_DONE) {
            uint8_t pkt_len = 0, pkt_offset = 0;
            sx1262_get_rx_buffer_status(&pkt_len, &pkt_offset);

            if (pkt_len > 0 && pkt_len <= 255) {
                uint8_t buf[256];
                sx1262_read_buffer(pkt_offset, buf, pkt_len);

                /* Instantaneous RSSI: GetRssiInst returns one byte, dBm = -val/2 */
                uint8_t rssi_raw = 0;
                sx1262_read_command(CMD_GET_RSSI_INST, &rssi_raw, 1);
                int rssi_dbm = -(int)rssi_raw / 2;

                ESP_LOGD(TAG, "RX done len=%u rssi=%d dBm", pkt_len, rssi_dbm);

                if (s_radio.rx_cb) {
                    s_radio.rx_cb(buf, pkt_len, rssi_dbm, s_radio.rx_cb_data);
                }
            } else {
                ESP_LOGW(TAG, "RX done but pkt_len=%u — discarding", pkt_len);
            }

            /* Re-arm continuous receive */
            if (s_radio.receiving) {
                sx1262_set_rx(0xFFFFFF);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t sx1262_init(const void *config)
{
    if (s_radio.initialized) {
        ESP_LOGW(TAG, "already initialized");
        return ESP_OK;
    }

    memcpy(&s_radio.cfg, config, sizeof(radio_sx1262_config_t));

    /* Sensible defaults */
    s_radio.frequency_hz     = 915000000;
    s_radio.tx_power_dbm     = 22;
    s_radio.spreading_factor = 7;
    s_radio.bandwidth_hz     = 125000;
    s_radio.receiving        = false;
    s_radio.rx_cb            = NULL;
    s_radio.rx_cb_data       = NULL;

    /* --- 1. Add SPI device ------------------------------------------------ */
    int clock_hz = s_radio.cfg.spi_clock_hz ? s_radio.cfg.spi_clock_hz : 8000000;
    spi_device_interface_config_t devcfg = {
        .clock_speed_hz = clock_hz,
        .mode           = 0,
        .spics_io_num   = s_radio.cfg.pin_cs,
        .queue_size     = 1,
        .pre_cb         = NULL,
        .post_cb        = NULL,
    };
    esp_err_t err = spi_bus_add_device(s_radio.cfg.spi_host, &devcfg, &s_radio.spi);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "spi_bus_add_device failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 2. Configure BUSY and DIO1 GPIO inputs --------------------------- */
    gpio_config_t io_conf = {
        .intr_type    = GPIO_INTR_DISABLE,
        .mode         = GPIO_MODE_INPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
    };
    io_conf.pin_bit_mask = (1ULL << s_radio.cfg.pin_busy);
    gpio_config(&io_conf);

    /* DIO1 will be configured for rising-edge interrupt after ISR install */
    io_conf.pin_bit_mask = (1ULL << s_radio.cfg.pin_dio1);
    gpio_config(&io_conf);

    /* --- 3. Hardware reset ------------------------------------------------ */
    gpio_reset_pin(s_radio.cfg.pin_reset);
    gpio_set_direction(s_radio.cfg.pin_reset, GPIO_MODE_OUTPUT);
    gpio_set_level(s_radio.cfg.pin_reset, 0);
    vTaskDelay(pdMS_TO_TICKS(2));
    gpio_set_level(s_radio.cfg.pin_reset, 1);
    vTaskDelay(pdMS_TO_TICKS(10));

    /* --- 4. Wait for BUSY to go low after reset --------------------------- */
    sx1262_wait_busy(500);

    /* --- 5. SetStandby(STDBY_RC) ----------------------------------------- */
    err = sx1262_set_standby();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetStandby failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 6. SetPacketType(LoRa) ------------------------------------------ */
    err = sx1262_set_packet_type_lora();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetPacketType failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 7. SetRfFrequency ----------------------------------------------- */
    err = sx1262_apply_rf_frequency(s_radio.frequency_hz);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetRfFrequency failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 8. SetPaConfig (SX1262 HP PA) ----------------------------------- */
    err = sx1262_set_pa_config_sx1262();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetPaConfig failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 9. SetTxParams -------------------------------------------------- */
    err = sx1262_apply_tx_params(s_radio.tx_power_dbm);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetTxParams failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 10. SetModulationParams ----------------------------------------- */
    err = sx1262_apply_modulation_params();
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetModulationParams failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 11. SetPacketParams (max payload, explicit header, CRC on) ------- */
    err = sx1262_apply_packet_params(255);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetPacketParams failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 12. SetBufferBaseAddress(0x00, 0x00) ----------------------------- */
    err = sx1262_set_buffer_base_address(0x00, 0x00);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetBufferBaseAddress failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 13. SetDioIrqParams: TxDone | RxDone | Timeout | CrcErr on DIO1 - */
    uint16_t irq_mask = IRQ_TX_DONE | IRQ_RX_DONE | IRQ_TIMEOUT | IRQ_CRC_ERR;
    err = sx1262_set_dio_irq_params(irq_mask, irq_mask, 0x0000, 0x0000);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "SetDioIrqParams failed: %s", esp_err_to_name(err));
        return err;
    }

    /* --- 14. ClearIrqStatus ---------------------------------------------- */
    sx1262_clear_irq(0xFFFF);

    /* --- 15. Create IRQ handler task before enabling ISR ----------------- */
    BaseType_t ret = xTaskCreate(radio_irq_task, "sx1262_irq", 4096, NULL,
                                 configMAX_PRIORITIES - 1, &s_radio.irq_task);
    if (ret != pdPASS) {
        ESP_LOGE(TAG, "Failed to create IRQ task");
        return ESP_ERR_NO_MEM;
    }

    /* --- 16. Install DIO1 GPIO ISR (rising edge) ------------------------- */
    gpio_set_intr_type(s_radio.cfg.pin_dio1, GPIO_INTR_POSEDGE);
    gpio_install_isr_service(0);
    gpio_isr_handler_add(s_radio.cfg.pin_dio1, dio1_isr, NULL);
    gpio_intr_enable(s_radio.cfg.pin_dio1);

    s_radio.initialized = true;
    ESP_LOGI(TAG, "SX1262 initialized — freq=%luHz sf=%u bw=%luHz pwr=%ddBm",
             (unsigned long)s_radio.frequency_hz,
             s_radio.spreading_factor,
             (unsigned long)s_radio.bandwidth_hz,
             s_radio.tx_power_dbm);
    return ESP_OK;
}

static void sx1262_deinit(void)
{
    if (!s_radio.initialized) return;

    gpio_intr_disable(s_radio.cfg.pin_dio1);
    gpio_isr_handler_remove(s_radio.cfg.pin_dio1);

    if (s_radio.irq_task) {
        vTaskDelete(s_radio.irq_task);
        s_radio.irq_task = NULL;
    }

    sx1262_set_standby();

    spi_bus_remove_device(s_radio.spi);
    s_radio.spi         = NULL;
    s_radio.initialized = false;
    s_radio.receiving   = false;
    ESP_LOGI(TAG, "SX1262 deinitialized");
}

static esp_err_t sx1262_set_frequency(uint32_t freq_hz)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    s_radio.frequency_hz = freq_hz;
    return sx1262_apply_rf_frequency(freq_hz);
}

static esp_err_t sx1262_set_tx_power(int8_t dbm)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    s_radio.tx_power_dbm = dbm;
    esp_err_t err = sx1262_set_pa_config_sx1262();
    if (err != ESP_OK) return err;
    return sx1262_apply_tx_params(dbm);
}

static esp_err_t sx1262_set_bandwidth(uint32_t bw_hz)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    s_radio.bandwidth_hz = bw_hz;
    return sx1262_apply_modulation_params();
}

static esp_err_t sx1262_set_spreading_factor(uint8_t sf)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    if (sf < 5 || sf > 12) {
        ESP_LOGE(TAG, "invalid SF %u (must be 5–12)", sf);
        return ESP_ERR_INVALID_ARG;
    }
    s_radio.spreading_factor = sf;
    return sx1262_apply_modulation_params();
}

static esp_err_t sx1262_send(const uint8_t *data, size_t len)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    if (!data || len == 0 || len > 255) return ESP_ERR_INVALID_ARG;

    /* Halt any ongoing receive */
    s_radio.receiving = false;
    esp_err_t err = sx1262_set_standby();
    if (err != ESP_OK) return err;

    /* Write payload into TX FIFO at offset 0 */
    err = sx1262_write_buffer(0x00, data, len);
    if (err != ESP_OK) return err;

    /* Update packet params with actual payload length */
    err = sx1262_apply_packet_params((uint8_t)len);
    if (err != ESP_OK) return err;

    /* Start transmit — no software timeout; IRQ_TX_DONE fires on completion */
    sx1262_set_tx(0x000000);

    ESP_LOGD(TAG, "TX started, len=%u", (unsigned)len);
    return ESP_OK;
}

static esp_err_t sx1262_start_receive(hal_radio_rx_cb_t cb, void *user_data)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;

    s_radio.rx_cb      = cb;
    s_radio.rx_cb_data = user_data;

    esp_err_t err = sx1262_set_standby();
    if (err != ESP_OK) return err;

    /* Reset packet params to accept any payload up to 255 bytes */
    err = sx1262_apply_packet_params(255);
    if (err != ESP_OK) return err;

    s_radio.receiving = true;
    sx1262_set_rx(0xFFFFFF); /* continuous RX — no timeout */

    ESP_LOGD(TAG, "continuous RX started");
    return ESP_OK;
}

static esp_err_t sx1262_stop_receive(void)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;
    s_radio.receiving = false;
    return sx1262_set_standby();
}

static int sx1262_get_rssi(void)
{
    if (!s_radio.initialized) return -128;
    uint8_t raw = 0;
    sx1262_read_command(CMD_GET_RSSI_INST, &raw, 1);
    return -(int)raw / 2;
}

static esp_err_t sx1262_sleep(bool enter)
{
    if (!s_radio.initialized) return ESP_ERR_INVALID_STATE;

    if (enter) {
        /* Warm start — retains buffer, configuration in retention memory */
        uint8_t sleep_cfg = 0x04;
        return sx1262_write_command(CMD_SET_SLEEP, &sleep_cfg, 1);
    } else {
        /* Wake via hardware reset, then full re-init */
        gpio_set_level(s_radio.cfg.pin_reset, 0);
        vTaskDelay(pdMS_TO_TICKS(2));
        gpio_set_level(s_radio.cfg.pin_reset, 1);
        vTaskDelay(pdMS_TO_TICKS(10));
        sx1262_wait_busy(500);

        /* Re-apply full configuration */
        esp_err_t err;
        err = sx1262_set_standby();           if (err != ESP_OK) return err;
        err = sx1262_set_packet_type_lora();  if (err != ESP_OK) return err;
        err = sx1262_apply_rf_frequency(s_radio.frequency_hz);  if (err != ESP_OK) return err;
        err = sx1262_set_pa_config_sx1262(); if (err != ESP_OK) return err;
        err = sx1262_apply_tx_params(s_radio.tx_power_dbm);      if (err != ESP_OK) return err;
        err = sx1262_apply_modulation_params();                   if (err != ESP_OK) return err;
        err = sx1262_apply_packet_params(255);                    if (err != ESP_OK) return err;
        err = sx1262_set_buffer_base_address(0x00, 0x00);        if (err != ESP_OK) return err;
        uint16_t irq_mask = IRQ_TX_DONE | IRQ_RX_DONE | IRQ_TIMEOUT | IRQ_CRC_ERR;
        err = sx1262_set_dio_irq_params(irq_mask, irq_mask, 0x0000, 0x0000);
        if (err != ESP_OK) return err;
        sx1262_clear_irq(0xFFFF);

        ESP_LOGD(TAG, "woke from sleep, config restored");
        return ESP_OK;
    }
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_radio_driver_t s_vtable = {
    .init                 = sx1262_init,
    .deinit               = sx1262_deinit,
    .set_frequency        = sx1262_set_frequency,
    .set_tx_power         = sx1262_set_tx_power,
    .set_bandwidth        = sx1262_set_bandwidth,
    .set_spreading_factor = sx1262_set_spreading_factor,
    .send                 = sx1262_send,
    .start_receive        = sx1262_start_receive,
    .stop_receive         = sx1262_stop_receive,
    .get_rssi             = sx1262_get_rssi,
    .sleep                = sx1262_sleep,
    .name                 = "SX1262",
};

const hal_radio_driver_t *drv_radio_sx1262_get(void)
{
    return &s_vtable;
}
