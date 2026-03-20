#pragma once

#include <stdint.h>
#include <stddef.h>
#include "esp_err.h"

typedef void *i2c_master_bus_handle_t;
typedef void *i2c_master_dev_handle_t;

#define I2C_NUM_0 0
#define I2C_ADDR_BIT_LEN_7 0
#define I2C_CLK_SRC_DEFAULT 0

typedef struct {
    int i2c_port;
    int sda_io_num;
    int scl_io_num;
    int clk_source;
    int glitch_ignore_cnt;
    struct { int enable_internal_pullup; } flags;
} i2c_master_bus_config_t;

typedef struct {
    int dev_addr_length;
    int device_address;
    int scl_speed_hz;
} i2c_device_config_t;

static inline esp_err_t i2c_new_master_bus(const i2c_master_bus_config_t *cfg, i2c_master_bus_handle_t *handle) {
    (void)cfg; *handle = (void*)1; return 0;
}
static inline esp_err_t i2c_master_bus_add_device(i2c_master_bus_handle_t bus, const i2c_device_config_t *cfg, i2c_master_dev_handle_t *dev) {
    (void)bus; (void)cfg; *dev = (void*)1; return 0;
}
static inline esp_err_t i2c_master_bus_rm_device(i2c_master_dev_handle_t dev) { (void)dev; return 0; }
static inline esp_err_t i2c_master_transmit(i2c_master_dev_handle_t dev, const uint8_t *data, size_t len, int timeout) {
    (void)dev; (void)data; (void)len; (void)timeout; return 0;
}
static inline esp_err_t i2c_master_transmit_receive(i2c_master_dev_handle_t dev, const uint8_t *tx, size_t tx_len, uint8_t *rx, size_t rx_len, int timeout) {
    (void)dev; (void)tx; (void)tx_len; (void)rx; (void)rx_len; (void)timeout; return 0;
}
