/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Simulator stub — driver/uart.h
 */
#pragma once
#include <stdint.h>
#include <stddef.h>
#include "esp_err.h"

#define UART_NUM_0 0
#define UART_NUM_1 1
#define UART_NUM_2 2
#define UART_DATA_8_BITS 3
#define UART_PARITY_DISABLE 0
#define UART_STOP_BITS_1 1
#define UART_HW_FLOWCTRL_DISABLE 0
#define UART_SCLK_DEFAULT 0
#define UART_PIN_NO_CHANGE (-1)

typedef struct {
    int baud_rate;
    int data_bits;
    int parity;
    int stop_bits;
    int flow_ctrl;
    int source_clk;
} uart_config_t;

/* Implemented in platform_stubs.c */
esp_err_t uart_param_config(int uart_num, const uart_config_t *cfg);
esp_err_t uart_set_pin(int uart_num, int tx, int rx, int rts, int cts);
esp_err_t uart_driver_install(int uart_num, int rx_buf_sz, int tx_buf_sz,
                              int queue_sz, void *queue, int flags);
esp_err_t uart_driver_delete(int uart_num);
int uart_read_bytes(int uart_num, void *buf, size_t len, int timeout_ms);
int uart_write_bytes(int uart_num, const void *data, size_t len);
