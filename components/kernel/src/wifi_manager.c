#include "thistle/wifi_manager.h"
#include "thistle/event.h"
#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "esp_netif_sntp.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/event_groups.h"
#include <string.h>
#include <time.h>
#include <sys/time.h>
#include <stdlib.h>

static const char *TAG = "wifi_mgr";

#define WIFI_CONNECTED_BIT BIT0
#define WIFI_FAIL_BIT      BIT1

static EventGroupHandle_t s_wifi_event_group = NULL;
static wifi_state_t s_state = WIFI_STATE_DISCONNECTED;
static esp_netif_t *s_netif = NULL;
static char s_ip_str[16] = {0};
static bool s_initialized = false;

/* Event handler */
static void wifi_event_handler(void *arg, esp_event_base_t event_base,
                                int32_t event_id, void *event_data)
{
    if (event_base == WIFI_EVENT) {
        if (event_id == WIFI_EVENT_STA_START) {
            esp_wifi_connect();
            s_state = WIFI_STATE_CONNECTING;
        } else if (event_id == WIFI_EVENT_STA_DISCONNECTED) {
            s_state = WIFI_STATE_DISCONNECTED;
            if (s_wifi_event_group) {
                xEventGroupSetBits(s_wifi_event_group, WIFI_FAIL_BIT);
            }
            event_publish_simple(EVENT_WIFI_DISCONNECTED);
            ESP_LOGI(TAG, "Disconnected from WiFi");
        }
    } else if (event_base == IP_EVENT && event_id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *event = (ip_event_got_ip_t *)event_data;
        snprintf(s_ip_str, sizeof(s_ip_str), IPSTR, IP2STR(&event->ip_info.ip));
        s_state = WIFI_STATE_CONNECTED;
        if (s_wifi_event_group) {
            xEventGroupSetBits(s_wifi_event_group, WIFI_CONNECTED_BIT);
        }
        event_publish_simple(EVENT_WIFI_CONNECTED);
        ESP_LOGI(TAG, "Connected, IP: %s", s_ip_str);
    }
}

esp_err_t wifi_manager_init(void)
{
    if (s_initialized) return ESP_OK;

    /* Initialize TCP/IP stack and default event loop */
    esp_err_t ret = esp_netif_init();
    if (ret != ESP_OK) { ESP_LOGE(TAG, "netif init: %s", esp_err_to_name(ret)); return ret; }
    ret = esp_event_loop_create_default();
    if (ret != ESP_OK) { ESP_LOGE(TAG, "event loop: %s", esp_err_to_name(ret)); return ret; }
    s_netif = esp_netif_create_default_wifi_sta();

    /* Initialize WiFi with default config */
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ret = esp_wifi_init(&cfg);
    if (ret != ESP_OK) { ESP_LOGE(TAG, "wifi init: %s", esp_err_to_name(ret)); return ret; }

    /* Register event handlers */
    esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, &wifi_event_handler, NULL);
    esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, &wifi_event_handler, NULL);

    /* Set WiFi mode to station */
    esp_wifi_set_mode(WIFI_MODE_STA);
    esp_wifi_start();

    s_wifi_event_group = xEventGroupCreate();
    s_initialized = true;
    ESP_LOGI(TAG, "WiFi manager initialized");
    return ESP_OK;
}

esp_err_t wifi_manager_scan(wifi_scan_result_t *results, uint8_t max_results, uint8_t *out_count)
{
    if (!s_initialized || !results || !out_count) return ESP_ERR_INVALID_ARG;

    wifi_scan_config_t scan_config = {
        .show_hidden = false,
        .scan_type = WIFI_SCAN_TYPE_ACTIVE,
        .scan_time.active.min = 100,
        .scan_time.active.max = 300,
    };

    ESP_LOGI(TAG, "Starting WiFi scan...");
    esp_err_t ret = esp_wifi_scan_start(&scan_config, true); /* blocking */
    if (ret != ESP_OK) return ret;

    uint16_t ap_count = 0;
    esp_wifi_scan_get_ap_num(&ap_count);

    uint16_t fetch = ap_count < max_results ? ap_count : max_results;
    wifi_ap_record_t *ap_records = calloc(fetch, sizeof(wifi_ap_record_t));
    if (!ap_records) return ESP_ERR_NO_MEM;

    esp_wifi_scan_get_ap_records(&fetch, ap_records);

    for (uint16_t i = 0; i < fetch; i++) {
        memset(&results[i], 0, sizeof(wifi_scan_result_t));
        strncpy(results[i].ssid, (const char *)ap_records[i].ssid, WIFI_SSID_MAX_LEN);
        results[i].ssid[WIFI_SSID_MAX_LEN] = '\0';
        results[i].rssi = ap_records[i].rssi;
        results[i].channel = ap_records[i].primary;
        results[i].is_open = (ap_records[i].authmode == WIFI_AUTH_OPEN);
    }

    *out_count = (uint8_t)fetch;
    free(ap_records);

    ESP_LOGI(TAG, "Scan complete: %d networks found", (int)fetch);
    return ESP_OK;
}

esp_err_t wifi_manager_connect(const char *ssid, const char *password, uint32_t timeout_ms)
{
    if (!s_initialized || !ssid) return ESP_ERR_INVALID_ARG;
    if (timeout_ms == 0) timeout_ms = 10000;

    wifi_config_t wifi_config = {0};
    strncpy((char *)wifi_config.sta.ssid, ssid, sizeof(wifi_config.sta.ssid) - 1);
    if (password) {
        strncpy((char *)wifi_config.sta.password, password, sizeof(wifi_config.sta.password) - 1);
    }

    /* Clear previous event bits */
    xEventGroupClearBits(s_wifi_event_group, WIFI_CONNECTED_BIT | WIFI_FAIL_BIT);

    esp_wifi_set_config(WIFI_IF_STA, &wifi_config);
    s_state = WIFI_STATE_CONNECTING;
    esp_wifi_connect();

    ESP_LOGI(TAG, "Connecting to '%s'...", ssid);

    /* Wait for connection or failure */
    EventBits_t bits = xEventGroupWaitBits(s_wifi_event_group,
                                            WIFI_CONNECTED_BIT | WIFI_FAIL_BIT,
                                            pdTRUE, pdFALSE,
                                            pdMS_TO_TICKS(timeout_ms));

    if (bits & WIFI_CONNECTED_BIT) {
        return ESP_OK;
    } else {
        s_state = WIFI_STATE_FAILED;
        return ESP_ERR_TIMEOUT;
    }
}

esp_err_t wifi_manager_disconnect(void)
{
    if (!s_initialized) return ESP_ERR_INVALID_STATE;
    esp_wifi_disconnect();
    s_state = WIFI_STATE_DISCONNECTED;
    return ESP_OK;
}

wifi_state_t wifi_manager_get_state(void)
{
    return s_state;
}

int8_t wifi_manager_get_rssi(void)
{
    if (s_state != WIFI_STATE_CONNECTED) return 0;
    wifi_ap_record_t ap;
    if (esp_wifi_sta_get_ap_info(&ap) == ESP_OK) {
        return ap.rssi;
    }
    return 0;
}

const char *wifi_manager_get_ip(void)
{
    if (s_state != WIFI_STATE_CONNECTED) return NULL;
    return s_ip_str;
}

esp_err_t wifi_manager_ntp_sync(void)
{
    if (s_state != WIFI_STATE_CONNECTED) {
        ESP_LOGW(TAG, "Cannot sync NTP: not connected to WiFi");
        return ESP_ERR_INVALID_STATE;
    }

    ESP_LOGI(TAG, "Starting NTP sync...");

    esp_sntp_config_t config = ESP_NETIF_SNTP_DEFAULT_CONFIG("pool.ntp.org");
    esp_netif_sntp_init(&config);

    /* Wait for time sync (up to 15 seconds) */
    int retry = 0;
    while (esp_netif_sntp_sync_wait(pdMS_TO_TICKS(1000)) != ESP_OK && retry < 15) {
        retry++;
        ESP_LOGD(TAG, "Waiting for NTP sync... (%d/15)", retry);
    }

    esp_netif_sntp_deinit();

    if (retry >= 15) {
        ESP_LOGW(TAG, "NTP sync timed out");
        return ESP_ERR_TIMEOUT;
    }

    /* Log the synced time */
    time_t now;
    struct tm timeinfo;
    time(&now);
    localtime_r(&now, &timeinfo);
    ESP_LOGI(TAG, "NTP synced: %04d-%02d-%02d %02d:%02d:%02d",
             timeinfo.tm_year + 1900, timeinfo.tm_mon + 1, timeinfo.tm_mday,
             timeinfo.tm_hour, timeinfo.tm_min, timeinfo.tm_sec);

    return ESP_OK;
}

void wifi_manager_get_time_str(char *buf, size_t buf_len)
{
    time_t now;
    struct tm timeinfo;
    time(&now);
    localtime_r(&now, &timeinfo);

    if (timeinfo.tm_year < (2024 - 1900)) {
        /* Time not set yet */
        strncpy(buf, "--:--", buf_len);
    } else {
        snprintf(buf, buf_len, "%02d:%02d", timeinfo.tm_hour, timeinfo.tm_min);
    }
}

void wifi_manager_get_date_str(char *buf, size_t buf_len)
{
    time_t now;
    struct tm timeinfo;
    time(&now);
    localtime_r(&now, &timeinfo);

    if (timeinfo.tm_year < (2024 - 1900)) {
        strncpy(buf, "----/--/--", buf_len);
    } else {
        snprintf(buf, buf_len, "%04d-%02d-%02d",
                 timeinfo.tm_year + 1900, timeinfo.tm_mon + 1, timeinfo.tm_mday);
    }
}
