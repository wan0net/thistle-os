#include "esp_timer.h"
#include "esp_err.h"
#include <stdlib.h>
#include <stdio.h>

/* esp_timer stubs — LVGL tick is driven by main loop */
struct esp_timer { int dummy; };

esp_err_t esp_timer_create(const esp_timer_create_args_t *args, esp_timer_handle_t *handle) {
    (void)args;
    *handle = (esp_timer_handle_t)calloc(1, sizeof(struct esp_timer));
    return ESP_OK;
}

esp_err_t esp_timer_start_periodic(esp_timer_handle_t handle, uint64_t period_us) {
    (void)handle; (void)period_us;
    return ESP_OK;
}

esp_err_t esp_timer_start_once(esp_timer_handle_t handle, uint64_t timeout_us) {
    (void)handle; (void)timeout_us;
    return ESP_OK;
}

esp_err_t esp_timer_delete(esp_timer_handle_t handle) {
    free(handle);
    return ESP_OK;
}

esp_err_t esp_timer_stop(esp_timer_handle_t handle) {
    (void)handle;
    return ESP_OK;
}

/* Stubs for subsystems not available in simulator */
esp_err_t ota_init(void) { return ESP_OK; }
esp_err_t permissions_init(void) { return ESP_OK; }
esp_err_t wifi_manager_init(void) { return ESP_OK; }

/* wifi_manager stubs for statusbar/launcher */
int wifi_manager_get_state(void) { return 0; }
int wifi_manager_get_rssi(void) { return 0; }
void wifi_manager_get_time_str(char *buf, unsigned long buf_len) {
    /* Use real system time */
    #include <time.h>
    time_t now; struct tm tm;
    time(&now); localtime_r(&now, &tm);
    snprintf(buf, buf_len, "%02d:%02d", tm.tm_hour, tm.tm_min);
}

/* ELF loader stub */
esp_err_t elf_loader_init(void) {
    printf("I (elf_loader) ELF loader disabled in simulator\n");
    return ESP_OK;
}
