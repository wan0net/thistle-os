#include "board_tdeck_pro.h"
#include "hal/board.h"
#include "drv_epaper_gdeq031t10.h"
#include "drv_kbd_tca8418.h"
#include "drv_touch_cst328.h"
#include "drv_radio_sx1262.h"
#include "drv_gps_mia_m10q.h"
#include "drv_audio_pcm5102a.h"
#include "drv_imu_bhi260ap.h"
#include "drv_power_tp4065b.h"
#include "drv_sdcard.h"
#include "esp_log.h"

static const char *TAG = "board";

// Store bus handles as module-level statics
static spi_bus_config_t spi_bus_cfg;
static i2c_master_bus_handle_t i2c_bus;

// Static driver configs — populated before registration
static const epaper_gdeq031t10_config_t epaper_config = {
    .spi_host    = BOARD_SPI_HOST,
    .pin_cs      = BOARD_EPAPER_CS,
    .pin_dc      = BOARD_EPAPER_DC,
    .pin_rst     = BOARD_EPAPER_RST,
    .pin_busy    = BOARD_EPAPER_BUSY,
    .spi_clock_hz = 4000000,
};

static const sdcard_config_t sd_config = {
    .spi_host    = BOARD_SPI_HOST,
    .pin_cs      = BOARD_SD_CS,
    .mount_point = "/sdcard",
    .max_files   = 5,
};

// kbd and touch configs need the runtime i2c_bus handle, so they are mutable
static kbd_tca8418_config_t kbd_config;
static touch_cst328_config_t touch_config;

esp_err_t board_init(void) {
    esp_err_t ret;

    ESP_LOGI(TAG, "Initializing T-Deck Pro board");

    // 0. Power and GPIO pre-init (from LilyGO factory firmware)
    // Disable deep sleep GPIO hold
    gpio_deep_sleep_hold_dis();

    // Power enables — must be HIGH before peripherals work
    gpio_config_t pwr_conf = {
        .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
        .pin_bit_mask = (1ULL << BOARD_1V8_EN) |
                        (1ULL << BOARD_GPS_EN) |
                        (1ULL << BOARD_MODEM_EN) |
                        (1ULL << BOARD_LORA_EN) |
                        (1ULL << BOARD_MOTOR),
    };
    gpio_config(&pwr_conf);
    gpio_set_level(BOARD_1V8_EN, 1);    // Enable 1.8V rail
    gpio_set_level(BOARD_GPS_EN, 1);    // Enable GPS
    gpio_set_level(BOARD_MODEM_EN, 1);  // Enable modem
    gpio_set_level(BOARD_LORA_EN, 1);   // Enable LoRa
    gpio_set_level(BOARD_MOTOR, 0);     // Motor OFF

    // Pull all SPI chip selects HIGH before bus init (unselected)
    gpio_config_t cs_conf = {
        .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
        .pin_bit_mask = (1ULL << BOARD_EPAPER_CS) |
                        (1ULL << BOARD_LORA_CS) |
                        (1ULL << BOARD_SD_CS),
    };
    gpio_config(&cs_conf);
    gpio_set_level(BOARD_EPAPER_CS, 1);
    gpio_set_level(BOARD_LORA_CS, 1);
    gpio_set_level(BOARD_SD_CS, 1);

    // Wait for power rails to stabilize
    vTaskDelay(pdMS_TO_TICKS(100));
    ESP_LOGI(TAG, "Power and GPIO pre-init done");

    // 1. Init SPI bus
    spi_bus_cfg = (spi_bus_config_t){
        .mosi_io_num = BOARD_SPI_MOSI,
        .miso_io_num = BOARD_SPI_MISO,
        .sclk_io_num = BOARD_SPI_SCLK,
        .quadwp_io_num = -1,
        .quadhd_io_num = -1,
        .max_transfer_sz = 320 * 240 / 8, // e-paper buffer size
    };
    ret = spi_bus_initialize(BOARD_SPI_HOST, &spi_bus_cfg, SPI_DMA_CH_AUTO);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "SPI bus init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    // 2. Init I2C bus
    i2c_master_bus_config_t i2c_cfg = {
        .i2c_port = BOARD_I2C_PORT,
        .sda_io_num = BOARD_I2C_SDA,
        .scl_io_num = BOARD_I2C_SCL,
        .clk_source = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt = 7,
        .flags.enable_internal_pullup = true,
    };
    ret = i2c_new_master_bus(&i2c_cfg, &i2c_bus);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "I2C bus init failed: %s", esp_err_to_name(ret));
        return ret;
    }

    // 3. Initialise I2C-dependent driver configs now that the bus handle is available
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

    // 4. Set board name
    hal_set_board_name("LilyGo T-Deck Pro");

    // 5. Register display driver (e-paper)
    hal_display_register(drv_epaper_gdeq031t10_get(), &epaper_config);

    // 6. Register input drivers
    hal_input_register(drv_kbd_tca8418_get(), &kbd_config);
    hal_input_register(drv_touch_cst328_get(), &touch_config);

    // 7. Register radio (stub for now)
    hal_radio_register(drv_radio_sx1262_get(), NULL);

    // 8. Register GPS (stub)
    hal_gps_register(drv_gps_mia_m10q_get(), NULL);

    // 9. Register audio (stub)
    hal_audio_register(drv_audio_pcm5102a_get(), NULL);

    // 10. Register power management (stub)
    hal_power_register(drv_power_tp4065b_get(), NULL);

    // 11. Register IMU (stub)
    hal_imu_register(drv_imu_bhi260ap_get(), NULL);

    // 12. Register SD card storage
    hal_storage_register(drv_sdcard_get(), &sd_config);

    ESP_LOGI(TAG, "T-Deck Pro board initialized");
    return ESP_OK;
}
