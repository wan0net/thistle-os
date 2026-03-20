/*
 * ble_manager.c — ThistleOS BLE GATT server (NimBLE)
 *
 * Exposes a Nordic UART Service (NUS) compatible GATT server so a
 * companion phone app can connect for notifications, file transfer,
 * and device control.
 *
 * Service:  6E400001-B5A3-F393-E0A9-E50E24DCCA9E
 * TX (notify):  6E400003-B5A3-F393-E0A9-E50E24DCCA9E  (device → phone)
 * RX (write):   6E400002-B5A3-F393-E0A9-E50E24DCCA9E  (phone → device)
 *
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "thistle/ble_manager.h"
#include "thistle/event.h"
#include "esp_log.h"
#include "esp_nimble_hci.h"
#include "nimble/nimble_port.h"
#include "nimble/nimble_port_freertos.h"
#include "host/ble_hs.h"
#include "host/util/util.h"
#include "services/gap/ble_svc_gap.h"
#include "services/gatt/ble_svc_gatt.h"
#include <string.h>

static const char *TAG = "ble_mgr";

/* ThistleOS custom service UUID: 6E400001-B5A3-F393-E0A9-E50E24DCCA9E (Nordic UART) */
static const ble_uuid128_t svc_uuid = BLE_UUID128_INIT(
    0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
    0x93, 0xF3, 0xA3, 0xB5, 0x01, 0x00, 0x40, 0x6E
);

/* TX characteristic UUID (notify) */
static const ble_uuid128_t tx_chr_uuid = BLE_UUID128_INIT(
    0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
    0x93, 0xF3, 0xA3, 0xB5, 0x03, 0x00, 0x40, 0x6E
);

/* RX characteristic UUID (write) */
static const ble_uuid128_t rx_chr_uuid = BLE_UUID128_INIT(
    0x9E, 0xCA, 0xDC, 0x24, 0x0E, 0xE5, 0xA9, 0xE0,
    0x93, 0xF3, 0xA3, 0xB5, 0x02, 0x00, 0x40, 0x6E
);

static struct {
    char device_name[BLE_DEVICE_NAME_MAX + 1];
    ble_state_t state;
    uint16_t conn_handle;
    uint16_t tx_attr_handle;
    ble_rx_cb_t rx_cb;
    void *rx_cb_data;
    bool initialized;
} s_ble;

/* GATT access callback for RX characteristic */
static int ble_gatt_rx_access(uint16_t conn_handle, uint16_t attr_handle,
                               struct ble_gatt_access_ctxt *ctxt, void *arg)
{
    (void)conn_handle; (void)attr_handle; (void)arg;

    if (ctxt->op == BLE_GATT_ACCESS_OP_WRITE_CHR) {
        uint16_t len = OS_MBUF_PKTLEN(ctxt->om);
        uint8_t buf[512];
        if (len > sizeof(buf)) len = sizeof(buf);

        int rc = ble_hs_mbuf_to_flat(ctxt->om, buf, len, NULL);
        if (rc == 0 && s_ble.rx_cb) {
            s_ble.rx_cb(buf, len, s_ble.rx_cb_data);
        }
        return 0;
    }
    return BLE_ATT_ERR_UNLIKELY;
}

/* GATT service definition */
static const struct ble_gatt_svc_def gatt_svr_svcs[] = {
    {
        .type = BLE_GATT_SVC_TYPE_PRIMARY,
        .uuid = &svc_uuid.u,
        .characteristics = (struct ble_gatt_chr_def[]){
            {
                /* TX (notify) */
                .uuid = &tx_chr_uuid.u,
                .flags = BLE_GATT_CHR_F_NOTIFY,
                .val_handle = &s_ble.tx_attr_handle,
            },
            {
                /* RX (write) */
                .uuid = &rx_chr_uuid.u,
                .access_cb = ble_gatt_rx_access,
                .flags = BLE_GATT_CHR_F_WRITE | BLE_GATT_CHR_F_WRITE_NO_RSP,
            },
            { 0 },  /* sentinel */
        },
    },
    { 0 },  /* sentinel */
};

/* GAP event handler */
static int ble_gap_event(struct ble_gap_event *event, void *arg)
{
    (void)arg;

    switch (event->type) {
    case BLE_GAP_EVENT_CONNECT:
        if (event->connect.status == 0) {
            s_ble.conn_handle = event->connect.conn_handle;
            s_ble.state = BLE_STATE_CONNECTED;
            ESP_LOGI(TAG, "BLE connected (handle=%d)", s_ble.conn_handle);
            event_publish_simple(EVENT_WIFI_CONNECTED); /* Reuse for now — TODO: add BLE events */
        } else {
            ESP_LOGW(TAG, "BLE connection failed: %d", event->connect.status);
            ble_manager_start_advertising();
        }
        break;

    case BLE_GAP_EVENT_DISCONNECT:
        s_ble.state = BLE_STATE_ADVERTISING;
        ESP_LOGI(TAG, "BLE disconnected, reason=%d", event->disconnect.reason);
        ble_manager_start_advertising();
        break;

    case BLE_GAP_EVENT_ADV_COMPLETE:
        ESP_LOGD(TAG, "Advertising complete");
        break;

    case BLE_GAP_EVENT_MTU:
        ESP_LOGI(TAG, "MTU updated: %d", event->mtu.value);
        break;

    default:
        break;
    }

    return 0;
}

/* NimBLE host task */
static void ble_host_task(void *param)
{
    (void)param;
    nimble_port_run();
    nimble_port_freertos_deinit();
}

/* Host sync callback — called when BLE stack is ready */
static void ble_on_sync(void)
{
    /* Use best available address */
    ble_hs_id_infer_auto(0, NULL);

    ESP_LOGI(TAG, "BLE stack synchronized, starting advertising");
    ble_manager_start_advertising();
}

static void ble_on_reset(int reason)
{
    ESP_LOGW(TAG, "BLE host reset, reason=%d", reason);
}

esp_err_t ble_manager_init(const char *device_name)
{
    if (s_ble.initialized) return ESP_OK;
    if (!device_name) device_name = "ThistleOS";

    strncpy(s_ble.device_name, device_name, BLE_DEVICE_NAME_MAX);
    s_ble.device_name[BLE_DEVICE_NAME_MAX] = '\0';
    s_ble.state = BLE_STATE_OFF;
    s_ble.conn_handle = BLE_HS_CONN_HANDLE_NONE;

    /* Initialize NimBLE */
    esp_err_t ret = nimble_port_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "nimble_port_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* Configure host callbacks */
    ble_hs_cfg.sync_cb = ble_on_sync;
    ble_hs_cfg.reset_cb = ble_on_reset;

    /* Initialize GATT services */
    ble_svc_gap_init();
    ble_svc_gatt_init();

    int rc = ble_gatts_count_cfg(gatt_svr_svcs);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gatts_count_cfg failed: %d", rc);
        return ESP_FAIL;
    }

    rc = ble_gatts_add_svcs(gatt_svr_svcs);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gatts_add_svcs failed: %d", rc);
        return ESP_FAIL;
    }

    /* Set device name */
    ble_svc_gap_device_name_set(s_ble.device_name);

    /* Start the NimBLE host task */
    nimble_port_freertos_init(ble_host_task);

    s_ble.initialized = true;
    ESP_LOGI(TAG, "BLE manager initialized: '%s'", s_ble.device_name);
    return ESP_OK;
}

esp_err_t ble_manager_start_advertising(void)
{
    if (!s_ble.initialized) return ESP_ERR_INVALID_STATE;

    struct ble_gap_adv_params adv_params = {
        .conn_mode = BLE_GAP_CONN_MODE_UND,
        .disc_mode = BLE_GAP_DISC_MODE_GEN,
        .itvl_min = BLE_GAP_ADV_ITVL_MS(100),
        .itvl_max = BLE_GAP_ADV_ITVL_MS(150),
    };

    struct ble_hs_adv_fields fields = {0};
    fields.flags = BLE_HS_ADV_F_DISC_GEN | BLE_HS_ADV_F_BREDR_UNSUP;
    fields.name = (uint8_t *)s_ble.device_name;
    fields.name_len = strlen(s_ble.device_name);
    fields.name_is_complete = 1;
    fields.tx_pwr_lvl_is_present = 1;
    fields.tx_pwr_lvl = BLE_HS_ADV_TX_PWR_LVL_AUTO;

    int rc = ble_gap_adv_set_fields(&fields);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gap_adv_set_fields failed: %d", rc);
        return ESP_FAIL;
    }

    /* Include service UUID in scan response */
    struct ble_hs_adv_fields rsp_fields = {0};
    rsp_fields.uuids128 = (ble_uuid128_t[]){ svc_uuid };
    rsp_fields.num_uuids128 = 1;
    rsp_fields.uuids128_is_complete = 1;

    rc = ble_gap_adv_rsp_set_fields(&rsp_fields);
    if (rc != 0) {
        ESP_LOGW(TAG, "ble_gap_adv_rsp_set_fields failed: %d (non-fatal)", rc);
    }

    rc = ble_gap_adv_start(BLE_OWN_ADDR_PUBLIC, NULL, BLE_HS_FOREVER,
                            &adv_params, ble_gap_event, NULL);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gap_adv_start failed: %d", rc);
        return ESP_FAIL;
    }

    s_ble.state = BLE_STATE_ADVERTISING;
    ESP_LOGI(TAG, "BLE advertising started");
    return ESP_OK;
}

esp_err_t ble_manager_stop_advertising(void)
{
    if (!s_ble.initialized) return ESP_ERR_INVALID_STATE;
    ble_gap_adv_stop();
    s_ble.state = BLE_STATE_OFF;
    return ESP_OK;
}

esp_err_t ble_manager_disconnect(void)
{
    if (s_ble.state != BLE_STATE_CONNECTED) return ESP_ERR_INVALID_STATE;
    ble_gap_terminate(s_ble.conn_handle, BLE_ERR_REM_USER_CONN_TERM);
    return ESP_OK;
}

esp_err_t ble_manager_send(const uint8_t *data, size_t len)
{
    if (s_ble.state != BLE_STATE_CONNECTED) return ESP_ERR_INVALID_STATE;
    if (!data || len == 0) return ESP_ERR_INVALID_ARG;

    struct os_mbuf *om = ble_hs_mbuf_from_flat(data, len);
    if (!om) return ESP_ERR_NO_MEM;

    int rc = ble_gatts_notify_custom(s_ble.conn_handle, s_ble.tx_attr_handle, om);
    if (rc != 0) {
        ESP_LOGE(TAG, "ble_gatts_notify_custom failed: %d", rc);
        return ESP_FAIL;
    }

    return ESP_OK;
}

esp_err_t ble_manager_send_notification(const char *title, const char *body)
{
    if (!title || !body) return ESP_ERR_INVALID_ARG;

    /* Simple text protocol: "NOTIF:title\nbody" */
    char buf[256];
    int len = snprintf(buf, sizeof(buf), "NOTIF:%s\n%s", title, body);
    if (len <= 0 || (size_t)len >= sizeof(buf)) return ESP_ERR_INVALID_SIZE;

    return ble_manager_send((const uint8_t *)buf, (size_t)len);
}

esp_err_t ble_manager_register_rx_cb(ble_rx_cb_t cb, void *user_data)
{
    s_ble.rx_cb = cb;
    s_ble.rx_cb_data = user_data;
    return ESP_OK;
}

ble_state_t ble_manager_get_state(void)
{
    return s_ble.state;
}

const char *ble_manager_get_peer_name(void)
{
    if (s_ble.state != BLE_STATE_CONNECTED) return NULL;
    return "Companion";  /* TODO: read from GAP */
}
