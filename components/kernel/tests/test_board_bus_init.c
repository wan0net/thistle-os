#include <assert.h>
#include <stdint.h>

#include "board_bus_init_test_shim.h"

esp_err_t board_bus_init_spi(int host, int mosi, int miso, int sclk, int max_transfer_bytes);
esp_err_t board_bus_init_i2c(int port, int sda, int scl, int freq_hz);

static esp_err_t spi_init_result;
static esp_err_t spi_register_result;
static esp_err_t i2c_register_result;
static esp_err_t spi_free_result;
static esp_err_t i2c_delete_result;
static int spi_free_calls;
static int i2c_delete_calls;
static int fake_i2c_handle;

const char *esp_err_to_name(esp_err_t error)
{
    (void)error;
    return "test";
}

esp_err_t spi_bus_initialize(spi_host_device_t host, const spi_bus_config_t *config, int dma_channel)
{
    (void)host;
    (void)config;
    (void)dma_channel;
    return spi_init_result;
}

esp_err_t spi_bus_free(spi_host_device_t host)
{
    (void)host;
    spi_free_calls++;
    return spi_free_result;
}

esp_err_t i2c_new_master_bus(const i2c_master_bus_config_t *config,
                             i2c_master_bus_handle_t *handle)
{
    (void)config;
    *handle = &fake_i2c_handle;
    return ESP_OK;
}

esp_err_t i2c_del_master_bus(i2c_master_bus_handle_t handle)
{
    assert(handle == &fake_i2c_handle);
    i2c_delete_calls++;
    return i2c_delete_result;
}

esp_err_t hal_bus_register_spi(int host_id, void *bus_handle)
{
    (void)host_id;
    (void)bus_handle;
    return spi_register_result;
}

esp_err_t hal_bus_register_i2c(int port, void *bus_handle)
{
    (void)port;
    (void)bus_handle;
    return i2c_register_result;
}

static void reset_mocks(void)
{
    spi_init_result = ESP_OK;
    spi_register_result = ESP_OK;
    i2c_register_result = ESP_OK;
    spi_free_result = ESP_OK;
    i2c_delete_result = ESP_OK;
    spi_free_calls = 0;
    i2c_delete_calls = 0;
}

int main(void)
{
    reset_mocks();
    spi_register_result = ESP_ERR_NO_MEM;
    assert(board_bus_init_spi(2, 1, 2, 3, 4096) == ESP_ERR_NO_MEM);
    assert(spi_free_calls == 1);

    reset_mocks();
    spi_register_result = ESP_ERR_NO_MEM;
    spi_free_result = ESP_ERR_INVALID_STATE;
    assert(board_bus_init_spi(2, 1, 2, 3, 4096) == ESP_ERR_NO_MEM);
    assert(spi_free_calls == 1);

    reset_mocks();
    spi_register_result = ESP_ERR_INVALID_ARG;
    assert(board_bus_init_spi(0, 1, 2, 3, 4096) == ESP_ERR_INVALID_ARG);
    assert(spi_free_calls == 1);

    reset_mocks();
    spi_init_result = ESP_ERR_INVALID_STATE;
    spi_register_result = ESP_ERR_NO_MEM;
    assert(board_bus_init_spi(2, 1, 2, 3, 4096) == ESP_ERR_NO_MEM);
    assert(spi_free_calls == 0);

    reset_mocks();
    i2c_register_result = ESP_ERR_NO_MEM;
    assert(board_bus_init_i2c(2, 1, 2, 400000) == ESP_ERR_NO_MEM);
    assert(i2c_delete_calls == 1);

    reset_mocks();
    i2c_register_result = ESP_ERR_NO_MEM;
    i2c_delete_result = ESP_ERR_INVALID_STATE;
    assert(board_bus_init_i2c(2, 1, 2, 400000) == ESP_ERR_NO_MEM);
    assert(i2c_delete_calls == 1);

    return 0;
}
