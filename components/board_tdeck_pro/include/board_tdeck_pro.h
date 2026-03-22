#pragma once

#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "driver/i2c_master.h"

// Pin map from: github.com/Xinyuan-LilyGO/T-Deck-Pro/examples/factory/utilities.h

/* === SPI Bus (shared: e-paper + LoRa + SD) === */
/* All three peripherals share ONE SPI bus. E-paper is write-only (no MISO).
 * Confirmed from LilyGO factory firmware: SPI.begin(36, 47, 33) */
#define BOARD_SPI_HOST       SPI2_HOST
#define BOARD_SPI_MOSI       GPIO_NUM_33
#define BOARD_SPI_MISO       GPIO_NUM_47
#define BOARD_SPI_SCLK       GPIO_NUM_36

/* === E-Paper Display (GDEQ031T10) === */
#define BOARD_EPAPER_CS      GPIO_NUM_34
#define BOARD_EPAPER_DC      GPIO_NUM_35
#define BOARD_EPAPER_RST     (-1)        /* Not connected */
#define BOARD_EPAPER_BUSY    GPIO_NUM_37

/* === LoRa Radio (SX1262) === */
#define BOARD_LORA_CS        GPIO_NUM_3
#define BOARD_LORA_RST       GPIO_NUM_4
#define BOARD_LORA_BUSY      GPIO_NUM_6
#define BOARD_LORA_DIO1      GPIO_NUM_5

/* === SD Card === */
#define BOARD_SD_CS          GPIO_NUM_48

/* === I2C Bus === */
#define BOARD_I2C_PORT       I2C_NUM_0
#define BOARD_I2C_SDA        GPIO_NUM_13
#define BOARD_I2C_SCL        GPIO_NUM_14
#define BOARD_I2C_FREQ_HZ    400000

/* === Keyboard (TCA8418) === */
#define BOARD_KBD_ADDR       0x34
#define BOARD_KBD_INT        GPIO_NUM_15
#define BOARD_KBD_LED        GPIO_NUM_42

/* === Touch (CST328) === */
#define BOARD_TOUCH_ADDR     0x1A
#define BOARD_TOUCH_INT      GPIO_NUM_12
#define BOARD_TOUCH_RST      GPIO_NUM_45

/* === GPS === */
#define BOARD_GPS_UART       UART_NUM_2
#define BOARD_GPS_TX         GPIO_NUM_43
#define BOARD_GPS_RX         GPIO_NUM_44
#define BOARD_GPS_PPS        GPIO_NUM_1

/* === Audio (I2S) === */
#define BOARD_I2S_BCK        GPIO_NUM_7
#define BOARD_I2S_WS         GPIO_NUM_9
#define BOARD_I2S_DATA       GPIO_NUM_8

/* === IMU (BHI260AP) === */
#define BOARD_IMU_ADDR       0x28
#define BOARD_IMU_INT        GPIO_NUM_21

/* === Light Sensor (LTR-553ALS) === */
#define BOARD_LIGHT_ADDR     0x23
#define BOARD_LIGHT_INT      GPIO_NUM_16

/* === Power === */
#define BOARD_BAT_ADC        GPIO_NUM_4   // shares with LORA_RST? check
#define BOARD_CHARGE_STATUS  GPIO_NUM_10

/* === Motor === */
#define BOARD_MOTOR          GPIO_NUM_2

/* === Power enables === */
#define BOARD_1V8_EN         GPIO_NUM_38
#define BOARD_GPS_EN         GPIO_NUM_39
#define BOARD_MODEM_EN       GPIO_NUM_41
#define BOARD_LORA_EN        GPIO_NUM_46

/* === Display === */
#define BOARD_DISPLAY_WIDTH  320
#define BOARD_DISPLAY_HEIGHT 240
