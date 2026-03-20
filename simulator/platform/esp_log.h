#pragma once

#include <stdio.h>

typedef enum {
    ESP_LOG_NONE,
    ESP_LOG_ERROR,
    ESP_LOG_WARN,
    ESP_LOG_INFO,
    ESP_LOG_DEBUG,
    ESP_LOG_VERBOSE
} esp_log_level_t;

#define ESP_LOGE(tag, fmt, ...) printf("\033[31mE (%s) " fmt "\033[0m\n", tag, ##__VA_ARGS__)
#define ESP_LOGW(tag, fmt, ...) printf("\033[33mW (%s) " fmt "\033[0m\n", tag, ##__VA_ARGS__)
#define ESP_LOGI(tag, fmt, ...) printf("\033[32mI (%s) " fmt "\033[0m\n", tag, ##__VA_ARGS__)
#define ESP_LOGD(tag, fmt, ...) /* disabled in sim */
#define ESP_LOGV(tag, fmt, ...) /* disabled in sim */

#define ESP_LOG_LEVEL(level, tag, fmt, ...) ESP_LOGI(tag, fmt, ##__VA_ARGS__)
