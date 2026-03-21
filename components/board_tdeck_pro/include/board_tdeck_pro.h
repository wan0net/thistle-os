#pragma once

#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "driver/i2c_master.h"

/* === SPI Bus (shared: display + radio + SD) === */
#define BOARD_SPI_HOST       SPI2_HOST
#define BOARD_SPI_MOSI       GPIO_NUM_33
#define BOARD_SPI_MISO       GPIO_NUM_47
#define BOARD_SPI_SCLK       GPIO_NUM_36

/* === E-Paper Display (GDEQ031T10) === */
#define BOARD_EPAPER_CS      GPIO_NUM_34
#define BOARD_EPAPER_DC      GPIO_NUM_35
#define BOARD_EPAPER_RST     (-1)        /* Not connected on T-Deck Pro */
#define BOARD_EPAPER_BUSY    GPIO_NUM_37

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
#define BOARD_KBD_ADDR       0x34
#define BOARD_KBD_INT        GPIO_NUM_46

/* === Touch (CST328) === */
#define BOARD_TOUCH_ADDR     0x1A
#define BOARD_TOUCH_INT      GPIO_NUM_16
#define BOARD_TOUCH_RST      GPIO_NUM_48

/* === GPS (MIA-M10Q) === */
#define BOARD_GPS_UART       UART_NUM_1
#define BOARD_GPS_TX         GPIO_NUM_17
#define BOARD_GPS_RX         GPIO_NUM_15

/* === Audio (PCM5102A I2S DAC) === */
#define BOARD_I2S_BCK        GPIO_NUM_7
#define BOARD_I2S_WS         GPIO_NUM_6
#define BOARD_I2S_DATA       GPIO_NUM_3

/* === IMU (BHI260AP) === */
#define BOARD_IMU_ADDR       0x28
#define BOARD_IMU_INT        GPIO_NUM_14

/* === Light Sensor (LTR-553ALS) === */
#define BOARD_LIGHT_ADDR     0x23

/* === Power === */
#define BOARD_BAT_ADC        GPIO_NUM_4
#define BOARD_CHARGE_STATUS  GPIO_NUM_10

/* === Misc === */
#define BOARD_LED            GPIO_NUM_13
#define BOARD_DISPLAY_WIDTH  320
#define BOARD_DISPLAY_HEIGHT 240
