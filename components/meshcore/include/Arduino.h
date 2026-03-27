// SPDX-License-Identifier: BSD-3-Clause
// Arduino.h stub for ESP-IDF builds of MeshCore.
// MeshCore includes <Arduino.h> for millis(), random(), etc.
// We provide minimal stubs backed by ESP-IDF APIs.
#pragma once

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <esp_timer.h>
#include <esp_random.h>

#ifdef __cplusplus
extern "C" {
#endif

static inline unsigned long millis(void) {
    return (unsigned long)(esp_timer_get_time() / 1000ULL);
}

static inline void delay(unsigned long ms) {
    vTaskDelay(ms / portTICK_PERIOD_MS);
}

static inline long random(long max) {
    return (long)(esp_random() % (uint32_t)max);
}

static inline long random(long min, long max) {
    if (min >= max) return min;
    return min + (long)(esp_random() % (uint32_t)(max - min));
}

// Serial stub — MeshCore uses Serial.print for debug logging
class FakeSerial {
public:
    void print(const char*) {}
    void print(int) {}
    void println(const char* s = "") { (void)s; }
    void println(int) {}
    void printf(const char*, ...) {}
};

extern FakeSerial Serial;

#ifdef __cplusplus
}
#endif
