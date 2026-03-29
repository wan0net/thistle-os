// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — LilyGo T-Deck board initialisation (LCD variant)

#include "board_tdeck.h"
#include "hal/board.h"
#include "drv_lcd_st7789.h"
#include "drv_kbd_tca8418.h"
#include "drv_touch_cst328.h"
#include "drv_radio_sx1262.h"
#include "drv_gps_mia_m10q.h"
#include "drv_audio_pcm5102a.h"
#include "drv_power_tp4065b.h"
#include "drv_sdcard.h"
#include "esp_log.h"

static const char *TAG = "board";

// Store bus handles as module-level statics
static spi_bus_config_t spi_bus_cfg;
static i2c_master_bus_handle_t i2c_bus;

// Static driver configs — populated before registration.
// lcd_config, kbd_config, touch_config and sd_config all need runtime values,
// so they are mutable structs (lcd needs spi_host from macro; kbd/touch need i2c_bus).
static lcd_st7789_config_t   lcd_config;
static kbd_tca8418_config_t  kbd_config;
static touch_cst328_config_t touch_config;
static sdcard_config_t       sd_config;

esp_err_t board_init(void) {
    esp_err_t ret;

    ESP_LOGI(TAG, "Initializing T-Deck board (LCD variant)");

    // 1. Init SPI bus
    // max_transfer_sz covers one full RGB565 frame (320*240*2 bytes = 153,600).
    spi_bus_cfg = (spi_bus_config_t){
        .mosi_io_num   = BOARD_SPI_MOSI,
        .miso_io_num   = BOARD_SPI_MISO,
        .sclk_io_num   = BOARD_SPI_SCLK,
        .quadwp_io_num = -1,
        .quadhd_io_num = -1,
        .max_transfer_sz = BOARD_DISPLAY_WIDTH * BOARD_DISPLAY_HEIGHT * 2,
    };
    ret = spi_bus_initialize(BOARD_SPI_HOST, &spi_bus_cfg, SPI_DMA_CH_AUTO);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "SPI bus init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    // 2. Init I2C bus
    i2c_master_bus_config_t i2c_cfg = {
        .i2c_port              = BOARD_I2C_PORT,
        .sda_io_num            = BOARD_I2C_SDA,
        .scl_io_num            = BOARD_I2C_SCL,
        .clk_source            = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt     = 7,
        .flags.enable_internal_pullup = true,
    };
    ret = i2c_new_master_bus(&i2c_cfg, &i2c_bus);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "I2C bus init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    // 3. Populate driver configs now that bus handles are available

    lcd_config = (lcd_st7789_config_t){
        .spi_host    = BOARD_SPI_HOST,
        .pin_cs      = BOARD_LCD_CS,
        .pin_dc      = BOARD_LCD_DC,
        .pin_rst     = BOARD_LCD_RST,
        .pin_bl      = BOARD_LCD_BL,
        .spi_clock_hz = 40000000,  // 40 MHz — ST7789 maximum rated clock
    };

    kbd_config = (kbd_tca8418_config_t){
        .i2c_bus  = i2c_bus,
        .i2c_addr = BOARD_KBD_ADDR,
        .pin_int  = BOARD_KBD_INT,
    };

    touch_config = (touch_cst328_config_t){
        .i2c_bus  = i2c_bus,
        .i2c_addr = BOARD_TOUCH_ADDR,
        .pin_int  = BOARD_TOUCH_INT,
        .pin_rst  = BOARD_TOUCH_RST,
        .max_x    = BOARD_DISPLAY_WIDTH,
        .max_y    = BOARD_DISPLAY_HEIGHT,
    };

    sd_config = (sdcard_config_t){
        .spi_host    = BOARD_SPI_HOST,
        .pin_cs      = BOARD_SD_CS,
        .mount_point = "/sdcard",
        .max_files   = 5,
    };

    // 4. Set board name
    hal_set_board_name("LilyGo T-Deck");

    // 5. Register display driver (ST7789 LCD)
    hal_display_register(drv_lcd_st7789_get(), &lcd_config);

    // 6. Register input drivers
    hal_input_register(drv_kbd_tca8418_get(), &kbd_config);
    hal_input_register(drv_touch_cst328_get(), &touch_config);

    // 7. Register radio (stub for now)
    hal_radio_register(drv_radio_sx1262_get(), NULL);

    // 8. Register GPS (stub)
    hal_gps_register(drv_gps_mia_m10q_get(), NULL);

    // 9. Register audio (stub)
    hal_audio_register(drv_audio_pcm5102a_get(), NULL);

    // 10. Register power management (battery ADC + charge status)
    static const struct { int adc_channel; int pin_charge_status; } power_config = {
        .adc_channel = 3,            /* ADC1 channel 3 = GPIO4 (BOARD_BAT_ADC) */
        .pin_charge_status = 10,     /* GPIO10 (BOARD_CHARGE_STATUS) */
    };
    hal_power_register(drv_power_tp4065b_get(), &power_config);

    // 11. Register SD card storage
    hal_storage_register(drv_sdcard_get(), &sd_config);

    ESP_LOGI(TAG, "T-Deck board initialized");
    return ESP_OK;
}
