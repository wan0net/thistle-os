#include "thistle/kernel.h"
#include "thistle/event.h"
#include "thistle/ipc.h"
#include "thistle/driver_manager.h"
#include "thistle/syscall.h"
#include "thistle/app_manager.h"
#include "thistle/elf_loader.h"
#include "thistle/ota.h"
#include "thistle/permissions.h"

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
