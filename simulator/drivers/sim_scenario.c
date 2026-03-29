/*
 * Simulator scenario engine — loads a JSON file and configures fake HAL
 * driver initial state.  Uses a minimal key-search parser (no JSON library).
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "sim_scenario.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_FILE_SIZE 4096

/* ---- internal state ---------------------------------------------------- */

typedef struct {
    /* power */
    uint16_t power_voltage_mv;
    uint8_t  power_percent;
    int      power_state;          /* 0=discharging 1=charging 2=charged 3=no_battery */

    /* gps */
    double   gps_lat;
    double   gps_lon;
    float    gps_alt;
    uint8_t  gps_sats;
    bool     gps_fix;

    /* imu */
    float    imu_accel[3];
    float    imu_gyro[3];
} scenario_t;

static scenario_t s_scenario = {
    /* defaults */
    .power_voltage_mv = 3850,
    .power_percent    = 72,
    .power_state      = 0,           /* discharging */

    .gps_lat  = 37.7749,
    .gps_lon  = -122.4194,
    .gps_alt  = 15.0f,
    .gps_sats = 10,
    .gps_fix  = true,

    .imu_accel = { 0.0f, 0.0f, 9.81f },
    .imu_gyro  = { 0.0f, 0.0f, 0.0f  },
};

/* ---- tiny helpers ------------------------------------------------------ */

/* Find the value portion after "key": in buf.  Returns pointer to the first
 * non-whitespace character after the colon, or NULL if not found. */
static const char *find_key_value(const char *buf, const char *key)
{
    char pattern[128];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(buf, pattern);
    if (!p) return NULL;
    p += strlen(pattern);
    /* skip whitespace and colon */
    while (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') p++;
    if (*p != ':') return NULL;
    p++;
    while (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') p++;
    return p;
}

/* Parse a power state string into an int.  Expects p to point at the
 * opening quote of the string value. */
static int parse_power_state(const char *p)
{
    if (!p || *p != '"') return 0;
    p++; /* skip opening quote */
    if (strncmp(p, "charging",    8) == 0) return 1;
    if (strncmp(p, "charged",     7) == 0) return 2;
    if (strncmp(p, "no_battery", 10) == 0) return 3;
    return 0; /* discharging or unknown */
}

/* Parse an array of 3 doubles from "[n, n, n]" into out[3]. */
static void parse_float3(const char *p, float out[3])
{
    if (!p || *p != '[') return;
    p++; /* skip '[' */
    for (int i = 0; i < 3; i++) {
        char *end;
        out[i] = (float)strtod(p, &end);
        p = end;
        /* skip comma / whitespace */
        while (*p == ',' || *p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') p++;
    }
}

/* ---- public API -------------------------------------------------------- */

int sim_scenario_load(const char *path)
{
    if (!path) return 0;

    FILE *f = fopen(path, "r");
    if (!f) {
        fprintf(stderr, "scenario: cannot open %s\n", path);
        return -1;
    }

    char buf[MAX_FILE_SIZE];
    size_t n = fread(buf, 1, sizeof(buf) - 1, f);
    fclose(f);
    buf[n] = '\0';

    const char *v;

    /* power.voltage_mv */
    v = find_key_value(buf, "voltage_mv");
    if (v) s_scenario.power_voltage_mv = (uint16_t)strtol(v, NULL, 10);

    /* power.percent */
    v = find_key_value(buf, "percent");
    if (v) s_scenario.power_percent = (uint8_t)strtol(v, NULL, 10);

    /* power.state */
    v = find_key_value(buf, "state");
    if (v) s_scenario.power_state = parse_power_state(v);

    /* gps.latitude */
    v = find_key_value(buf, "latitude");
    if (v) s_scenario.gps_lat = strtod(v, NULL);

    /* gps.longitude */
    v = find_key_value(buf, "longitude");
    if (v) s_scenario.gps_lon = strtod(v, NULL);

    /* gps.altitude_m */
    v = find_key_value(buf, "altitude_m");
    if (v) s_scenario.gps_alt = (float)strtod(v, NULL);

    /* gps.satellites */
    v = find_key_value(buf, "satellites");
    if (v) s_scenario.gps_sats = (uint8_t)strtol(v, NULL, 10);

    /* gps.fix_valid */
    v = find_key_value(buf, "fix_valid");
    if (v) s_scenario.gps_fix = (strncmp(v, "true", 4) == 0);

    /* imu.accel */
    v = find_key_value(buf, "accel");
    if (v) parse_float3(v, s_scenario.imu_accel);

    /* imu.gyro */
    v = find_key_value(buf, "gyro");
    if (v) parse_float3(v, s_scenario.imu_gyro);

    {
        char _msg[128];
        snprintf(_msg, sizeof(_msg), "Scenario loaded: %s", path);
        printf("%s\n", _msg);
        extern void sim_assert_check_line(const char *line);
        sim_assert_check_line(_msg);
    }
    return 0;
}

void sim_scenario_get_power(uint16_t *voltage_mv, uint8_t *percent, int *state)
{
    if (voltage_mv) *voltage_mv = s_scenario.power_voltage_mv;
    if (percent)    *percent    = s_scenario.power_percent;
    if (state)      *state      = s_scenario.power_state;
}

void sim_scenario_get_gps(double *lat, double *lon, float *alt, uint8_t *sats, bool *fix)
{
    if (lat)  *lat  = s_scenario.gps_lat;
    if (lon)  *lon  = s_scenario.gps_lon;
    if (alt)  *alt  = s_scenario.gps_alt;
    if (sats) *sats = s_scenario.gps_sats;
    if (fix)  *fix  = s_scenario.gps_fix;
}

void sim_scenario_get_imu(float accel[3], float gyro[3])
{
    if (accel) memcpy(accel, s_scenario.imu_accel, sizeof(s_scenario.imu_accel));
    if (gyro)  memcpy(gyro,  s_scenario.imu_gyro,  sizeof(s_scenario.imu_gyro));
}
