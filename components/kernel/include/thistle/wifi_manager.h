#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

#define WIFI_SSID_MAX_LEN    32
#define WIFI_PASS_MAX_LEN    64
#define WIFI_SCAN_MAX_RESULTS 20

typedef struct {
    char ssid[WIFI_SSID_MAX_LEN + 1];
    int8_t rssi;
    uint8_t channel;
    bool is_open;          /* true if no encryption */
} wifi_scan_result_t;

typedef enum {
    WIFI_STATE_DISCONNECTED,
    WIFI_STATE_CONNECTING,
    WIFI_STATE_CONNECTED,
    WIFI_STATE_FAILED,
} wifi_state_t;

/* Initialize WiFi subsystem (must call before other wifi_ functions) */
esp_err_t wifi_manager_init(void);

/* Scan for available networks. Results written to results[], count to out_count. */
esp_err_t wifi_manager_scan(wifi_scan_result_t *results, uint8_t max_results, uint8_t *out_count);

/* Connect to a WiFi network. Blocks up to timeout_ms (0 = 10s default). */
esp_err_t wifi_manager_connect(const char *ssid, const char *password, uint32_t timeout_ms);

/* Disconnect from current network */
esp_err_t wifi_manager_disconnect(void);

/* Get current WiFi state */
wifi_state_t wifi_manager_get_state(void);

/* Get current RSSI (returns 0 if not connected) */
int8_t wifi_manager_get_rssi(void);

/* Get current IP address as string (returns NULL if not connected) */
const char *wifi_manager_get_ip(void);

/* Save WiFi credentials to system.json for auto-connect on boot */
esp_err_t wifi_manager_save_credentials(const char *ssid, const char *password);

/* Load saved WiFi credentials from system.json and attempt to connect */
esp_err_t wifi_manager_auto_connect(void);

/* Sync time via NTP (call after WiFi connected). Updates system time. */
esp_err_t wifi_manager_ntp_sync(void);

/* Get current time as formatted string "HH:MM" */
void wifi_manager_get_time_str(char *buf, size_t buf_len);

/* Get current date as formatted string "YYYY-MM-DD" */
void wifi_manager_get_date_str(char *buf, size_t buf_len);
