// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Simcom A7682E 4G LTE modem driver

#include "drv_modem_a7682e.h"

#include "driver/uart.h"
#include "driver/gpio.h"
#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <string.h>
#include <stdlib.h>
#include <stdio.h>

static const char *TAG = "a7682e";

/* Default timeouts and timing constants (milliseconds) -------------------- */
#define A7682E_DEFAULT_BAUD       115200
#define A7682E_DEFAULT_AT_TIMEOUT 5000   ///< Default AT command timeout
#define A7682E_PWRON_PULSE_MS     1500   ///< PWRKEY low duration for power-on
#define A7682E_PWROFF_PULSE_MS    3000   ///< PWRKEY low duration for power-off
#define A7682E_BOOT_TIMEOUT_MS    10000  ///< Max wait for "RDY" after power-on
#define A7682E_READY_TIMEOUT_MS   1000   ///< Timeout for is_ready() probe
#define A7682E_UART_BUF_SIZE      512    ///< UART ring buffer (must be ≥256)
#define A7682E_RSP_BUF_SIZE       512    ///< Internal response accumulator

/* Driver state ------------------------------------------------------------ */
static struct {
    a7682e_config_t cfg;
    bool            initialized;
    bool            powered_on;
    char            rsp_buf[A7682E_RSP_BUF_SIZE];
} s_modem;

/* =========================================================================
 * Internal helpers
 * ====================================================================== */

/**
 * @brief Read bytes from UART until a terminal string is found or timeout.
 *
 * Appends bytes to buf (null-terminated).  Stops early when either
 * "OK\r\n" or "ERROR\r\n" is present anywhere in the accumulated buffer.
 *
 * @param buf         Output buffer (will be null-terminated).
 * @param buf_size    Total size of buf in bytes.
 * @param timeout_ms  Maximum time to wait for a terminal string.
 * @return Number of bytes accumulated (excluding null terminator).
 */
static int modem_read_response(char *buf, size_t buf_size, uint32_t timeout_ms)
{
    int      idx   = 0;
    TickType_t start = xTaskGetTickCount();

    buf[0] = '\0';

    while ((xTaskGetTickCount() - start) < pdMS_TO_TICKS(timeout_ms)) {
        uint8_t byte;
        int     len = uart_read_bytes(s_modem.cfg.uart_num, &byte, 1,
                                      pdMS_TO_TICKS(50));
        if (len > 0) {
            if (idx < (int)(buf_size - 1)) {
                buf[idx++] = (char)byte;
                buf[idx]   = '\0';
            }
            /* Check for terminal strings */
            if (strstr(buf, "OK\r\n") || strstr(buf, "ERROR\r\n")) {
                break;
            }
        }
    }
    return idx;
}

/**
 * @brief Flush the UART RX FIFO.
 */
static void modem_flush_rx(void)
{
    uart_flush_input(s_modem.cfg.uart_num);
}

/**
 * @brief Write a string to the modem UART (no CR/LF appended).
 */
static void modem_uart_write(const char *str)
{
    uart_write_bytes(s_modem.cfg.uart_num, str, strlen(str));
}

/* =========================================================================
 * Public API — Lifecycle
 * ====================================================================== */

esp_err_t drv_a7682e_init(const a7682e_config_t *config)
{
    if (!config) {
        return ESP_ERR_INVALID_ARG;
    }

    memcpy(&s_modem.cfg, config, sizeof(s_modem.cfg));

    /* Apply default baud rate */
    if (s_modem.cfg.baud_rate == 0) {
        s_modem.cfg.baud_rate = A7682E_DEFAULT_BAUD;
    }

    /* -----------------------------------------------------------------
     * UART driver installation
     * -------------------------------------------------------------- */
    uart_config_t uart_cfg = {
        .baud_rate           = (int)s_modem.cfg.baud_rate,
        .data_bits           = UART_DATA_8_BITS,
        .parity              = UART_PARITY_DISABLE,
        .stop_bits           = UART_STOP_BITS_1,
        .flow_ctrl           = UART_HW_FLOWCTRL_DISABLE,
        .source_clk          = UART_SCLK_DEFAULT,
    };

    esp_err_t ret = uart_param_config(s_modem.cfg.uart_num, &uart_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "uart_param_config failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ret = uart_set_pin(s_modem.cfg.uart_num,
                       s_modem.cfg.pin_tx,
                       s_modem.cfg.pin_rx,
                       UART_PIN_NO_CHANGE,
                       UART_PIN_NO_CHANGE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "uart_set_pin failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ret = uart_driver_install(s_modem.cfg.uart_num,
                              A7682E_UART_BUF_SIZE * 2, /* RX ring buffer */
                              A7682E_UART_BUF_SIZE,     /* TX ring buffer */
                              0, NULL, 0);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "uart_driver_install failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* -----------------------------------------------------------------
     * GPIO configuration — PWRKEY and RESET, both initially de-asserted
     * (high).  The A7682E is active-low on both lines.
     * -------------------------------------------------------------- */
    gpio_config_t gpio_cfg = {
        .pin_bit_mask = (1ULL << s_modem.cfg.pin_pwrkey) |
                        (1ULL << s_modem.cfg.pin_reset),
        .mode         = GPIO_MODE_OUTPUT,
        .pull_up_en   = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type    = GPIO_INTR_DISABLE,
    };
    ret = gpio_config(&gpio_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "gpio_config failed: %s", esp_err_to_name(ret));
        uart_driver_delete(s_modem.cfg.uart_num);
        return ret;
    }

    gpio_set_level(s_modem.cfg.pin_pwrkey, 1);
    gpio_set_level(s_modem.cfg.pin_reset,  1);

    s_modem.initialized = true;
    s_modem.powered_on  = false;

    ESP_LOGI(TAG, "driver initialised (UART%d, %lu baud, TX=%d RX=%d "
             "PWRKEY=%d RST=%d)",
             (int)s_modem.cfg.uart_num, (unsigned long)s_modem.cfg.baud_rate,
             (int)s_modem.cfg.pin_tx,   (int)s_modem.cfg.pin_rx,
             (int)s_modem.cfg.pin_pwrkey, (int)s_modem.cfg.pin_reset);
    return ESP_OK;
}

void drv_a7682e_deinit(void)
{
    if (!s_modem.initialized) {
        return;
    }

    if (s_modem.powered_on) {
        drv_a7682e_power(false);
    }

    uart_driver_delete(s_modem.cfg.uart_num);

    /* Release GPIO — reset pins to input/floating */
    gpio_reset_pin(s_modem.cfg.pin_pwrkey);
    gpio_reset_pin(s_modem.cfg.pin_reset);

    s_modem.initialized = false;
    s_modem.powered_on  = false;
    ESP_LOGI(TAG, "driver de-initialised");
}

/* =========================================================================
 * Public API — Power control
 * ====================================================================== */

esp_err_t drv_a7682e_power(bool on)
{
    if (!s_modem.initialized) {
        ESP_LOGE(TAG, "power: driver not initialised");
        return ESP_ERR_INVALID_STATE;
    }

    if (on) {
        /* ---------------------------------------------------------------
         * Power-on sequence
         *   1. Pull PWRKEY low for 1500 ms (Simcom AN specifies ≥1 s)
         *   2. Release PWRKEY high
         *   3. Wait up to 10 s for "RDY" unsolicited result code
         *   4. Disable echo (ATE0)
         *   5. Verify with AT ping
         * ------------------------------------------------------------ */
        ESP_LOGI(TAG, "power-on: asserting PWRKEY for %d ms",
                 A7682E_PWRON_PULSE_MS);

        modem_flush_rx();
        gpio_set_level(s_modem.cfg.pin_pwrkey, 0);
        vTaskDelay(pdMS_TO_TICKS(A7682E_PWRON_PULSE_MS));
        gpio_set_level(s_modem.cfg.pin_pwrkey, 1);

        ESP_LOGI(TAG, "power-on: waiting for RDY (up to %d ms)",
                 A7682E_BOOT_TIMEOUT_MS);

        int  n   = modem_read_response(s_modem.rsp_buf, sizeof(s_modem.rsp_buf),
                                       A7682E_BOOT_TIMEOUT_MS);
        bool rdy = (n > 0) &&
                   (strstr(s_modem.rsp_buf, "RDY") ||
                    strstr(s_modem.rsp_buf, "OK"));

        if (!rdy) {
            ESP_LOGW(TAG, "power-on: no RDY received within %d ms — "
                     "modem may already be on or hardware absent",
                     A7682E_BOOT_TIMEOUT_MS);
            /* Treat as a soft warning; continue anyway */
        } else {
            ESP_LOGI(TAG, "power-on: RDY received");
        }

        /* Disable echo so response parsing is reliable */
        modem_flush_rx();
        modem_uart_write("ATE0\r\n");
        modem_read_response(s_modem.rsp_buf, sizeof(s_modem.rsp_buf), 2000);

        /* Confirm modem is responding */
        modem_flush_rx();
        modem_uart_write("AT\r\n");
        int len = modem_read_response(s_modem.rsp_buf, sizeof(s_modem.rsp_buf),
                                      A7682E_DEFAULT_AT_TIMEOUT);
        if (len > 0 && strstr(s_modem.rsp_buf, "OK")) {
            s_modem.powered_on = true;
            ESP_LOGI(TAG, "power-on: modem ready");
            return ESP_OK;
        }

        ESP_LOGE(TAG, "power-on: modem did not respond to AT after boot");
        return ESP_ERR_TIMEOUT;

    } else {
        /* ---------------------------------------------------------------
         * Power-off sequence
         *   1. Attempt graceful shutdown via AT+CPOF
         *   2. Fall back to PWRKEY pulse (3 s) if modem does not respond
         * ------------------------------------------------------------ */
        ESP_LOGI(TAG, "power-off: sending AT+CPOF");

        modem_flush_rx();
        modem_uart_write("AT+CPOF\r\n");
        int len = modem_read_response(s_modem.rsp_buf, sizeof(s_modem.rsp_buf),
                                      3000);

        if (len == 0 || !strstr(s_modem.rsp_buf, "OK")) {
            ESP_LOGW(TAG, "power-off: AT+CPOF no response, using PWRKEY pulse");
            gpio_set_level(s_modem.cfg.pin_pwrkey, 0);
            vTaskDelay(pdMS_TO_TICKS(A7682E_PWROFF_PULSE_MS));
            gpio_set_level(s_modem.cfg.pin_pwrkey, 1);
        }

        s_modem.powered_on = false;
        ESP_LOGI(TAG, "power-off: done");
        return ESP_OK;
    }
}

/* =========================================================================
 * Public API — AT command interface
 * ====================================================================== */

esp_err_t drv_a7682e_send_at(const char *cmd, char *buf, size_t buf_len,
                              uint32_t timeout_ms)
{
    if (!s_modem.initialized) {
        ESP_LOGE(TAG, "send_at: driver not initialised");
        return ESP_ERR_INVALID_STATE;
    }
    if (!cmd) {
        return ESP_ERR_INVALID_ARG;
    }

    if (timeout_ms == 0) {
        timeout_ms = A7682E_DEFAULT_AT_TIMEOUT;
    }

    /* Use internal buffer if caller passes NULL */
    char  *rsp     = (buf && buf_len > 0) ? buf : s_modem.rsp_buf;
    size_t rsp_len = (buf && buf_len > 0) ? buf_len : sizeof(s_modem.rsp_buf);

    modem_flush_rx();

    /* Send "cmd\r\n" */
    modem_uart_write(cmd);
    modem_uart_write("\r\n");

    ESP_LOGD(TAG, "AT>> %s", cmd);

    int n = modem_read_response(rsp, rsp_len, timeout_ms);

    if (n > 0) {
        ESP_LOGD(TAG, "AT<< %s", rsp);
    }

    if (n > 0 && strstr(rsp, "OK\r\n")) {
        return ESP_OK;
    }
    if (n > 0 && strstr(rsp, "ERROR\r\n")) {
        ESP_LOGW(TAG, "send_at: ERROR response to '%s'", cmd);
        return ESP_FAIL;
    }

    ESP_LOGW(TAG, "send_at: timeout waiting for response to '%s'", cmd);
    return ESP_ERR_TIMEOUT;
}

bool drv_a7682e_is_ready(void)
{
    if (!s_modem.initialized) {
        return false;
    }

    modem_flush_rx();
    modem_uart_write("AT\r\n");
    int len = modem_read_response(s_modem.rsp_buf, sizeof(s_modem.rsp_buf),
                                  A7682E_READY_TIMEOUT_MS);
    return (len > 0 && strstr(s_modem.rsp_buf, "OK") != NULL);
}

/* =========================================================================
 * Public API — Network helpers
 * ====================================================================== */

int drv_a7682e_get_signal_rssi(void)
{
    char rsp[64] = {0};
    esp_err_t ret = drv_a7682e_send_at("AT+CSQ", rsp, sizeof(rsp),
                                        A7682E_DEFAULT_AT_TIMEOUT);
    if (ret != ESP_OK) {
        return -999;
    }

    /* Response format: "\r\n+CSQ: <rssi>,<ber>\r\n\r\nOK\r\n" */
    const char *p = strstr(rsp, "+CSQ:");
    if (!p) {
        ESP_LOGW(TAG, "get_signal_rssi: could not parse +CSQ in: %s", rsp);
        return -999;
    }

    int rssi = 0, ber = 0;
    if (sscanf(p, "+CSQ: %d,%d", &rssi, &ber) < 1) {
        ESP_LOGW(TAG, "get_signal_rssi: sscanf failed on: %s", p);
        return -999;
    }

    if (rssi == 99) {
        /* 99 = not detectable / unknown */
        return -999;
    }

    /* Convert to dBm: dBm = −113 + 2×rssi  (range 0–31 → −113 to −51 dBm) */
    int dbm = -113 + 2 * rssi;
    ESP_LOGD(TAG, "signal: rssi=%d → %d dBm", rssi, dbm);
    return dbm;
}

a7682e_net_reg_t drv_a7682e_get_network_reg(void)
{
    char rsp[64] = {0};
    esp_err_t ret = drv_a7682e_send_at("AT+CREG?", rsp, sizeof(rsp),
                                        A7682E_DEFAULT_AT_TIMEOUT);
    if (ret != ESP_OK) {
        return A7682E_NET_UNKNOWN;
    }

    /* Response format: "\r\n+CREG: <n>,<stat>\r\n\r\nOK\r\n" */
    const char *p = strstr(rsp, "+CREG:");
    if (!p) {
        ESP_LOGW(TAG, "get_network_reg: could not parse +CREG in: %s", rsp);
        return A7682E_NET_UNKNOWN;
    }

    int n = 0, stat = 0;
    /* The response may be "+CREG: <stat>" (1 field) or "+CREG: <n>,<stat>" */
    int parsed = sscanf(p, "+CREG: %d,%d", &n, &stat);
    if (parsed == 2) {
        /* n,stat form — stat is the registration status */
    } else if (parsed == 1) {
        /* Single-field form — value is stat */
        stat = n;
    } else {
        ESP_LOGW(TAG, "get_network_reg: sscanf failed on: %s", p);
        return A7682E_NET_UNKNOWN;
    }

    switch (stat) {
        case 0:  return A7682E_NET_NOT_REGISTERED;
        case 1:  return A7682E_NET_REGISTERED_HOME;
        case 2:  return A7682E_NET_SEARCHING;
        case 3:  return A7682E_NET_DENIED;
        case 5:  return A7682E_NET_REGISTERED_ROAM;
        default: return A7682E_NET_UNKNOWN;
    }
}

/* =========================================================================
 * Public API — Data / connectivity stubs (future use)
 * ====================================================================== */

esp_err_t drv_a7682e_connect_tcp(const char *host, uint16_t port)
{
    /* TODO: Use AT+CIPSTART or AT+CSOC to open a TCP socket.
     *       Sequence:
     *         AT+CREG? — verify network registration
     *         AT+CGDCONT=1,"IP","<apn>"
     *         AT+CIPSTART="TCP","<host>",<port>
     *         Wait for "CONNECT OK"
     */
    ESP_LOGW(TAG, "connect_tcp: not implemented (host=%s port=%u)",
             host ? host : "(null)", (unsigned)port);
    (void)host;
    (void)port;
    return ESP_ERR_NOT_SUPPORTED;
}

esp_err_t drv_a7682e_send_data(const uint8_t *data, size_t len)
{
    /* TODO: Use AT+CIPSEND=<len>, wait for '>', transmit data, check SEND OK. */
    ESP_LOGW(TAG, "send_data: not implemented (len=%zu)", len);
    (void)data;
    (void)len;
    return ESP_ERR_NOT_SUPPORTED;
}

esp_err_t drv_a7682e_http_get(const char *url, char *buf, size_t buf_len)
{
    /* TODO: Use AT+HTTPINIT, AT+HTTPPARA, AT+HTTPACTION=0, AT+HTTPREAD. */
    ESP_LOGW(TAG, "http_get: not implemented (url=%s)", url ? url : "(null)");
    (void)url;
    if (buf && buf_len > 0) {
        buf[0] = '\0';
    }
    return ESP_ERR_NOT_SUPPORTED;
}
