#pragma once

#include <stdint.h>
#include "esp_err.h"

typedef int gpio_num_t;

#define GPIO_NUM_NC (-1)
#define GPIO_NUM_0   0
#define GPIO_NUM_1   1
#define GPIO_NUM_2   2
#define GPIO_NUM_3   3
#define GPIO_NUM_4   4
#define GPIO_NUM_5   5
#define GPIO_NUM_6   6
#define GPIO_NUM_7   7
#define GPIO_NUM_8   8
#define GPIO_NUM_9   9
#define GPIO_NUM_10 10
#define GPIO_NUM_11 11
#define GPIO_NUM_12 12
#define GPIO_NUM_13 13
#define GPIO_NUM_14 14
#define GPIO_NUM_15 15
#define GPIO_NUM_16 16
#define GPIO_NUM_17 17
#define GPIO_NUM_18 18
#define GPIO_NUM_19 19
#define GPIO_NUM_20 20
#define GPIO_NUM_21 21
#define GPIO_NUM_22 22
#define GPIO_NUM_23 23
#define GPIO_NUM_24 24
#define GPIO_NUM_25 25
#define GPIO_NUM_26 26
#define GPIO_NUM_27 27
#define GPIO_NUM_28 28
#define GPIO_NUM_29 29
#define GPIO_NUM_30 30
#define GPIO_NUM_31 31
#define GPIO_NUM_32 32
#define GPIO_NUM_33 33
#define GPIO_NUM_34 34
#define GPIO_NUM_35 35
#define GPIO_NUM_36 36
#define GPIO_NUM_37 37
#define GPIO_NUM_38 38
#define GPIO_NUM_39 39
#define GPIO_NUM_40 40
#define GPIO_NUM_41 41
#define GPIO_NUM_42 42
#define GPIO_NUM_43 43
#define GPIO_NUM_44 44
#define GPIO_NUM_45 45
#define GPIO_NUM_46 46
#define GPIO_NUM_47 47
#define GPIO_NUM_48 48

typedef enum { GPIO_MODE_INPUT, GPIO_MODE_OUTPUT, GPIO_MODE_INPUT_OUTPUT } gpio_mode_t;
typedef enum { GPIO_PULLUP_DISABLE, GPIO_PULLUP_ENABLE } gpio_pullup_t;
typedef enum { GPIO_PULLDOWN_DISABLE, GPIO_PULLDOWN_ENABLE } gpio_pulldown_t;
typedef enum { GPIO_INTR_DISABLE, GPIO_INTR_NEGEDGE, GPIO_INTR_POSEDGE } gpio_intr_type_t;

typedef struct {
    gpio_mode_t mode;
    gpio_pullup_t pull_up_en;
    gpio_pulldown_t pull_down_en;
    gpio_intr_type_t intr_type;
    uint64_t pin_bit_mask;
} gpio_config_t;

static inline esp_err_t gpio_config(const gpio_config_t *cfg) { (void)cfg; return 0; }
static inline esp_err_t gpio_set_level(gpio_num_t pin, uint32_t level) { (void)pin; (void)level; return 0; }
static inline int gpio_get_level(gpio_num_t pin) { (void)pin; return 0; }
static inline esp_err_t gpio_install_isr_service(int flags) { (void)flags; return 0; }
static inline esp_err_t gpio_isr_handler_add(gpio_num_t pin, void(*fn)(void*), void *arg) { (void)pin; (void)fn; (void)arg; return 0; }
static inline esp_err_t gpio_isr_handler_remove(gpio_num_t pin) { (void)pin; return 0; }
