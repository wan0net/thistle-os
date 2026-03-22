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

/* App-area dimensions (240x296 portrait after the 24px status bar) */
#define APP_AREA_W      240
#define APP_AREA_H      296
#define DOCK_H           50
#define APPS_BTN_H       30
#define ICON_SIZE        38
#define CELL_SIZE        70
#define DRAWER_COLS       4

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
static bool      s_drawer_visible = false;

/* ------------------------------------------------------------------ */
/* App icon letter mapping                                             */
/* ------------------------------------------------------------------ */

static const char *app_icon_letter(const char *app_id)
{
    if (strstr(app_id, "settings"))    return "S";
    if (strstr(app_id, "filemgr"))     return "F";
    if (strstr(app_id, "reader"))      return "R";
    if (strstr(app_id, "messenger"))   return "M";
    if (strstr(app_id, "navigator"))   return "N";
    if (strstr(app_id, "notes"))       return "No";
    if (strstr(app_id, "assistant"))   return "AI";
    if (strstr(app_id, "appstore"))    return "St";
    if (strstr(app_id, "wifiscanner")) return "Wi";
    if (strstr(app_id, "flashlight"))  return "FL";
    if (strstr(app_id, "weather"))     return "Wx";
    if (strstr(app_id, "terminal"))    return "Tm";
    if (strstr(app_id, "vault"))       return "Vt";
    return "?";
}

/* ------------------------------------------------------------------ */
/* Click handlers                                                      */
/* ------------------------------------------------------------------ */

static void close_app_drawer(void);

static void app_cell_clicked_cb(lv_event_t *e)
{
    const char *app_id = (const char *)lv_obj_get_user_data(lv_event_get_target(e));
    if (!app_id) {
        ESP_LOGW(TAG, "app cell clicked: no app_id");
        return;
    }

    close_app_drawer();

    ESP_LOGI(TAG, "Launching app from drawer: %s", app_id);
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

/* ------------------------------------------------------------------ */
/* App grid cell                                                       */
/* ------------------------------------------------------------------ */

static lv_obj_t *create_app_cell(lv_obj_t *parent, const char *letter,
                                  const char *name, const char *app_id)
{
    const theme_colors_t *c = theme_get_colors();

    lv_obj_t *cell = lv_obj_create(parent);
    lv_obj_set_size(cell, CELL_SIZE, CELL_SIZE);
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

    /* Icon letter */
    lv_obj_t *icon = lv_label_create(cell);
    lv_label_set_text(icon, letter);
    lv_obj_set_style_text_font(icon, &lv_font_montserrat_22, LV_PART_MAIN);
    lv_obj_set_style_text_color(icon, c->text, LV_PART_MAIN);

    /* App name (small, below icon) */
    lv_obj_t *lbl = lv_label_create(cell);
    lv_label_set_text(lbl, name);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl, c->text_secondary, LV_PART_MAIN);
    lv_label_set_long_mode(lbl, LV_LABEL_LONG_DOT);
    lv_obj_set_width(lbl, CELL_SIZE - 6);
    lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);

    /* Click handler — store app_id as user data */
    lv_obj_add_flag(cell, LV_OBJ_FLAG_CLICKABLE);
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

    lv_obj_set_user_data(btn, (void *)app_id);
    lv_obj_add_event_cb(btn, dock_icon_clicked_cb, LV_EVENT_CLICKED, NULL);

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

        const char *letter = app_icon_letter(apps[i]->id);
        create_app_cell(s_drawer_grid, letter, apps[i]->name, apps[i]->id);
    }
}

static void open_app_drawer(void)
{
    const theme_colors_t *colors = theme_get_colors();

    if (!s_app_drawer) {
        /* Full-screen overlay on top of the home screen */
        s_app_drawer = lv_obj_create(s_root);
        lv_obj_set_size(s_app_drawer, APP_AREA_W, APP_AREA_H);
        lv_obj_set_pos(s_app_drawer, 0, 0);
        lv_obj_set_style_bg_color(s_app_drawer, colors->bg, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_app_drawer, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_width(s_app_drawer, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_all(s_app_drawer, 0, LV_PART_MAIN);
        lv_obj_set_style_radius(s_app_drawer, 0, LV_PART_MAIN);
        lv_obj_clear_flag(s_app_drawer, LV_OBJ_FLAG_SCROLLABLE);

        /* --- Header bar --- */
        lv_obj_t *header = lv_obj_create(s_app_drawer);
        lv_obj_set_size(header, APP_AREA_W, 30);
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
        lv_obj_set_size(grid_scroll, APP_AREA_W, APP_AREA_H - 30);
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
/* "Apps" button callback                                              */
/* ------------------------------------------------------------------ */

static void apps_btn_clicked_cb(lv_event_t *e)
{
    (void)e;
    if (s_drawer_visible) {
        close_app_drawer();
    } else {
        open_app_drawer();
    }
}

/* ------------------------------------------------------------------ */
/* Clock update timer callback                                         */
/* ------------------------------------------------------------------ */

static void launcher_clock_update(lv_timer_t *timer)
{
    lv_obj_t *clock_label = (lv_obj_t *)lv_timer_get_user_data(timer);
    char time_buf[8];
    wifi_manager_get_time_str(time_buf, sizeof(time_buf));
    lv_label_set_text(clock_label, time_buf);
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

    /* ------------------------------------------------------------------
     * Wallpaper area — sits above the Apps button and dock.
     * Height: APP_AREA_H minus dock minus apps-button row.
     * ------------------------------------------------------------------ */
    int wallpaper_h = APP_AREA_H - APPS_BTN_H - DOCK_H;

    lv_obj_t *wallpaper = lv_obj_create(s_root);
    lv_obj_set_pos(wallpaper, 0, 0);
    lv_obj_set_size(wallpaper, APP_AREA_W, wallpaper_h);
    lv_obj_set_style_bg_color(wallpaper, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(wallpaper, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(wallpaper, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(wallpaper, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(wallpaper, 0, LV_PART_MAIN);
    lv_obj_clear_flag(wallpaper, LV_OBJ_FLAG_SCROLLABLE);

    /* Center column: clock + brand */
    lv_obj_set_flex_flow(wallpaper, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(wallpaper,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(wallpaper, 6, LV_PART_MAIN);

    /* Large clock */
    lv_obj_t *lbl_clock = lv_label_create(wallpaper);
    char time_init[8];
    wifi_manager_get_time_str(time_init, sizeof(time_init));
    lv_label_set_text(lbl_clock, time_init);
    lv_obj_set_style_text_font(lbl_clock, &lv_font_montserrat_22, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_clock, colors->text, LV_PART_MAIN);

    lv_timer_create(launcher_clock_update, 10000, lbl_clock);

    /* Branding subtitle */
    lv_obj_t *lbl_brand = lv_label_create(wallpaper);
    lv_label_set_text(lbl_brand, "ThistleOS");
    lv_obj_set_style_text_font(lbl_brand, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_brand, colors->text_secondary, LV_PART_MAIN);

    /* ------------------------------------------------------------------
     * "Apps" button — centered strip between wallpaper and dock
     * ------------------------------------------------------------------ */
    lv_obj_t *apps_row = lv_obj_create(s_root);
    lv_obj_set_pos(apps_row, 0, wallpaper_h);
    lv_obj_set_size(apps_row, APP_AREA_W, APPS_BTN_H);
    lv_obj_set_style_bg_color(apps_row, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(apps_row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(apps_row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(apps_row, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(apps_row, 0, LV_PART_MAIN);
    lv_obj_clear_flag(apps_row, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(apps_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(apps_row,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);

    lv_obj_t *apps_btn = lv_button_create(apps_row);
    lv_obj_set_size(apps_btn, 90, 22);
    lv_obj_set_style_bg_color(apps_btn, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(apps_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(apps_btn, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(apps_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(apps_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_shadow_width(apps_btn, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(apps_btn, 2, LV_PART_MAIN);
    lv_obj_set_style_bg_color(apps_btn, colors->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(apps_btn, LV_OPA_COVER, LV_STATE_PRESSED);
    lv_obj_add_event_cb(apps_btn, apps_btn_clicked_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *apps_lbl = lv_label_create(apps_btn);
    lv_label_set_text(apps_lbl, "Apps ^");
    lv_obj_set_style_text_font(apps_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(apps_lbl, colors->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(apps_lbl, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_center(apps_lbl);

    /* ------------------------------------------------------------------
     * Favorites dock — bottom DOCK_H px, 1px top border
     * ------------------------------------------------------------------ */
    lv_obj_t *dock = lv_obj_create(s_root);
    lv_obj_set_pos(dock, 0, APP_AREA_H - DOCK_H);
    lv_obj_set_size(dock, APP_AREA_W, DOCK_H);
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

    /* Populate dock from favorites list */
    for (int i = 0; i < MAX_DOCK_FAVORITES; i++) {
        if (!s_dock_favorites[i]) break;
        const char *id     = s_dock_favorites[i];
        const char *letter = app_icon_letter(id);
        create_dock_icon(dock, letter, id);
    }

    /* Drawer starts hidden; created lazily on first open */
    s_app_drawer    = NULL;
    s_drawer_grid   = NULL;
    s_drawer_visible = false;

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
    /* If the drawer happens to be open, close it cleanly */
    if (s_drawer_visible) {
        close_app_drawer();
    }
    if (s_root) {
        lv_obj_add_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}
