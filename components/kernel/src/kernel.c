#include "thistle/kernel.h"
#include "thistle/event.h"
#include "thistle/ipc.h"
#include "thistle/driver_manager.h"
#include "thistle/driver_loader.h"
#include "thistle/syscall.h"
#include "thistle/app_manager.h"
#include "thistle/elf_loader.h"
#include "thistle/ota.h"
#include "thistle/permissions.h"
#include "thistle/signing.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char *TAG = "kernel";

static int64_t s_boot_time_us = 0;

esp_err_t kernel_init(void)
{
    s_boot_time_us = esp_timer_get_time();
    ESP_LOGI(TAG, "ThistleOS v" THISTLE_VERSION_STRING " starting");

    esp_err_t ret;

    ESP_LOGI(TAG, "Initializing event bus");
    ret = event_bus_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "event_bus_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing IPC");
    ret = ipc_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "ipc_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing driver manager");
    ret = driver_manager_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "driver_manager_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Starting all drivers");
    ret = driver_manager_start_all();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "driver_manager_start_all failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing syscall table");
    ret = syscall_table_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "syscall_table_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Scanning for runtime drivers on SD card");
    driver_loader_init();
    int loaded_drv_count = driver_loader_scan_and_load();
    if (loaded_drv_count > 0) {
        ESP_LOGI(TAG, "Loaded %d runtime driver(s) from SD card", loaded_drv_count);
    }

    ESP_LOGI(TAG, "Initializing app manager");
    ret = app_manager_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "app_manager_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing permissions subsystem");
    ret = permissions_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "permissions_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing signing subsystem");
    static const uint8_t dev_signing_key[32] = {
        0x54, 0x48, 0x49, 0x53, 0x54, 0x4C, 0x45, 0x4F,  /* "THISTLEO" */
        0x53, 0x5F, 0x44, 0x45, 0x56, 0x5F, 0x4B, 0x45,  /* "S_DEV_KE" */
        0x59, 0x5F, 0x32, 0x30, 0x32, 0x36, 0x00, 0x00,  /* "Y_2026.." */
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    };
    ret = signing_init(dev_signing_key);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "signing_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing ELF loader");
    ret = elf_loader_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "elf_loader_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Initializing OTA subsystem");
    ret = ota_init();
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "ota_init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "Publishing SYSTEM_BOOT event");
    ret = event_publish_simple(EVENT_SYSTEM_BOOT);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to publish SYSTEM_BOOT: %s", esp_err_to_name(ret));
        /* Non-fatal — continue */
    }

    ESP_LOGI(TAG, "Kernel init complete (uptime: %" PRIu32 " ms)", kernel_uptime_ms());
    return ESP_OK;
}

void kernel_run(void)
{
    ESP_LOGI(TAG, "Entering kernel main loop");

    /* The LVGL tick and render are driven by the UI component.
     * This loop is the kernel heartbeat — it keeps the idle task
     * alive and provides a hook for future watchdog / power management. */
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(10));
    }
}

uint32_t kernel_uptime_ms(void)
{
    return (uint32_t)((esp_timer_get_time() - s_boot_time_us) / 1000);
}
