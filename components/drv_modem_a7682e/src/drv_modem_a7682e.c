// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Simcom A7682E 4G LTE modem driver (esp_modem backend)

#include "drv_modem_a7682e.h"

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

static const char *TAG = "a7682e";

/* Default timing constants (milliseconds) --------------------------------- */
#define A7682E_DEFAULT_BAUD       115200
#define A7682E_DEFAULT_AT_TIMEOUT 5000
#define A7682E_PWRON_PULSE_MS     1500
#define A7682E_PWROFF_PULSE_MS    3000
#define A7682E_BOOT_WAIT_MS       5000  ///< Wait after PWRKEY pulse for modem boot
#define A7682E_PPP_TIMEOUT_S      30    ///< Seconds to wait for IP after PPP start

/* Driver state ------------------------------------------------------------ */
static struct {
    a7682e_config_t  cfg;
    esp_modem_dce_t *dce;        ///< Data Communication Equipment handle
    esp_netif_t     *ppp_netif;  ///< PPP network interface
    bool             initialized;
    bool             powered_on;
    bool             ppp_connected;
    a7682e_sms_cb_t  sms_cb;
    void            *sms_cb_data;
    bool             sms_initialized;
} s_modem;

/* =========================================================================
 * Internal — PPP event handler
 * ====================================================================== */

static void ppp_event_handler(void *arg, esp_event_base_t event_base,
                               int32_t event_id, void *event_data)
{
    if (event_base == IP_EVENT && event_id == IP_EVENT_PPP_GOT_IP) {
        ip_event_got_ip_t *event = (ip_event_got_ip_t *)event_data;
        ESP_LOGI(TAG, "PPP connected, IP: " IPSTR, IP2STR(&event->ip_info.ip));
        s_modem.ppp_connected = true;
    } else if (event_base == IP_EVENT && event_id == IP_EVENT_PPP_LOST_IP) {
        ESP_LOGI(TAG, "PPP disconnected");
        s_modem.ppp_connected = false;
    }
}

/* =========================================================================
 * Public API — Lifecycle
 * ====================================================================== */

esp_err_t drv_a7682e_init(const a7682e_config_t *config)
{
    if (s_modem.initialized) {
        return ESP_OK;
    }
    if (!config) {
        return ESP_ERR_INVALID_ARG;
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

    s_modem.initialized = true;
    ESP_LOGI(TAG, "driver initialised (esp_modem backend, UART%d, %lu baud, "
             "TX=%d RX=%d PWRKEY=%d RST=%d)",
             (int)s_modem.cfg.uart_num, (unsigned long)s_modem.cfg.baud_rate,
             (int)s_modem.cfg.pin_tx, (int)s_modem.cfg.pin_rx,
             (int)s_modem.cfg.pin_pwrkey, (int)s_modem.cfg.pin_reset);
    return ESP_OK;
}

void drv_a7682e_deinit(void)
{
    if (!s_modem.initialized) {
        return;
    }

    if (s_modem.ppp_connected) {
        drv_a7682e_stop_ppp();
    }

    if (s_modem.dce) {
        esp_modem_destroy(s_modem.dce);
        s_modem.dce = NULL;
    }

    if (s_modem.ppp_netif) {
        esp_netif_destroy(s_modem.ppp_netif);
        s_modem.ppp_netif = NULL;
    }

    /* Release GPIO */
    if (s_modem.cfg.pin_pwrkey >= 0) {
        gpio_reset_pin(s_modem.cfg.pin_pwrkey);
    }
    if (s_modem.cfg.pin_reset >= 0) {
        gpio_reset_pin(s_modem.cfg.pin_reset);
    }

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
            return ESP_ERR_NO_MEM;
        }

        /* Register PPP IP event handlers */
        esp_event_handler_register(IP_EVENT, IP_EVENT_PPP_GOT_IP,
                                   &ppp_event_handler, NULL);
        esp_event_handler_register(IP_EVENT, IP_EVENT_PPP_LOST_IP,
                                   &ppp_event_handler, NULL);

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
            esp_netif_destroy(s_modem.ppp_netif);
            s_modem.ppp_netif = NULL;
            return ESP_FAIL;
        }

        s_modem.powered_on = true;
        ESP_LOGI(TAG, "power-on: modem ready (esp_modem DCE created)");

    } else if (!on && s_modem.powered_on) {
        /* Power-off sequence:
         *   1. Stop PPP if active
         *   2. Destroy DCE (sends AT+CPOF internally if possible)
         *   3. Fallback PWRKEY pulse
         */
        if (s_modem.ppp_connected) {
            drv_a7682e_stop_ppp();
        }

        if (s_modem.dce) {
            esp_modem_destroy(s_modem.dce);
            s_modem.dce = NULL;
        }

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

    return ESP_OK;
}

/* =========================================================================
 * Public API — AT command interface
 * ====================================================================== */

esp_err_t drv_a7682e_send_at(const char *cmd, char *buf, size_t buf_len,
                              uint32_t timeout_ms)
{
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "send_at: modem not powered on");
        return ESP_ERR_INVALID_STATE;
    }
    if (!cmd) {
        return ESP_ERR_INVALID_ARG;
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
    if (!s_modem.dce) {
        return -999;
    }

    int rssi = 0, ber = 0;
    esp_err_t ret = esp_modem_get_signal_quality(s_modem.dce, &rssi, &ber);
    if (ret != ESP_OK) {
        return -999;
    }

    if (rssi == 99) {
        /* 99 = not detectable / unknown */
        return -999;
    }

    /* Convert to dBm: dBm = −113 + 2×rssi (range 0–31 → −113 to −51 dBm) */
    int dbm = -113 + 2 * rssi;
    ESP_LOGD(TAG, "signal: rssi=%d → %d dBm", rssi, dbm);
    return dbm;
}

a7682e_net_reg_t drv_a7682e_get_network_reg(void)
{
    if (!s_modem.dce) {
        return A7682E_NET_UNKNOWN;
    }

    char resp[64] = {0};
    if (esp_modem_at(s_modem.dce, "AT+CREG?", resp, 2000) != ESP_OK) {
        return A7682E_NET_UNKNOWN;
    }

    /* Response format: "+CREG: <n>,<stat>" or "+CREG: <stat>" */
    const char *p = strstr(resp, "+CREG:");
    if (!p) {
        ESP_LOGW(TAG, "get_network_reg: could not parse +CREG in: %s", resp);
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
 * Public API — PPP data connection
 * ====================================================================== */

esp_err_t drv_a7682e_start_ppp(void)
{
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "start_ppp: modem not powered on");
        return ESP_ERR_INVALID_STATE;
    }
    if (s_modem.ppp_connected) {
        return ESP_OK;
    }

    ESP_LOGI(TAG, "start_ppp: switching to PPP data mode...");

    /* Switch modem to PPP/data mode — esp_modem sends ATD*99# internally */
    esp_err_t ret = esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "start_ppp: failed to enter PPP mode: %s",
                 esp_err_to_name(ret));
        return ret;
    }

    /* Wait for IP_EVENT_PPP_GOT_IP (event handler sets ppp_connected) */
    for (int i = 0; i < A7682E_PPP_TIMEOUT_S && !s_modem.ppp_connected; i++) {
        vTaskDelay(pdMS_TO_TICKS(1000));
    }

    if (!s_modem.ppp_connected) {
        ESP_LOGW(TAG, "start_ppp: timed out waiting for IP address");
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
        return ESP_ERR_TIMEOUT;
    }

    ESP_LOGI(TAG, "start_ppp: PPP up — TCP/IP stack routed through 4G");
    return ESP_OK;
}

esp_err_t drv_a7682e_stop_ppp(void)
{
    if (!s_modem.dce) {
        return ESP_ERR_INVALID_STATE;
    }

    ESP_LOGI(TAG, "stop_ppp: returning to AT command mode");
    esp_err_t ret = esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
    s_modem.ppp_connected = false;
    return ret;
}

bool drv_a7682e_ppp_connected(void)
{
    return s_modem.ppp_connected;
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
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "sms_init: modem not powered on");
        return ESP_ERR_INVALID_STATE;
    }

    bool ppp_was_active = s_modem.ppp_connected;
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

restore:
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    }
    return ret;
}

esp_err_t drv_a7682e_send_sms(const char *phone, const char *msg)
{
    if (!phone || !msg) {
        return ESP_ERR_INVALID_ARG;
    }
    if (strlen(phone) >= 32) {
        ESP_LOGE(TAG, "send_sms: phone number too long (max 31 chars)");
        return ESP_ERR_INVALID_ARG;
    }
    if (strlen(msg) > 160) {
        ESP_LOGE(TAG, "send_sms: message too long (max 160 chars for GSM 7-bit)");
        return ESP_ERR_INVALID_ARG;
    }
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "send_sms: modem not powered on");
        return ESP_ERR_INVALID_STATE;
    }

    bool ppp_was_active = s_modem.ppp_connected;
    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_COMMAND);
    }

    char resp[128] = {0};

    /* Ensure text mode is active before sending */
    esp_modem_at(s_modem.dce, "AT+CMGF=1", resp, 2000);

    /* Build combined AT+CMGS command with message body and Ctrl+Z terminator.
     * The esp_modem AT handler passes the full string to the modem;
     * the embedded \r triggers the ">" prompt and the modem reads the text
     * up to the Ctrl+Z (0x1A) as the message body. */
    char cmd[320] = {0};
    snprintf(cmd, sizeof(cmd), "AT+CMGS=\"%s\"\r%s\x1A", phone, msg);

    ESP_LOGI(TAG, "send_sms: sending to %s", phone);
    esp_err_t ret = esp_modem_at(s_modem.dce, cmd, resp, 15000);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "send_sms: failed (%s), resp: %s",
                 esp_err_to_name(ret), resp);
    } else {
        ESP_LOGI(TAG, "send_sms: sent successfully, resp: %s", resp);
    }

    if (ppp_was_active) {
        esp_modem_set_mode(s_modem.dce, ESP_MODEM_MODE_DATA);
    }
    return ret;
}

esp_err_t drv_a7682e_read_sms(int index, char *sender, size_t sender_len,
                               char *body, size_t body_len)
{
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "read_sms: modem not powered on");
        return ESP_ERR_INVALID_STATE;
    }

    bool ppp_was_active = s_modem.ppp_connected;
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
    return ret;
}

esp_err_t drv_a7682e_delete_sms(int index)
{
    if (!s_modem.dce || !s_modem.powered_on) {
        ESP_LOGE(TAG, "delete_sms: modem not powered on");
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
    return ret;
}

void drv_a7682e_register_sms_cb(a7682e_sms_cb_t cb, void *user_data)
{
    s_modem.sms_cb      = cb;
    s_modem.sms_cb_data = user_data;
    /* TODO: Wire s_modem.sms_cb into an esp_modem URC handler once the
     * esp_modem library exposes a URC registration API.  The handler should
     * parse "+CMTI: \"ME\",<index>" lines and invoke sms_cb(index, sms_cb_data).
     * For now, callers can poll AT+CMGL="ALL" or use AT+CNMI URCs read via a
     * dedicated UART receive task. */
    ESP_LOGD(TAG, "register_sms_cb: callback %s",
             cb ? "registered" : "unregistered");
}
