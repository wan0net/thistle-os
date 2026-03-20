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

/* ELF loader stub — not available in simulator */
esp_err_t elf_loader_init(void) {
    printf("I (elf_loader) ELF loader disabled in simulator\n");
    return ESP_OK;
}
