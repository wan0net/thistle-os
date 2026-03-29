/*
 * Virtual SPI bus — replaces ESP-IDF spi_* inline stubs.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "sim_spi_bus.h"
#include "driver/spi_master.h"
#include <string.h>
#include <stdio.h>

typedef struct {
    int host_id;
} sim_spi_bus_t;

static sim_spi_bus_t s_buses[SIM_SPI_MAX_BUSES];
static bool s_initialized = false;

void sim_spi_bus_init(void)
{
    memset(s_buses, 0, sizeof(s_buses));
    s_initialized = true;
}

void *sim_spi_bus_get(int index)
{
    if (index < 0 || index >= SIM_SPI_MAX_BUSES) return NULL;
    return &s_buses[index];
}

/* --- ESP-IDF SPI API implementations --- */

esp_err_t spi_bus_initialize(spi_host_device_t host, const spi_bus_config_t *cfg, int dma)
{
    (void)host; (void)cfg; (void)dma;
    return ESP_OK;
}

esp_err_t spi_bus_add_device(spi_host_device_t host,
                             const spi_device_interface_config_t *cfg,
                             spi_device_handle_t *handle)
{
    (void)host; (void)cfg;
    if (handle) *handle = (void *)(uintptr_t)1;
    return ESP_OK;
}

esp_err_t spi_bus_remove_device(spi_device_handle_t handle)
{
    (void)handle;
    return ESP_OK;
}

esp_err_t spi_device_polling_transmit(spi_device_handle_t handle, spi_transaction_t *t)
{
    (void)handle; (void)t;
    return ESP_OK;
}
