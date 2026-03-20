// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — LilyGo T-Deck board pin definitions (LCD variant)
//
// The T-Deck uses an ESP32-S3 with ST7789 320x240 TFT LCD.
// These pin assignments are based on the LilyGo T-Deck schematic.
// Verify against the actual schematic before production use.
#pragma once

#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "driver/i2c_master.h"
#include "driver/uart.h"

/* === SPI Bus (shared: display + radio + SD) === */
#define BOARD_SPI_HOST       SPI2_HOST
#define BOARD_SPI_MOSI       GPIO_NUM_41
#define BOARD_SPI_MISO       GPIO_NUM_38
#define BOARD_SPI_SCLK       GPIO_NUM_40

/* === LCD Display (ST7789 320x240) === */
#define BOARD_LCD_CS         GPIO_NUM_12
#define BOARD_LCD_DC         GPIO_NUM_11
#define BOARD_LCD_RST        GPIO_NUM_NC    /* No dedicated reset pin */
#define BOARD_LCD_BL         GPIO_NUM_42   /* Backlight PWM */

/* === LoRa Radio (SX1262) === */
#define BOARD_LORA_CS        GPIO_NUM_9
#define BOARD_LORA_RST       GPIO_NUM_5
#define BOARD_LORA_BUSY      GPIO_NUM_36
#define BOARD_LORA_DIO1      GPIO_NUM_45

/* === SD Card === */
#define BOARD_SD_CS          GPIO_NUM_43

/* === I2C Bus === */
#define BOARD_I2C_PORT       I2C_NUM_0
#define BOARD_I2C_SDA        GPIO_NUM_18
#define BOARD_I2C_SCL        GPIO_NUM_8
#define BOARD_I2C_FREQ_HZ    400000

/* === Keyboard (TCA8418) === */
#define BOARD_KBD_ADDR       0x55
#define BOARD_KBD_INT        GPIO_NUM_46

/* === Touch (CST328) === */
#define BOARD_TOUCH_ADDR     0x5D
#define BOARD_TOUCH_INT      GPIO_NUM_16
#define BOARD_TOUCH_RST      GPIO_NUM_NC

/* === GPS (optional, external via Qwiic / UART) === */
#define BOARD_GPS_UART       UART_NUM_1
#define BOARD_GPS_TX         GPIO_NUM_17
#define BOARD_GPS_RX         GPIO_NUM_15

/* === Audio (I2S to PCM5102A) === */
#define BOARD_I2S_BCK        GPIO_NUM_7
#define BOARD_I2S_WS         GPIO_NUM_6
#define BOARD_I2S_DATA       GPIO_NUM_3

/* === Power === */
#define BOARD_BAT_ADC        GPIO_NUM_4
#define BOARD_CHARGE_STATUS  GPIO_NUM_10

/* === Trackball === */
#define BOARD_TRACKBALL_UP    GPIO_NUM_21
#define BOARD_TRACKBALL_DOWN  GPIO_NUM_46
#define BOARD_TRACKBALL_LEFT  GPIO_NUM_39
#define BOARD_TRACKBALL_RIGHT GPIO_NUM_2
#define BOARD_TRACKBALL_CLICK GPIO_NUM_3

/* === Display geometry === */
#define BOARD_DISPLAY_WIDTH  320
#define BOARD_DISPLAY_HEIGHT 240
