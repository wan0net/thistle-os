// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — U-blox MIA-M10Q GPS driver

#include "drv_gps_mia_m10q.h"
#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/uart.h"
#include <string.h>
#include <stdlib.h>
#include <math.h>
#include <time.h>

static const char *TAG = "mia_m10q";

#define UART_RX_BUF_SIZE   1024
#define RX_TASK_STACK_SIZE 4096
#define RX_TASK_PRIORITY   5
#define KNOTS_TO_KMH       1.852f

// UBX-RXM-PMREQ: put receiver into backup mode
static const uint8_t UBX_RXM_PMREQ_BACKUP[] = {
    0xB5, 0x62,       // sync chars
    0x02, 0x41,       // class, id
    0x08, 0x00,       // length (8 bytes payload)
    0x00, 0x00, 0x00, 0x00,  // duration = 0 (indefinite)
    0x02, 0x00, 0x00, 0x00,  // flags: backup
    0x4D, 0x3B        // checksum
};

// ---------------------------------------------------------------------------
// Driver state
// ---------------------------------------------------------------------------

static struct {
    gps_mia_m10q_config_t cfg;
    hal_gps_cb_t          cb;
    void                 *cb_data;
    hal_gps_position_t    last_position;
    TaskHandle_t          rx_task;
    portMUX_TYPE          spinlock;
    bool                  initialized;
    bool                  enabled;
    char                  nmea_buf[256];
    int                   nmea_idx;
} s_gps = {
    .spinlock = portMUX_INITIALIZER_UNLOCKED,
};

// ---------------------------------------------------------------------------
// NMEA helpers
// ---------------------------------------------------------------------------

/* XOR checksum of all bytes between '$' and '*' (exclusive). */
static bool nmea_verify_checksum(const char *sentence)
{
    if (sentence == NULL || sentence[0] != '$') {
        return false;
    }

    const char *p   = sentence + 1;  // skip '$'
    uint8_t     crc = 0;

    while (*p && *p != '*') {
        crc ^= (uint8_t)*p;
        p++;
    }

    if (*p != '*') {
        return false;  // no checksum field
    }

    p++;  // skip '*'

    char hex[3] = { p[0], p[1], '\0' };
    uint8_t received = (uint8_t)strtol(hex, NULL, 16);

    return crc == received;
}

/*
 * Split a sentence into fields separated by commas.
 * fields[] is filled with pointers into a mutable copy; returns field count.
 * Caller provides the buffer and its size.
 */
static int nmea_split(char *buf, char **fields, int max_fields)
{
    int count = 0;
    char *p   = buf;

    while (count < max_fields) {
        fields[count++] = p;
        p = strchr(p, ',');
        if (!p) break;
        *p++ = '\0';
    }

    return count;
}

/*
 * Convert NMEA coordinate (ddmm.mmmm or dddmm.mmmm) + direction to
 * signed decimal degrees.
 */
static double nmea_parse_coord(const char *field, const char *dir)
{
    if (!field || field[0] == '\0') return 0.0;

    double raw  = atof(field);
    double deg  = floor(raw / 100.0);
    double mins = raw - deg * 100.0;
    double dd   = deg + mins / 60.0;

    if (dir && (dir[0] == 'S' || dir[0] == 'W')) {
        dd = -dd;
    }

    return dd;
}

/*
 * Convert NMEA UTC time (hhmmss.ss) and date (ddmmyy) to a Unix timestamp.
 * Returns 0 if either string is empty.
 */
static uint32_t nmea_to_timestamp(const char *time_str, const char *date_str)
{
    if (!time_str || time_str[0] == '\0' || !date_str || date_str[0] == '\0') {
        return 0;
    }

    struct tm t = { 0 };

    // hhmmss.ss
    char tmp[7];
    strncpy(tmp, time_str, 6);
    tmp[6] = '\0';
    t.tm_hour = (tmp[0] - '0') * 10 + (tmp[1] - '0');
    t.tm_min  = (tmp[2] - '0') * 10 + (tmp[3] - '0');
    t.tm_sec  = (tmp[4] - '0') * 10 + (tmp[5] - '0');

    // ddmmyy
    t.tm_mday = (date_str[0] - '0') * 10 + (date_str[1] - '0');
    t.tm_mon  = (date_str[2] - '0') * 10 + (date_str[3] - '0') - 1;  // 0-based
    t.tm_year = (date_str[4] - '0') * 10 + (date_str[5] - '0') + 100; // years since 1900

    t.tm_isdst = 0;

    time_t ts = mktime(&t);
    if (ts == (time_t)-1) return 0;

    // mktime uses local time; adjust to UTC by subtracting timezone offset
    // On bare-metal ESP-IDF the timezone is typically UTC, so this is safe.
    return (uint32_t)ts;
}

// ---------------------------------------------------------------------------
// NMEA sentence processors
// ---------------------------------------------------------------------------

/*
 * $GNRMC,hhmmss.ss,A,llll.ll,N,yyyyy.yy,W,speed,heading,ddmmyy,...*CS
 * Fields (0-based after split):
 *   0  sentence id
 *   1  UTC time
 *   2  status (A/V)
 *   3  latitude
 *   4  N/S
 *   5  longitude
 *   6  E/W
 *   7  speed (knots)
 *   8  course (degrees)
 *   9  date (ddmmyy)
 */
static void process_gnrmc(char **f, int nf)
{
    if (nf < 10) return;

    bool valid = (f[2][0] == 'A');

    double lat     = nmea_parse_coord(f[3], f[4]);
    double lon     = nmea_parse_coord(f[5], f[6]);
    float  speed   = (f[7][0] != '\0') ? (float)atof(f[7]) * KNOTS_TO_KMH : 0.0f;
    float  heading = (f[8][0] != '\0') ? (float)atof(f[8]) : 0.0f;
    uint32_t ts    = nmea_to_timestamp(f[1], f[9]);

    portENTER_CRITICAL(&s_gps.spinlock);
    s_gps.last_position.latitude    = lat;
    s_gps.last_position.longitude   = lon;
    s_gps.last_position.speed_kmh   = speed;
    s_gps.last_position.heading_deg = heading;
    s_gps.last_position.fix_valid   = valid;
    if (ts != 0) {
        s_gps.last_position.timestamp = ts;
    }
    portEXIT_CRITICAL(&s_gps.spinlock);
}

/*
 * $GNGGA,hhmmss.ss,llll.ll,N,yyyyy.yy,W,quality,numSV,hdop,alt,M,...*CS
 * Fields (0-based):
 *   0  sentence id
 *   1  UTC time
 *   2  latitude
 *   3  N/S
 *   4  longitude
 *   5  E/W
 *   6  fix quality (0=none, 1=GPS, 2=DGPS)
 *   7  number of satellites
 *   8  HDOP
 *   9  altitude (MSL)
 */
static void process_gngga(char **f, int nf)
{
    if (nf < 10) return;

    int   quality    = (f[6][0] != '\0') ? atoi(f[6]) : 0;
    int   satellites = (f[7][0] != '\0') ? atoi(f[7]) : 0;
    float altitude   = (f[9][0] != '\0') ? (float)atof(f[9]) : 0.0f;

    portENTER_CRITICAL(&s_gps.spinlock);
    s_gps.last_position.satellites = (uint8_t)(satellites < 0 ? 0 : satellites);
    s_gps.last_position.altitude_m = altitude;
    if (quality == 0) {
        s_gps.last_position.fix_valid = false;
    }
    portEXIT_CRITICAL(&s_gps.spinlock);
}

// ---------------------------------------------------------------------------
// Sentence dispatcher
// ---------------------------------------------------------------------------

static void nmea_process_sentence(const char *sentence)
{
    if (!nmea_verify_checksum(sentence)) {
        ESP_LOGD(TAG, "Checksum fail: %s", sentence);
        return;
    }

    // Work on a mutable copy so nmea_split can null-terminate fields
    char buf[256];
    strncpy(buf, sentence, sizeof(buf) - 1);
    buf[sizeof(buf) - 1] = '\0';

    // Strip trailing whitespace / CR / LF
    int len = strlen(buf);
    while (len > 0 && (buf[len - 1] == '\r' || buf[len - 1] == '\n' || buf[len - 1] == ' ')) {
        buf[--len] = '\0';
    }

    char *fields[20];
    int nf = nmea_split(buf, fields, 20);
    if (nf < 1) return;

    if (strcmp(fields[0], "$GNRMC") == 0 || strcmp(fields[0], "$GPRMC") == 0) {
        process_gnrmc(fields, nf);

        // Fire callback after updating from RMC (position + validity are fresh)
        if (s_gps.cb) {
            hal_gps_position_t snap;
            portENTER_CRITICAL(&s_gps.spinlock);
            snap = s_gps.last_position;
            portEXIT_CRITICAL(&s_gps.spinlock);

            if (snap.fix_valid) {
                s_gps.cb(&snap, s_gps.cb_data);
            }
        }
    } else if (strcmp(fields[0], "$GNGGA") == 0 || strcmp(fields[0], "$GPGGA") == 0) {
        process_gngga(fields, nf);
    }
}

// ---------------------------------------------------------------------------
// UART RX task
// ---------------------------------------------------------------------------

static void gps_rx_task(void *arg)
{
    uint8_t byte;

    while (1) {
        int len = uart_read_bytes(s_gps.cfg.uart_num, &byte, 1, pdMS_TO_TICKS(100));
        if (len > 0) {
            if (byte == '$') {
                s_gps.nmea_idx = 0;
            }
            if (s_gps.nmea_idx < (int)(sizeof(s_gps.nmea_buf) - 1)) {
                s_gps.nmea_buf[s_gps.nmea_idx++] = (char)byte;
            }
            if (byte == '\n') {
                s_gps.nmea_buf[s_gps.nmea_idx] = '\0';
                nmea_process_sentence(s_gps.nmea_buf);
                s_gps.nmea_idx = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t mia_m10q_init(const void *config)
{
    if (!config) return ESP_ERR_INVALID_ARG;
    if (s_gps.initialized) return ESP_ERR_INVALID_STATE;

    memcpy(&s_gps.cfg, config, sizeof(s_gps.cfg));

    uint32_t baud = s_gps.cfg.baud_rate ? s_gps.cfg.baud_rate : 9600;

    uart_config_t uart_cfg = {
        .baud_rate  = (int)baud,
        .data_bits  = UART_DATA_8_BITS,
        .parity     = UART_PARITY_DISABLE,
        .stop_bits  = UART_STOP_BITS_1,
        .flow_ctrl  = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
    };

    esp_err_t ret = uart_param_config(s_gps.cfg.uart_num, &uart_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "uart_param_config failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ret = uart_set_pin(s_gps.cfg.uart_num,
                       s_gps.cfg.pin_tx, s_gps.cfg.pin_rx,
                       UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "uart_set_pin failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ret = uart_driver_install(s_gps.cfg.uart_num,
                              UART_RX_BUF_SIZE, 0,
                              0, NULL, 0);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "uart_driver_install failed: %s", esp_err_to_name(ret));
        return ret;
    }

    memset(&s_gps.last_position, 0, sizeof(s_gps.last_position));
    s_gps.nmea_idx    = 0;
    s_gps.rx_task     = NULL;
    s_gps.enabled     = false;
    s_gps.initialized = true;

    ESP_LOGI(TAG, "Initialized on UART%d @ %lu baud (TX=%d RX=%d)",
             s_gps.cfg.uart_num, (unsigned long)baud,
             s_gps.cfg.pin_tx, s_gps.cfg.pin_rx);

    return ESP_OK;
}

static void mia_m10q_deinit(void)
{
    if (!s_gps.initialized) return;

    if (s_gps.rx_task) {
        vTaskDelete(s_gps.rx_task);
        s_gps.rx_task = NULL;
    }

    uart_driver_delete(s_gps.cfg.uart_num);

    s_gps.initialized = false;
    s_gps.enabled     = false;

    ESP_LOGI(TAG, "Deinitialized");
}

static esp_err_t mia_m10q_enable(void)
{
    if (!s_gps.initialized) return ESP_ERR_INVALID_STATE;
    if (s_gps.enabled)      return ESP_OK;

    BaseType_t rc = xTaskCreate(gps_rx_task, "gps_rx",
                                RX_TASK_STACK_SIZE, NULL,
                                RX_TASK_PRIORITY, &s_gps.rx_task);
    if (rc != pdPASS) {
        ESP_LOGE(TAG, "Failed to create RX task");
        return ESP_ERR_NO_MEM;
    }

    s_gps.enabled = true;
    ESP_LOGI(TAG, "Enabled — RX task running");
    return ESP_OK;
}

static esp_err_t mia_m10q_disable(void)
{
    if (!s_gps.initialized) return ESP_ERR_INVALID_STATE;
    if (!s_gps.enabled)     return ESP_OK;

    if (s_gps.rx_task) {
        vTaskDelete(s_gps.rx_task);
        s_gps.rx_task = NULL;
    }

    s_gps.enabled = false;
    ESP_LOGI(TAG, "Disabled — RX task stopped");
    return ESP_OK;
}

static esp_err_t mia_m10q_get_position(hal_gps_position_t *pos)
{
    if (!pos) return ESP_ERR_INVALID_ARG;

    portENTER_CRITICAL(&s_gps.spinlock);
    *pos = s_gps.last_position;
    portEXIT_CRITICAL(&s_gps.spinlock);

    return pos->fix_valid ? ESP_OK : ESP_ERR_INVALID_STATE;
}

static esp_err_t mia_m10q_register_callback(hal_gps_cb_t cb, void *user_data)
{
    s_gps.cb      = cb;
    s_gps.cb_data = user_data;
    return ESP_OK;
}

static esp_err_t mia_m10q_sleep(bool enter)
{
    if (!s_gps.initialized) return ESP_ERR_INVALID_STATE;

    if (enter) {
        // Send UBX-RXM-PMREQ to put the receiver into backup mode
        int written = uart_write_bytes(s_gps.cfg.uart_num,
                                       (const char *)UBX_RXM_PMREQ_BACKUP,
                                       sizeof(UBX_RXM_PMREQ_BACKUP));
        if (written < 0) {
            ESP_LOGE(TAG, "Failed to send UBX-RXM-PMREQ");
            return ESP_FAIL;
        }
        ESP_LOGI(TAG, "Sent UBX-RXM-PMREQ — receiver entering backup mode");
    } else {
        // Wake: any UART activity wakes the MIA-M10Q from backup mode.
        // Send a single 0xFF byte as a wakeup pulse.
        const uint8_t wake = 0xFF;
        uart_write_bytes(s_gps.cfg.uart_num, (const char *)&wake, 1);
        // Give the receiver time to re-initialise
        vTaskDelay(pdMS_TO_TICKS(500));
        ESP_LOGI(TAG, "Sent wakeup byte — receiver resuming");
    }

    return ESP_OK;
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_gps_driver_t s_vtable = {
    .init              = mia_m10q_init,
    .deinit            = mia_m10q_deinit,
    .enable            = mia_m10q_enable,
    .disable           = mia_m10q_disable,
    .get_position      = mia_m10q_get_position,
    .register_callback = mia_m10q_register_callback,
    .sleep             = mia_m10q_sleep,
    .name              = "MIA-M10Q",
};

const hal_gps_driver_t *drv_gps_mia_m10q_get(void)
{
    return &s_vtable;
}
