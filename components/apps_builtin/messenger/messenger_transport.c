/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger transport backend implementations
 *
 * Four backends are registered at init time:
 *
 *  MSG_TRANSPORT_LORA     — uses hal_registry_t->radio (fully implemented)
 *  MSG_TRANSPORT_SMS      — A7682E 4G modem (send/receive via AT commands)
 *  MSG_TRANSPORT_BLE      — ble_manager relay stub (logs TODO, returns NOT_SUPPORTED)
 *  MSG_TRANSPORT_INTERNET — HTTP/WebSocket stub    (logs TODO, returns NOT_SUPPORTED)
 *
 * Adding a new backend: implement the five function pointers, fill a
 * static msg_transport_driver_t, and call messenger_register_transport()
 * from messenger_transport_init() below.
 */

#include "messenger/messenger_transport.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "stdlib.h"
#include "string.h"

#include "hal/board.h"   /* hal_get_registry() / hal_registry_t */
#include "drv_modem_a7682e.h"
#include "thistle/ble_manager.h"

static const char *TAG = "msg_transport";

/* ------------------------------------------------------------------ */
/* Registry storage                                                     */
/* ------------------------------------------------------------------ */

static const msg_transport_driver_t *s_registry[MSG_TRANSPORT_COUNT];
static int                           s_count;

/* ------------------------------------------------------------------ */
/* LoRa transport                                                       */
/* ------------------------------------------------------------------ */

/*
 * The LoRa transport wraps the HAL radio driver.  It keeps a single
 * shared RX callback so the radio driver (which has only one slot) can
 * be pointed at our shim, which then calls the messenger RX callback.
 *
 * Packet wire format (matches the original messenger_ui.c):
 *   bytes 0-3  : sender_id  (uint32_t, little-endian)
 *   bytes 4-5  : text_len   (uint16_t, little-endian)
 *   bytes 6+   : UTF-8 text (not null-terminated on wire)
 */

#define LORA_MAX_TEXT 249  /* 255 - 6 byte header */

static msg_rx_cb_t  s_lora_rx_cb;
static uint32_t     s_lora_device_id;  /* set on first send/start_receive */

static void lora_radio_rx_shim(const uint8_t *data, size_t len,
                                int rssi, void *user_data)
{
    (void)rssi;
    (void)user_data;

    if (!s_lora_rx_cb) return;
    if (len < 7) return;  /* minimum: 4 id + 2 len + 1 char */

    uint32_t sender_id;
    uint16_t msg_len;
    memcpy(&sender_id, data, 4);
    memcpy(&msg_len, data + 4, 2);

    /* Drop our own re-broadcasts */
    if (sender_id == s_lora_device_id) return;

    if (msg_len > len - 6) msg_len = (uint16_t)(len - 6);
    if (msg_len > LORA_MAX_TEXT) msg_len = LORA_MAX_TEXT;

    char sender[16];
    snprintf(sender, sizeof(sender), "Node-%08X", (unsigned)sender_id);

    char text[LORA_MAX_TEXT + 1];
    memcpy(text, data + 6, msg_len);
    text[msg_len] = '\0';

    s_lora_rx_cb(MSG_TRANSPORT_LORA, sender, text);
}

static bool lora_is_available(void)
{
    const hal_registry_t *reg = hal_get_registry();
    return (reg && reg->radio && reg->radio->send && reg->radio->start_receive);
}

static esp_err_t lora_send(const char *dest, const char *text)
{
    (void)dest;  /* LoRa is broadcast — dest is ignored */

    if (!lora_is_available()) {
        return ESP_ERR_NOT_SUPPORTED;
    }

    if (!text || text[0] == '\0') {
        return ESP_ERR_INVALID_ARG;
    }

    /* Ensure we have a device ID */
    if (s_lora_device_id == 0) {
        s_lora_device_id = (uint32_t)esp_timer_get_time() ^ 0xDEADBEEF;
        if (s_lora_device_id == 0) s_lora_device_id = 0xCAFEBABE;
    }

    size_t text_len = strlen(text);
    if (text_len > LORA_MAX_TEXT) text_len = LORA_MAX_TEXT;

    uint8_t packet[255];
    memcpy(packet, &s_lora_device_id, 4);
    uint16_t len16 = (uint16_t)text_len;
    memcpy(packet + 4, &len16, 2);
    memcpy(packet + 6, text, text_len);

    const hal_registry_t *reg = hal_get_registry();
    esp_err_t err = reg->radio->send(packet, 6 + text_len);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "LoRa send failed: %s", esp_err_to_name(err));
    }
    return err;
}

static esp_err_t lora_start_receive(msg_rx_cb_t cb)
{
    if (!lora_is_available()) return ESP_ERR_NOT_SUPPORTED;

    /* Ensure device ID is set so the shim can filter self-messages */
    if (s_lora_device_id == 0) {
        s_lora_device_id = (uint32_t)esp_timer_get_time() ^ 0xDEADBEEF;
        if (s_lora_device_id == 0) s_lora_device_id = 0xCAFEBABE;
    }

    s_lora_rx_cb = cb;
    const hal_registry_t *reg = hal_get_registry();
    esp_err_t err = reg->radio->start_receive(lora_radio_rx_shim, NULL);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "LoRa start_receive failed: %s", esp_err_to_name(err));
        s_lora_rx_cb = NULL;
    }
    return err;
}

static void lora_stop_receive(void)
{
    s_lora_rx_cb = NULL;
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->radio && reg->radio->stop_receive) {
        reg->radio->stop_receive();
    }
}

static const msg_transport_driver_t s_lora_driver = {
    .type           = MSG_TRANSPORT_LORA,
    .name           = "LoRa",
    .icon           = "[~]",
    .is_available   = lora_is_available,
    .send           = lora_send,
    .start_receive  = lora_start_receive,
    .stop_receive   = lora_stop_receive,
};

/* ------------------------------------------------------------------ */
/* SMS transport (A7682E modem)                                         */
/* ------------------------------------------------------------------ */

static msg_rx_cb_t s_sms_rx_cb;

static bool sms_is_available(void)
{
    return drv_a7682e_is_ready();
}

static esp_err_t sms_send(const char *dest, const char *text)
{
    if (!dest || !text) return ESP_ERR_INVALID_ARG;
    return drv_a7682e_send_sms(dest, text);
}

static void sms_incoming_handler(int index, void *user_data)
{
    (void)user_data;
    if (!s_sms_rx_cb) return;

    char sender[32] = {0};
    char body[200]  = {0};
    if (drv_a7682e_read_sms(index, sender, sizeof(sender),
                             body, sizeof(body)) == ESP_OK) {
        s_sms_rx_cb(MSG_TRANSPORT_SMS, sender, body);
        drv_a7682e_delete_sms(index);
    }
}

static esp_err_t sms_start_receive(msg_rx_cb_t cb)
{
    s_sms_rx_cb = cb;
    esp_err_t ret = drv_a7682e_sms_init();
    if (ret != ESP_OK) return ret;
    drv_a7682e_register_sms_cb(sms_incoming_handler, NULL);
    return ESP_OK;
}

static void sms_stop_receive(void)
{
    drv_a7682e_register_sms_cb(NULL, NULL);
    s_sms_rx_cb = NULL;
}

static const msg_transport_driver_t s_sms_driver = {
    .type           = MSG_TRANSPORT_SMS,
    .name           = "SMS",
    .icon           = "[M]",
    .is_available   = sms_is_available,
    .send           = sms_send,
    .start_receive  = sms_start_receive,
    .stop_receive   = sms_stop_receive,
};

/* ------------------------------------------------------------------ */
/* BLE relay transport (stub)                                           */
/* ------------------------------------------------------------------ */

/*
 * BLE framing for messenger traffic:
 *   M1|<sender>|<text>
 * where <sender> is "Node-XXXXXXXX" and <text> is UTF-8 payload.
 */
#define BLE_MAX_TEXT 220

static msg_rx_cb_t s_ble_rx_cb;
static uint32_t    s_ble_device_id;

static void ble_build_sender(char *out, size_t out_len)
{
    if (s_ble_device_id == 0) {
        s_ble_device_id = (uint32_t)esp_timer_get_time() ^ 0xB1E01234;
        if (s_ble_device_id == 0) s_ble_device_id = 0xB1E00001;
    }
    snprintf(out, out_len, "Node-%08X", (unsigned)s_ble_device_id);
}

static void ble_rx_handler(const uint8_t *data, size_t len, void *user_data)
{
    (void)user_data;
    if (!s_ble_rx_cb || !data || len < 6) return;

    char frame[BLE_MAX_TEXT + 48];
    size_t copy_len = len < (sizeof(frame) - 1) ? len : (sizeof(frame) - 1);
    memcpy(frame, data, copy_len);
    frame[copy_len] = '\0';

    if (strncmp(frame, "M1|", 3) != 0) return;

    char *sender = frame + 3;
    char *sep = strchr(sender, '|');
    if (!sep) return;
    *sep = '\0';

    char *text = sep + 1;
    if (text[0] == '\0') return;

    if (strncmp(sender, "Node-", 5) == 0) {
        unsigned long sender_id = strtoul(sender + 5, NULL, 16);
        if ((uint32_t)sender_id == s_ble_device_id) return;
    }

    s_ble_rx_cb(MSG_TRANSPORT_BLE, sender, text);
}

static bool ble_is_available(void)
{
    return (ble_manager_get_state() == BLE_STATE_CONNECTED);
}

static esp_err_t ble_send(const char *dest, const char *text)
{
    (void)dest;
    if (!ble_is_available()) return ESP_ERR_NOT_SUPPORTED;
    if (!text || text[0] == '\0') return ESP_ERR_INVALID_ARG;

    char sender[16];
    ble_build_sender(sender, sizeof(sender));

    size_t text_len = strlen(text);
    if (text_len > BLE_MAX_TEXT) text_len = BLE_MAX_TEXT;

    char frame[BLE_MAX_TEXT + 48];
    int written = snprintf(frame, sizeof(frame), "M1|%s|%.*s",
                           sender, (int)text_len, text);
    if (written <= 0 || written >= (int)sizeof(frame)) {
        return ESP_ERR_INVALID_SIZE;
    }

    esp_err_t err = ble_manager_send((const uint8_t *)frame, (size_t)written);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "ble_manager_send failed: %s", esp_err_to_name(err));
    }
    return err;
}

static esp_err_t ble_start_receive(msg_rx_cb_t cb)
{
    if (!ble_is_available()) return ESP_ERR_NOT_SUPPORTED;
    s_ble_rx_cb = cb;
    esp_err_t err = ble_manager_register_rx_cb(ble_rx_handler, NULL);
    if (err != ESP_OK) {
        s_ble_rx_cb = NULL;
    }
    return err;
}

static void ble_stop_receive(void)
{
    s_ble_rx_cb = NULL;
    ble_manager_register_rx_cb(NULL, NULL);
}

static const msg_transport_driver_t s_ble_driver = {
    .type           = MSG_TRANSPORT_BLE,
    .name           = "BLE",
    .icon           = "[B]",
    .is_available   = ble_is_available,
    .send           = ble_send,
    .start_receive  = ble_start_receive,
    .stop_receive   = ble_stop_receive,
};

/* ------------------------------------------------------------------ */
/* Internet transport (stub)                                            */
/* ------------------------------------------------------------------ */

static bool internet_is_available(void)
{
    /* Hidden until readiness/send/rx are implemented end-to-end. */
    return false;
}

static esp_err_t internet_send(const char *dest, const char *text)
{
    (void)dest;
    (void)text;
    /*
     * TODO: POST to messaging API endpoint using esp_http_client.
     * The endpoint URL and auth token should be read from NVS.
     */
    ESP_LOGW(TAG, "Internet send: not yet implemented");
    return ESP_ERR_NOT_SUPPORTED;
}

static esp_err_t internet_start_receive(msg_rx_cb_t cb)
{
    (void)cb;
    /*
     * TODO: open a WebSocket connection and dispatch received frames
     * to cb() on the LVGL task via lv_async_call.
     */
    ESP_LOGW(TAG, "Internet start_receive: not yet implemented");
    return ESP_ERR_NOT_SUPPORTED;
}

static void internet_stop_receive(void)
{
    /* TODO: close WebSocket / unregister polling timer */
}

static const msg_transport_driver_t s_internet_driver = {
    .type           = MSG_TRANSPORT_INTERNET,
    .name           = "Internet",
    .icon           = "[W]",
    .is_available   = internet_is_available,
    .send           = internet_send,
    .start_receive  = internet_start_receive,
    .stop_receive   = internet_stop_receive,
};

/* ------------------------------------------------------------------ */
/* Registry implementation                                              */
/* ------------------------------------------------------------------ */

void messenger_register_transport(const msg_transport_driver_t *driver)
{
    if (!driver) return;
    if (driver->type >= MSG_TRANSPORT_COUNT) {
        ESP_LOGE(TAG, "invalid transport type %d", (int)driver->type);
        return;
    }
    s_registry[driver->type] = driver;
    s_count++;
    ESP_LOGI(TAG, "registered transport: %s", driver->name);
}

const msg_transport_driver_t *messenger_get_transport(msg_transport_t type)
{
    if (type >= MSG_TRANSPORT_COUNT) return NULL;
    return s_registry[type];
}

int messenger_get_available_transports(const msg_transport_driver_t **out, int max)
{
    int n = 0;
    for (int i = 0; i < MSG_TRANSPORT_COUNT && n < max; i++) {
        if (s_registry[i] && s_registry[i]->is_available()) {
            out[n++] = s_registry[i];
        }
    }
    return n;
}

void messenger_transport_init(void)
{
    memset(s_registry, 0, sizeof(s_registry));
    s_count = 0;

    messenger_register_transport(&s_lora_driver);
    messenger_register_transport(&s_sms_driver);
    messenger_register_transport(&s_ble_driver);
    messenger_register_transport(&s_internet_driver);

    ESP_LOGI(TAG, "transport layer ready (%d backends)", MSG_TRANSPORT_COUNT);
}
