// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Simcom A7682E 4G LTE modem driver (esp_modem backend)

#include "drv_modem_a7682e.h"
#include "a7682e_sms_validation.h"
#include "a7682e_lifecycle.h"

#include "esp_modem_api.h"
#include "esp_modem_config.h"
#include "esp_netif.h"
#include "esp_netif_ppp.h"
#include "esp_event.h"
#include "esp_log.h"
#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <string.h>
#include <stdlib.h>
#include <stdatomic.h>

static const char *TAG = "a7682e";

/* Default timing constants (milliseconds) --------------------------------- */
#define A7682E_DEFAULT_BAUD       115200
#define A7682E_DEFAULT_AT_TIMEOUT 5000
#define A7682E_PWRON_PULSE_MS     1500
#define A7682E_PWROFF_PULSE_MS    3000
#define A7682E_BOOT_WAIT_MS       5000  ///< Wait after PWRKEY pulse for modem boot
#define A7682E_PPP_TIMEOUT_S      30    ///< Seconds to wait for IP after PPP start
#define A7682E_SMS_POLL_MS        3000

/* Driver state ------------------------------------------------------------ */
static struct {
    a7682e_config_t  cfg;
    esp_modem_dce_t *dce;        ///< Data Communication Equipment handle
    esp_netif_t     *ppp_netif;  ///< PPP network interface
    bool             powered_on;
    atomic_bool      ppp_connected;
    a7682e_sms_cb_t  sms_cb;
    void            *sms_cb_data;
    bool             sms_initialized;
    TaskHandle_t     sms_poll_task;
    bool             got_ip_handler_registered;
    bool             lost_ip_handler_registered;
} s_modem;

static void sms_poll_task(void *arg);
static void sms_poll_start_if_needed(void);
static void sms_poll_stop(void);
static esp_err_t stop_ppp_locked(void);
static void destroy_transport_locked(void);

/* =========================================================================
 * Internal — PPP event handler
 * ====================================================================== */

static void ppp_event_handler(void *arg, esp_event_base_t event_base,
                               int32_t event_id, void *event_data)
{
    if (event_base == IP_EVENT && event_id == IP_EVENT_PPP_GOT_IP) {
        ip_event_got_ip_t *event = (ip_event_got_ip_t *)event_data;
        ESP_LOGI(TAG, "PPP connected, IP: " IPSTR, IP2STR(&event->ip_info.ip));
        atomic_store(&s_modem.ppp_connected, true);
    } else if (event_base == IP_EVENT && event_id == IP_EVENT_PPP_LOST_IP) {
        ESP_LOGI(TAG, "PPP disconnected");
        atomic_store(&s_modem.ppp_connected, false);
    }
}

static void destroy_transport_locked(void)
{
    if (s_modem.got_ip_handler_registered) {
        esp_event_handler_unregister(IP_EVENT, IP_EVENT_PPP_GOT_IP,
                                     &ppp_event_handler);
        s_modem.got_ip_handler_registered = false;
    }
    if (s_modem.lost_ip_handler_registered) {
        esp_event_handler_unregister(IP_EVENT, IP_EVENT_PPP_LOST_IP,
                                     &ppp_event_handler);
        s_modem.lost_ip_handler_registered = false;
    }
    if (s_modem.dce) {
        esp_modem_destroy(s_modem.dce);
        s_modem.dce = NULL;
    }
    if (s_modem.ppp_netif) {
        esp_netif_destroy(s_modem.ppp_netif);
        s_modem.ppp_netif = NULL;
    }
    atomic_store(&s_modem.ppp_connected, false);
}

/* =========================================================================
 * Public API — Lifecycle
 * ====================================================================== */

esp_err_t drv_a7682e_init(const a7682e_config_t *config)
{
    if (!config) {
        return ESP_ERR_INVALID_ARG;
    }

    a7682e_init_result_t begin = a7682e_lifecycle_begin_init();
    if (begin == A7682E_INIT_ALREADY_RUNNING) {
        return ESP_OK;
    }
    if (begin != A7682E_INIT_ACQUIRED) {
        return ESP_ERR_INVALID_STATE;
    }

    memcpy(&s_modem.cfg, config, sizeof(a7682e_config_t));

    /* Apply default baud rate */
    if (s_modem.cfg.baud_rate == 0) {
        s_modem.cfg.baud_rate = A7682E_DEFAULT_BAUD;
    }

    /* Configure PWRKEY and RESET GPIOs, both initially de-asserted (high).
     * A7682E is active-low on both lines. */
    if (s_modem.cfg.pin_pwrkey >= 0) {
        gpio_set_direction(s_modem.cfg.pin_pwrkey, GPIO_MODE_OUTPUT);
        gpio_set_level(s_modem.cfg.pin_pwrkey, 1);
    }
    if (s_modem.cfg.pin_reset >= 0) {
        gpio_set_direction(s_modem.cfg.pin_reset, GPIO_MODE_OUTPUT);
        gpio_set_level(s_modem.cfg.pin_reset, 1);
    }

    atomic_store(&s_modem.ppp_connected, false);
    ESP_LOGI(TAG, "driver initialised (esp_modem backend, UART%d, %lu baud, "
             "TX=%d RX=%d PWRKEY=%d RST=%d)",
             (int)s_modem.cfg.uart_num, (unsigned long)s_modem.cfg.baud_rate,
             (int)s_modem.cfg.pin_tx, (int)s_modem.cfg.pin_rx,
             (int)s_modem.cfg.pin_pwrkey, (int)s_modem.cfg.pin_reset);
    a7682e_lifecycle_finish_init(true);
    return ESP_OK;
}

void drv_a7682e_deinit(void)
{
    if (!a7682e_lifecycle_begin_stop()) {
        return;
    }

    sms_poll_stop();

    if (atomic_load(&s_modem.ppp_connected)) {
        stop_ppp_locked();
    }

    destroy_transport_locked();

    /* Release GPIO */
    if (s_modem.cfg.pin_pwrkey >= 0) {
        gpio_reset_pin(s_modem.cfg.pin_pwrkey);
    }
    if (s_modem.cfg.pin_reset >= 0) {
        gpio_reset_pin(s_modem.cfg.pin_reset);
    }

    s_modem.powered_on  = false;
    s_modem.sms_initialized = false;
    s_modem.sms_cb = NULL;
    s_modem.sms_cb_data = NULL;
    atomic_store(&s_modem.ppp_connected, false);
    ESP_LOGI(TAG, "driver de-initialised");
    a7682e_lifecycle_finish_stop();
}

/* =========================================================================
 * Public API — Power control
 * ====================================================================== */

esp_err_t drv_a7682e_power(bool on)
{
    if (!a7682e_lifecycle_begin_operation()) {
        ESP_LOGE(TAG, "power: driver not initialised");
        return ESP_ERR_INVALID_STATE;
    }

    esp_err_t result = ESP_OK;

    if (on && !s_modem.powered_on) {
        /* Power-on sequence:
         *   1. Pull PWRKEY low for 1500 ms (Simcom AN specifies ≥1 s)
         *   2. Release PWRKEY high
         *   3. Wait for modem to boot
         *   4. Create esp_modem DCE and PPP netif
         */
        ESP_LOGI(TAG, "power-on: asserting PWRKEY for %d ms",
                 A7682E_PWRON_PULSE_MS);

        if (s_modem.cfg.pin_pwrkey >= 0) {
            gpio_set_level(s_modem.cfg.pin_pwrkey, 0);
            vTaskDelay(pdMS_TO_TICKS(A7682E_PWRON_PULSE_MS));
            gpio_set_level(s_modem.cfg.pin_pwrkey, 1);
            ESP_LOGI(TAG, "power-on: waiting %d ms for modem to boot",
                     A7682E_BOOT_WAIT_MS);
            vTaskDelay(pdMS_TO_TICKS(A7682E_BOOT_WAIT_MS));
        }

        /* Build esp_modem DTE (UART) configuration */
        esp_modem_dte_config_t dte_config = ESP_MODEM_DTE_DEFAULT_CONFIG();
        dte_config.uart_config.tx_io_num  = s_modem.cfg.pin_tx;
        dte_config.uart_config.rx_io_num  = s_modem.cfg.pin_rx;
        dte_config.uart_config.port_num   = s_modem.cfg.uart_num;
        dte_config.uart_config.baud_rate  = (int)s_modem.cfg.baud_rate;

        /* Create PPP network interface */
        esp_netif_config_t netif_ppp_config = ESP_NETIF_DEFAULT_PPP();
        s_modem.ppp_netif = esp_netif_new(&netif_ppp_config);
        if (!s_modem.ppp_netif) {
            ESP_LOGE(TAG, "power-on: failed to create PPP netif");
            result = ESP_ERR_NO_MEM;
            goto done;
        }

        /* Register PPP IP event handlers */
        esp_err_t event_ret = esp_event_handler_register(
            IP_EVENT, IP_EVENT_PPP_GOT_IP, &ppp_event_handler, NULL);
        if (event_ret != ESP_OK) {
            result = event_ret;
            destroy_transport_locked();
            goto done;
        }
        s_modem.got_ip_handler_registered = true;
        event_ret = esp_event_handler_register(
            IP_EVENT, IP_EVENT_PPP_LOST_IP, &ppp_event_handler, NULL);
        if (event_ret != ESP_OK) {
            result = event_ret;
            destroy_transport_locked();
            goto done;
        }
        s_modem.lost_ip_handler_registered = true;

        /* Create the modem DCE.
         * ESP_MODEM_DCE_SIM7600 is the closest supported device type for the
         * A7682E — both are Simcom LTE Cat-1 modems sharing the same AT
         * command set.
         *
         * Note: if you observe garbled PPP frames, disable CMUX defragmentation
         * by setting dte_config.uart_config.rx_buffer_size to a larger value
         * (≥4096) or by calling esp_modem_set_preferred_mode(dce, ESP_MODEM_MODE_COMMAND)
         * before entering data mode.
         */
        esp_modem_dce_config_t dce_config = ESP_MODEM_DCE_DEFAULT_CONFIG("");
        s_modem.dce = esp_modem_new_dev(ESP_MODEM_DCE_SIM7600,
                                        &dte_config, &dce_config,
                                        s_modem.ppp_netif);
        if (!s_modem.dce) {
            ESP_LOGE(TAG, "power-on: failed to create esp_modem DCE");
            destroy_transport_locked();
            result = ESP_FAIL;
            goto done;
        }

        s_modem.powered_on = true;
        ESP_LOGI(TAG, "power-on: modem ready (esp_modem DCE created)");

    } else if (!on && s_modem.powered_on) {
        sms_poll_stop();
        /* Power-off sequence:
         *   1. Stop PPP if active
         *   2. Destroy DCE (sends AT+CPOF internally if possible)
         *   3. Fallback PWRKEY pulse
         */
        if (atomic_load(&s_modem.ppp_connected)) {
            stop_ppp_locked();
        }

        destroy_transport_locked();

        if (s_modem.cfg.pin_pwrkey >= 0) {
            ESP_LOGI(TAG, "power-off: PWRKEY pulse (%d ms)",
                     A7682E_PWROFF_PULSE_MS);
            gpio_set_level(s_modem.cfg.pin_pwrkey, 0);
            vTaskDelay(pdMS_TO_TICKS(A7682E_PWROFF_PULSE_MS));
            gpio_set_level(s_modem.cfg.pin_pwrkey, 1);
        }

        s_modem.powered_on = false;
        ESP_LOGI(TAG, "power-off: done");
    }

done:
    a7682e_lifecycle_finish_operation();
    return result;
}

static void sms_poll_task(void *arg)
{
    (void)arg;

    while (true) {
        if (!a7682e_lifecycle_begin_operation()) {
            vTaskDelay(pdMS_TO_TICKS(A7682E_SMS_POLL_MS));
            continue;
        }
        if (!s_modem.sms_cb || !s_modem.sms_initialized || !s_modem.powered_on || !s_modem.dce) {
            a7682e_lifecycle_finish_operation();
            vTaskDelay(pdMS_TO_TICKS(A7682E_SMS_POLL_MS));
            continue;
        }

        bool ppp_was_active = atomic_load(&s_modem.ppp_connected);
        if (ppp_was_active) {
            esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
        }

        int unread_indices[16];
        size_t unread_count = 0;
        a7682e_sms_cb_t callback = s_modem.sms_cb;
        void *callback_data = s_modem.sms_cb_data;
        char resp[768] = {0};
        if (esp_modem_at(s_modem.dce, "AT+CMGL=\"REC UNREAD\"", resp, 5000) == ESP_OK) {
            const char *p = resp;
            while ((p = strstr(p, "+CMGL:")) != NULL) {
                int index = 0;
                if (sscanf(p, "+CMGL: %d", &index) == 1 && index > 0
                    && unread_count < sizeof(unread_indices) / sizeof(unread_indices[0])) {
                    unread_indices[unread_count++] = index;
                }
                p += 6;
            }
        }

        if (ppp_was_active) {
            esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
        }

        a7682e_lifecycle_finish_operation();
        for (size_t i = 0; i < unread_count; ++i) {
            callback(unread_indices[i], callback_data);
        }

        vTaskDelay(pdMS_TO_TICKS(A7682E_SMS_POLL_MS));
    }
}

static void sms_poll_start_if_needed(void)
{
    if (!s_modem.sms_cb || !s_modem.sms_initialized || !s_modem.powered_on || !s_modem.dce) {
        return;
    }
    if (s_modem.sms_poll_task) {
        return;
    }
    if (xTaskCreate(sms_poll_task, "a7682e_sms", 4096, NULL, 4, &s_modem.sms_poll_task) != pdPASS) {
        s_modem.sms_poll_task = NULL;
        ESP_LOGW(TAG, "sms poll task create failed");
    }
}

static void sms_poll_stop(void)
{
    if (s_modem.sms_poll_task) {
        vTaskDelete(s_modem.sms_poll_task);
        s_modem.sms_poll_task = NULL;
    }
}

/* =========================================================================
 * Public API — AT command interface
 * ====================================================================== */

esp_err_t drv_a7682e_send_at(const char *cmd, char *buf, size_t buf_len,
                              uint32_t timeout_ms)
{
    if (!cmd) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "send_at: modem not powered on");
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_INVALID_STATE;
    }

    if (timeout_ms == 0) {
        timeout_ms = A7682E_DEFAULT_AT_TIMEOUT;
    }

    char resp[512] = {0};
    esp_err_t ret = esp_modem_at(s_modem.dce, cmd, resp, (int)timeout_ms);

    if (buf && buf_len > 0) {
        strncpy(buf, resp, buf_len - 1);
        buf[buf_len - 1] = '\0';
    }

    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "send_at: '%s' returned %s", cmd, esp_err_to_name(ret));
    } else {
        ESP_LOGD(TAG, "AT<< %s", resp);
    }

    a7682e_lifecycle_finish_operation();
    return ret;
}

bool drv_a7682e_is_ready(void)
{
    char resp[64];
    return (drv_a7682e_send_at("AT", resp, sizeof(resp), 1000) == ESP_OK);
}

/* =========================================================================
 * Public API — Network helpers
 * ====================================================================== */

int drv_a7682e_get_signal_rssi(void)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return -999;
    }
    if (!s_modem.dce) {
        a7682e_lifecycle_finish_operation();
        return -999;
    }

    int rssi = 0, ber = 0;
    esp_err_t ret = esp_modem_get_signal_quality(s_modem.dce, &rssi, &ber);
    if (ret != ESP_OK) {
        a7682e_lifecycle_finish_operation();
        return -999;
    }

    if (rssi == 99) {
        /* 99 = not detectable / unknown */
        a7682e_lifecycle_finish_operation();
        return -999;
    }

    /* Convert to dBm: dBm = −113 + 2×rssi (range 0–31 → −113 to −51 dBm) */
    int dbm = -113 + 2 * rssi;
    ESP_LOGD(TAG, "signal: rssi=%d → %d dBm", rssi, dbm);
    a7682e_lifecycle_finish_operation();
    return dbm;
}

a7682e_net_reg_t drv_a7682e_get_network_reg(void)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return A7682E_NET_UNKNOWN;
    }
    if (!s_modem.dce) {
        a7682e_lifecycle_finish_operation();
        return A7682E_NET_UNKNOWN;
    }

    char resp[64] = {0};
    if (esp_modem_at(s_modem.dce, "AT+CREG?", resp, 2000) != ESP_OK) {
        a7682e_lifecycle_finish_operation();
        return A7682E_NET_UNKNOWN;
    }

    /* Response format: "+CREG: <n>,<stat>" or "+CREG: <stat>" */
    const char *p = strstr(resp, "+CREG:");
    if (!p) {
        ESP_LOGW(TAG, "get_network_reg: could not parse +CREG in: %s", resp);
        a7682e_lifecycle_finish_operation();
        return A7682E_NET_UNKNOWN;
    }

    int n = 0, stat = 0;
    int parsed = sscanf(p, "+CREG: %d,%d", &n, &stat);
    if (parsed == 2) {
        /* n,stat form — stat is the registration status */
    } else if (parsed == 1) {
        /* Single-field form — the value is stat */
        stat = n;
    } else {
        ESP_LOGW(TAG, "get_network_reg: sscanf failed on: %s", p);
        a7682e_lifecycle_finish_operation();
        return A7682E_NET_UNKNOWN;
    }

    a7682e_net_reg_t result;
    switch (stat) {
        case 0:  result = A7682E_NET_NOT_REGISTERED; break;
        case 1:  result = A7682E_NET_REGISTERED_HOME; break;
        case 2:  result = A7682E_NET_SEARCHING; break;
        case 3:  result = A7682E_NET_DENIED; break;
        case 5:  result = A7682E_NET_REGISTERED_ROAM; break;
        default: result = A7682E_NET_UNKNOWN; break;
    }
    a7682e_lifecycle_finish_operation();
    return result;
}

/* =========================================================================
 * Public API — PPP data connection
 * ====================================================================== */

esp_err_t drv_a7682e_start_ppp(void)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "start_ppp: modem not powered on");
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_INVALID_STATE;
    }
    if (atomic_load(&s_modem.ppp_connected)) {
        a7682e_lifecycle_finish_operation();
        return ESP_OK;
    }

    ESP_LOGI(TAG, "start_ppp: switching to PPP data mode...");

    /* Switch modem to PPP/data mode — esp_modem sends ATD*99# internally */
    esp_err_t ret = esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "start_ppp: failed to enter PPP mode: %s",
                 esp_err_to_name(ret));
        a7682e_lifecycle_finish_operation();
        return ret;
    }

    /* Wait for IP_EVENT_PPP_GOT_IP (event handler sets ppp_connected) */
    for (int i = 0; i < A7682E_PPP_TIMEOUT_S
         && !atomic_load(&s_modem.ppp_connected); i++) {
        vTaskDelay(pdMS_TO_TICKS(1000));
    }

    if (!atomic_load(&s_modem.ppp_connected)) {
        ESP_LOGW(TAG, "start_ppp: timed out waiting for IP address");
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_TIMEOUT;
    }

    ESP_LOGI(TAG, "start_ppp: PPP up — TCP/IP stack routed through 4G");
    a7682e_lifecycle_finish_operation();
    return ESP_OK;
}

static esp_err_t stop_ppp_locked(void)
{
    if (!s_modem.dce) {
        return ESP_ERR_INVALID_STATE;
    }

    ESP_LOGI(TAG, "stop_ppp: returning to AT command mode");
    esp_err_t ret = esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
    atomic_store(&s_modem.ppp_connected, false);
    return ret;
}

esp_err_t drv_a7682e_stop_ppp(void)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    esp_err_t ret = stop_ppp_locked();
    a7682e_lifecycle_finish_operation();
    return ret;
}

bool drv_a7682e_ppp_connected(void)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return false;
    }
    bool connected = atomic_load(&s_modem.ppp_connected);
    a7682e_lifecycle_finish_operation();
    return connected;
}

/* =========================================================================
 * Public API — Legacy stubs (superseded by PPP + standard networking)
 * ====================================================================== */

esp_err_t drv_a7682e_connect_tcp(const char *host, uint16_t port)
{
    (void)host;
    (void)port;
    /* With PPP active, use standard lwIP sockets or esp_http_client instead
     * of raw AT+CIPSTART commands. */
    ESP_LOGW(TAG, "connect_tcp: use lwIP sockets with PPP active");
    return ESP_ERR_NOT_SUPPORTED;
}

esp_err_t drv_a7682e_send_data(const uint8_t *data, size_t len)
{
    (void)data;
    (void)len;
    ESP_LOGW(TAG, "send_data: use standard sockets with PPP active");
    return ESP_ERR_NOT_SUPPORTED;
}

esp_err_t drv_a7682e_http_get(const char *url, char *buf, size_t buf_len)
{
    (void)url;
    (void)buf;
    (void)buf_len;
    /* With PPP active, esp_http_client routes through the PPP netif
     * automatically — no driver-level AT+HTTPACTION needed. */
    ESP_LOGW(TAG, "http_get: use esp_http_client with PPP active "
             "(works just like WiFi)");
    return ESP_ERR_NOT_SUPPORTED;
}

/* =========================================================================
 * Public API — SMS
 * ====================================================================== */

esp_err_t drv_a7682e_sms_init(void)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "sms_init: modem not powered on");
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_INVALID_STATE;
    }

    bool ppp_was_active = atomic_load(&s_modem.ppp_connected);
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
    }

    char resp[64] = {0};

    esp_err_t ret = esp_modem_at(s_modem.dce, "AT+CMGF=1", resp, 2000);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "sms_init: AT+CMGF=1 failed: %s", esp_err_to_name(ret));
        goto restore;
    }
    ESP_LOGD(TAG, "sms_init: text mode enabled");

    ret = esp_modem_at(s_modem.dce, "AT+CPMS=\"ME\",\"ME\",\"ME\"", resp, 2000);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "sms_init: AT+CPMS failed: %s", esp_err_to_name(ret));
        goto restore;
    }
    ESP_LOGD(TAG, "sms_init: preferred storage set to ME");

    ret = esp_modem_at(s_modem.dce, "AT+CNMI=2,1,0,0,0", resp, 2000);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "sms_init: AT+CNMI failed: %s", esp_err_to_name(ret));
        goto restore;
    }
    ESP_LOGD(TAG, "sms_init: +CMTI URC forwarding enabled");

    s_modem.sms_initialized = true;
    ESP_LOGI(TAG, "sms_init: SMS subsystem ready");
    sms_poll_start_if_needed();

restore:
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    }
    a7682e_lifecycle_finish_operation();
    return ret;
}

esp_err_t drv_a7682e_send_sms(const char *phone, const char *msg)
{
    if (!a7682e_sms_phone_is_valid(phone)
        || !a7682e_sms_message_is_valid(msg)) {
        ESP_LOGE(TAG, "send_sms: rejected unsafe recipient or message text");
        return ESP_ERR_INVALID_ARG;
    }
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "send_sms: modem not powered on");
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_INVALID_STATE;
    }

    bool ppp_was_active = atomic_load(&s_modem.ppp_connected);
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
    }

    /* Ensure text mode is active before sending */
    esp_err_t ret = esp_modem_sms_txt_mode(s_modem.dce, true);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "send_sms: failed to enable text mode: %s",
                 esp_err_to_name(ret));
        goto restore;
    }

    ESP_LOGI(TAG, "send_sms: sending to %s", phone);
    /* esp_modem_send_sms performs the prompt-aware two-stage exchange: it
     * submits the validated recipient command, waits for the CMGS prompt, and
     * only then transmits the validated message body and terminator. */
    ret = esp_modem_send_sms(s_modem.dce, phone, msg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "send_sms: failed: %s", esp_err_to_name(ret));
    } else {
        ESP_LOGI(TAG, "send_sms: sent successfully");
    }

restore:
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    }
    a7682e_lifecycle_finish_operation();
    return ret;
}

esp_err_t drv_a7682e_read_sms(int index, char *sender, size_t sender_len,
                               char *body, size_t body_len)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "read_sms: modem not powered on");
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_INVALID_STATE;
    }

    bool ppp_was_active = atomic_load(&s_modem.ppp_connected);
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
    }

    char cmd[32]   = {0};
    char resp[512] = {0};
    snprintf(cmd, sizeof(cmd), "AT+CMGR=%d", index);

    esp_err_t ret = esp_modem_at(s_modem.dce, cmd, resp, 5000);
    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "read_sms: AT+CMGR=%d failed: %s",
                 index, esp_err_to_name(ret));
        ret = ESP_ERR_NOT_FOUND;
        goto restore;
    }

    /* Response format (text mode):
     *   +CMGR: "REC READ","<sender>","","<timestamp>"\r\n
     *   <message body>\r\n
     *   \r\nOK
     */
    const char *header = strstr(resp, "+CMGR:");
    if (!header) {
        ESP_LOGW(TAG, "read_sms: no +CMGR in response: %s", resp);
        ret = ESP_ERR_NOT_FOUND;
        goto restore;
    }

    /* Extract sender: second quoted field on the +CMGR: line */
    if (sender && sender_len > 0) {
        const char *q1 = strchr(header, '"');           /* open quote of status */
        if (q1) {
            const char *q2 = strchr(q1 + 1, '"');       /* close quote of status */
            if (q2) {
                const char *q3 = strchr(q2 + 1, '"');   /* open quote of sender */
                if (q3) {
                    const char *q4 = strchr(q3 + 1, '"'); /* close quote of sender */
                    if (q4) {
                        size_t len = (size_t)(q4 - q3 - 1);
                        if (len >= sender_len) {
                            len = sender_len - 1;
                        }
                        memcpy(sender, q3 + 1, len);
                        sender[len] = '\0';
                    }
                }
            }
        }
    }

    /* Extract body: line immediately following the +CMGR: header line */
    if (body && body_len > 0) {
        const char *eol = strchr(header, '\n');
        if (eol) {
            eol++; /* skip the '\n' */
            /* Skip a leading '\r' if present */
            if (*eol == '\r') {
                eol++;
            }
            /* Body ends at the next '\r' or '\n' */
            const char *end = strpbrk(eol, "\r\n");
            size_t len = end ? (size_t)(end - eol) : strlen(eol);
            if (len >= body_len) {
                len = body_len - 1;
            }
            memcpy(body, eol, len);
            body[len] = '\0';
        }
    }

    ESP_LOGD(TAG, "read_sms: index=%d sender='%s'",
             index, (sender ? sender : "(not requested)"));

restore:
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    }
    a7682e_lifecycle_finish_operation();
    return ret;
}

esp_err_t drv_a7682e_delete_sms(int index)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "delete_sms: modem not powered on");
        a7682e_lifecycle_finish_operation();
        return ESP_ERR_INVALID_STATE;
    }

    char cmd[32]  = {0};
    char resp[64] = {0};
    snprintf(cmd, sizeof(cmd), "AT+CMGD=%d", index);

    esp_err_t ret = esp_modem_at(s_modem.dce, cmd, resp, 5000);
    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "delete_sms: AT+CMGD=%d failed: %s",
                 index, esp_err_to_name(ret));
    } else {
        ESP_LOGD(TAG, "delete_sms: index %d deleted", index);
    }
    a7682e_lifecycle_finish_operation();
    return ret;
}

void drv_a7682e_register_sms_cb(a7682e_sms_cb_t cb, void *user_data)
{
    if (!a7682e_lifecycle_begin_operation()) {
        return;
    }
    s_modem.sms_cb      = cb;
    s_modem.sms_cb_data = user_data;
    if (cb) {
        sms_poll_start_if_needed();
    } else {
        sms_poll_stop();
    }
    ESP_LOGD(TAG, "register_sms_cb: callback %s",
             cb ? "registered" : "unregistered");
    a7682e_lifecycle_finish_operation();
}
