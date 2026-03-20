/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — App Store UI
 *
 * Two tabs:
 *   Catalog  — browse and install apps from local catalog.json on SD card
 *   Installed — manage apps already in /sdcard/apps/
 *
 * Catalog is read from THISTLE_SDCARD/appstore/catalog.json.
 * Download is an MVP stub — shows a toast when connectivity is required.
 */
#include "appstore/appstore_app.h"

#include "hal/sdcard_path.h"
#include "ui/theme.h"
#include "ui/toast.h"
#include "thistle/app_manager.h"

#include "lvgl.h"
#include "esp_log.h"
#include "esp_err.h"

#include <dirent.h>
#include <sys/stat.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <stdint.h>

static const char *TAG = "appstore_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W      320
#define APP_AREA_H      216
#define TITLE_BAR_H      30
#define ITEM_H           40   /* taller rows — name + description */
#define ITEM_H_INST      30
#define ITEM_PAD_LEFT     8
#define ITEM_PAD_RIGHT    6
#define STATUS_BAR_H     20
#define CONTENT_H       (APP_AREA_H - TITLE_BAR_H - STATUS_BAR_H)

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

#define MAX_CATALOG_APPS 20
#define APPS_DIR         THISTLE_SDCARD "/apps"
#define CATALOG_PATH     THISTLE_SDCARD "/appstore/catalog.json"

typedef struct {
    char     id[64];
    char     name[32];
    char     version[16];
    char     author[32];
    char     description[128];
    char     permissions[64];   /* comma-separated list from JSON */
    uint32_t size_kb;
    bool     is_signed;
    bool     is_installed;      /* true if APPS_DIR/<id>.app.elf exists */
    char     download_url[128];
} appstore_entry_t;

/* Installed-app record (scanned from /sdcard/apps/) */
#define MAX_INSTALLED_APPS 20

typedef struct {
    char     filename[64];      /* e.g. "com.meshcore.chat.app.elf" */
    char     display_name[48];  /* derived from filename */
    uint32_t size_kb;
} installed_entry_t;

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

typedef enum {
    TAB_CATALOG,
    TAB_INSTALLED,
} appstore_tab_t;

static struct {
    lv_obj_t *root;

    /* Catalog tab */
    lv_obj_t *catalog_screen;
    lv_obj_t *catalog_list;
    lv_obj_t *catalog_status_label;
    lv_obj_t *cat_tab_btn;
    lv_obj_t *inst_tab_btn;

    /* Detail sub-screen */
    lv_obj_t *detail_screen;

    /* Installed tab */
    lv_obj_t *installed_screen;
    lv_obj_t *installed_list;
    lv_obj_t *installed_status_label;

    /* Data */
    appstore_entry_t  apps[MAX_CATALOG_APPS];
    int               app_count;
    int               selected_idx;

    installed_entry_t installed[MAX_INSTALLED_APPS];
    int               installed_count;

    appstore_tab_t    current_tab;
} s_store;

/* ------------------------------------------------------------------ */
/* Style helpers                                                        */
/* ------------------------------------------------------------------ */

static void style_panel(lv_obj_t *obj)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_set_style_bg_color(obj, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(obj, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(obj, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(obj, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(obj, 0, LV_PART_MAIN);
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLLABLE);
}

static void style_title_bar(lv_obj_t *obj)
{
    style_panel(obj);
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_set_style_pad_left(obj, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(obj, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_border_side(obj, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(obj, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(obj, 1, LV_PART_MAIN);
}

static lv_obj_t *create_separator(lv_obj_t *parent)
{
    const theme_colors_t *tc = theme_get_colors();
    lv_obj_t *sep = lv_obj_create(parent);
    lv_obj_set_size(sep, LV_PCT(100), 1);
    lv_obj_set_style_bg_color(sep, tc->text, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(sep, LV_OPA_50, LV_PART_MAIN);
    lv_obj_set_style_border_width(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(sep, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(sep, 0, LV_PART_MAIN);
    lv_obj_clear_flag(sep, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);
    return sep;
}

/* Small tab-like button */
static lv_obj_t *create_tab_button(lv_obj_t *parent, const char *label, bool active)
{
    const theme_colors_t *tc = theme_get_colors();

    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_size(btn, 90, 22);
    lv_obj_set_style_bg_color(btn, active ? tc->primary : tc->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_shadow_width(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn, 2, LV_PART_MAIN);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, active ? lv_color_white() : tc->text, LV_PART_MAIN);
    lv_obj_center(lbl);

    return btn;
}

/* Standard action button used on detail screen */
static lv_obj_t *create_action_button(lv_obj_t *parent, const char *label, int width)
{
    const theme_colors_t *tc = theme_get_colors();

    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_size(btn, width, 26);
    lv_obj_set_style_bg_color(btn, tc->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_shadow_width(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn, 4, LV_PART_MAIN);

    lv_obj_set_style_bg_color(btn, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_center(lbl);

    return btn;
}

/* ------------------------------------------------------------------ */
/* Minimal JSON parser helpers (strstr-based, same approach as theme.c)*/
/* ------------------------------------------------------------------ */

/* Extract string value for key from json text, writing into out[out_len]. */
static bool json_get_string(const char *json, const char *key, char *out, size_t out_len)
{
    char search[80];
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

/* Extract integer value for key. */
static bool json_get_int(const char *json, const char *key, int *out)
{
    char search[80];
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

/* Extract boolean value ("true"/"false") for key. */
static bool json_get_bool(const char *json, const char *key, bool *out)
{
    char search[80];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *pos = strstr(json, search);
    if (!pos) return false;

    pos = strchr(pos + strlen(search), ':');
    if (!pos) return false;
    pos++;
    while (*pos == ' ') pos++;

    if (strncmp(pos, "true", 4) == 0) {
        *out = true;
        return true;
    }
    if (strncmp(pos, "false", 5) == 0) {
        *out = false;
        return true;
    }
    return false;
}

/*
 * Extract a JSON array of strings for key into out_buf (comma-joined).
 * Works for simple arrays like ["radio", "gps"] and [].
 */
static void json_get_string_array(const char *json, const char *key,
                                  char *out_buf, size_t out_len)
{
    out_buf[0] = '\0';

    char search[80];
    snprintf(search, sizeof(search), "\"%s\"", key);
    const char *pos = strstr(json, search);
    if (!pos) return;

    pos = strchr(pos + strlen(search), '[');
    if (!pos) return;
    pos++; /* skip '[' */

    /* Collect comma-joined strings until ']' */
    size_t written = 0;
    while (*pos && *pos != ']') {
        /* Skip whitespace */
        while (*pos == ' ' || *pos == '\n' || *pos == '\r' || *pos == '\t') pos++;
        if (*pos == ']') break;
        if (*pos == '"') {
            pos++; /* skip quote */
            const char *end = strchr(pos, '"');
            if (!end) break;

            if (written > 0 && written < out_len - 1) {
                out_buf[written++] = ',';
            }
            size_t len = (size_t)(end - pos);
            if (written + len >= out_len) len = out_len - written - 1;
            memcpy(out_buf + written, pos, len);
            written += len;
            out_buf[written] = '\0';
            pos = end + 1; /* skip closing quote */
        } else {
            pos++;
        }
    }
}

/* ------------------------------------------------------------------ */
/* Installed-app check                                                  */
/* ------------------------------------------------------------------ */

static bool app_is_installed(const char *app_id)
{
    char path[256];
    snprintf(path, sizeof(path), "%s/%s.app.elf", APPS_DIR, app_id);
    struct stat st;
    return (stat(path, &st) == 0);
}

/* Ensure the /sdcard/apps/ directory exists */
static void ensure_apps_dir(void)
{
    struct stat st;
    if (stat(APPS_DIR, &st) != 0) {
        if (mkdir(APPS_DIR, 0755) != 0) {
            ESP_LOGW(TAG, "mkdir %s failed", APPS_DIR);
        } else {
            ESP_LOGI(TAG, "Created %s", APPS_DIR);
        }
    }
}

/* ------------------------------------------------------------------ */
/* Catalog loading                                                      */
/* ------------------------------------------------------------------ */

static esp_err_t load_catalog(void)
{
    s_store.app_count = 0;

    FILE *f = fopen(CATALOG_PATH, "r");
    if (!f) {
        ESP_LOGW(TAG, "No catalog file at %s — place one on SD card", CATALOG_PATH);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long fsize = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (fsize <= 0) {
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    char *buf = (char *)malloc((size_t)fsize + 1);
    if (!buf) {
        fclose(f);
        ESP_LOGE(TAG, "OOM loading catalog");
        return ESP_ERR_NO_MEM;
    }

    size_t got = fread(buf, 1, (size_t)fsize, f);
    buf[got] = '\0';
    fclose(f);

    /*
     * Split entries on object boundaries.  Each catalog entry is a JSON
     * object { ... }.  We find the start of each '{' and extract fields
     * using json_get_string/int/bool within the object text.
     */
    const char *p = buf;
    while (*p && s_store.app_count < MAX_CATALOG_APPS) {
        /* Find next object open brace */
        p = strchr(p, '{');
        if (!p) break;

        /* Find matching closing brace */
        const char *end = strchr(p, '}');
        if (!end) break;

        /* Null-terminate a temporary copy of this object */
        size_t obj_len = (size_t)(end - p) + 1;
        char *obj = (char *)malloc(obj_len + 1);
        if (!obj) break;
        memcpy(obj, p, obj_len);
        obj[obj_len] = '\0';

        appstore_entry_t *e = &s_store.apps[s_store.app_count];
        memset(e, 0, sizeof(*e));

        int size_val = 0;
        json_get_string(obj, "id",          e->id,           sizeof(e->id));
        json_get_string(obj, "name",        e->name,         sizeof(e->name));
        json_get_string(obj, "version",     e->version,      sizeof(e->version));
        json_get_string(obj, "author",      e->author,       sizeof(e->author));
        json_get_string(obj, "description", e->description,  sizeof(e->description));
        json_get_string(obj, "download_url",e->download_url, sizeof(e->download_url));
        json_get_string_array(obj, "permissions", e->permissions, sizeof(e->permissions));
        json_get_int(obj,  "size_kb",    &size_val);
        json_get_bool(obj, "signed",     &e->is_signed);

        e->size_kb = (uint32_t)size_val;
        e->is_installed = (e->id[0] != '\0') && app_is_installed(e->id);

        if (e->id[0] != '\0' && e->name[0] != '\0') {
            s_store.app_count++;
        }

        free(obj);
        p = end + 1;
    }

    free(buf);

    ESP_LOGI(TAG, "Catalog loaded: %d apps", s_store.app_count);
    return ESP_OK;
}

/* ------------------------------------------------------------------ */
/* Installed apps scan                                                  */
/* ------------------------------------------------------------------ */

static void scan_installed_apps(void)
{
    s_store.installed_count = 0;

    DIR *d = opendir(APPS_DIR);
    if (!d) {
        ESP_LOGW(TAG, "Cannot open %s", APPS_DIR);
        return;
    }

    struct dirent *ent;
    while ((ent = readdir(d)) != NULL && s_store.installed_count < MAX_INSTALLED_APPS) {
        if (ent->d_type != DT_REG && ent->d_type != DT_UNKNOWN) continue;

        /* Only *.app.elf files */
        const char *dot = strstr(ent->d_name, ".app.elf");
        if (!dot) continue;

        installed_entry_t *inst = &s_store.installed[s_store.installed_count];
        strncpy(inst->filename, ent->d_name, sizeof(inst->filename) - 1);
        inst->filename[sizeof(inst->filename) - 1] = '\0';

        /* Display name: strip ".app.elf" suffix */
        strncpy(inst->display_name, ent->d_name, sizeof(inst->display_name) - 1);
        inst->display_name[sizeof(inst->display_name) - 1] = '\0';
        char *suffix = strstr(inst->display_name, ".app.elf");
        if (suffix) *suffix = '\0';

        /* Get file size */
        char full_path[512];
        snprintf(full_path, sizeof(full_path), "%s/%s", APPS_DIR, ent->d_name);
        struct stat st;
        inst->size_kb = 0;
        if (stat(full_path, &st) == 0) {
            inst->size_kb = (uint32_t)(st.st_size / 1024);
        }

        s_store.installed_count++;
    }
    closedir(d);

    ESP_LOGI(TAG, "Found %d installed apps", s_store.installed_count);
}

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void show_catalog(void);
static void show_installed(void);
static void show_detail(int idx);
static void build_catalog_list(void);
static void build_installed_list(void);

/* ------------------------------------------------------------------ */
/* Tab-switching callbacks                                              */
/* ------------------------------------------------------------------ */

static void tab_catalog_cb(lv_event_t *e)
{
    (void)e;
    if (s_store.current_tab == TAB_CATALOG) return;
    show_catalog();
}

static void tab_installed_cb(lv_event_t *e)
{
    (void)e;
    if (s_store.current_tab == TAB_INSTALLED) return;
    show_installed();
}

/* ------------------------------------------------------------------ */
/* Catalog detail screen                                               */
/* ------------------------------------------------------------------ */

static void back_to_catalog_cb(lv_event_t *e)
{
    (void)e;
    if (s_store.detail_screen) {
        lv_obj_delete(s_store.detail_screen);
        s_store.detail_screen = NULL;
    }
    if (s_store.current_tab == TAB_CATALOG && s_store.catalog_screen) {
        lv_obj_remove_flag(s_store.catalog_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_store.current_tab == TAB_INSTALLED && s_store.installed_screen) {
        lv_obj_remove_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);
    }
}

static void install_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_store.selected_idx;
    if (idx < 0 || idx >= s_store.app_count) return;

    appstore_entry_t *app = &s_store.apps[idx];

    /* Check if already installed */
    app->is_installed = app_is_installed(app->id);
    if (app->is_installed) {
        toast_info("Already installed");
        return;
    }

    /* MVP: network download not yet implemented */
    toast_warn("Download requires network connection");
    ESP_LOGI(TAG, "Would download: %s", app->download_url);
}

static void remove_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_store.selected_idx;
    if (idx < 0 || idx >= s_store.app_count) return;

    appstore_entry_t *app = &s_store.apps[idx];

    char path[256];
    snprintf(path, sizeof(path), "%s/%s.app.elf", APPS_DIR, app->id);

    if (remove(path) == 0) {
        app->is_installed = false;
        toast_show("App removed", TOAST_SUCCESS, 2000);
        ESP_LOGI(TAG, "Removed: %s", path);
        /* Go back and refresh the catalog list */
        back_to_catalog_cb(NULL);
        build_catalog_list();
    } else {
        toast_warn("Remove failed");
        ESP_LOGW(TAG, "Failed to remove: %s", path);
    }
}

static void remove_installed_btn_cb(lv_event_t *e)
{
    lv_obj_t *btn  = lv_event_get_target(e);
    const char *fn = (const char *)lv_obj_get_user_data(btn);
    if (!fn) return;

    char path[256];
    snprintf(path, sizeof(path), "%s/%s", APPS_DIR, fn);

    if (remove(path) == 0) {
        toast_show("App removed", TOAST_SUCCESS, 2000);
        ESP_LOGI(TAG, "Removed: %s", path);
        scan_installed_apps();
        build_installed_list();
    } else {
        toast_warn("Remove failed");
    }
}

static void show_detail(int idx)
{
    if (idx < 0 || idx >= s_store.app_count) return;

    s_store.selected_idx = idx;
    const appstore_entry_t *app = &s_store.apps[idx];

    const theme_colors_t *tc = theme_get_colors();

    /* Hide the current tab screen */
    if (s_store.catalog_screen)  lv_obj_add_flag(s_store.catalog_screen,  LV_OBJ_FLAG_HIDDEN);
    if (s_store.installed_screen) lv_obj_add_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);

    /* Build detail screen */
    lv_obj_t *screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(screen, 0, 0);
    lv_obj_set_size(screen, APP_AREA_W, APP_AREA_H);
    style_panel(screen);
    s_store.detail_screen = screen;

    /* Title bar: "< Back" */
    lv_obj_t *title_bar = lv_obj_create(screen);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    style_title_bar(title_bar);
    lv_obj_add_flag(title_bar, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_bg_color(title_bar, tc->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(title_bar, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_event_cb(title_bar, back_to_catalog_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *back_lbl = lv_label_create(title_bar);
    lv_label_set_text(back_lbl, "< Back");
    lv_obj_set_style_text_font(back_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(back_lbl, tc->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(back_lbl, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_align(back_lbl, LV_ALIGN_LEFT_MID, 0, 0);

    /* Scrollable content area */
    lv_obj_t *content = lv_obj_create(screen);
    lv_obj_set_pos(content, 0, TITLE_BAR_H);
    lv_obj_set_size(content, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(content, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(content, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(content, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(content, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(content, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(content, 6, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(content, 6, LV_PART_MAIN);
    lv_obj_set_style_radius(content, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(content, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(content, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_style_pad_row(content, 4, LV_PART_MAIN);
    lv_obj_add_flag(content, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_scroll_dir(content, LV_DIR_VER);

    /* App name (larger font) */
    lv_obj_t *name_lbl = lv_label_create(content);
    lv_label_set_text(name_lbl, app->name);
    lv_obj_set_width(name_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);

    create_separator(content);

    /* Meta info rows */
    char line[160];

    snprintf(line, sizeof(line), "Version: %s", app->version);
    lv_obj_t *ver_lbl = lv_label_create(content);
    lv_label_set_text(ver_lbl, line);
    lv_obj_set_width(ver_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(ver_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(ver_lbl, tc->text, LV_PART_MAIN);

    snprintf(line, sizeof(line), "Author: %s", app->author);
    lv_obj_t *auth_lbl = lv_label_create(content);
    lv_label_set_text(auth_lbl, line);
    lv_obj_set_width(auth_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(auth_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(auth_lbl, tc->text, LV_PART_MAIN);

    snprintf(line, sizeof(line), "Size: %u KB", (unsigned)app->size_kb);
    lv_obj_t *size_lbl = lv_label_create(content);
    lv_label_set_text(size_lbl, line);
    lv_obj_set_width(size_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(size_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(size_lbl, tc->text, LV_PART_MAIN);

    if (app->permissions[0] != '\0') {
        snprintf(line, sizeof(line), "Permissions: %s", app->permissions);
    } else {
        snprintf(line, sizeof(line), "Permissions: none");
    }
    lv_obj_t *perm_lbl = lv_label_create(content);
    lv_label_set_text(perm_lbl, line);
    lv_obj_set_width(perm_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(perm_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(perm_lbl, tc->text, LV_PART_MAIN);

    snprintf(line, sizeof(line), "Signed: %s", app->is_signed ? "Yes" : "No");
    lv_obj_t *sign_lbl = lv_label_create(content);
    lv_label_set_text(sign_lbl, line);
    lv_obj_set_width(sign_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(sign_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(sign_lbl, tc->text, LV_PART_MAIN);

    create_separator(content);

    /* Description */
    lv_obj_t *desc_lbl = lv_label_create(content);
    lv_label_set_text(desc_lbl, app->description);
    lv_obj_set_width(desc_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(desc_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(desc_lbl, tc->text_secondary, LV_PART_MAIN);
    lv_label_set_long_mode(desc_lbl, LV_LABEL_LONG_WRAP);

    create_separator(content);

    /* Action buttons row */
    lv_obj_t *btn_row = lv_obj_create(content);
    lv_obj_set_size(btn_row, LV_PCT(100), 34);
    lv_obj_set_style_bg_opa(btn_row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(btn_row, LV_OBJ_FLAG_SCROLLABLE);

    /* Re-check installed state */
    bool installed = app_is_installed(app->id);

    lv_obj_t *install_btn = create_action_button(btn_row, "Install", 100);
    lv_obj_align(install_btn, LV_ALIGN_LEFT_MID, 0, 0);
    lv_obj_add_event_cb(install_btn, install_btn_cb, LV_EVENT_CLICKED, NULL);

    if (installed) {
        lv_obj_t *remove_btn = create_action_button(btn_row, "Remove", 100);
        lv_obj_align(remove_btn, LV_ALIGN_LEFT_MID, 110, 0);
        lv_obj_add_event_cb(remove_btn, remove_btn_cb, LV_EVENT_CLICKED, NULL);
    }
}

/* ------------------------------------------------------------------ */
/* Catalog row click                                                    */
/* ------------------------------------------------------------------ */

static void catalog_row_clicked_cb(lv_event_t *e)
{
    lv_obj_t *row = lv_event_get_target(e);
    intptr_t idx = (intptr_t)lv_obj_get_user_data(row);
    show_detail((int)idx);
}

/* ------------------------------------------------------------------ */
/* Build catalog list                                                   */
/* ------------------------------------------------------------------ */

static void build_catalog_list(void)
{
    if (!s_store.catalog_list) return;

    lv_obj_clean(s_store.catalog_list);

    const theme_colors_t *tc = theme_get_colors();

    if (s_store.app_count == 0) {
        lv_obj_t *empty_lbl = lv_label_create(s_store.catalog_list);
        lv_label_set_text(empty_lbl, "No apps in catalog.\nPlace catalog.json on SD card.");
        lv_obj_set_style_text_font(empty_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(empty_lbl, tc->text, LV_PART_MAIN);
        lv_obj_set_style_pad_all(empty_lbl, 8, LV_PART_MAIN);
        lv_label_set_long_mode(empty_lbl, LV_LABEL_LONG_WRAP);
        lv_obj_set_width(empty_lbl, APP_AREA_W - 16);
        return;
    }

    for (int i = 0; i < s_store.app_count; i++) {
        const appstore_entry_t *app = &s_store.apps[i];

        /* Row container */
        lv_obj_t *row = lv_obj_create(s_store.catalog_list);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H);
        lv_obj_set_style_bg_color(row, tc->bg, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_bg_color(row, tc->primary, LV_STATE_PRESSED);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 3, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 3, LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);

        /* Store index for click callback */
        lv_obj_set_user_data(row, (void *)(intptr_t)i);
        lv_obj_add_event_cb(row, catalog_row_clicked_cb, LV_EVENT_CLICKED, NULL);

        /* Top line: "> Name   v1.0.0" */
        char top_line[64];
        snprintf(top_line, sizeof(top_line), "> %s%s   %s",
                 app->name,
                 app->is_installed ? " [i]" : "",
                 app->version);

        /* Use flex column layout so name and description stack vertically */
        lv_obj_set_flex_flow(row, LV_FLEX_FLOW_COLUMN);
        lv_obj_set_style_pad_row(row, 2, LV_PART_MAIN);

        lv_obj_t *name_lbl = lv_label_create(row);
        lv_label_set_text(name_lbl, top_line);
        lv_obj_set_width(name_lbl, LV_PCT(100));
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);
        lv_label_set_long_mode(name_lbl, LV_LABEL_LONG_DOT);

        lv_obj_t *desc_lbl = lv_label_create(row);
        lv_label_set_text(desc_lbl, app->description);
        lv_obj_set_width(desc_lbl, LV_PCT(100));
        lv_obj_set_style_text_font(desc_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(desc_lbl, tc->text_secondary, LV_PART_MAIN);
        lv_label_set_long_mode(desc_lbl, LV_LABEL_LONG_DOT);

        create_separator(s_store.catalog_list);
    }

    /* Update status label */
    if (s_store.catalog_status_label) {
        char status[64];
        snprintf(status, sizeof(status), "Catalog loaded (%d apps)", s_store.app_count);
        lv_label_set_text(s_store.catalog_status_label, status);
    }
}

/* ------------------------------------------------------------------ */
/* Refresh catalog callback                                             */
/* ------------------------------------------------------------------ */

static void refresh_catalog_cb(lv_event_t *e)
{
    (void)e;

    if (s_store.catalog_status_label) {
        lv_label_set_text(s_store.catalog_status_label, "Loading...");
    }

    esp_err_t err = load_catalog();
    build_catalog_list();

    if (err == ESP_ERR_NOT_FOUND) {
        if (s_store.catalog_status_label) {
            lv_label_set_text(s_store.catalog_status_label, "catalog.json not found on SD");
        }
        toast_warn("catalog.json missing from SD card");
    } else if (err != ESP_OK) {
        if (s_store.catalog_status_label) {
            lv_label_set_text(s_store.catalog_status_label, "Catalog load error");
        }
        toast_warn("Failed to load catalog");
    }
}

/* ------------------------------------------------------------------ */
/* Build installed list                                                 */
/* ------------------------------------------------------------------ */

static void build_installed_list(void)
{
    if (!s_store.installed_list) return;

    lv_obj_clean(s_store.installed_list);

    const theme_colors_t *tc = theme_get_colors();

    if (s_store.installed_count == 0) {
        lv_obj_t *empty_lbl = lv_label_create(s_store.installed_list);
        lv_label_set_text(empty_lbl, "No apps installed.\nBrowse the Catalog tab.");
        lv_obj_set_style_text_font(empty_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(empty_lbl, tc->text, LV_PART_MAIN);
        lv_obj_set_style_pad_all(empty_lbl, 8, LV_PART_MAIN);
        lv_label_set_long_mode(empty_lbl, LV_LABEL_LONG_WRAP);
        lv_obj_set_width(empty_lbl, APP_AREA_W - 16);
        return;
    }

    for (int i = 0; i < s_store.installed_count; i++) {
        const installed_entry_t *inst = &s_store.installed[i];

        lv_obj_t *row = lv_obj_create(s_store.installed_list);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H_INST);
        lv_obj_set_style_bg_color(row, tc->bg, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_width(row, 0, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);

        /* Name + size */
        char info[80];
        if (inst->size_kb > 0) {
            snprintf(info, sizeof(info), "%s  (%u KB)", inst->display_name, (unsigned)inst->size_kb);
        } else {
            snprintf(info, sizeof(info), "%s", inst->display_name);
        }

        lv_obj_t *name_lbl = lv_label_create(row);
        lv_label_set_text(name_lbl, info);
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);
        lv_obj_align(name_lbl, LV_ALIGN_LEFT_MID, 0, 0);
        lv_label_set_long_mode(name_lbl, LV_LABEL_LONG_DOT);
        lv_obj_set_width(name_lbl, APP_AREA_W - ITEM_PAD_LEFT - ITEM_PAD_RIGHT - 70);

        /* Remove button — stores filename as user_data */
        lv_obj_t *rem_btn = create_action_button(row, "Remove", 65);
        lv_obj_align(rem_btn, LV_ALIGN_RIGHT_MID, 0, 0);
        lv_obj_set_user_data(rem_btn, (void *)inst->filename);
        lv_obj_add_event_cb(rem_btn, remove_installed_btn_cb, LV_EVENT_CLICKED, NULL);

        create_separator(s_store.installed_list);
    }

    if (s_store.installed_status_label) {
        char status[64];
        snprintf(status, sizeof(status), "%d app(s) installed", s_store.installed_count);
        lv_label_set_text(s_store.installed_status_label, status);
    }
}

/* ------------------------------------------------------------------ */
/* Show catalog tab                                                     */
/* ------------------------------------------------------------------ */

static void update_tab_button_styles(void)
{
    const theme_colors_t *tc = theme_get_colors();

    if (s_store.cat_tab_btn) {
        bool active = (s_store.current_tab == TAB_CATALOG);
        lv_obj_set_style_bg_color(s_store.cat_tab_btn, active ? tc->primary : tc->surface, LV_PART_MAIN);
        /* Update child label color */
        lv_obj_t *lbl = lv_obj_get_child(s_store.cat_tab_btn, 0);
        if (lbl) lv_obj_set_style_text_color(lbl, active ? lv_color_white() : tc->text, LV_PART_MAIN);
    }
    if (s_store.inst_tab_btn) {
        bool active = (s_store.current_tab == TAB_INSTALLED);
        lv_obj_set_style_bg_color(s_store.inst_tab_btn, active ? tc->primary : tc->surface, LV_PART_MAIN);
        lv_obj_t *lbl = lv_obj_get_child(s_store.inst_tab_btn, 0);
        if (lbl) lv_obj_set_style_text_color(lbl, active ? lv_color_white() : tc->text, LV_PART_MAIN);
    }
}

static void show_catalog(void)
{
    s_store.current_tab = TAB_CATALOG;
    update_tab_button_styles();

    if (s_store.installed_screen) {
        lv_obj_add_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_store.catalog_screen) {
        lv_obj_remove_flag(s_store.catalog_screen, LV_OBJ_FLAG_HIDDEN);
    }
}

static void show_installed(void)
{
    s_store.current_tab = TAB_INSTALLED;
    update_tab_button_styles();

    if (s_store.catalog_screen) {
        lv_obj_add_flag(s_store.catalog_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_store.installed_screen) {
        lv_obj_remove_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);
    }

    scan_installed_apps();
    build_installed_list();
}

/* ------------------------------------------------------------------ */
/* Public API — build UI once                                           */
/* ------------------------------------------------------------------ */

esp_err_t appstore_ui_create(lv_obj_t *parent)
{
    if (s_store.root) {
        ESP_LOGW(TAG, "UI already created");
        return ESP_OK;
    }

    memset(&s_store, 0, sizeof(s_store));
    s_store.selected_idx = -1;
    s_store.current_tab  = TAB_CATALOG;

    const theme_colors_t *tc = theme_get_colors();

    /* Root container — fills the entire app area */
    s_store.root = lv_obj_create(parent);
    lv_obj_set_size(s_store.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_store.root, 0, 0);
    style_panel(s_store.root);

    /* ----------------------------------------------------------------
     * Shared title bar across both tabs
     * ---------------------------------------------------------------- */
    lv_obj_t *title_bar = lv_obj_create(s_store.root);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    style_title_bar(title_bar);

    /* Title text */
    lv_obj_t *title_lbl = lv_label_create(title_bar);
    lv_label_set_text(title_lbl, "App Store");
    lv_obj_set_style_text_font(title_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(title_lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(title_lbl, LV_ALIGN_LEFT_MID, 0, 0);

    /* Tab buttons: "Catalog" and "Installed" on the right */
    lv_obj_t *cat_btn = create_tab_button(title_bar, "Catalog", true);
    lv_obj_align(cat_btn, LV_ALIGN_RIGHT_MID, -96, 0);
    lv_obj_add_event_cb(cat_btn, tab_catalog_cb, LV_EVENT_CLICKED, NULL);
    s_store.cat_tab_btn = cat_btn;

    lv_obj_t *inst_btn = create_tab_button(title_bar, "Installed", false);
    lv_obj_align(inst_btn, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_add_event_cb(inst_btn, tab_installed_cb, LV_EVENT_CLICKED, NULL);
    s_store.inst_tab_btn = inst_btn;

    /* ----------------------------------------------------------------
     * Catalog screen
     * ---------------------------------------------------------------- */
    lv_obj_t *cat_screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(cat_screen, 0, TITLE_BAR_H);
    lv_obj_set_size(cat_screen, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    style_panel(cat_screen);
    s_store.catalog_screen = cat_screen;

    /* Scrollable list area */
    lv_obj_t *cat_list = lv_obj_create(cat_screen);
    lv_obj_set_pos(cat_list, 0, 0);
    lv_obj_set_size(cat_list, APP_AREA_W, APP_AREA_H - TITLE_BAR_H - STATUS_BAR_H - 30);
    lv_obj_set_style_bg_color(cat_list, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(cat_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(cat_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(cat_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(cat_list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(cat_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(cat_list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_add_flag(cat_list, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_scroll_dir(cat_list, LV_DIR_VER);
    s_store.catalog_list = cat_list;

    /* Refresh button */
    lv_obj_t *refresh_btn_area = lv_obj_create(cat_screen);
    int refresh_y = APP_AREA_H - TITLE_BAR_H - STATUS_BAR_H - 30;
    lv_obj_set_pos(refresh_btn_area, 0, refresh_y);
    lv_obj_set_size(refresh_btn_area, APP_AREA_W, 28);
    lv_obj_set_style_bg_color(refresh_btn_area, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(refresh_btn_area, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(refresh_btn_area, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(refresh_btn_area, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(refresh_btn_area, 0, LV_PART_MAIN);
    lv_obj_clear_flag(refresh_btn_area, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *refresh_btn = create_action_button(refresh_btn_area, "Refresh Catalog", 140);
    lv_obj_align(refresh_btn, LV_ALIGN_LEFT_MID, ITEM_PAD_LEFT, 0);
    lv_obj_add_event_cb(refresh_btn, refresh_catalog_cb, LV_EVENT_CLICKED, NULL);

    /* Status bar at bottom of catalog screen */
    lv_obj_t *cat_status_bar = lv_obj_create(cat_screen);
    lv_obj_set_pos(cat_status_bar, 0, APP_AREA_H - TITLE_BAR_H - STATUS_BAR_H);
    lv_obj_set_size(cat_status_bar, APP_AREA_W, STATUS_BAR_H);
    lv_obj_set_style_bg_color(cat_status_bar, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(cat_status_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(cat_status_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(cat_status_bar, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(cat_status_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(cat_status_bar, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(cat_status_bar, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(cat_status_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(cat_status_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(cat_status_bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(cat_status_bar, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *cat_status_lbl = lv_label_create(cat_status_bar);
    lv_label_set_text(cat_status_lbl, "Status: Loading...");
    lv_obj_set_style_text_font(cat_status_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(cat_status_lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(cat_status_lbl, LV_ALIGN_LEFT_MID, 0, 0);
    s_store.catalog_status_label = cat_status_lbl;

    /* ----------------------------------------------------------------
     * Installed screen (hidden by default)
     * ---------------------------------------------------------------- */
    lv_obj_t *inst_screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(inst_screen, 0, TITLE_BAR_H);
    lv_obj_set_size(inst_screen, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    style_panel(inst_screen);
    lv_obj_add_flag(inst_screen, LV_OBJ_FLAG_HIDDEN);
    s_store.installed_screen = inst_screen;

    /* Scrollable installed list */
    lv_obj_t *inst_list = lv_obj_create(inst_screen);
    lv_obj_set_pos(inst_list, 0, 0);
    lv_obj_set_size(inst_list, APP_AREA_W, APP_AREA_H - TITLE_BAR_H - STATUS_BAR_H);
    lv_obj_set_style_bg_color(inst_list, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(inst_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(inst_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(inst_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(inst_list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(inst_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(inst_list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_add_flag(inst_list, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_scroll_dir(inst_list, LV_DIR_VER);
    s_store.installed_list = inst_list;

    /* Status bar at bottom of installed screen */
    lv_obj_t *inst_status_bar = lv_obj_create(inst_screen);
    lv_obj_set_pos(inst_status_bar, 0, APP_AREA_H - TITLE_BAR_H - STATUS_BAR_H);
    lv_obj_set_size(inst_status_bar, APP_AREA_W, STATUS_BAR_H);
    lv_obj_set_style_bg_color(inst_status_bar, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(inst_status_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(inst_status_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(inst_status_bar, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(inst_status_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(inst_status_bar, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(inst_status_bar, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(inst_status_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(inst_status_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(inst_status_bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(inst_status_bar, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *inst_status_lbl = lv_label_create(inst_status_bar);
    lv_label_set_text(inst_status_lbl, "Installed apps");
    lv_obj_set_style_text_font(inst_status_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(inst_status_lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(inst_status_lbl, LV_ALIGN_LEFT_MID, 0, 0);
    s_store.installed_status_label = inst_status_lbl;

    /* ----------------------------------------------------------------
     * Load catalog and populate
     * ---------------------------------------------------------------- */
    ensure_apps_dir();

    esp_err_t err = load_catalog();
    build_catalog_list();

    if (err == ESP_ERR_NOT_FOUND) {
        lv_label_set_text(s_store.catalog_status_label, "Status: catalog.json not found");
    } else if (err != ESP_OK) {
        lv_label_set_text(s_store.catalog_status_label, "Status: catalog load error");
    }

    ESP_LOGI(TAG, "App Store UI created");
    return ESP_OK;
}

void appstore_ui_show(void)
{
    if (s_store.root) {
        lv_obj_remove_flag(s_store.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void appstore_ui_hide(void)
{
    if (s_store.root) {
        lv_obj_add_flag(s_store.root, LV_OBJ_FLAG_HIDDEN);
    }
}
