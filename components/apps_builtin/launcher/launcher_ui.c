#include "launcher/launcher_app.h"

#include "lvgl.h"
#include "esp_log.h"
#include "thistle/app_manager.h"
#include "thistle/wifi_manager.h"
#include "ui/toast.h"
#include "ui/theme.h"

#include <string.h>

static const char *TAG = "launcher_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

/* App-area dimensions — set from parent in launcher_ui_create() */
static int s_app_w = 240;
static int s_app_h = 296;
#define DOCK_H           50
#define ICON_SIZE        38
#define CELL_SIZE        70

/* ------------------------------------------------------------------ */
/* Favorites config                                                     */
/* ------------------------------------------------------------------ */

#define MAX_DOCK_FAVORITES 6
static const char *s_dock_favorites[MAX_DOCK_FAVORITES] = {
    "com.thistle.settings",
    "com.thistle.filemgr",
    "com.thistle.messenger",
    "com.thistle.reader",
    NULL,
    NULL,
};

/* ------------------------------------------------------------------ */
/* State                                                               */
/* ------------------------------------------------------------------ */

static lv_obj_t *s_root        = NULL;
static lv_obj_t *s_app_drawer  = NULL;
static lv_obj_t *s_drawer_grid = NULL;
static lv_obj_t *s_folder      = NULL;
static lv_obj_t *s_folder_grid = NULL;
static bool      s_drawer_visible = false;
static bool      s_folder_visible = false;

/* ------------------------------------------------------------------ */
/* Stable semantic icon mapping.  LVGL's built-in symbols keep the
 * launcher legible without per-WM bitmap assets.                      */
/* ------------------------------------------------------------------ */

static const char *app_icon_symbol(const char *app_id)
{
    if (strstr(app_id, "settings"))    return LV_SYMBOL_SETTINGS;
    if (strstr(app_id, "filemgr"))     return LV_SYMBOL_DIRECTORY;
    if (strstr(app_id, "reader"))      return LV_SYMBOL_FILE;
    if (strstr(app_id, "messenger"))   return LV_SYMBOL_ENVELOPE;
    if (strstr(app_id, "navigator"))   return LV_SYMBOL_GPS;
    if (strstr(app_id, "notes"))       return LV_SYMBOL_EDIT;
    if (strstr(app_id, "assistant"))   return LV_SYMBOL_EYE_OPEN;
    if (strstr(app_id, "appstore"))    return LV_SYMBOL_DOWNLOAD;
    if (strstr(app_id, "wifiscanner")) return LV_SYMBOL_WIFI;
    if (strstr(app_id, "flashlight"))  return LV_SYMBOL_POWER;
    if (strstr(app_id, "weather"))     return LV_SYMBOL_REFRESH;
    if (strstr(app_id, "terminal"))    return LV_SYMBOL_KEYBOARD;
    if (strstr(app_id, "vault"))       return LV_SYMBOL_WARNING;
    if (strstr(app_id, "radio"))       return LV_SYMBOL_AUDIO;
    return LV_SYMBOL_FILE;
}

static bool is_launcher(const char *app_id)
{
    return strcmp(app_id, "com.thistle.launcher") == 0 ||
           strcmp(app_id, "com.thistle.tk_launcher") == 0 ||
           strcmp(app_id, "com.thistle.tk-launcher") == 0;
}

static bool is_tool_app(const char *app_id)
{
    return strstr(app_id, "settings") || strstr(app_id, "terminal") ||
           strstr(app_id, "wifiscanner") || strstr(app_id, "flashlight");
}

/* ------------------------------------------------------------------ */
/* Click handlers                                                      */
/* ------------------------------------------------------------------ */

static void close_app_drawer(void);
static void close_folder(void);
static void apps_btn_clicked_cb(lv_event_t *e);

static void app_cell_clicked_cb(lv_event_t *e)
{
    const char *app_id = (const char *)lv_obj_get_user_data(lv_event_get_target(e));
    if (!app_id) {
        ESP_LOGW(TAG, "app cell clicked: no app_id");
        return;
    }

    close_app_drawer();
    close_folder();

    ESP_LOGI(TAG, "Launching app from launcher: %s", app_id);
    esp_err_t ret = app_manager_launch(app_id);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to launch %s: %s", app_id, esp_err_to_name(ret));
        toast_warn("App not available");
    }
}

static void dock_icon_clicked_cb(lv_event_t *e)
{
    const char *app_id = (const char *)lv_obj_get_user_data(lv_event_get_target(e));
    if (!app_id) {
        ESP_LOGW(TAG, "dock icon pressed: app not installed");
        return;
    }

    ESP_LOGI(TAG, "Launching app from dock: %s", app_id);
    esp_err_t ret = app_manager_launch(app_id);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "Failed to launch %s: %s", app_id, esp_err_to_name(ret));
        toast_warn("App not available");
    }
}

static void drawer_close_btn_cb(lv_event_t *e)
{
    (void)e;
    close_app_drawer();
}

static void folder_close_btn_cb(lv_event_t *e)
{
    (void)e;
    close_folder();
}

static void folder_backdrop_clicked_cb(lv_event_t *e)
{
    if (lv_event_get_target(e) == s_folder) {
        close_folder();
    }
}

/* ------------------------------------------------------------------ */
/* App grid cell                                                       */
/* ------------------------------------------------------------------ */

static lv_obj_t *create_icon_cell(lv_obj_t *parent, const char *symbol,
                                  const char *name, int width, int height)
{
    const theme_colors_t *c = theme_get_colors();

    lv_obj_t *cell = lv_obj_create(parent);
    lv_obj_set_size(cell, width, height);
    lv_obj_set_style_bg_color(cell, c->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(cell, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(cell, c->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(cell, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(cell, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_all(cell, 4, LV_PART_MAIN);
    lv_obj_set_flex_flow(cell, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(cell,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_clear_flag(cell, LV_OBJ_FLAG_SCROLLABLE);

    /* Pressed state */
    lv_obj_set_style_bg_color(cell, c->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(cell, LV_OPA_COVER, LV_STATE_PRESSED);

    /* Semantic icon */
    lv_obj_t *icon = lv_label_create(cell);
    lv_label_set_text(icon, symbol);
    lv_obj_set_style_text_font(icon, &lv_font_montserrat_22, LV_PART_MAIN);
    lv_obj_set_style_text_color(icon, c->text, LV_PART_MAIN);

    /* App name (small, below icon) */
    lv_obj_t *lbl = lv_label_create(cell);
    lv_label_set_text(lbl, name);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, c->text_secondary, LV_PART_MAIN);
    lv_label_set_long_mode(lbl, LV_LABEL_LONG_DOT);
    lv_obj_set_width(lbl, width - 6);
    lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);

    lv_obj_add_flag(cell, LV_OBJ_FLAG_CLICKABLE);

    return cell;
}

static lv_obj_t *create_app_cell(lv_obj_t *parent, const char *symbol,
                                  const char *name, const char *app_id,
                                  int width, int height)
{
    lv_obj_t *cell = create_icon_cell(parent, symbol, name, width, height);
    lv_obj_set_user_data(cell, (void *)app_id);
    lv_obj_add_event_cb(cell, app_cell_clicked_cb, LV_EVENT_CLICKED, NULL);
    return cell;
}

/* ------------------------------------------------------------------ */
/* Dock icon helper                                                    */
/* ------------------------------------------------------------------ */

static lv_obj_t *create_dock_icon(lv_obj_t *parent, const char *label,
                                   const char *app_id)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_size(btn, ICON_SIZE, ICON_SIZE);

    lv_obj_set_style_bg_color(btn, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, colors->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_shadow_width(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(btn, 0, LV_PART_MAIN);

    /* Pressed state */
    lv_obj_set_style_bg_color(btn, colors->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_STATE_PRESSED);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, colors->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_center(lbl);

    if (app_id) {
        lv_obj_set_user_data(btn, (void *)app_id);
        lv_obj_add_event_cb(btn, dock_icon_clicked_cb, LV_EVENT_CLICKED, NULL);
    } else {
        lv_obj_add_event_cb(btn, apps_btn_clicked_cb, LV_EVENT_CLICKED, NULL);
    }

    return btn;
}

/* ------------------------------------------------------------------ */
/* App drawer                                                          */
/* ------------------------------------------------------------------ */

static void populate_app_drawer(void)
{
    const app_manifest_t *apps[20];
    int count = app_manager_list_apps(apps, 20);

    for (int i = 0; i < count; i++) {
        /* Skip the launcher itself */
        if (strcmp(apps[i]->id, "com.thistle.launcher") == 0) continue;

        if (is_launcher(apps[i]->id)) continue;

        create_app_cell(s_drawer_grid, app_icon_symbol(apps[i]->id),
                        apps[i]->name, apps[i]->id, CELL_SIZE, CELL_SIZE);
    }
}

static void open_app_drawer(void)
{
    const theme_colors_t *colors = theme_get_colors();

    if (!s_app_drawer) {
        /* Full-screen overlay on top of the home screen */
        s_app_drawer = lv_obj_create(s_root);
        lv_obj_set_size(s_app_drawer, s_app_w, s_app_h);
        lv_obj_set_pos(s_app_drawer, 0, 0);
        lv_obj_set_style_bg_color(s_app_drawer, colors->bg, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_app_drawer, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_width(s_app_drawer, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(s_app_drawer, 0, LV_PART_MAIN);
        lv_obj_set_style_radius(s_app_drawer, 0, LV_PART_MAIN);
        lv_obj_clear_flag(s_app_drawer, LV_OBJ_FLAG_SCROLLABLE);

        /* --- Header bar --- */
        lv_obj_t *header = lv_obj_create(s_app_drawer);
        lv_obj_set_size(header, s_app_w, 30);
        lv_obj_set_pos(header, 0, 0);
        lv_obj_set_style_bg_color(header, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(header, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_side(header, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(header, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_border_width(header, 1, LV_PART_MAIN);
        lv_obj_set_style_pad_all(header, 4, LV_PART_MAIN);
        lv_obj_set_style_radius(header, 0, LV_PART_MAIN);
        lv_obj_clear_flag(header, LV_OBJ_FLAG_SCROLLABLE);

        lv_obj_t *title = lv_label_create(header);
        lv_label_set_text(title, "All Apps");
        lv_obj_set_style_text_font(title, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(title, colors->text, LV_PART_MAIN);
        lv_obj_align(title, LV_ALIGN_LEFT_MID, 4, 0);

        lv_obj_t *close_btn = lv_button_create(header);
        lv_obj_set_size(close_btn, 22, 22);
        lv_obj_align(close_btn, LV_ALIGN_RIGHT_MID, -4, 0);
        lv_obj_set_style_bg_color(close_btn, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(close_btn, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_color(close_btn, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_border_width(close_btn, 1, LV_PART_MAIN);
        lv_obj_set_style_radius(close_btn, 4, LV_PART_MAIN);
        lv_obj_set_style_shadow_width(close_btn, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(close_btn, 2, LV_PART_MAIN);
        lv_obj_set_style_bg_color(close_btn, colors->primary, LV_STATE_PRESSED);
        lv_obj_add_event_cb(close_btn, drawer_close_btn_cb, LV_EVENT_CLICKED, NULL);

        lv_obj_t *close_lbl = lv_label_create(close_btn);
        lv_label_set_text(close_lbl, "X");
        lv_obj_set_style_text_font(close_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(close_lbl, colors->text, LV_PART_MAIN);
        lv_obj_center(close_lbl);

        /* --- Scrollable grid area --- */
        lv_obj_t *grid_scroll = lv_obj_create(s_app_drawer);
        lv_obj_set_pos(grid_scroll, 0, 30);
        lv_obj_set_size(grid_scroll, s_app_w, s_app_h - 30);
        lv_obj_set_style_bg_opa(grid_scroll, LV_OPA_TRANSP, LV_PART_MAIN);
        lv_obj_set_style_border_width(grid_scroll, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(grid_scroll, 6, LV_PART_MAIN);
        lv_obj_set_style_pad_row(grid_scroll, 6, LV_PART_MAIN);
        lv_obj_set_style_pad_column(grid_scroll, 6, LV_PART_MAIN);
        lv_obj_set_style_radius(grid_scroll, 0, LV_PART_MAIN);
        lv_obj_set_scroll_dir(grid_scroll, LV_DIR_VER);

        /* 4-column flex wrap */
        lv_obj_set_flex_flow(grid_scroll, LV_FLEX_FLOW_ROW_WRAP);
        lv_obj_set_flex_align(grid_scroll,
                              LV_FLEX_ALIGN_START,
                              LV_FLEX_ALIGN_CENTER,
                              LV_FLEX_ALIGN_START);

        s_drawer_grid = grid_scroll;

        populate_app_drawer();
    }

    lv_obj_clear_flag(s_app_drawer, LV_OBJ_FLAG_HIDDEN);
    lv_obj_move_foreground(s_app_drawer);
    s_drawer_visible = true;
}

static void close_app_drawer(void)
{
    if (s_app_drawer) {
        lv_obj_add_flag(s_app_drawer, LV_OBJ_FLAG_HIDDEN);
    }
    s_drawer_visible = false;
}

/* ------------------------------------------------------------------ */
/* Tools folder                                                        */
/* ------------------------------------------------------------------ */

static void open_folder(void)
{
    const theme_colors_t *colors = theme_get_colors();

    if (!s_folder) {
        int panel_w = s_app_w > 250 ? 230 : s_app_w - 24;
        int panel_h = s_app_h > 250 ? 184 : s_app_h - 28;

        s_folder = lv_obj_create(s_root);
        lv_obj_set_size(s_folder, s_app_w, s_app_h);
        lv_obj_set_pos(s_folder, 0, 0);
        lv_obj_set_style_bg_color(s_folder, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_folder, LV_OPA_60, LV_PART_MAIN);
        lv_obj_set_style_border_width(s_folder, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(s_folder, 0, LV_PART_MAIN);
        lv_obj_set_style_radius(s_folder, 0, LV_PART_MAIN);
        lv_obj_clear_flag(s_folder, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(s_folder, LV_OBJ_FLAG_CLICKABLE);
        lv_obj_add_event_cb(s_folder, folder_backdrop_clicked_cb,
                            LV_EVENT_CLICKED, NULL);

        lv_obj_t *panel = lv_obj_create(s_folder);
        lv_obj_set_size(panel, panel_w, panel_h);
        lv_obj_center(panel);
        lv_obj_set_style_bg_color(panel, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(panel, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_color(panel, colors->primary, LV_PART_MAIN);
        lv_obj_set_style_border_width(panel, 1, LV_PART_MAIN);
        lv_obj_set_style_radius(panel, 12, LV_PART_MAIN);
        lv_obj_set_style_pad_all(panel, 6, LV_PART_MAIN);
        lv_obj_clear_flag(panel, LV_OBJ_FLAG_SCROLLABLE);

        lv_obj_t *title = lv_label_create(panel);
        lv_label_set_text(title, "Tools");
        lv_obj_set_style_text_color(title, colors->text, LV_PART_MAIN);
        lv_obj_set_style_text_font(title, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_align(title, LV_ALIGN_TOP_LEFT, 4, 2);

        lv_obj_t *close_btn = lv_button_create(panel);
        lv_obj_set_size(close_btn, 24, 24);
        lv_obj_align(close_btn, LV_ALIGN_TOP_RIGHT, 0, -2);
        lv_obj_set_style_bg_color(close_btn, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_border_width(close_btn, 0, LV_PART_MAIN);
        lv_obj_set_style_shadow_width(close_btn, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(close_btn, 0, LV_PART_MAIN);
        lv_obj_add_event_cb(close_btn, folder_close_btn_cb, LV_EVENT_CLICKED, NULL);
        lv_obj_t *close_label = lv_label_create(close_btn);
        lv_label_set_text(close_label, LV_SYMBOL_CLOSE);
        lv_obj_set_style_text_color(close_label, colors->text, LV_PART_MAIN);
        lv_obj_center(close_label);

        s_folder_grid = lv_obj_create(panel);
        lv_obj_set_pos(s_folder_grid, 0, 28);
        lv_obj_set_size(s_folder_grid, panel_w - 12, panel_h - 40);
        lv_obj_set_style_bg_opa(s_folder_grid, LV_OPA_TRANSP, LV_PART_MAIN);
        lv_obj_set_style_border_width(s_folder_grid, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(s_folder_grid, 2, LV_PART_MAIN);
        lv_obj_set_style_pad_row(s_folder_grid, 4, LV_PART_MAIN);
        lv_obj_set_style_pad_column(s_folder_grid, 4, LV_PART_MAIN);
        lv_obj_set_flex_flow(s_folder_grid, LV_FLEX_FLOW_ROW_WRAP);
        lv_obj_set_flex_align(s_folder_grid, LV_FLEX_ALIGN_CENTER,
                              LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_START);

        const app_manifest_t *apps[20];
        int count = app_manager_list_apps(apps, 20);
        int cell_w = (panel_w - 28) / 3;
        int cell_h = panel_h > 170 ? 66 : 58;
        for (int i = 0; i < count; i++) {
            if (!is_tool_app(apps[i]->id)) continue;
            create_app_cell(s_folder_grid, app_icon_symbol(apps[i]->id),
                            apps[i]->name, apps[i]->id, cell_w, cell_h);
        }
    }

    close_app_drawer();
    lv_obj_clear_flag(s_folder, LV_OBJ_FLAG_HIDDEN);
    lv_obj_move_foreground(s_folder);
    s_folder_visible = true;
}

static void close_folder(void)
{
    if (s_folder) lv_obj_add_flag(s_folder, LV_OBJ_FLAG_HIDDEN);
    s_folder_visible = false;
}

static void folder_btn_clicked_cb(lv_event_t *e)
{
    (void)e;
    if (s_folder_visible) close_folder();
    else open_folder();
}

/* ------------------------------------------------------------------ */
/* "Apps" button callback                                              */
/* ------------------------------------------------------------------ */

static void apps_btn_clicked_cb(lv_event_t *e)
{
    (void)e;
    if (s_drawer_visible) {
        close_app_drawer();
    } else {
        close_folder();
        open_app_drawer();
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                          */
/* ------------------------------------------------------------------ */

esp_err_t launcher_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "creating ThistleOS launcher UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Read actual dimensions from parent */
    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;  /* fallback */
    if (s_app_h == 0) s_app_h = 296;

    const theme_colors_t *colors = theme_get_colors();

    /* Root container — fills the entire app area */
    s_root = lv_obj_create(parent);
    lv_obj_set_size(s_root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_root, 0, 0);
    lv_obj_set_style_bg_opa(s_root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_root, LV_OBJ_FLAG_SCROLLABLE);

    /* Home deliberately has no app grid. The system status bar and persistent
     * bottom dock are the only chrome; Apps in the dock opens the full grid. */
    const app_manifest_t *apps[28];
    int app_count = app_manager_list_apps(apps, 28);

    /* ------------------------------------------------------------------
     * Favorites dock — bottom DOCK_H px, 1px top border
     * ------------------------------------------------------------------ */
    lv_obj_t *dock = lv_obj_create(s_root);
    lv_obj_set_pos(dock, 0, s_app_h - DOCK_H);
    lv_obj_set_size(dock, s_app_w, DOCK_H);
    lv_obj_set_style_bg_color(dock, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(dock, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(dock, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(dock, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(dock, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_all(dock, 6, LV_PART_MAIN);
    lv_obj_set_style_radius(dock, 0, LV_PART_MAIN);
    lv_obj_clear_flag(dock, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_set_flex_flow(dock, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(dock,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(dock, 8, LV_PART_MAIN);

    /* Populate the dock only with installed apps, preserving the stable
     * preference order. */
    const char *dock_ids[4] = {NULL, NULL, NULL, NULL};
    int dock_count = 0;
    for (int i = 0; i < MAX_DOCK_FAVORITES; i++) {
        if (!s_dock_favorites[i]) break;
        for (int j = 0; j < app_count && dock_count < 4; j++) {
            if (strcmp(s_dock_favorites[i], apps[j]->id) == 0) {
                dock_ids[dock_count++] = apps[j]->id;
                break;
            }
        }
    }
    for (int i = 0; i < app_count && dock_count < 4; i++) {
        if (is_launcher(apps[i]->id) || is_tool_app(apps[i]->id)) continue;
        bool duplicate = false;
        for (int j = 0; j < dock_count; j++) {
            if (strcmp(dock_ids[j], apps[i]->id) == 0) duplicate = true;
        }
        if (!duplicate) dock_ids[dock_count++] = apps[i]->id;
    }
    int apps_position = dock_count < 2 ? dock_count : 2;
    for (int i = 0; i < dock_count; i++) {
        if (i == apps_position) {
            create_dock_icon(dock, LV_SYMBOL_LIST, NULL);
        }
        create_dock_icon(dock, app_icon_symbol(dock_ids[i]), dock_ids[i]);
    }
    if (apps_position == dock_count) {
        create_dock_icon(dock, LV_SYMBOL_LIST, NULL);
    }

    /* Overlays start hidden and are created lazily on first open. */
    s_app_drawer    = NULL;
    s_drawer_grid   = NULL;
    s_folder        = NULL;
    s_folder_grid   = NULL;
    s_drawer_visible = false;
    s_folder_visible = false;

    return ESP_OK;
}

void launcher_ui_show(void)
{
    if (s_root) {
        lv_obj_clear_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}

void launcher_ui_hide(void)
{
    /* Close transient layers so returning always shows the home screen. */
    if (s_drawer_visible) {
        close_app_drawer();
    }
    if (s_folder_visible) {
        close_folder();
    }
    if (s_root) {
        lv_obj_add_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}
