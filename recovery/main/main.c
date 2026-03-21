#include <stdio.h>
#include <string.h>
#include "esp_log.h"
#include "esp_ota_ops.h"
#include "esp_partition.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "recovery_wifi.h"
#include "recovery_ota.h"
#include "recovery_ui.h"

static const char *TAG = "recovery";

void app_main(void)
{
    ESP_LOGI(TAG, "ThistleOS Recovery v0.1.0");
    ESP_LOGI(TAG, "=========================");

    recovery_ui_init();
    recovery_ui_print("ThistleOS Recovery v0.1.0");
    recovery_ui_print("========================");

    /* Step 1: Check if ota_1 has valid firmware */
    const esp_partition_t *ota1 = esp_partition_find_first(
        ESP_PARTITION_TYPE_APP, ESP_PARTITION_SUBTYPE_APP_OTA_1, NULL);

    if (ota1) {
        esp_ota_img_states_t state;
        esp_err_t ret = esp_ota_get_state_partition(ota1, &state);

        if (ret == ESP_OK && state == ESP_OTA_IMG_VALID) {
            recovery_ui_print("Main OS found in ota_1 -- booting...");
            ESP_LOGI(TAG, "Setting boot partition to ota_1 and rebooting");
            esp_ota_set_boot_partition(ota1);
            vTaskDelay(pdMS_TO_TICKS(1000));
            esp_restart();
            return; /* unreachable */
        }

        if (ret == ESP_OK && state == ESP_OTA_IMG_PENDING_VERIFY) {
            recovery_ui_print("Main OS pending verification -- booting to verify...");
            esp_ota_set_boot_partition(ota1);
            vTaskDelay(pdMS_TO_TICKS(1000));
            esp_restart();
            return;
        }

        recovery_ui_print("Main OS not found or invalid in ota_1");
    } else {
        recovery_ui_print("ERROR: No ota_1 partition found!");
    }

    /* Step 2: Check SD card for firmware update */
    recovery_ui_print("");
    recovery_ui_print("Checking SD card for firmware...");

    if (recovery_ota_check_sd()) {
        recovery_ui_print("Firmware found on SD card!");
        recovery_ui_print("Installing...");

        esp_err_t ret = recovery_ota_apply_sd();
        if (ret == ESP_OK) {
            recovery_ui_print("Firmware installed! Rebooting...");
            vTaskDelay(pdMS_TO_TICKS(1000));
            esp_restart();
        } else {
            recovery_ui_print("SD firmware install FAILED");
        }
    } else {
        recovery_ui_print("No firmware on SD card");
    }

    /* Step 3: Connect to WiFi and download from app store */
    recovery_ui_print("");
    recovery_ui_print("Starting WiFi to download firmware...");

    esp_err_t wifi_ret = recovery_wifi_init();
    if (wifi_ret != ESP_OK) {
        recovery_ui_print("WiFi init failed!");
        goto wait_mode;
    }

    /* Scan and show available networks */
    recovery_ui_print("Scanning WiFi networks...");
    recovery_wifi_scan_result_t networks[10];
    int net_count = 0;
    recovery_wifi_scan(networks, 10, &net_count);

    for (int i = 0; i < net_count; i++) {
        char line[64];
        snprintf(line, sizeof(line), "  [%d] %s (%d dBm)%s",
                 i + 1, networks[i].ssid, networks[i].rssi,
                 networks[i].is_open ? " [open]" : "");
        recovery_ui_print(line);
    }

    /* Try open networks first, then prompt for password via UART */
    bool connected = false;
    for (int i = 0; i < net_count && !connected; i++) {
        if (networks[i].is_open) {
            char msg[64];
            snprintf(msg, sizeof(msg), "Connecting to %s...", networks[i].ssid);
            recovery_ui_print(msg);
            if (recovery_wifi_connect(networks[i].ssid, NULL, 10000) == ESP_OK) {
                connected = true;
                recovery_ui_print("Connected!");
            }
        }
    }

    if (!connected) {
        /* Prompt on UART for network selection */
        recovery_ui_print("");
        recovery_ui_print("No open networks. Enter WiFi credentials via serial:");
        recovery_ui_print("  Format: SSID,password");
        recovery_ui_print("  (or press Enter to retry scan)");

        char input[128];
        while (!connected) {
            if (recovery_ui_readline(input, sizeof(input), 30000)) {
                char *comma = strchr(input, ',');
                if (comma) {
                    *comma = '\0';
                    const char *ssid = input;
                    const char *pass = comma + 1;
                    char msg[64];
                    snprintf(msg, sizeof(msg), "Connecting to %s...", ssid);
                    recovery_ui_print(msg);
                    if (recovery_wifi_connect(ssid, pass, 10000) == ESP_OK) {
                        connected = true;
                        recovery_ui_print("Connected!");
                    } else {
                        recovery_ui_print("Connection failed. Try again.");
                    }
                }
            } else {
                recovery_ui_print("Timeout. Retrying scan...");
                break;
            }
        }
    }

    if (connected) {
        /* Download firmware from app store */
        recovery_ui_print("");
        recovery_ui_print("Downloading ThistleOS firmware...");

        esp_err_t dl_ret = recovery_ota_download_and_flash(
            "https://wan0net.github.io/thistle-apps/catalog.json");

        if (dl_ret == ESP_OK) {
            recovery_ui_print("Firmware installed! Rebooting...");
            vTaskDelay(pdMS_TO_TICKS(1000));
            esp_restart();
        } else {
            recovery_ui_print("Download failed!");
        }
    }

wait_mode:
    /* Step 4: Wait mode -- prompt user */
    recovery_ui_print("");
    recovery_ui_print("========================================");
    recovery_ui_print("  ThistleOS Recovery -- Waiting for input");
    recovery_ui_print("========================================");
    recovery_ui_print("Options:");
    recovery_ui_print("  1. Insert SD card with thistle_os.bin");
    recovery_ui_print("  2. Connect via serial: SSID,password");
    recovery_ui_print("  3. Type 'scan' to rescan WiFi");
    recovery_ui_print("  4. Type 'reboot' to restart");

    while (1) {
        char input[128];
        if (recovery_ui_readline(input, sizeof(input), 5000)) {
            if (strcmp(input, "reboot") == 0) {
                esp_restart();
            } else if (strcmp(input, "scan") == 0) {
                recovery_wifi_scan(networks, 10, &net_count);
                for (int i = 0; i < net_count; i++) {
                    char line[64];
                    snprintf(line, sizeof(line), "  [%d] %s (%d dBm)",
                             i + 1, networks[i].ssid, networks[i].rssi);
                    recovery_ui_print(line);
                }
            } else {
                char *comma = strchr(input, ',');
                if (comma) {
                    *comma = '\0';
                    if (recovery_wifi_connect(input, comma + 1, 10000) == ESP_OK) {
                        recovery_ui_print("Connected! Downloading...");
                        recovery_ota_download_and_flash(
                            "https://wan0net.github.io/thistle-apps/catalog.json");
                    }
                }
            }
        }

        /* Periodically check SD card */
        if (recovery_ota_check_sd()) {
            recovery_ui_print("SD card firmware detected!");
            if (recovery_ota_apply_sd() == ESP_OK) {
                recovery_ui_print("Installed! Rebooting...");
                vTaskDelay(pdMS_TO_TICKS(1000));
                esp_restart();
            }
        }

        vTaskDelay(pdMS_TO_TICKS(100));
    }
}
