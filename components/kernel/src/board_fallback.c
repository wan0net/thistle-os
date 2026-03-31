/*
 * I2C-based board auto-detection fallback.
 *
 * When no board.json exists on SPIFFS, this code probes known I2C addresses
 * across common pin configurations to fingerprint the attached hardware.
 * On a match it writes the corresponding board.json to SPIFFS so the Rust
 * board_config_init() can load it on retry.
 *
 * Declared __attribute__((weak)) so that an explicitly linked board component
 * (board_tdeck, board_tdeck_pro, etc.) can override it with a strong symbol.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "hal/board.h"
#include "esp_log.h"
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>

/* ESP-IDF I2C master driver — used only on real hardware */
#if !defined(THISTLE_SIMULATOR)
#include "driver/i2c_master.h"
#endif

static const char *TAG = "board_detect";

/* ── I2C pin combinations to try ──────────────────────────────────────────── */

typedef struct {
    int sda;
    int scl;
} i2c_pins_t;

/* Ordered by popularity: most ESP32-S3 boards use one of these */
static const i2c_pins_t I2C_PIN_COMBOS[] = {
    { 18,  8 },   /* T-Deck, T-Deck Plus */
    { 18, 17 },   /* T-Display-S3, T3-S3 */
    { 13, 14 },   /* T-Deck Pro */
    { 17, 18 },   /* Heltec V3 */
    {  1,  2 },   /* Cardputer */
    {  4,  5 },   /* RAK3312 */
};
#define NUM_PIN_COMBOS (sizeof(I2C_PIN_COMBOS) / sizeof(I2C_PIN_COMBOS[0]))

/* ── Known I2C device addresses ───────────────────────────────────────────── */

#define ADDR_TCA8418   0x34   /* TCA8418 keyboard controller */
#define ADDR_CST328    0x1A   /* CST328 capacitive touch */
#define ADDR_BHI260AP  0x28   /* BHI260AP IMU (T-Deck Pro only) */
#define ADDR_CST816    0x15   /* CST816 capacitive touch (T-Display-S3) */
#define ADDR_SSD1306   0x3C   /* SSD1306 OLED display */
#define ADDR_CARDKB    0x5F   /* CardKB keyboard (Cardputer) */

/* ── I2C probe helpers (real hardware only) ───────────────────────────────── */

#if !defined(THISTLE_SIMULATOR)

/* Probe a single I2C address — returns true if device ACKs */
static bool i2c_probe(i2c_master_bus_handle_t bus, uint8_t addr)
{
    i2c_device_config_t dev_cfg = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7,
        .device_address = addr,
        .scl_speed_hz = 100000,
    };
    i2c_master_dev_handle_t dev = NULL;
    if (i2c_master_bus_add_device(bus, &dev_cfg, &dev) != ESP_OK) {
        return false;
    }

    /* Try a zero-length write (address-only transaction).
     * ESP_OK means the device ACKed its address. */
    uint8_t dummy = 0;
    esp_err_t ret = i2c_master_transmit(dev, &dummy, 0, 50);
    i2c_master_bus_rm_device(dev);
    return (ret == ESP_OK);
}

/* Scan result for a single pin combo */
typedef struct {
    bool has_tca8418;
    bool has_cst328;
    bool has_bhi260ap;
    bool has_cst816;
    bool has_ssd1306;
    bool has_cardkb;
    int  sda;
    int  scl;
} scan_result_t;

/* Try one pin combo: init bus, probe all known addresses, tear down */
static bool try_pin_combo(const i2c_pins_t *pins, scan_result_t *out)
{
    memset(out, 0, sizeof(*out));
    out->sda = pins->sda;
    out->scl = pins->scl;

    i2c_master_bus_config_t bus_cfg = {
        .i2c_port = I2C_NUM_0,
        .sda_io_num = pins->sda,
        .scl_io_num = pins->scl,
        .clk_source = I2C_CLK_SRC_DEFAULT,
        .glitch_ignore_cnt = 7,
        .flags.enable_internal_pullup = true,
    };
    i2c_master_bus_handle_t bus = NULL;
    if (i2c_new_master_bus(&bus_cfg, &bus) != ESP_OK) {
        return false;
    }

    out->has_tca8418  = i2c_probe(bus, ADDR_TCA8418);
    out->has_cst328   = i2c_probe(bus, ADDR_CST328);
    out->has_bhi260ap = i2c_probe(bus, ADDR_BHI260AP);
    out->has_cst816   = i2c_probe(bus, ADDR_CST816);
    out->has_ssd1306  = i2c_probe(bus, ADDR_SSD1306);
    out->has_cardkb   = i2c_probe(bus, ADDR_CARDKB);

    i2c_del_master_bus(bus);

    /* Return true if ANY device responded */
    return out->has_tca8418 || out->has_cst328 || out->has_bhi260ap ||
           out->has_cst816  || out->has_ssd1306 || out->has_cardkb;
}
#endif /* !THISTLE_SIMULATOR */

/* ── Embedded board JSON configs ──────────────────────────────────────────── *
 * Each string is the full board.json content for one board.  ~500 bytes each,
 * total ~4 KB flash — trivial on a 4.5 MB partition.                         */

static const char *JSON_TDECK =
"{\n"
"    \"board\": { \"name\": \"LilyGo T-Deck\", \"arch\": \"esp32s3\", \"board_id\": \"tdeck\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"spi\": [{ \"host\": 2, \"mosi\": 41, \"miso\": 38, \"sclk\": 40, \"max_transfer_bytes\": 9600 }],\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 18, \"scl\": 8, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.lcd-st7789\", \"hal\": \"display\", \"entry\": \"lcd-st7789.drv.elf\",\n"
"          \"config\": { \"cs\": 12, \"dc\": 11, \"rst\": -1, \"bl\": 15, \"spi_bus\": 0, \"width\": 320, \"height\": 240 } },\n"
"        { \"id\": \"com.thistle.drv.kbd-tca8418\", \"hal\": \"input\", \"entry\": \"kbd-tca8418.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x34\", \"pin_int\": 46 } },\n"
"        { \"id\": \"com.thistle.drv.touch-cst328\", \"hal\": \"input\", \"entry\": \"touch-cst328.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x1A\", \"pin_int\": 16, \"pin_rst\": -1, \"max_x\": 320, \"max_y\": 240 } },\n"
"        { \"id\": \"com.thistle.drv.radio-sx1262\", \"hal\": \"radio\", \"entry\": \"radio-sx1262.drv.elf\",\n"
"          \"config\": { \"spi_bus\": 0, \"cs\": 7, \"dio1\": 33, \"rst\": 8, \"busy\": 34 } },\n"
"        { \"id\": \"com.thistle.drv.gps-mia-m10q\", \"hal\": \"gps\", \"entry\": \"gps-mia-m10q.drv.elf\",\n"
"          \"config\": { \"uart_num\": 1, \"tx\": 43, \"rx\": 44, \"baud_rate\": 9600 } }\n"
"    ]\n"
"}\n";

static const char *JSON_TDECK_PRO =
"{\n"
"    \"board\": { \"name\": \"LilyGo T-Deck Pro\", \"arch\": \"esp32s3\", \"board_id\": \"tdeck-pro\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"spi\": [{ \"host\": 2, \"mosi\": 33, \"miso\": 47, \"sclk\": 36, \"max_transfer_bytes\": 9600 }],\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 13, \"scl\": 14, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.epaper-gdeq031t10\", \"hal\": \"display\", \"entry\": \"epaper-gdeq031t10.drv.elf\",\n"
"          \"config\": { \"spi_bus\": 0, \"cs\": 34, \"dc\": 35, \"rst\": -1, \"busy\": 37, \"spi_clock_hz\": 2000000 } },\n"
"        { \"id\": \"com.thistle.drv.kbd-tca8418\", \"hal\": \"input\", \"entry\": \"kbd-tca8418.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x34\", \"pin_int\": 15 } },\n"
"        { \"id\": \"com.thistle.drv.touch-cst328\", \"hal\": \"input\", \"entry\": \"touch-cst328.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x1A\", \"pin_int\": 12, \"pin_rst\": 45, \"max_x\": 240, \"max_y\": 320 } },\n"
"        { \"id\": \"com.thistle.drv.radio-sx1262\", \"hal\": \"radio\", \"entry\": \"radio-sx1262.drv.elf\",\n"
"          \"config\": { \"spi_bus\": 0, \"cs\": 3, \"dio1\": 5, \"rst\": 4, \"busy\": 6 } },\n"
"        { \"id\": \"com.thistle.drv.gps-mia-m10q\", \"hal\": \"gps\", \"entry\": \"gps-mia-m10q.drv.elf\",\n"
"          \"config\": { \"uart_num\": 2, \"tx\": 43, \"rx\": 44, \"baud_rate\": 9600 } },\n"
"        { \"id\": \"com.thistle.drv.audio-pcm5102a\", \"hal\": \"audio\", \"entry\": \"audio-pcm5102a.drv.elf\",\n"
"          \"config\": { \"i2s_bck\": 7, \"i2s_ws\": 9, \"i2s_data\": 8 } },\n"
"        { \"id\": \"com.thistle.drv.imu-bhi260ap\", \"hal\": \"imu\", \"entry\": \"imu-bhi260ap.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x28\", \"pin_int\": 21 } },\n"
"        { \"id\": \"com.thistle.drv.power-tp4065b\", \"hal\": \"power\", \"entry\": \"power-tp4065b.drv.elf\",\n"
"          \"config\": { \"adc_pin\": 4, \"charge_pin\": 10 } },\n"
"        { \"id\": \"com.thistle.drv.sdcard\", \"hal\": \"storage\", \"entry\": \"sdcard.drv.elf\",\n"
"          \"config\": { \"spi_bus\": 0, \"pin_cs\": 48, \"mount_point\": \"/sdcard\" } }\n"
"    ]\n"
"}\n";

static const char *JSON_TDISPLAY =
"{\n"
"    \"board\": { \"name\": \"LilyGo T-Display-S3\", \"arch\": \"esp32s3\", \"board_id\": \"tdisplay-s3\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 18, \"scl\": 17, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.lcd-st7789\", \"hal\": \"display\", \"entry\": \"lcd-st7789.drv.elf\",\n"
"          \"config\": { \"cs\": 6, \"dc\": 7, \"rst\": 5, \"bl\": 38, \"spi_bus\": 0, \"width\": 170, \"height\": 320 } },\n"
"        { \"id\": \"com.thistle.drv.touch-cst816\", \"hal\": \"input\", \"entry\": \"touch-cst816.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x15\", \"pin_int\": 16, \"pin_rst\": 21, \"max_x\": 170, \"max_y\": 320 } }\n"
"    ]\n"
"}\n";

static const char *JSON_CARDPUTER =
"{\n"
"    \"board\": { \"name\": \"M5Stack Cardputer\", \"arch\": \"esp32s3\", \"board_id\": \"cardputer\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"spi\": [{ \"host\": 2, \"mosi\": 35, \"miso\": -1, \"sclk\": 36, \"max_transfer_bytes\": 4096 }],\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 1, \"scl\": 2, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.lcd-st7789\", \"hal\": \"display\", \"entry\": \"lcd-st7789.drv.elf\",\n"
"          \"config\": { \"cs\": 37, \"dc\": 34, \"rst\": 33, \"bl\": 38, \"spi_bus\": 0, \"width\": 240, \"height\": 135, \"x_offset\": 40, \"y_offset\": 52 } },\n"
"        { \"id\": \"com.thistle.drv.kbd-cardkb\", \"hal\": \"input\", \"entry\": \"kbd-cardkb.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x5F\" } },\n"
"        { \"id\": \"com.thistle.drv.sdcard\", \"hal\": \"storage\", \"entry\": \"sdcard.drv.elf\",\n"
"          \"config\": { \"spi_host\": 1, \"spi_mosi\": 14, \"spi_miso\": 39, \"spi_sclk\": 40, \"pin_cs\": 12, \"mount_point\": \"/sdcard\" } }\n"
"    ]\n"
"}\n";

static const char *JSON_HELTEC_V3 =
"{\n"
"    \"board\": { \"name\": \"Heltec WiFi LoRa 32 V3\", \"arch\": \"esp32s3\", \"board_id\": \"heltec-v3\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"spi\": [{ \"host\": 2, \"mosi\": 10, \"miso\": 11, \"sclk\": 9, \"max_transfer_bytes\": 4096 }],\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 17, \"scl\": 18, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.oled-ssd1306\", \"hal\": \"display\", \"entry\": \"oled-ssd1306.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x3C\", \"pin_rst\": 21, \"pin_vext\": 36 } },\n"
"        { \"id\": \"com.thistle.drv.radio-sx1262\", \"hal\": \"radio\", \"entry\": \"radio-sx1262.drv.elf\",\n"
"          \"config\": { \"spi_bus\": 0, \"cs\": 8, \"dio1\": 14, \"rst\": 12, \"busy\": 13, \"tcxo\": true } },\n"
"        { \"id\": \"com.thistle.drv.power-heltec-v3\", \"hal\": \"power\", \"entry\": \"power-tp4065b.drv.elf\",\n"
"          \"config\": { \"adc_pin\": 1, \"charge_pin\": -1, \"adc_ctrl_pin\": 37 } }\n"
"    ]\n"
"}\n";

static const char *JSON_T3_S3 =
"{\n"
"    \"board\": { \"name\": \"LilyGo T3-S3 LoRa\", \"arch\": \"esp32s3\", \"board_id\": \"t3-s3\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"spi\": [{ \"host\": 2, \"mosi\": 11, \"miso\": 2, \"sclk\": 14, \"max_transfer_bytes\": 4096 }],\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 18, \"scl\": 17, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.oled-ssd1306\", \"hal\": \"display\", \"entry\": \"oled-ssd1306.drv.elf\",\n"
"          \"config\": { \"i2c_bus\": 0, \"i2c_addr\": \"0x3C\", \"pin_rst\": -1 } },\n"
"        { \"id\": \"com.thistle.drv.radio-sx1262\", \"hal\": \"radio\", \"entry\": \"radio-sx1262.drv.elf\",\n"
"          \"config\": { \"spi_bus\": 0, \"cs\": 8, \"dio1\": 33, \"rst\": 12, \"busy\": 34 } }\n"
"    ]\n"
"}\n";

static const char *JSON_RAK3312 =
"{\n"
"    \"board\": { \"name\": \"RAK WisBlock RAK3312\", \"arch\": \"esp32s3\", \"board_id\": \"rak3312\", \"version\": \"1.0\" },\n"
"    \"buses\": {\n"
"        \"spi\": [],\n"
"        \"i2c\": [{ \"port\": 0, \"sda\": 4, \"scl\": 5, \"freq_hz\": 400000 }]\n"
"    },\n"
"    \"drivers\": [\n"
"        { \"id\": \"com.thistle.drv.radio-sx1262\", \"hal\": \"radio\", \"entry\": \"radio-sx1262.drv.elf\",\n"
"          \"config\": { \"spi_bus\": -1, \"cs\": -1, \"dio1\": -1, \"rst\": -1, \"busy\": -1, \"internal\": true } }\n"
"    ]\n"
"}\n";

/* ── Write board.json to SPIFFS ───────────────────────────────────────────── */

static esp_err_t write_board_json(const char *json_content)
{
    /* Ensure config directory exists */
    mkdir("/spiffs/config", 0755);

    FILE *f = fopen("/spiffs/config/board.json", "w");
    if (!f) {
        ESP_LOGE(TAG, "Failed to create /spiffs/config/board.json");
        return ESP_FAIL;
    }
    fputs(json_content, f);
    fclose(f);
    ESP_LOGI(TAG, "Wrote detected board config to /spiffs/config/board.json");
    return ESP_OK;
}

/* ── Fingerprint matching ─────────────────────────────────────────────────── *
 * Given I2C scan results, match against known board fingerprints.            *
 * Returns the JSON string for the matched board, or NULL.                    */

#if !defined(THISTLE_SIMULATOR)
static const char *match_fingerprint(const scan_result_t *r)
{
    /* T-Deck Pro: TCA8418 keyboard + BHI260AP IMU on SDA=13/SCL=14 */
    if (r->has_tca8418 && r->has_bhi260ap) {
        ESP_LOGI(TAG, "Detected: T-Deck Pro (keyboard + IMU on SDA=%d SCL=%d)",
                 r->sda, r->scl);
        return JSON_TDECK_PRO;
    }

    /* T-Deck / T-Deck Plus: TCA8418 + CST328, no BHI260AP */
    if (r->has_tca8418 && r->has_cst328) {
        ESP_LOGI(TAG, "Detected: T-Deck (keyboard + touch on SDA=%d SCL=%d)",
                 r->sda, r->scl);
        return JSON_TDECK;
    }

    /* T-Deck with only keyboard responding */
    if (r->has_tca8418) {
        ESP_LOGI(TAG, "Detected: T-Deck (keyboard only on SDA=%d SCL=%d)",
                 r->sda, r->scl);
        return JSON_TDECK;
    }

    /* T-Display-S3: CST816 touch, no keyboard */
    if (r->has_cst816) {
        ESP_LOGI(TAG, "Detected: T-Display-S3 (CST816 touch on SDA=%d SCL=%d)",
                 r->sda, r->scl);
        return JSON_TDISPLAY;
    }

    /* Cardputer: CardKB keyboard */
    if (r->has_cardkb) {
        ESP_LOGI(TAG, "Detected: Cardputer (CardKB on SDA=%d SCL=%d)",
                 r->sda, r->scl);
        return JSON_CARDPUTER;
    }

    /* SSD1306 OLED: Heltec V3 or T3-S3.
     * Disambiguate by I2C pin combo — Heltec uses SDA=17/SCL=18,
     * T3-S3 uses SDA=18/SCL=17. */
    if (r->has_ssd1306) {
        if (r->sda == 17 && r->scl == 18) {
            ESP_LOGI(TAG, "Detected: Heltec V3 (OLED on SDA=17 SCL=18)");
            return JSON_HELTEC_V3;
        }
        /* Default to T3-S3 for SDA=18/SCL=17 or other pin combos */
        ESP_LOGI(TAG, "Detected: T3-S3 (OLED on SDA=%d SCL=%d)", r->sda, r->scl);
        return JSON_T3_S3;
    }

    return NULL;
}
#endif /* !THISTLE_SIMULATOR */

/* ── Public API: detect board and write config ────────────────────────────── */

esp_err_t board_detect_and_write(void)
{
#if defined(THISTLE_SIMULATOR)
    ESP_LOGW(TAG, "Board detection not available in simulator");
    return ESP_ERR_NOT_SUPPORTED;
#else
    ESP_LOGI(TAG, "Auto-detecting board via I2C scan...");

    const char *matched_json = NULL;

    /* Try each known pin combination until we get a match */
    for (int i = 0; i < (int)NUM_PIN_COMBOS; i++) {
        ESP_LOGI(TAG, "Trying I2C pins SDA=%d SCL=%d...",
                 I2C_PIN_COMBOS[i].sda, I2C_PIN_COMBOS[i].scl);

        scan_result_t result;
        bool any_device = try_pin_combo(&I2C_PIN_COMBOS[i], &result);

        if (any_device) {
            ESP_LOGI(TAG, "I2C scan (SDA=%d SCL=%d): TCA8418=%d CST328=%d "
                     "BHI260=%d CST816=%d SSD1306=%d CardKB=%d",
                     result.sda, result.scl,
                     result.has_tca8418, result.has_cst328, result.has_bhi260ap,
                     result.has_cst816, result.has_ssd1306, result.has_cardkb);

            matched_json = match_fingerprint(&result);
            if (matched_json) {
                break;
            }
        }
    }

    if (!matched_json) {
        /* No I2C devices found on any pin combo — headless board */
        ESP_LOGI(TAG, "No I2C devices found on any pin combo — assuming headless (RAK3312)");
        matched_json = JSON_RAK3312;
    }

    /* Write the detected config to SPIFFS */
    return write_board_json(matched_json);
#endif
}

/* ── Legacy fallback — still weak so compiled board components override ──── */

__attribute__((weak))
esp_err_t board_init(void)
{
    ESP_LOGW(TAG, "No board.json found — running in safe mode");
    ESP_LOGW(TAG, "Insert SD card with config/boards/<board>.json and reboot");
    ESP_LOGW(TAG, "Or use Recovery mode to provision the device");

    hal_set_board_name("Unknown (safe mode)");

    return ESP_OK;
}
