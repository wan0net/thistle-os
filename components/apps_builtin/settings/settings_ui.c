#include "settings/settings_app.h"

#include "lvgl.h"
#include "esp_log.h"

static const char *TAG = "settings_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W      320
#define APP_AREA_H      216
#define TITLE_BAR_H      30
#define ITEM_H           30
#define ITEM_PAD_LEFT     8
#define ITEM_PAD_RIGHT    6

/* ------------------------------------------------------------------ */
/* Settings item definitions                                            */
/* ------------------------------------------------------------------ */

typedef struct {
    const char *name;
    const char *value; /* NULL = no value shown */
} settings_item_t;

static const settings_item_t s_items[] = {
    { "Display",   NULL       },
    { "WiFi",      "Off"      },
    { "Bluetooth", "Off"      },
    { "Radio",     "915 MHz"  },
    { "Storage",   NULL       },
    { "About",     NULL       },
};
#define ITEMS_COUNT (sizeof(s_items) / sizeof(s_items[0]))

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static lv_obj_t *s_root = NULL;

/* ------------------------------------------------------------------ */
/* Internal callbacks                                                   */
/* ------------------------------------------------------------------ */

static void item_clicked_cb(lv_event_t *e)
{
    const char *name = (const char *)lv_event_get_user_data(e);
    ESP_LOGI(TAG, "Settings: opened %s", name ? name : "?");
}

/* ------------------------------------------------------------------ */
/* Helper: create one list row                                          */
/* ------------------------------------------------------------------ */

static void create_list_item(lv_obj_t *list, const settings_item_t *item)
{
    /* Row container */
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);

    /* Bottom separator: 1px black border on bottom edge only */
    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);

    /* Pressed state: invert */
    lv_obj_set_style_bg_color(row, lv_color_black(), LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);

    /* Flex row layout, vertically centred */
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 4, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

    /* Make the row tappable */
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(row, item_clicked_cb, LV_EVENT_CLICKED,
                        (void *)item->name);

    /* Name label — flex_grow=1 so it fills available space */
    lv_obj_t *lbl_name = lv_label_create(row);
    lv_label_set_text(lbl_name, item->name);
    lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, lv_color_white(), LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);

    /* Value label (optional, dimmer via smaller font) */
    if (item->value != NULL) {
        lv_obj_t *lbl_val = lv_label_create(row);
        lv_label_set_text(lbl_val, item->value);
        lv_obj_set_style_text_font(lbl_val, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_val, lv_color_black(), LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_val, lv_color_white(), LV_STATE_PRESSED);
        /* Inherit pressed state colour from parent via opa trick isn't
         * available in LVGL 9 child state propagation, so we keep it black
         * and let the row bg inversion carry the visual weight. */
    }

    /* Chevron indicator */
    lv_obj_t *lbl_chevron = lv_label_create(row);
    lv_label_set_text(lbl_chevron, ">");
    lv_obj_set_style_text_font(lbl_chevron, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chevron, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_chevron, lv_color_white(), LV_STATE_PRESSED);
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t settings_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating settings UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Root container — fills the entire app area, transparent bg */
    s_root = lv_obj_create(parent);
    lv_obj_set_size(s_root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_root, 0, 0);
    lv_obj_set_style_bg_opa(s_root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_root, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Title bar (30px)
     * ---------------------------------------------------------------- */
    lv_obj_t *title_bar = lv_obj_create(s_root);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    lv_obj_set_style_bg_color(title_bar, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(title_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(title_bar, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(title_bar, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(title_bar, 0, LV_PART_MAIN);
    /* 1px bottom border separates title bar from list */
    lv_obj_set_style_border_side(title_bar, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(title_bar, lv_color_black(), LV_PART_MAIN);
    lv_obj_set_style_border_width(title_bar, 1, LV_PART_MAIN);
    lv_obj_clear_flag(title_bar, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *lbl_title = lv_label_create(title_bar);
    lv_label_set_text(lbl_title, "Settings");
    lv_obj_set_style_text_font(lbl_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_title, lv_color_black(), LV_PART_MAIN);
    lv_obj_align(lbl_title, LV_ALIGN_LEFT_MID, 0, 0);

    /* ----------------------------------------------------------------
     * Scrollable list container below title bar
     * ---------------------------------------------------------------- */
    lv_obj_t *list = lv_obj_create(s_root);
    lv_obj_set_pos(list, 0, TITLE_BAR_H);
    lv_obj_set_size(list, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(list, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(list, 0, LV_PART_MAIN);

    /* Vertical flex column; items stretch to full width */
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(list,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);

    /* Scrollbar: minimal, black thumb, no track */
    lv_obj_set_scrollbar_mode(list, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(list, lv_color_black(), LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(list, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(list, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(list, 0, LV_PART_SCROLLBAR);

    /* Populate list items */
    for (size_t i = 0; i < ITEMS_COUNT; i++) {
        create_list_item(list, &s_items[i]);
    }

    return ESP_OK;
}

void settings_ui_show(void)
{
    if (s_root) {
        lv_obj_clear_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}

void settings_ui_hide(void)
{
    if (s_root) {
        lv_obj_add_flag(s_root, LV_OBJ_FLAG_HIDDEN);
    }
}
