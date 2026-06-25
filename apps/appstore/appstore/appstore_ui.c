/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — App Store UI
 *
 * Three tabs:
 *   Catalog  — browse and install apps from the live remote catalog
 *   Firmware — firmware and driver updates from the catalog
 *   Installed — manage apps already in /sdcard/apps/
 *
 * The catalog is loaded from the local cache at startup.
 * "Refresh from Server" fetches the live catalog, saves it as the cache,
 * and rebuilds both the Catalog and Firmware tab lists.
 *
 * Install button calls appstore_install_entry() which downloads, verifies
 * SHA-256 hash, and verifies the Ed25519 signature.
 */
#include "appstore/appstore_app.h"

#include "hal/sdcard_path.h"
#include "ui/theme.h"
#include "ui/toast.h"
#include "thistle/app_manager.h"
#include "thistle/appstore_client.h"
#include "thistle/wifi_manager.h"
#include "thistle/net_manager.h"
#include "thistle/ota.h"

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

static int s_app_w = 240;
static int s_app_h = 296;
#define TITLE_BAR_H      30
#define ITEM_H           40   /* taller rows — name + description */
#define ITEM_H_INST      30
#define ITEM_PAD_LEFT     8
#define ITEM_PAD_RIGHT    6
#define STATUS_BAR_H     20
static int s_content_h = 246; /* s_app_h - TITLE_BAR_H - STATUS_BAR_H */

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

#define MAX_CATALOG_APPS CATALOG_MAX_ENTRIES
#define APPS_DIR         THISTLE_SDCARD "/apps"
#define CATALOG_PATH     THISTLE_SDCARD "/appstore/catalog.json"

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
    TAB_FIRMWARE,
    TAB_INSTALLED,
} appstore_tab_t;

static struct {
    lv_obj_t *root;

    /* Tab buttons */
    lv_obj_t *cat_tab_btn;
    lv_obj_t *fw_tab_btn;
    lv_obj_t *inst_tab_btn;

    /* Catalog tab */
    lv_obj_t *catalog_screen;
    lv_obj_t *catalog_list;
    lv_obj_t *catalog_status_label;

    /* Firmware tab */
    lv_obj_t *firmware_screen;
    lv_obj_t *firmware_list;
    lv_obj_t *firmware_status_label;

    /* Detail sub-screen */
    lv_obj_t *detail_screen;

    /* Installed tab */
    lv_obj_t *installed_screen;
    lv_obj_t *installed_list;
    lv_obj_t *installed_status_label;

    /* Catalog data (apps + drivers + firmware from both local and remote) */
    catalog_entry_t  entries[MAX_CATALOG_APPS];
    int              entry_count;
    int              selected_idx;  /* index into entries[] */

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
    lv_obj_set_size(btn, 80, 22);
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

/* Standard action button used on detail/list screens */
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
/* Installed-app check                                                  */
/* ------------------------------------------------------------------ */

static bool app_is_installed(const char *app_id)
{
    char path[300];
    snprintf(path, sizeof(path), "%s/%s.app.elf", APPS_DIR, app_id);
    struct stat st;
    return (stat(path, &st) == 0);
}

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
/* Local catalog cache — save fetched catalog to SD card               */
/* ------------------------------------------------------------------ */

/*
 * Save a freshly-fetched catalog back to the local cache so that the
 * app store can still show entries when the device is offline.
 *
 * We write a minimal JSON array: one flat object per entry, using only
 * the fields that appstore_client.c's json_str/json_int helpers can read.
 */
static void save_catalog_cache(void)
{
    /* Ensure the appstore directory exists */
    char dir[128];
    snprintf(dir, sizeof(dir), "%s/appstore", THISTLE_SDCARD);
    struct stat st;
    if (stat(dir, &st) != 0) {
        mkdir(dir, 0755);
    }

    FILE *f = fopen(CATALOG_PATH, "w");
    if (!f) {
        ESP_LOGW(TAG, "Cannot write catalog cache: %s", CATALOG_PATH);
        return;
    }

    fprintf(f, "[\n");
    for (int i = 0; i < s_store.entry_count; i++) {
        const catalog_entry_t *e = &s_store.entries[i];
        const char *type_str =
            (e->type == CATALOG_TYPE_FIRMWARE) ? "firmware" :
            (e->type == CATALOG_TYPE_DRIVER)   ? "driver"   : "app";

        fprintf(f,
            "    {\n"
            "        \"id\": \"%s\",\n"
            "        \"type\": \"%s\",\n"
            "        \"name\": \"%s\",\n"
            "        \"version\": \"%s\",\n"
            "        \"author\": \"%s\",\n"
            "        \"description\": \"%s\",\n"
            "        \"size_bytes\": %u,\n"
            "        \"url\": \"%s\",\n"
            "        \"sig_url\": \"%s\",\n"
            "        \"sha256\": \"%s\",\n"
            "        \"permissions\": \"%s\",\n"
            "        \"min_os_version\": \"%s\"\n"
            "    }%s\n",
            e->id, type_str, e->name, e->version, e->author,
            e->description, (unsigned)e->size_bytes,
            e->url, e->sig_url, e->sha256_hex,
            e->permissions, e->min_os_version,
            (i < s_store.entry_count - 1) ? "," : "");
    }
    fprintf(f, "]\n");
    fclose(f);
    ESP_LOGI(TAG, "Catalog cache saved: %d entries", s_store.entry_count);
}

/* ------------------------------------------------------------------ */
/* Catalog loading from local cache                                     */
/* ------------------------------------------------------------------ */

static esp_err_t load_catalog_local(void)
{
    s_store.entry_count = 0;

    FILE *f = fopen(CATALOG_PATH, "r");
    if (!f) {
        ESP_LOGW(TAG, "No catalog cache at %s", CATALOG_PATH);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long fsize = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (fsize <= 0) {
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    char *buf = malloc((size_t)fsize + 1);
    if (!buf) {
        fclose(f);
        ESP_LOGE(TAG, "OOM loading catalog cache");
        return ESP_ERR_NO_MEM;
    }

    size_t got = fread(buf, 1, (size_t)fsize, f);
    buf[got] = '\0';
    fclose(f);

    /*
     * Re-use the same object-extraction approach as appstore_client.c:
     * scan for '{' ... '}' pairs and parse fields with the inline helpers.
     */
    const char *cursor = buf;
    while (s_store.entry_count < MAX_CATALOG_APPS) {
        const char *obj_start = strchr(cursor, '{');
        if (!obj_start) break;

        const char *obj_end = strchr(obj_start + 1, '}');
        if (!obj_end) break;

        size_t obj_len = (size_t)(obj_end - obj_start) + 1;
        char  *obj     = malloc(obj_len + 1);
        if (!obj) break;
        memcpy(obj, obj_start, obj_len);
        obj[obj_len] = '\0';

        catalog_entry_t *e = &s_store.entries[s_store.entry_count];
        memset(e, 0, sizeof(*e));

        /* Reuse the same minimal inline JSON helpers */
        #define _STR(k, f) do { \
            char _s[80]; snprintf(_s, sizeof(_s), "\"%s\"", k); \
            const char *_p = strstr(obj, _s); \
            if (_p) { \
                _p = strchr(_p + strlen(_s), '"'); \
                if (_p) { _p++; const char *_e = strchr(_p, '"'); \
                    if (_e) { size_t _l = (size_t)(_e-_p); \
                        if (_l >= sizeof(e->f)) _l = sizeof(e->f)-1; \
                        memcpy(e->f, _p, _l); e->f[_l] = '\0'; } } } \
        } while(0)

        _STR("id",           id);
        _STR("name",         name);
        _STR("version",      version);
        _STR("author",       author);
        _STR("description",  description);
        _STR("url",          url);
        _STR("sig_url",      sig_url);
        _STR("sha256",       sha256_hex);
        _STR("permissions",  permissions);
        _STR("min_os_version", min_os_version);

        #undef _STR

        /* size_bytes */
        {
            const char *p = strstr(obj, "\"size_bytes\"");
            if (p) {
                p = strchr(p + 12, ':');
                if (p) { p++; while (*p == ' ') p++; e->size_bytes = (uint32_t)atoi(p); }
            }
        }

        /* type */
        char type_str[16] = {0};
        {
            const char *p = strstr(obj, "\"type\"");
            if (p) {
                p = strchr(p + 6, '"');
                if (p) {
                    p++;
                    const char *end = strchr(p, '"');
                    if (end) {
                        size_t len = (size_t)(end - p);
                        if (len >= sizeof(type_str)) len = sizeof(type_str) - 1;
                        memcpy(type_str, p, len);
                        type_str[len] = '\0';
                    }
                }
            }
        }
        if (strcmp(type_str, "firmware") == 0)   e->type = CATALOG_TYPE_FIRMWARE;
        else if (strcmp(type_str, "driver") == 0) e->type = CATALOG_TYPE_DRIVER;
        else                                       e->type = CATALOG_TYPE_APP;

        e->is_signed    = (e->sig_url[0] != '\0');
        e->is_installed = (e->id[0] != '\0') && app_is_installed(e->id);

        free(obj);

        if (e->id[0] != '\0' && e->name[0] != '\0') {
            s_store.entry_count++;
        }
        cursor = obj_end + 1;
    }

    free(buf);
    ESP_LOGI(TAG, "Loaded %d entries from local cache", s_store.entry_count);
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

        const char *dot = strstr(ent->d_name, ".app.elf");
        if (!dot) continue;

        installed_entry_t *inst = &s_store.installed[s_store.installed_count];
        strncpy(inst->filename, ent->d_name, sizeof(inst->filename) - 1);
        inst->filename[sizeof(inst->filename) - 1] = '\0';

        strncpy(inst->display_name, ent->d_name, sizeof(inst->display_name) - 1);
        inst->display_name[sizeof(inst->display_name) - 1] = '\0';
        char *suffix = strstr(inst->display_name, ".app.elf");
        if (suffix) *suffix = '\0';

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
static void show_firmware(void);
static void show_installed(void);
static void show_detail(int idx);
static void build_catalog_list(void);
static void build_firmware_list(void);
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

static void tab_firmware_cb(lv_event_t *e)
{
    (void)e;
    if (s_store.current_tab == TAB_FIRMWARE) return;
    show_firmware();
}

static void tab_installed_cb(lv_event_t *e)
{
    (void)e;
    if (s_store.current_tab == TAB_INSTALLED) return;
    show_installed();
}

/* ------------------------------------------------------------------ */
/* Detail screen                                                        */
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
    if (s_store.current_tab == TAB_FIRMWARE && s_store.firmware_screen) {
        lv_obj_remove_flag(s_store.firmware_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_store.current_tab == TAB_INSTALLED && s_store.installed_screen) {
        lv_obj_remove_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);
    }
}

/* ------------------------------------------------------------------ */
/* Install / remove callbacks                                           */
/* ------------------------------------------------------------------ */

static void install_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_store.selected_idx;
    if (idx < 0 || idx >= s_store.entry_count) return;

    catalog_entry_t *entry = &s_store.entries[idx];

    if (!net_is_connected()) {
        toast_warn("No network connection");
        return;
    }

    /* Re-check installed state */
    if (entry->type == CATALOG_TYPE_APP && app_is_installed(entry->id)) {
        toast_info("Already installed");
        return;
    }

    toast_show("Downloading...", TOAST_INFO, 30000);

    esp_err_t ret = appstore_install_entry(entry, NULL, NULL);
    if (ret == ESP_OK) {
        toast_show("Installed!", TOAST_SUCCESS, 3000);
        entry->is_installed = true;
        /* Go back and refresh the list so the [i] badge appears */
        back_to_catalog_cb(NULL);
        build_catalog_list();
    } else if (ret == ESP_ERR_INVALID_CRC) {
        toast_warn("Download corrupted or signature invalid!");
    } else if (ret == ESP_ERR_NOT_SUPPORTED) {
        toast_warn("Downloads not available in simulator");
    } else {
        toast_warn("Download failed");
    }
}

/* "Update OS" button — shown only for CATALOG_TYPE_FIRMWARE entries */
static void ota_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_store.selected_idx;
    if (idx < 0 || idx >= s_store.entry_count) return;

    catalog_entry_t *entry = &s_store.entries[idx];
    if (entry->type != CATALOG_TYPE_FIRMWARE) return;

    if (!net_is_connected()) {
        toast_warn("No network connection");
        return;
    }

    toast_show("Downloading firmware...", TOAST_INFO, 60000);

    esp_err_t ret = appstore_install_entry(entry, NULL, NULL);
    if (ret == ESP_OK) {
        /* File is now at /sdcard/update/thistle_os.bin — apply via OTA */
        toast_show("Applying update, rebooting...", TOAST_SUCCESS, 5000);
        esp_err_t ota_ret = ota_apply_from_sd(NULL, NULL);
        if (ota_ret != ESP_OK) {
            toast_warn("OTA apply failed");
            ESP_LOGE(TAG, "ota_apply_from_sd: %s", esp_err_to_name(ota_ret));
        }
        /* ota_apply_from_sd reboots on success, so we won't reach here */
    } else if (ret == ESP_ERR_INVALID_CRC) {
        toast_warn("Firmware corrupted or signature invalid!");
    } else if (ret == ESP_ERR_NOT_SUPPORTED) {
        toast_warn("Downloads not available in simulator");
    } else {
        toast_warn("Firmware download failed");
    }
}

static void remove_btn_cb(lv_event_t *e)
{
    (void)e;
    int idx = s_store.selected_idx;
    if (idx < 0 || idx >= s_store.entry_count) return;

    catalog_entry_t *entry = &s_store.entries[idx];

    char path[300];
    snprintf(path, sizeof(path), "%s/%s.app.elf", APPS_DIR, entry->id);

    if (remove(path) == 0) {
        entry->is_installed = false;
        toast_show("App removed", TOAST_SUCCESS, 2000);
        ESP_LOGI(TAG, "Removed: %s", path);
        back_to_catalog_cb(NULL);
        build_catalog_list();
    } else {
        toast_warn("Remove failed");
        ESP_LOGW(TAG, "Failed to remove: %s", path);
    }
}

static void remove_installed_btn_cb(lv_event_t *e)
{
    lv_obj_t    *btn = lv_event_get_target(e);
    const char  *fn  = (const char *)lv_obj_get_user_data(btn);
    if (!fn) return;

    char path[300];
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

/* ------------------------------------------------------------------ */
/* Show detail screen (shared by Catalog and Firmware tabs)            */
/* ------------------------------------------------------------------ */

static void show_detail(int idx)
{
    if (idx < 0 || idx >= s_store.entry_count) return;

    s_store.selected_idx = idx;
    const catalog_entry_t *entry = &s_store.entries[idx];

    const theme_colors_t *tc = theme_get_colors();

    /* Hide the active tab screen */
    if (s_store.catalog_screen)  lv_obj_add_flag(s_store.catalog_screen,  LV_OBJ_FLAG_HIDDEN);
    if (s_store.firmware_screen) lv_obj_add_flag(s_store.firmware_screen, LV_OBJ_FLAG_HIDDEN);
    if (s_store.installed_screen) lv_obj_add_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);

    lv_obj_t *screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(screen, 0, 0);
    lv_obj_set_size(screen, s_app_w, s_app_h);
    style_panel(screen);
    s_store.detail_screen = screen;

    /* Title bar — "< Back" */
    lv_obj_t *title_bar = lv_obj_create(screen);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, s_app_w, TITLE_BAR_H);
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

    /* Scrollable content */
    lv_obj_t *content = lv_obj_create(screen);
    lv_obj_set_pos(content, 0, TITLE_BAR_H);
    lv_obj_set_size(content, s_app_w, s_app_h - TITLE_BAR_H);
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

    /* App / firmware name */
    lv_obj_t *name_lbl = lv_label_create(content);
    lv_label_set_text(name_lbl, entry->name);
    lv_obj_set_width(name_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);

    create_separator(content);

    char line[192];

    /* Type badge */
    const char *type_label =
        (entry->type == CATALOG_TYPE_FIRMWARE) ? "Type: Firmware Update" :
        (entry->type == CATALOG_TYPE_DRIVER)   ? "Type: Driver"          : "Type: App";
    lv_obj_t *type_lbl = lv_label_create(content);
    lv_label_set_text(type_lbl, type_label);
    lv_obj_set_width(type_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(type_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(type_lbl, tc->primary, LV_PART_MAIN);

    snprintf(line, sizeof(line), "Version: %s", entry->version);
    lv_obj_t *ver_lbl = lv_label_create(content);
    lv_label_set_text(ver_lbl, line);
    lv_obj_set_width(ver_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(ver_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(ver_lbl, tc->text, LV_PART_MAIN);

    snprintf(line, sizeof(line), "Author: %s", entry->author);
    lv_obj_t *auth_lbl = lv_label_create(content);
    lv_label_set_text(auth_lbl, line);
    lv_obj_set_width(auth_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(auth_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(auth_lbl, tc->text, LV_PART_MAIN);

    if (entry->size_bytes > 0) {
        uint32_t kb = (entry->size_bytes + 1023) / 1024;
        snprintf(line, sizeof(line), "Size: %u KB", (unsigned)kb);
    } else {
        snprintf(line, sizeof(line), "Size: unknown");
    }
    lv_obj_t *size_lbl = lv_label_create(content);
    lv_label_set_text(size_lbl, line);
    lv_obj_set_width(size_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(size_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(size_lbl, tc->text, LV_PART_MAIN);

    if (entry->permissions[0] != '\0') {
        snprintf(line, sizeof(line), "Permissions: %s", entry->permissions);
    } else {
        snprintf(line, sizeof(line), "Permissions: none");
    }
    lv_obj_t *perm_lbl = lv_label_create(content);
    lv_label_set_text(perm_lbl, line);
    lv_obj_set_width(perm_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(perm_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(perm_lbl, tc->text, LV_PART_MAIN);

    snprintf(line, sizeof(line), "Signed: %s", entry->is_signed ? "Yes" : "No");
    lv_obj_t *sign_lbl = lv_label_create(content);
    lv_label_set_text(sign_lbl, line);
    lv_obj_set_width(sign_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(sign_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(sign_lbl, tc->text, LV_PART_MAIN);

    create_separator(content);

    lv_obj_t *desc_lbl = lv_label_create(content);
    lv_label_set_text(desc_lbl, entry->description);
    lv_obj_set_width(desc_lbl, LV_PCT(100));
    lv_obj_set_style_text_font(desc_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(desc_lbl, tc->text_secondary, LV_PART_MAIN);
    lv_label_set_long_mode(desc_lbl, LV_LABEL_LONG_WRAP);

    create_separator(content);

    /* Action buttons */
    lv_obj_t *btn_row = lv_obj_create(content);
    lv_obj_set_size(btn_row, LV_PCT(100), 34);
    lv_obj_set_style_bg_opa(btn_row, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn_row, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(btn_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(btn_row, LV_OBJ_FLAG_SCROLLABLE);

    if (entry->type == CATALOG_TYPE_FIRMWARE || entry->type == CATALOG_TYPE_DRIVER) {
        /* Firmware / driver: single "Update OS" or "Install Driver" button */
        const char *btn_label = (entry->type == CATALOG_TYPE_FIRMWARE) ?
                                "Update OS" : "Install Driver";
        lv_obj_t *ota_btn = create_action_button(btn_row, btn_label, 120);
        lv_obj_align(ota_btn, LV_ALIGN_LEFT_MID, 0, 0);
        lv_obj_add_event_cb(ota_btn, ota_btn_cb, LV_EVENT_CLICKED, NULL);
    } else {
        /* App: Install + optionally Remove */
        lv_obj_t *install_btn = create_action_button(btn_row, "Install", 100);
        lv_obj_align(install_btn, LV_ALIGN_LEFT_MID, 0, 0);
        lv_obj_add_event_cb(install_btn, install_btn_cb, LV_EVENT_CLICKED, NULL);

        if (app_is_installed(entry->id)) {
            lv_obj_t *remove_btn = create_action_button(btn_row, "Remove", 100);
            lv_obj_align(remove_btn, LV_ALIGN_LEFT_MID, 110, 0);
            lv_obj_add_event_cb(remove_btn, remove_btn_cb, LV_EVENT_CLICKED, NULL);
        }
    }
}

/* ------------------------------------------------------------------ */
/* Catalog row click                                                    */
/* ------------------------------------------------------------------ */

static void catalog_row_clicked_cb(lv_event_t *e)
{
    lv_obj_t *row  = lv_event_get_target(e);
    intptr_t  idx  = (intptr_t)lv_obj_get_user_data(row);
    show_detail((int)idx);
}

/* ------------------------------------------------------------------ */
/* Build catalog list (apps + drivers only)                            */
/* ------------------------------------------------------------------ */

static void build_catalog_list(void)
{
    if (!s_store.catalog_list) return;

    lv_obj_clean(s_store.catalog_list);

    const theme_colors_t *tc = theme_get_colors();

    /* Count non-firmware entries */
    int app_count = 0;
    for (int i = 0; i < s_store.entry_count; i++) {
        if (s_store.entries[i].type != CATALOG_TYPE_FIRMWARE) app_count++;
    }

    if (app_count == 0) {
        lv_obj_t *empty_lbl = lv_label_create(s_store.catalog_list);
        lv_label_set_text(empty_lbl,
            "No apps in catalog.\nRefresh from server or place catalog.json on SD card.");
        lv_obj_set_style_text_font(empty_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(empty_lbl, tc->text, LV_PART_MAIN);
        lv_obj_set_style_pad_all(empty_lbl, 8, LV_PART_MAIN);
        lv_label_set_long_mode(empty_lbl, LV_LABEL_LONG_WRAP);
        lv_obj_set_width(empty_lbl, s_app_w - 16);
        return;
    }

    for (int i = 0; i < s_store.entry_count; i++) {
        const catalog_entry_t *e = &s_store.entries[i];
        if (e->type == CATALOG_TYPE_FIRMWARE) continue;

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
        lv_obj_set_user_data(row, (void *)(intptr_t)i);
        lv_obj_add_event_cb(row, catalog_row_clicked_cb, LV_EVENT_CLICKED, NULL);

        lv_obj_set_flex_flow(row, LV_FLEX_FLOW_COLUMN);
        lv_obj_set_style_pad_row(row, 2, LV_PART_MAIN);

        char top_line[80];
        snprintf(top_line, sizeof(top_line), "> %s%s%s   %s",
                 e->name,
                 e->is_installed ? " [i]" : "",
                 (e->type == CATALOG_TYPE_DRIVER) ? " [drv]" : "",
                 e->version);

        lv_obj_t *name_lbl = lv_label_create(row);
        lv_label_set_text(name_lbl, top_line);
        lv_obj_set_width(name_lbl, LV_PCT(100));
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);
        lv_label_set_long_mode(name_lbl, LV_LABEL_LONG_DOT);

        lv_obj_t *desc_lbl = lv_label_create(row);
        lv_label_set_text(desc_lbl, e->description);
        lv_obj_set_width(desc_lbl, LV_PCT(100));
        lv_obj_set_style_text_font(desc_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(desc_lbl, tc->text_secondary, LV_PART_MAIN);
        lv_label_set_long_mode(desc_lbl, LV_LABEL_LONG_DOT);

        create_separator(s_store.catalog_list);
    }

    if (s_store.catalog_status_label) {
        char status[80];
        snprintf(status, sizeof(status), "Catalog: %d app(s)", app_count);
        lv_label_set_text(s_store.catalog_status_label, status);
    }
}

/* ------------------------------------------------------------------ */
/* Build firmware list (CATALOG_TYPE_FIRMWARE entries only)            */
/* ------------------------------------------------------------------ */

static void build_firmware_list(void)
{
    if (!s_store.firmware_list) return;

    lv_obj_clean(s_store.firmware_list);

    const theme_colors_t *tc = theme_get_colors();

    int fw_count = 0;
    for (int i = 0; i < s_store.entry_count; i++) {
        if (s_store.entries[i].type == CATALOG_TYPE_FIRMWARE) fw_count++;
    }

    if (fw_count == 0) {
        lv_obj_t *empty_lbl = lv_label_create(s_store.firmware_list);
        lv_label_set_text(empty_lbl,
            "No firmware updates available.\nRefresh from server to check.");
        lv_obj_set_style_text_font(empty_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(empty_lbl, tc->text, LV_PART_MAIN);
        lv_obj_set_style_pad_all(empty_lbl, 8, LV_PART_MAIN);
        lv_label_set_long_mode(empty_lbl, LV_LABEL_LONG_WRAP);
        lv_obj_set_width(empty_lbl, s_app_w - 16);

        /* Current version info */
        char ver_line[64];
        snprintf(ver_line, sizeof(ver_line), "Running: %s (%s)",
                 ota_get_current_version(), ota_get_running_partition());
        lv_obj_t *ver_lbl = lv_label_create(s_store.firmware_list);
        lv_label_set_text(ver_lbl, ver_line);
        lv_obj_set_style_text_font(ver_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(ver_lbl, tc->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_pad_all(ver_lbl, 8, LV_PART_MAIN);
        return;
    }

    /* Current version row */
    {
        char ver_line[80];
        snprintf(ver_line, sizeof(ver_line), "Running: v%s  [%s]",
                 ota_get_current_version(), ota_get_running_partition());
        lv_obj_t *ver_lbl = lv_label_create(s_store.firmware_list);
        lv_label_set_text(ver_lbl, ver_line);
        lv_obj_set_style_text_font(ver_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(ver_lbl, tc->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_pad_left(ver_lbl, ITEM_PAD_LEFT, LV_PART_MAIN);
        lv_obj_set_style_pad_top(ver_lbl, 4, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(ver_lbl, 4, LV_PART_MAIN);
        create_separator(s_store.firmware_list);
    }

    for (int i = 0; i < s_store.entry_count; i++) {
        const catalog_entry_t *e = &s_store.entries[i];
        if (e->type != CATALOG_TYPE_FIRMWARE) continue;

        lv_obj_t *row = lv_obj_create(s_store.firmware_list);
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
        lv_obj_set_user_data(row, (void *)(intptr_t)i);
        lv_obj_add_event_cb(row, catalog_row_clicked_cb, LV_EVENT_CLICKED, NULL);

        lv_obj_set_flex_flow(row, LV_FLEX_FLOW_COLUMN);
        lv_obj_set_style_pad_row(row, 2, LV_PART_MAIN);

        char top_line[80];
        snprintf(top_line, sizeof(top_line), "> %s  v%s", e->name, e->version);

        lv_obj_t *name_lbl = lv_label_create(row);
        lv_label_set_text(name_lbl, top_line);
        lv_obj_set_width(name_lbl, LV_PCT(100));
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);
        lv_label_set_long_mode(name_lbl, LV_LABEL_LONG_DOT);

        lv_obj_t *desc_lbl = lv_label_create(row);
        lv_label_set_text(desc_lbl, e->description);
        lv_obj_set_width(desc_lbl, LV_PCT(100));
        lv_obj_set_style_text_font(desc_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(desc_lbl, tc->text_secondary, LV_PART_MAIN);
        lv_label_set_long_mode(desc_lbl, LV_LABEL_LONG_DOT);

        create_separator(s_store.firmware_list);
    }

    if (s_store.firmware_status_label) {
        char status[80];
        snprintf(status, sizeof(status), "%d firmware update(s) available", fw_count);
        lv_label_set_text(s_store.firmware_status_label, status);
    }
}

/* ------------------------------------------------------------------ */
/* Refresh from server callback                                         */
/* ------------------------------------------------------------------ */

static void refresh_server_cb(lv_event_t *e)
{
    (void)e;

    if (!net_is_connected()) {
        toast_warn("No network connection");
        if (s_store.catalog_status_label) {
            lv_label_set_text(s_store.catalog_status_label, "Not connected");
        }
        return;
    }

    if (s_store.catalog_status_label) {
        lv_label_set_text(s_store.catalog_status_label, "Fetching catalog...");
    }

    catalog_entry_t fetched[CATALOG_MAX_ENTRIES];
    int count = 0;

    esp_err_t err = appstore_fetch_catalog(NULL, fetched, CATALOG_MAX_ENTRIES, &count);

    if (err == ESP_OK && count > 0) {
        /* Replace in-memory catalog with the fresh data */
        memcpy(s_store.entries, fetched, sizeof(catalog_entry_t) * (size_t)count);
        s_store.entry_count = count;

        /* Mark installed state */
        for (int i = 0; i < s_store.entry_count; i++) {
            catalog_entry_t *en = &s_store.entries[i];
            en->is_installed = (en->type == CATALOG_TYPE_APP) && app_is_installed(en->id);
        }

        /* Persist as local cache */
        save_catalog_cache();

        build_catalog_list();
        build_firmware_list();
        toast_show("Catalog updated", TOAST_SUCCESS, 2000);

        if (s_store.catalog_status_label) {
            char status[80];
            snprintf(status, sizeof(status), "Fetched %d entries from server", count);
            lv_label_set_text(s_store.catalog_status_label, status);
        }
    } else if (err == ESP_ERR_NOT_SUPPORTED) {
        /* Simulator build */
        if (s_store.catalog_status_label) {
            lv_label_set_text(s_store.catalog_status_label, "Network N/A (simulator)");
        }
        toast_warn("Downloads not available in simulator");
    } else {
        /* Network error — fall back to local cache */
        ESP_LOGW(TAG, "Server fetch failed (%s), using local cache",
                 esp_err_to_name(err));
        esp_err_t load_err = load_catalog_local();
        build_catalog_list();
        build_firmware_list();

        if (s_store.catalog_status_label) {
            lv_label_set_text(s_store.catalog_status_label,
                              (load_err == ESP_OK) ? "Server failed — showing cache"
                                                   : "Server failed — no cache");
        }
        toast_warn("Server unavailable — showing cached catalog");
    }
}

/* ------------------------------------------------------------------ */
/* Refresh from local cache callback                                    */
/* ------------------------------------------------------------------ */

static void refresh_local_cb(lv_event_t *e)
{
    (void)e;

    if (s_store.catalog_status_label) {
        lv_label_set_text(s_store.catalog_status_label, "Loading cache...");
    }

    esp_err_t err = load_catalog_local();
    build_catalog_list();
    build_firmware_list();

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
        lv_obj_set_width(empty_lbl, s_app_w - 16);
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

        char info[80];
        if (inst->size_kb > 0) {
            snprintf(info, sizeof(info), "%s  (%u KB)",
                     inst->display_name, (unsigned)inst->size_kb);
        } else {
            snprintf(info, sizeof(info), "%s", inst->display_name);
        }

        lv_obj_t *name_lbl = lv_label_create(row);
        lv_label_set_text(name_lbl, info);
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl, tc->text, LV_PART_MAIN);
        lv_obj_align(name_lbl, LV_ALIGN_LEFT_MID, 0, 0);
        lv_label_set_long_mode(name_lbl, LV_LABEL_LONG_DOT);
        lv_obj_set_width(name_lbl, s_app_w - ITEM_PAD_LEFT - ITEM_PAD_RIGHT - 70);

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
/* Tab-switch implementations                                           */
/* ------------------------------------------------------------------ */

static void update_tab_button_styles(void)
{
    const theme_colors_t *tc = theme_get_colors();

    lv_obj_t *btns[3] = { s_store.cat_tab_btn, s_store.fw_tab_btn, s_store.inst_tab_btn };
    appstore_tab_t tabs[3] = { TAB_CATALOG, TAB_FIRMWARE, TAB_INSTALLED };

    for (int i = 0; i < 3; i++) {
        if (!btns[i]) continue;
        bool active = (s_store.current_tab == tabs[i]);
        lv_obj_set_style_bg_color(btns[i], active ? tc->primary : tc->surface, LV_PART_MAIN);
        lv_obj_t *lbl = lv_obj_get_child(btns[i], 0);
        if (lbl) {
            lv_obj_set_style_text_color(lbl, active ? lv_color_white() : tc->text, LV_PART_MAIN);
        }
    }
}

static void show_catalog(void)
{
    s_store.current_tab = TAB_CATALOG;
    update_tab_button_styles();

    if (s_store.firmware_screen)  lv_obj_add_flag(s_store.firmware_screen,  LV_OBJ_FLAG_HIDDEN);
    if (s_store.installed_screen) lv_obj_add_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);
    if (s_store.catalog_screen)   lv_obj_remove_flag(s_store.catalog_screen, LV_OBJ_FLAG_HIDDEN);
}

static void show_firmware(void)
{
    s_store.current_tab = TAB_FIRMWARE;
    update_tab_button_styles();

    if (s_store.catalog_screen)   lv_obj_add_flag(s_store.catalog_screen,   LV_OBJ_FLAG_HIDDEN);
    if (s_store.installed_screen) lv_obj_add_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);
    if (s_store.firmware_screen)  lv_obj_remove_flag(s_store.firmware_screen, LV_OBJ_FLAG_HIDDEN);
}

static void show_installed(void)
{
    s_store.current_tab = TAB_INSTALLED;
    update_tab_button_styles();

    if (s_store.catalog_screen)   lv_obj_add_flag(s_store.catalog_screen,   LV_OBJ_FLAG_HIDDEN);
    if (s_store.firmware_screen)  lv_obj_add_flag(s_store.firmware_screen,  LV_OBJ_FLAG_HIDDEN);
    if (s_store.installed_screen) lv_obj_remove_flag(s_store.installed_screen, LV_OBJ_FLAG_HIDDEN);

    scan_installed_apps();
    build_installed_list();
}

/* ------------------------------------------------------------------ */
/* Helper: build a scrollable list + status bar inside a parent screen */
/* ------------------------------------------------------------------ */

static lv_obj_t *build_list_area(lv_obj_t *parent, int list_h)
{
    const theme_colors_t *tc = theme_get_colors();

    lv_obj_t *list = lv_obj_create(parent);
    lv_obj_set_pos(list, 0, 0);
    lv_obj_set_size(list, s_app_w, list_h);
    lv_obj_set_style_bg_color(list, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_add_flag(list, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_scroll_dir(list, LV_DIR_VER);
    return list;
}

static lv_obj_t *build_status_bar(lv_obj_t *parent, int y, const char *init_text)
{
    const theme_colors_t *tc = theme_get_colors();

    lv_obj_t *bar = lv_obj_create(parent);
    lv_obj_set_pos(bar, 0, y);
    lv_obj_set_size(bar, s_app_w, STATUS_BAR_H);
    lv_obj_set_style_bg_color(bar, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(bar, tc->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(bar, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(bar, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(bar, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(bar, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(bar, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *lbl = lv_label_create(bar);
    lv_label_set_text(lbl, init_text);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 0, 0);
    return lbl;
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

    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;
    if (s_app_h == 0) s_app_h = 296;
    s_content_h = s_app_h - TITLE_BAR_H - STATUS_BAR_H;

    const theme_colors_t *tc = theme_get_colors();

    /* Root */
    s_store.root = lv_obj_create(parent);
    lv_obj_set_size(s_store.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_store.root, 0, 0);
    style_panel(s_store.root);

    /* ---------------------------------------------------------------
     * Shared title bar with three tab buttons
     * --------------------------------------------------------------- */
    lv_obj_t *title_bar = lv_obj_create(s_store.root);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, s_app_w, TITLE_BAR_H);
    style_title_bar(title_bar);

    lv_obj_t *title_lbl = lv_label_create(title_bar);
    lv_label_set_text(title_lbl, "App Store");
    lv_obj_set_style_text_font(title_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(title_lbl, tc->text, LV_PART_MAIN);
    lv_obj_align(title_lbl, LV_ALIGN_LEFT_MID, 0, 0);

    /* Three tab buttons on the right, 80px wide each with 2px gap */
    lv_obj_t *inst_btn = create_tab_button(title_bar, "Installed", false);
    lv_obj_align(inst_btn, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_add_event_cb(inst_btn, tab_installed_cb, LV_EVENT_CLICKED, NULL);
    s_store.inst_tab_btn = inst_btn;

    lv_obj_t *fw_btn = create_tab_button(title_bar, "Firmware", false);
    lv_obj_align(fw_btn, LV_ALIGN_RIGHT_MID, -82, 0);
    lv_obj_add_event_cb(fw_btn, tab_firmware_cb, LV_EVENT_CLICKED, NULL);
    s_store.fw_tab_btn = fw_btn;

    lv_obj_t *cat_btn = create_tab_button(title_bar, "Catalog", true);
    lv_obj_align(cat_btn, LV_ALIGN_RIGHT_MID, -164, 0);
    lv_obj_add_event_cb(cat_btn, tab_catalog_cb, LV_EVENT_CLICKED, NULL);
    s_store.cat_tab_btn = cat_btn;

    /* ---------------------------------------------------------------
     * Catalog screen
     * List + refresh button row + status bar
     * --------------------------------------------------------------- */
    int btn_row_h    = 28;
    int cat_list_h   = s_app_h - TITLE_BAR_H - STATUS_BAR_H - btn_row_h;
    int cat_btn_y    = cat_list_h;
    int cat_status_y = cat_list_h + btn_row_h;

    lv_obj_t *cat_screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(cat_screen, 0, TITLE_BAR_H);
    lv_obj_set_size(cat_screen, s_app_w, s_app_h - TITLE_BAR_H);
    style_panel(cat_screen);
    s_store.catalog_screen = cat_screen;

    s_store.catalog_list = build_list_area(cat_screen, cat_list_h);

    /* Button row: "Refresh Cache" + "Refresh from Server" */
    lv_obj_t *cat_btn_area = lv_obj_create(cat_screen);
    lv_obj_set_pos(cat_btn_area, 0, cat_btn_y);
    lv_obj_set_size(cat_btn_area, s_app_w, btn_row_h);
    lv_obj_set_style_bg_color(cat_btn_area, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(cat_btn_area, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(cat_btn_area, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(cat_btn_area, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(cat_btn_area, 0, LV_PART_MAIN);
    lv_obj_clear_flag(cat_btn_area, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *reload_local_btn = create_action_button(cat_btn_area, "Reload Cache", 120);
    lv_obj_align(reload_local_btn, LV_ALIGN_LEFT_MID, ITEM_PAD_LEFT, 0);
    lv_obj_add_event_cb(reload_local_btn, refresh_local_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *refresh_srv_btn = create_action_button(cat_btn_area, "Refresh from Server", 160);
    lv_obj_align(refresh_srv_btn, LV_ALIGN_LEFT_MID, ITEM_PAD_LEFT + 128, 0);
    lv_obj_add_event_cb(refresh_srv_btn, refresh_server_cb, LV_EVENT_CLICKED, NULL);

    s_store.catalog_status_label = build_status_bar(cat_screen, cat_status_y, "Loading...");

    /* ---------------------------------------------------------------
     * Firmware screen (hidden by default)
     * --------------------------------------------------------------- */
    int fw_list_h   = s_app_h - TITLE_BAR_H - STATUS_BAR_H - btn_row_h;
    int fw_btn_y    = fw_list_h;
    int fw_status_y = fw_list_h + btn_row_h;

    lv_obj_t *fw_screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(fw_screen, 0, TITLE_BAR_H);
    lv_obj_set_size(fw_screen, s_app_w, s_app_h - TITLE_BAR_H);
    style_panel(fw_screen);
    lv_obj_add_flag(fw_screen, LV_OBJ_FLAG_HIDDEN);
    s_store.firmware_screen = fw_screen;

    s_store.firmware_list = build_list_area(fw_screen, fw_list_h);

    lv_obj_t *fw_btn_area = lv_obj_create(fw_screen);
    lv_obj_set_pos(fw_btn_area, 0, fw_btn_y);
    lv_obj_set_size(fw_btn_area, s_app_w, btn_row_h);
    lv_obj_set_style_bg_color(fw_btn_area, tc->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(fw_btn_area, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(fw_btn_area, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(fw_btn_area, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(fw_btn_area, 0, LV_PART_MAIN);
    lv_obj_clear_flag(fw_btn_area, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *fw_refresh_btn = create_action_button(fw_btn_area, "Refresh from Server", 160);
    lv_obj_align(fw_refresh_btn, LV_ALIGN_LEFT_MID, ITEM_PAD_LEFT, 0);
    lv_obj_add_event_cb(fw_refresh_btn, refresh_server_cb, LV_EVENT_CLICKED, NULL);

    s_store.firmware_status_label = build_status_bar(fw_screen, fw_status_y, "Firmware updates");

    /* ---------------------------------------------------------------
     * Installed screen (hidden by default)
     * --------------------------------------------------------------- */
    lv_obj_t *inst_screen = lv_obj_create(s_store.root);
    lv_obj_set_pos(inst_screen, 0, TITLE_BAR_H);
    lv_obj_set_size(inst_screen, s_app_w, s_app_h - TITLE_BAR_H);
    style_panel(inst_screen);
    lv_obj_add_flag(inst_screen, LV_OBJ_FLAG_HIDDEN);
    s_store.installed_screen = inst_screen;

    s_store.installed_list = build_list_area(inst_screen,
                                              s_app_h - TITLE_BAR_H - STATUS_BAR_H);

    s_store.installed_status_label =
        build_status_bar(inst_screen, s_app_h - TITLE_BAR_H - STATUS_BAR_H, "Installed apps");

    /* ---------------------------------------------------------------
     * Initial data load from local cache
     * --------------------------------------------------------------- */
    ensure_apps_dir();

    esp_err_t err = load_catalog_local();
    build_catalog_list();
    build_firmware_list();

    if (err == ESP_ERR_NOT_FOUND) {
        lv_label_set_text(s_store.catalog_status_label, "catalog.json not found on SD");
    } else if (err != ESP_OK) {
        lv_label_set_text(s_store.catalog_status_label, "Catalog load error");
    }

    ESP_LOGI(TAG, "App Store UI created (%d entries loaded)", s_store.entry_count);
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

void appstore_ui_destroy(void)
{
    if (s_store.root) {
        lv_obj_delete(s_store.root);
        s_store.root = NULL;
    }
}
