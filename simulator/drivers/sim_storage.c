#include "sim_storage.h"
#include <stdio.h>
#include <stdbool.h>

static esp_err_t sim_storage_init(const void *config)
{
    (void)config;
    return ESP_OK;
}

static void sim_storage_deinit(void)
{
}

static esp_err_t sim_storage_mount(const char *mp)
{
    (void)mp;
    printf("Simulator storage: mounted at %s (host filesystem)\n", mp ? mp : "(null)");
    return ESP_OK;
}

static esp_err_t sim_storage_unmount(void)
{
    return ESP_OK;
}

static bool sim_storage_is_mounted(void)
{
    return true;
}

static uint64_t sim_storage_total(void)
{
    return 16ULL * 1024 * 1024 * 1024;
}

static uint64_t sim_storage_free(void)
{
    return 14ULL * 1024 * 1024 * 1024;
}

static const hal_storage_driver_t sim_storage_driver = {
    .init           = sim_storage_init,
    .deinit         = sim_storage_deinit,
    .mount          = sim_storage_mount,
    .unmount        = sim_storage_unmount,
    .is_mounted     = sim_storage_is_mounted,
    .get_total_bytes = sim_storage_total,
    .get_free_bytes  = sim_storage_free,
    .type           = HAL_STORAGE_TYPE_SD,
    .name           = "Simulated SD",
};

const hal_storage_driver_t *sim_storage_get(void)
{
    return &sim_storage_driver;
}
