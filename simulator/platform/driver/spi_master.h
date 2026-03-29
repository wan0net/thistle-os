#pragma once

#include <stdint.h>
#include <stddef.h>
#include "esp_err.h"

typedef int spi_host_device_t;
#define SPI2_HOST 1
#define SPI3_HOST 2
#define SPI_DMA_CH_AUTO 3

typedef struct {} spi_bus_config_t;
typedef void *spi_device_handle_t;
typedef struct {
    int clock_speed_hz;
    int mode;
    int spics_io_num;
    int queue_size;
} spi_device_interface_config_t;

typedef struct {
    size_t length;
    const void *tx_buffer;
    void *rx_buffer;
    size_t rxlength;
} spi_transaction_t;

/* Implemented in sim_spi_bus.c */
esp_err_t spi_bus_initialize(spi_host_device_t host, const spi_bus_config_t *cfg, int dma);
esp_err_t spi_bus_add_device(spi_host_device_t host, const spi_device_interface_config_t *cfg, spi_device_handle_t *handle);
esp_err_t spi_bus_remove_device(spi_device_handle_t handle);
esp_err_t spi_device_polling_transmit(spi_device_handle_t handle, spi_transaction_t *t);
