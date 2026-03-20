#include "hal/sdcard_path.h"
#include "ui/theme.h"
#include "ui/statusbar.h"
#include "esp_log.h"
#include <dirent.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *TAG = "theme";

/* Default monochrome theme — optimized for e-paper (black on white) */
static theme_colors_t s_current_theme;

/* Name of the currently active theme */
static char s_current_theme_name[32] = "Default";

static void theme_colors_init_defaults(void)
{
    s_current_theme.primary        = lv_color_black();
    s_current_theme.secondary      = lv_color_black();
    s_current_theme.bg             = lv_color_white();
    s_current_theme.surface        = lv_color_white();
    s_current_theme.text           = lv_color_black();
    s_current_theme.text_secondary = lv_color_black();
    s_current_theme.radius         = 2;
    s_current_theme.padding        = 4;
}

/* LVGL theme + style storage (heap-allocated via lv_theme_create) */
static lv_theme_t  *s_theme;
static lv_style_t  s_style_btn;
static lv_style_t  s_style_label;
static lv_style_t  s_style_panel;

/* -------------------------------------------------------------------------
 * LVGL theme apply callback
 * Called by LVGL for every widget — we pattern-match on widget class.
 * ------------------------------------------------------------------------- */
static void theme_apply_cb(lv_theme_t *th, lv_obj_t *obj)
{
    (void)th;

    if (lv_obj_check_type(obj, &lv_button_class)) {
        lv_obj_add_style(obj, &s_style_btn, LV_PART_MAIN);
    } else if (lv_obj_check_type(obj, &lv_label_class)) {
        lv_obj_add_style(obj, &s_style_label, LV_PART_MAIN);
    } else if (lv_obj_check_type(obj, &lv_obj_class)) {
        /* Plain lv_obj acts as a panel/container */
        lv_obj_add_style(obj, &s_style_panel, LV_PART_MAIN);
    }
}

/* -------------------------------------------------------------------------
 * Minimal JSON helpers — no external library required
 * ------------------------------------------------------------------------- */

/* Parse "#RRGGBB" hex string to lv_color_t */
static lv_color_t parse_hex_color(const char *hex)
{
    if (!hex || hex[0] != '#' || strlen(hex) != 7) {
        return lv_color_black();
    }
    unsigned int r, g, b;
    sscanf(hex + 1, "%02x%02x%02x", &r, &g, &b);
    return lv_color_make(r, g, b);
}

/* Find a string value for a key in JSON text (simple, not a full parser) */
static bool json_get_string(const char *json, const char *key, char *out, size_t out_len)
{
    char search[64];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *pos = strstr(json, search);
    if (!pos) return false;

    pos = strchr(pos + strlen(search), '"');
    if (!pos) return false;
    pos++; /* skip opening quote */

    const char *end = strchr(pos, '"');
    if (!end) return false;

    size_t len = (size_t)(end - pos);
    if (len >= out_len) len = out_len - 1;
    memcpy(out, pos, len);
    out[len] = '\0';
    return true;
}

/* Find an integer value for a key */
static bool json_get_int(const char *json, const char *key, int *out)
{
    char search[64];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *pos = strstr(json, search);
    if (!pos) return false;

    pos = strchr(pos + strlen(search), ':');
    if (!pos) return false;
    pos++;
    while (*pos == ' ') pos++;

    *out = atoi(pos);
    return true;
}

/* -------------------------------------------------------------------------
 * Public API
 * ------------------------------------------------------------------------- */

esp_err_t theme_init(lv_display_t *disp)
{
    /* Button style: white fill, 1 px black border, black text */
    lv_style_init(&s_style_btn);
    lv_style_set_bg_color(&s_style_btn, lv_color_white());
    lv_style_set_bg_opa(&s_style_btn, LV_OPA_COVER);
    lv_style_set_border_color(&s_style_btn, lv_color_black());
    lv_style_set_border_width(&s_style_btn, 1);
    lv_style_set_text_color(&s_style_btn, lv_color_black());
    lv_style_set_radius(&s_style_btn, s_current_theme.radius);
    lv_style_set_pad_all(&s_style_btn, s_current_theme.padding);

    /* Label style: black text, transparent background */
    lv_style_init(&s_style_label);
    lv_style_set_text_color(&s_style_label, lv_color_black());
    lv_style_set_bg_opa(&s_style_label, LV_OPA_TRANSP);

    /* Panel/container style: white background, thin black border */
    lv_style_init(&s_style_panel);
    lv_style_set_bg_color(&s_style_panel, lv_color_white());
    lv_style_set_bg_opa(&s_style_panel, LV_OPA_COVER);
    lv_style_set_border_color(&s_style_panel, lv_color_black());
    lv_style_set_border_width(&s_style_panel, 1);
    lv_style_set_radius(&s_style_panel, 0);

    /* Initialize default colors */
    theme_colors_init_defaults();

    /* Reset name to Default */
    strncpy(s_current_theme_name, "Default", sizeof(s_current_theme_name) - 1);
    s_current_theme_name[sizeof(s_current_theme_name) - 1] = '\0';

    /* Create theme and wire into LVGL */
    s_theme = lv_theme_simple_init(disp);
    lv_theme_set_apply_cb(s_theme, theme_apply_cb);

    if (disp != NULL) {
        lv_display_set_theme(disp, s_theme);
    }

    ESP_LOGI(TAG, "default monochrome theme applied");
    return ESP_OK;
}

esp_err_t theme_load(const char *json_path)
{
    if (!json_path) return ESP_ERR_INVALID_ARG;

    /* Read the JSON file */
    FILE *f = fopen(json_path, "r");
    if (!f) {
        ESP_LOGE(TAG, "Cannot open theme: %s", json_path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size <= 0 || size > 4096) {
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    char *json = malloc(size + 1);
    if (!json) { fclose(f); return ESP_ERR_NO_MEM; }

    fread(json, 1, size, f);
    json[size] = '\0';
    fclose(f);

    /* Parse colors */
    char hex[8];
    if (json_get_string(json, "primary", hex, sizeof(hex)))
        s_current_theme.primary = parse_hex_color(hex);
    if (json_get_string(json, "secondary", hex, sizeof(hex)))
        s_current_theme.secondary = parse_hex_color(hex);
    if (json_get_string(json, "bg", hex, sizeof(hex)))
        s_current_theme.bg = parse_hex_color(hex);
    if (json_get_string(json, "surface", hex, sizeof(hex)))
        s_current_theme.surface = parse_hex_color(hex);
    if (json_get_string(json, "text", hex, sizeof(hex)))
        s_current_theme.text = parse_hex_color(hex);
    if (json_get_string(json, "text_secondary", hex, sizeof(hex)))
        s_current_theme.text_secondary = parse_hex_color(hex);

    /* Parse component properties */
    int val;
    if (json_get_int(json, "radius", &val))
        s_current_theme.radius = (uint8_t)val;
    if (json_get_int(json, "padding", &val))
        s_current_theme.padding = (uint8_t)val;

    free(json);

    /* Re-apply styles with new colors */
    lv_style_set_bg_color(&s_style_btn, s_current_theme.bg);
    lv_style_set_border_color(&s_style_btn, s_current_theme.text);
    lv_style_set_text_color(&s_style_btn, s_current_theme.text);
    lv_style_set_radius(&s_style_btn, s_current_theme.radius);
    lv_style_set_pad_all(&s_style_btn, s_current_theme.padding);

    lv_style_set_text_color(&s_style_label, s_current_theme.text);

    lv_style_set_bg_color(&s_style_panel, s_current_theme.bg);
    lv_style_set_border_color(&s_style_panel, s_current_theme.text);

    /* Force LVGL to re-render everything with new styles */
    lv_obj_report_style_change(NULL);

    /* Refresh status bar colors immediately */
    statusbar_refresh_theme();

    /* Track the active theme name (basename without path) */
    const char *basename = strrchr(json_path, '/');
    basename = basename ? basename + 1 : json_path;
    strncpy(s_current_theme_name, basename, sizeof(s_current_theme_name) - 1);
    s_current_theme_name[sizeof(s_current_theme_name) - 1] = '\0';

    ESP_LOGI(TAG, "Theme loaded: %s", json_path);
    return ESP_OK;
}

const char *theme_get_current_name(void)
{
    return s_current_theme_name;
}

const theme_colors_t *theme_get_colors(void)
{
    return &s_current_theme;
}

esp_err_t theme_apply(lv_display_t *disp)
{
    if (disp == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    lv_display_set_theme(disp, s_theme);
    ESP_LOGI(TAG, "theme applied to display");
    return ESP_OK;
}

int theme_list_available(char names[][32], int max_count)
{
    if (!names || max_count <= 0) return 0;

    DIR *dir = opendir(THISTLE_SDCARD "/themes");
    if (!dir) {
        ESP_LOGW(TAG, "Cannot open /sdcard/themes (SD card not mounted?)");
        return 0;
    }

    int count = 0;
    struct dirent *entry;
    while (count < max_count && (entry = readdir(dir)) != NULL) {
        /* Only accept regular files ending in .json */
        if (entry->d_type != DT_REG) continue;

        const char *name = entry->d_name;
        size_t len = strlen(name);
        if (len < 5) continue; /* shorter than "a.json" — skip */
        if (strcmp(name + len - 5, ".json") != 0) continue;

        strncpy(names[count], name, 31);
        names[count][31] = '\0';
        count++;
    }

    closedir(dir);
    return count;
}
