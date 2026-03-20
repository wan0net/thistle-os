/*
 * Simulator WiFi — fake scan results, fake connection, real time.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "thistle/wifi_manager.h"
#include "esp_err.h"
#include <string.h>
#include <stdio.h>
#include <time.h>
#include <unistd.h>

static wifi_state_t s_state = WIFI_STATE_DISCONNECTED;
static char s_connected_ssid[WIFI_SSID_MAX_LEN + 1] = {0};

/* Fake network list */
static const wifi_scan_result_t s_fake_networks[] = {
    { .ssid = "HomeNetwork_5G",    .rssi = -42, .channel = 36, .is_open = false },
    { .ssid = "CoffeeShop_Free",   .rssi = -58, .channel = 6,  .is_open = true },
    { .ssid = "OfficeWiFi",        .rssi = -65, .channel = 11, .is_open = false },
    { .ssid = "Neighbor_2.4G",     .rssi = -73, .channel = 1,  .is_open = false },
    { .ssid = "IoT_Network",       .rssi = -80, .channel = 3,  .is_open = false },
    { .ssid = "ThistleOS_Test",    .rssi = -35, .channel = 6,  .is_open = true },
};
#define FAKE_NETWORK_COUNT (sizeof(s_fake_networks) / sizeof(s_fake_networks[0]))

esp_err_t wifi_manager_init(void)
{
    printf("[sim_wifi] WiFi manager initialized (simulator)\n");
    return ESP_OK;
}

esp_err_t wifi_manager_scan(wifi_scan_result_t *results, uint8_t max_results, uint8_t *out_count)
{
    if (!results || !out_count) return ESP_ERR_INVALID_ARG;

    /* Simulate a brief scan delay */
    usleep(200000); /* 200ms */

    uint8_t count = max_results < FAKE_NETWORK_COUNT ? max_results : FAKE_NETWORK_COUNT;
    memcpy(results, s_fake_networks, count * sizeof(wifi_scan_result_t));
    *out_count = count;

    printf("[sim_wifi] Scan complete: %d networks found\n", count);
    return ESP_OK;
}

esp_err_t wifi_manager_connect(const char *ssid, const char *password, uint32_t timeout_ms)
{
    (void)timeout_ms;
    if (!ssid) return ESP_ERR_INVALID_ARG;

    printf("[sim_wifi] Connecting to '%s'...\n", ssid);
    s_state = WIFI_STATE_CONNECTING;

    /* Simulate connection delay */
    usleep(500000); /* 500ms */

    /* Check if it's in our fake network list */
    bool found = false;
    for (int i = 0; i < (int)FAKE_NETWORK_COUNT; i++) {
        if (strcmp(ssid, s_fake_networks[i].ssid) == 0) {
            /* If secured and no password, fail */
            if (!s_fake_networks[i].is_open && (!password || password[0] == '\0')) {
                printf("[sim_wifi] Connection failed: password required\n");
                s_state = WIFI_STATE_FAILED;
                return ESP_FAIL;
            }
            found = true;
            break;
        }
    }

    if (!found) {
        printf("[sim_wifi] Network '%s' not found\n", ssid);
        s_state = WIFI_STATE_FAILED;
        return ESP_FAIL;
    }

    strncpy(s_connected_ssid, ssid, WIFI_SSID_MAX_LEN);
    s_state = WIFI_STATE_CONNECTED;
    printf("[sim_wifi] Connected to '%s' (IP: 192.168.1.42)\n", ssid);
    return ESP_OK;
}

esp_err_t wifi_manager_disconnect(void)
{
    s_state = WIFI_STATE_DISCONNECTED;
    s_connected_ssid[0] = '\0';
    printf("[sim_wifi] Disconnected\n");
    return ESP_OK;
}

wifi_state_t wifi_manager_get_state(void)
{
    return s_state;
}

int8_t wifi_manager_get_rssi(void)
{
    if (s_state != WIFI_STATE_CONNECTED) return 0;
    /* Find the connected network's RSSI */
    for (int i = 0; i < (int)FAKE_NETWORK_COUNT; i++) {
        if (strcmp(s_connected_ssid, s_fake_networks[i].ssid) == 0) {
            return s_fake_networks[i].rssi;
        }
    }
    return -55;
}

const char *wifi_manager_get_ip(void)
{
    if (s_state != WIFI_STATE_CONNECTED) return NULL;
    return "192.168.1.42";
}

esp_err_t wifi_manager_ntp_sync(void)
{
    /* Host already has correct time — just return success */
    printf("[sim_wifi] NTP sync (using host time)\n");
    return ESP_OK;
}

void wifi_manager_get_time_str(char *buf, size_t buf_len)
{
    time_t now;
    struct tm tm_info;
    time(&now);
    localtime_r(&now, &tm_info);
    snprintf(buf, buf_len, "%02d:%02d", tm_info.tm_hour, tm_info.tm_min);
}

void wifi_manager_get_date_str(char *buf, size_t buf_len)
{
    time_t now;
    struct tm tm_info;
    time(&now);
    localtime_r(&now, &tm_info);
    snprintf(buf, buf_len, "%04d-%02d-%02d",
             tm_info.tm_year + 1900, tm_info.tm_mon + 1, tm_info.tm_mday);
}
