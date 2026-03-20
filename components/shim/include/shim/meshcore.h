#pragma once

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Initialize MeshCore shim (call from C code) */
esp_err_t meshcore_shim_init(void);

#ifdef __cplusplus
}

/* C++ MeshCore shim classes — only visible to C++ code */

#include "hal/board.h"

/**
 * ThistleMeshBoard — Routes MeshCore board calls through ThistleOS HAL.
 *
 * MeshCore expects a board class with methods like:
 *   begin(), getBatteryVoltage(), setLED(), isButtonPressed()
 */
class ThistleMeshBoard {
public:
    void begin();
    float getBatteryVoltage();
    uint8_t getBatteryPercent();
    void setLED(bool on);
    bool isButtonPressed();
    void reboot();
    const char* getDeviceName();
};

/**
 * ThistleMeshRadio — Routes MeshCore radio calls through ThistleOS LoRa HAL.
 *
 * MeshCore expects a radio class wrapping RadioLib's SX1262:
 *   begin(), startReceive(), readData(), transmit(), setFrequency(), etc.
 */
class ThistleMeshRadio {
public:
    int begin(float freq, float bw, uint8_t sf, uint8_t cr, uint8_t syncWord, int8_t power);
    int startReceive();
    int readData(uint8_t* data, size_t len);
    int transmit(const uint8_t* data, size_t len);
    int setFrequency(float freq);
    int setOutputPower(int8_t power);
    int setBandwidth(float bw);
    int setSpreadingFactor(uint8_t sf);
    float getRSSI();
    float getSNR();
    int sleep();
    int standby();
};

/**
 * ThistleMeshDisplay — Routes MeshCore display calls through ThistleOS LVGL.
 *
 * MeshCore's display interface draws text and simple graphics.
 */
class ThistleMeshDisplay {
public:
    void begin();
    void clear();
    void display();  /* flush to screen */
    void drawText(int x, int y, const char* text);
    void drawTextCentered(int y, const char* text);
    void setTextSize(uint8_t size);
    void setTextColor(bool inverted);
    void fillRect(int x, int y, int w, int h, bool color);
    void drawLine(int x1, int y1, int x2, int y2);
    int getWidth();
    int getHeight();
};

#endif /* __cplusplus */
