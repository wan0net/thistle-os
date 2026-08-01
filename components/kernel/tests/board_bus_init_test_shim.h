#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef int esp_err_t;
typedef int spi_host_device_t;
typedef int i2c_port_t;
typedef int gpio_num_t;
typedef void *i2c_master_bus_handle_t;

typedef struct {
    int mosi_io_num;
    int miso_io_num;
    int sclk_io_num;
    int quadwp_io_num;
    int quadhd_io_num;
    int max_transfer_sz;
} spi_bus_config_t;

typedef struct {
    i2c_port_t i2c_port;
    gpio_num_t sda_io_num;
    gpio_num_t scl_io_num;
    int clk_source;
    int glitch_ignore_cnt;
    struct {
        bool enable_internal_pullup;
    } flags;
} i2c_master_bus_config_t;

#define ESP_OK 0
#define ESP_ERR_NO_MEM 0x101
#define ESP_ERR_INVALID_ARG 0x102
#define ESP_ERR_INVALID_STATE 0x103
#define SPI_DMA_CH_AUTO 3
#define I2C_CLK_SRC_DEFAULT 0
#define ESP_LOGE(...) ((void)0)
#define ESP_LOGW(...) ((void)0)
#define ESP_LOGI(...) ((void)0)

const char *esp_err_to_name(esp_err_t error);
esp_err_t spi_bus_initialize(spi_host_device_t host, const spi_bus_config_t *config, int dma_channel);
esp_err_t spi_bus_free(spi_host_device_t host);
esp_err_t i2c_new_master_bus(const i2c_master_bus_config_t *config,
                             i2c_master_bus_handle_t *handle);
esp_err_t i2c_del_master_bus(i2c_master_bus_handle_t handle);
esp_err_t hal_bus_register_spi(int host_id, void *bus_handle);
esp_err_t hal_bus_register_i2c(int port, void *bus_handle);
