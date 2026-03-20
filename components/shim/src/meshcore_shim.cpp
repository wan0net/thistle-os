// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS MeshCore Shim — routes MeshCore hardware calls through ThistleOS HAL

#include "shim/meshcore.h"
#include "hal/board.h"
#include "esp_log.h"
#include "esp_system.h"
#include <cstring>

static const char *TAG = "meshcore_shim";

extern "C" esp_err_t meshcore_shim_init(void)
{
    ESP_LOGI(TAG, "MeshCore shim initialized");
    return ESP_OK;
}

/* =========================================================================
 * ThistleMeshBoard
 * ========================================================================= */

void ThistleMeshBoard::begin()
{
    ESP_LOGI(TAG, "Board::begin()");
    /* Board is already initialized by ThistleOS kernel */
}

float ThistleMeshBoard::getBatteryVoltage()
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->power && reg->power->get_battery_mv) {
        return reg->power->get_battery_mv() / 1000.0f;
    }
    return 0.0f;
}

uint8_t ThistleMeshBoard::getBatteryPercent()
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->power && reg->power->get_battery_percent) {
        return reg->power->get_battery_percent();
    }
    return 0;
}

void ThistleMeshBoard::setLED(bool on)
{
    /* T-Deck Pro doesn't have a user LED exposed via HAL — no-op */
    (void)on;
}

bool ThistleMeshBoard::isButtonPressed()
{
    /* Could poll a specific key from the keyboard HAL — stub for now */
    return false;
}

void ThistleMeshBoard::reboot()
{
    esp_restart();
}

const char* ThistleMeshBoard::getDeviceName()
{
    const hal_registry_t *reg = hal_get_registry();
    return reg ? reg->board_name : "ThistleOS";
}

/* =========================================================================
 * ThistleMeshRadio
 * ========================================================================= */

int ThistleMeshRadio::begin(float freq, float bw, uint8_t sf, uint8_t cr, uint8_t syncWord, int8_t power)
{
    ESP_LOGI(TAG, "Radio::begin(%.1f MHz, BW=%.0f, SF=%d, CR=%d, pwr=%d)",
             freq, bw, sf, cr, power);

    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio) {
        ESP_LOGE(TAG, "No radio driver registered");
        return -1;
    }

    const hal_radio_driver_t *radio = reg->radio;

    if (radio->set_frequency) {
        radio->set_frequency((uint32_t)(freq * 1000000.0f));
    }
    if (radio->set_bandwidth) {
        radio->set_bandwidth((uint32_t)(bw * 1000.0f));
    }
    if (radio->set_spreading_factor) {
        radio->set_spreading_factor(sf);
    }
    if (radio->set_tx_power) {
        radio->set_tx_power(power);
    }

    return 0;
}

/* RX callback bridge — stores received data for readData() */
static uint8_t s_rx_buf[256];
static size_t s_rx_len = 0;
static int s_rx_rssi = 0;
static volatile bool s_rx_ready = false;

static void meshcore_rx_callback(const uint8_t *data, size_t len, int rssi, void *user_data)
{
    (void)user_data;
    size_t copy_len = len < sizeof(s_rx_buf) ? len : sizeof(s_rx_buf);
    memcpy(s_rx_buf, data, copy_len);
    s_rx_len = copy_len;
    s_rx_rssi = rssi;
    s_rx_ready = true;
}

int ThistleMeshRadio::startReceive()
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->start_receive) return -1;

    s_rx_ready = false;
    return reg->radio->start_receive(meshcore_rx_callback, NULL) == ESP_OK ? 0 : -1;
}

int ThistleMeshRadio::readData(uint8_t* data, size_t len)
{
    if (!s_rx_ready) return 0;

    size_t copy_len = len < s_rx_len ? len : s_rx_len;
    memcpy(data, s_rx_buf, copy_len);
    s_rx_ready = false;
    return (int)copy_len;
}

int ThistleMeshRadio::transmit(const uint8_t* data, size_t len)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->send) return -1;

    return reg->radio->send(data, len) == ESP_OK ? 0 : -1;
}

int ThistleMeshRadio::setFrequency(float freq)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->set_frequency) return -1;
    return reg->radio->set_frequency((uint32_t)(freq * 1000000.0f)) == ESP_OK ? 0 : -1;
}

int ThistleMeshRadio::setOutputPower(int8_t power)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->set_tx_power) return -1;
    return reg->radio->set_tx_power(power) == ESP_OK ? 0 : -1;
}

int ThistleMeshRadio::setBandwidth(float bw)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->set_bandwidth) return -1;
    return reg->radio->set_bandwidth((uint32_t)(bw * 1000.0f)) == ESP_OK ? 0 : -1;
}

int ThistleMeshRadio::setSpreadingFactor(uint8_t sf)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->set_spreading_factor) return -1;
    return reg->radio->set_spreading_factor(sf) == ESP_OK ? 0 : -1;
}

float ThistleMeshRadio::getRSSI()
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->get_rssi) return -999.0f;
    return (float)reg->radio->get_rssi();
}

float ThistleMeshRadio::getSNR()
{
    /* SNR not available via our HAL — return a default */
    return 0.0f;
}

int ThistleMeshRadio::sleep()
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->sleep) return -1;
    return reg->radio->sleep(true) == ESP_OK ? 0 : -1;
}

int ThistleMeshRadio::standby()
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->radio || !reg->radio->sleep) return -1;
    return reg->radio->sleep(false) == ESP_OK ? 0 : -1;
}

/* =========================================================================
 * ThistleMeshDisplay
 * ========================================================================= */

/* MeshCore display routes to LVGL via the app's display area.
 * For now, these are stubs that log — full LVGL integration
 * comes when MeshCore is actually compiled as an app. */

void ThistleMeshDisplay::begin()
{
    ESP_LOGI(TAG, "Display::begin()");
}

void ThistleMeshDisplay::clear()
{
    ESP_LOGD(TAG, "Display::clear()");
    /* TODO: clear LVGL app area */
}

void ThistleMeshDisplay::display()
{
    ESP_LOGD(TAG, "Display::display()");
    /* TODO: trigger LVGL refresh */
}

void ThistleMeshDisplay::drawText(int x, int y, const char* text)
{
    ESP_LOGD(TAG, "Display::drawText(%d,%d,'%s')", x, y, text);
    /* TODO: create/update LVGL label at position */
}

void ThistleMeshDisplay::drawTextCentered(int y, const char* text)
{
    ESP_LOGD(TAG, "Display::drawTextCentered(%d,'%s')", y, text);
}

void ThistleMeshDisplay::setTextSize(uint8_t size)
{
    (void)size;
}

void ThistleMeshDisplay::setTextColor(bool inverted)
{
    (void)inverted;
}

void ThistleMeshDisplay::fillRect(int x, int y, int w, int h, bool color)
{
    (void)x; (void)y; (void)w; (void)h; (void)color;
}

void ThistleMeshDisplay::drawLine(int x1, int y1, int x2, int y2)
{
    (void)x1; (void)y1; (void)x2; (void)y2;
}

int ThistleMeshDisplay::getWidth()
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->display) return reg->display->width;
    return 320;
}

int ThistleMeshDisplay::getHeight()
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->display) return reg->display->height;
    return 240;
}
