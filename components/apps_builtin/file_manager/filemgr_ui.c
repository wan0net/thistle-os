#include "file_manager/filemgr_app.h"

#include "hal/board.h"
#include "hal/sdcard_path.h"
#include "ui/theme.h"

#include "lvgl.h"
#include "esp_log.h"

#include <dirent.h>
#include <sys/stat.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

static const char *TAG = "filemgr_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W      320
#define APP_AREA_H      216
#define TITLE_BAR_H      30
#define STORAGE_BAR_H    24
#define LIST_H          (APP_AREA_H - TITLE_BAR_H - STORAGE_BAR_H)
#define ITEM_H           30
#define ITEM_PAD_LEFT     8
#define ITEM_PAD_RIGHT    6
#define MAX_ENTRIES      50

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

typedef struct {
    char     name[256];
    bool     is_dir;
    off_t    size;
} fm_entry_t;

static struct {
    char        current_path[256];
    lv_obj_t   *root;
    lv_obj_t   *title_label;
    lv_obj_t   *list_container;
    lv_obj_t   *storage_label;
} s_fm;

/* ------------------------------------------------------------------ */
/* Helpers                                                              */
/* ------------------------------------------------------------------ */

static void format_size(off_t size, char *buf, size_t buf_len)
{
    if (size < 1024)
        snprintf(buf, buf_len, "%ld B", (long)size);
    else if (size < 1024 * 1024)
        snprintf(buf, buf_len, "%.1f KB", size / 1024.0);
    else if (size < (off_t)1024 * 1024 * 1024)
        snprintf(buf, buf_len, "%.1f MB", size / (1024.0 * 1024.0));
    else
        snprintf(buf, buf_len, "%.1f GB", size / (1024.0 * 1024.0 * 1024.0));
}

/* Compare function: directories first, then alphabetical within each group */
static int entry_cmp(const void *a, const void *b)
{
    const fm_entry_t *ea = (const fm_entry_t *)a;
    const fm_entry_t *eb = (const fm_entry_t *)b;

    if (ea->is_dir != eb->is_dir) {
        return ea->is_dir ? -1 : 1;
    }
    return strcmp(ea->name, eb->name);
}

/* Forward declaration */
static void navigate_to(const char *path);

/* ------------------------------------------------------------------ */
/* File-type indicator by extension                                     */
/* ------------------------------------------------------------------ */

static const char *file_type_indicator(const char *name)
{
    /* Check compound extension first */
    const char *app_ext = strstr(name, ".app.elf");
    if (app_ext && app_ext[8] == '\0') return "[A]";

    const char *dot = strrchr(name, '.');
    if (!dot) return "[F]";

    if (strcmp(dot, ".txt") == 0 || strcmp(dot, ".md") == 0 ||
        strcmp(dot, ".log") == 0) return "[T]";

    if (strcmp(dot, ".json") == 0 || strcmp(dot, ".csv") == 0) return "[D]";

    if (strcmp(dot, ".bin") == 0 || strcmp(dot, ".img") == 0) return "[B]";

    return "[F]";
}

/* ------------------------------------------------------------------ */
/* Title-bar click handler — navigate to parent                        */
/* ------------------------------------------------------------------ */

static void title_clicked_cb(lv_event_t *e)
{
    (void)e;
    if (strcmp(s_fm.current_path, THISTLE_SDCARD) == 0) return;

    char parent[256];
    strncpy(parent, s_fm.current_path, sizeof(parent) - 1);
    parent[sizeof(parent) - 1] = '\0';

    char *last_slash = strrchr(parent, '/');
    if (last_slash && last_slash != parent) {
        *last_slash = '\0';
    } else {
        strncpy(parent, THISTLE_SDCARD, sizeof(parent) - 1);
    }

    if (strncmp(parent, THISTLE_SDCARD, strlen(THISTLE_SDCARD)) != 0) {
        strncpy(parent, THISTLE_SDCARD, sizeof(parent) - 1);
    }

    navigate_to(parent);
}

/* ------------------------------------------------------------------ */
/* Delete handler — frees malloc'd user_data on object deletion        */
/* ------------------------------------------------------------------ */

static void entry_delete_cb(lv_event_t *e)
{
    lv_obj_t *row      = lv_event_get_target(e);
    void     *user_data = lv_obj_get_user_data(row);
    if (user_data) {
        free(user_data);
        lv_obj_set_user_data(row, NULL);
    }
}

/* ------------------------------------------------------------------ */
/* Click handler — carries full path in user_data                      */
/* ------------------------------------------------------------------ */

static void entry_clicked_cb(lv_event_t *e)
{
    lv_obj_t   *row      = lv_event_get_target(e);
    const char *full_path = (const char *)lv_obj_get_user_data(row);

    if (!full_path) return;

    /* Determine if it is a directory by stat */
    struct stat st;
    if (stat(full_path, &st) == 0 && S_ISDIR(st.st_mode)) {
        navigate_to(full_path);
    } else {
        ESP_LOGI(TAG, "Selected: %s", full_path);
    }
}

/* ------------------------------------------------------------------ */
/* Row creation                                                         */
/* ------------------------------------------------------------------ */

static void create_entry_row(lv_obj_t *list, const char *base_path,
                             const fm_entry_t *entry)
{
    const theme_colors_t *clr = theme_get_colors();

    /* Build full path — stored as user_data for the click handler */
    char *full_path = malloc(512);
    if (!full_path) return;

    if (strcmp(base_path, "/") == 0) {
        snprintf(full_path, 512, "/%s", entry->name);
    } else {
        snprintf(full_path, 512, "%s/%s", base_path, entry->name);
    }

    /* Row container */
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);

    /* 1px bottom border separator */
    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, clr->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);

    /* Pressed state: primary highlight */
    lv_obj_set_style_bg_color(row, clr->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);

    /* Flex row, items vertically centred */
    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 6, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

    /* Clickable; user_data carries the full path */
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_user_data(row, full_path);
    lv_obj_add_event_cb(row, entry_clicked_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_event_cb(row, entry_delete_cb,  LV_EVENT_DELETE,  NULL);

    /* Type indicator: extension-aware for files, "[D]" for dirs */
    lv_obj_t *lbl_type = lv_label_create(row);
    lv_label_set_text(lbl_type, entry->is_dir ? "[D]" : file_type_indicator(entry->name));
    lv_obj_set_style_text_font(lbl_type, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_type, clr->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_type, clr->bg, LV_STATE_PRESSED);

    /* Filename — flex_grow=1 so it fills remaining space */
    lv_obj_t *lbl_name = lv_label_create(row);
    lv_label_set_text(lbl_name, entry->name);
    lv_label_set_long_mode(lbl_name, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, clr->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, clr->bg, LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);

    /* Size label — only for files */
    if (!entry->is_dir) {
        char size_str[16];
        format_size(entry->size, size_str, sizeof(size_str));

        lv_obj_t *lbl_size = lv_label_create(row);
        lv_label_set_text(lbl_size, size_str);
        lv_obj_set_style_text_font(lbl_size, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_size, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_size, clr->bg, LV_STATE_PRESSED);
    }
}

/* ------------------------------------------------------------------ */
/* Parent-directory ".." row                                           */
/* ------------------------------------------------------------------ */

static void parent_clicked_cb(lv_event_t *e)
{
    lv_obj_t   *row        = lv_event_get_target(e);
    const char *parent_path = (const char *)lv_obj_get_user_data(row);
    if (parent_path) {
        navigate_to(parent_path);
    }
}

static void create_parent_row(lv_obj_t *list, const char *parent_path)
{
    const theme_colors_t *clr = theme_get_colors();

    char *stored_path = malloc(256);
    if (!stored_path) return;
    strncpy(stored_path, parent_path, 255);
    stored_path[255] = '\0';

    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), ITEM_H);
    lv_obj_set_style_bg_color(row, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(row, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(row, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);

    lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(row, clr->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(row, 1, LV_PART_MAIN);

    lv_obj_set_style_bg_color(row, clr->primary, LV_STATE_PRESSED);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_STATE_PRESSED);

    lv_obj_set_flex_flow(row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(row,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(row, 6, LV_PART_MAIN);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_user_data(row, stored_path);
    lv_obj_add_event_cb(row, parent_clicked_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_event_cb(row, entry_delete_cb,   LV_EVENT_DELETE,  NULL);

    lv_obj_t *lbl_type = lv_label_create(row);
    lv_label_set_text(lbl_type, "[D]");
    lv_obj_set_style_text_font(lbl_type, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_type, clr->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_type, clr->bg, LV_STATE_PRESSED);

    lv_obj_t *lbl_name = lv_label_create(row);
    lv_label_set_text(lbl_name, "..");
    lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, clr->text, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_name, clr->bg, LV_STATE_PRESSED);
    lv_obj_set_flex_grow(lbl_name, 1);
}

/* ------------------------------------------------------------------ */
/* navigate_to                                                          */
/* ------------------------------------------------------------------ */

static void navigate_to(const char *path)
{
    /* Store current path */
    strncpy(s_fm.current_path, path, sizeof(s_fm.current_path) - 1);
    s_fm.current_path[sizeof(s_fm.current_path) - 1] = '\0';

    /* Update title label */
    char title_buf[280];
    snprintf(title_buf, sizeof(title_buf), "< Files: %s", path);
    lv_label_set_text(s_fm.title_label, title_buf);

    /* Clear existing list entries */
    lv_obj_clean(s_fm.list_container);

    /* Reset scroll position to top */
    lv_obj_scroll_to_y(s_fm.list_container, 0, LV_ANIM_OFF);

    /* Open directory */
    DIR *dir = opendir(path);
    if (!dir) {
        const theme_colors_t *clr = theme_get_colors();
        ESP_LOGW(TAG, "Cannot open directory: %s", path);
        lv_obj_t *lbl = lv_label_create(s_fm.list_container);
        lv_label_set_text(lbl, "SD card not mounted");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text, LV_PART_MAIN);
        lv_obj_set_style_pad_all(lbl, 8, LV_PART_MAIN);
        return;
    }

    /* Add ".." row if not at root mount point */
    if (strcmp(path, THISTLE_SDCARD) != 0) {
        /* Compute parent path */
        char parent[256];
        strncpy(parent, path, sizeof(parent) - 1);
        parent[sizeof(parent) - 1] = '\0';

        char *last_slash = strrchr(parent, '/');
        if (last_slash && last_slash != parent) {
            *last_slash = '\0';
        } else {
            /* Would go above root of VFS — clamp to /sdcard */
            strncpy(parent, THISTLE_SDCARD, sizeof(parent) - 1);
        }

        /* Make sure we don't go above /sdcard */
        if (strncmp(parent, THISTLE_SDCARD, strlen(THISTLE_SDCARD)) != 0) {
            strncpy(parent, THISTLE_SDCARD, sizeof(parent) - 1);
        }

        create_parent_row(s_fm.list_container, parent);
    }

    /* Collect directory entries */
    fm_entry_t entries[MAX_ENTRIES];
    int        count = 0;

    struct dirent *de;
    while ((de = readdir(dir)) != NULL && count < MAX_ENTRIES) {
        /* Skip "." and ".." */
        if (strcmp(de->d_name, ".") == 0 || strcmp(de->d_name, "..") == 0) {
            continue;
        }

        /* Skip hidden files (dot-files) */
        if (de->d_name[0] == '.') {
            continue;
        }

        /* Build full path for stat */
        char full[512];
        if (strcmp(path, "/") == 0) {
            snprintf(full, sizeof(full), "/%s", de->d_name);
        } else {
            snprintf(full, sizeof(full), "%s/%s", path, de->d_name);
        }

        struct stat st;
        if (stat(full, &st) != 0) {
            /* If stat fails, still show the entry; treat as file with 0 size */
            memset(&st, 0, sizeof(st));
        }

        strncpy(entries[count].name, de->d_name, sizeof(entries[count].name) - 1);
        entries[count].name[sizeof(entries[count].name) - 1] = '\0';
        entries[count].is_dir = S_ISDIR(st.st_mode);
        entries[count].size   = st.st_size;
        count++;
    }

    closedir(dir);

    /* Sort: directories first, then alphabetical */
    qsort(entries, count, sizeof(fm_entry_t), entry_cmp);

    /* Populate list rows */
    for (int i = 0; i < count; i++) {
        create_entry_row(s_fm.list_container, path, &entries[i]);
    }

    /* Show "Empty directory" message if no entries (after filtering) */
    if (count == 0) {
        const theme_colors_t *clr = theme_get_colors();
        lv_obj_t *lbl_empty = lv_label_create(s_fm.list_container);
        lv_label_set_text(lbl_empty, "Empty directory");
        lv_obj_set_style_text_font(lbl_empty, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_empty, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_width(lbl_empty, LV_PCT(100));
        lv_obj_set_style_text_align(lbl_empty, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_style_pad_top(lbl_empty, 16, LV_PART_MAIN);
    }

    /* Update storage info bar */
    const hal_registry_t *reg = hal_get_registry();
    bool found_sd = false;

    for (uint8_t i = 0; i < reg->storage_count; i++) {
        const hal_storage_driver_t *drv = reg->storage[i];
        if (!drv) continue;
        if (drv->type == HAL_STORAGE_TYPE_SD && drv->is_mounted && drv->is_mounted()) {
            uint64_t total = drv->get_total_bytes ? drv->get_total_bytes() : 0;
            uint64_t free_ = drv->get_free_bytes  ? drv->get_free_bytes()  : 0;

            char total_str[16], free_str[16];
            format_size((off_t)total, total_str, sizeof(total_str));
            format_size((off_t)free_,  free_str,  sizeof(free_str));

            char storage_buf[64];
            snprintf(storage_buf, sizeof(storage_buf), "Free: %s / %s", free_str, total_str);
            lv_label_set_text(s_fm.storage_label, storage_buf);
            found_sd = true;
            break;
        }
    }

    if (!found_sd) {
        lv_label_set_text(s_fm.storage_label, "SD card not mounted");
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t filemgr_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating file manager UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Root container — fills the entire app area, transparent bg */
    s_fm.root = lv_obj_create(parent);
    lv_obj_set_size(s_fm.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_fm.root, 0, 0);
    lv_obj_set_style_bg_opa(s_fm.root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_fm.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_fm.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_fm.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_fm.root, LV_OBJ_FLAG_SCROLLABLE);

    const theme_colors_t *clr = theme_get_colors();

    /* ----------------------------------------------------------------
     * Title bar (30px)
     * ---------------------------------------------------------------- */
    lv_obj_t *title_bar = lv_obj_create(s_fm.root);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    lv_obj_set_style_bg_color(title_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(title_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(title_bar, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(title_bar, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(title_bar, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(title_bar, clr->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(title_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_bg_color(title_bar, clr->primary, LV_STATE_PRESSED);
    lv_obj_clear_flag(title_bar, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(title_bar, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(title_bar, title_clicked_cb, LV_EVENT_CLICKED, NULL);

    s_fm.title_label = lv_label_create(title_bar);
    {
        char init_title[128];
        snprintf(init_title, sizeof(init_title), "< Files: %s", THISTLE_SDCARD);
        lv_label_set_text(s_fm.title_label, init_title);
    }
    lv_label_set_long_mode(s_fm.title_label, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_font(s_fm.title_label, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_fm.title_label, clr->text, LV_PART_MAIN);
    lv_obj_align(s_fm.title_label, LV_ALIGN_LEFT_MID, 0, 0);

    /* ----------------------------------------------------------------
     * Scrollable list container
     * ---------------------------------------------------------------- */
    s_fm.list_container = lv_obj_create(s_fm.root);
    lv_obj_set_pos(s_fm.list_container, 0, TITLE_BAR_H);
    lv_obj_set_size(s_fm.list_container, APP_AREA_W, LIST_H);
    lv_obj_set_style_bg_color(s_fm.list_container, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_fm.list_container, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_fm.list_container, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_fm.list_container, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_fm.list_container, 0, LV_PART_MAIN);

    lv_obj_set_flex_flow(s_fm.list_container, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_fm.list_container,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);

    /* Thin scrollbar using primary color, no track */
    lv_obj_set_scrollbar_mode(s_fm.list_container, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_fm.list_container, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_fm.list_container, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_fm.list_container, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_fm.list_container, 0, LV_PART_SCROLLBAR);

    /* ----------------------------------------------------------------
     * Storage info bar (24px) at the bottom
     * ---------------------------------------------------------------- */
    lv_obj_t *storage_bar = lv_obj_create(s_fm.root);
    lv_obj_set_pos(storage_bar, 0, TITLE_BAR_H + LIST_H);
    lv_obj_set_size(storage_bar, APP_AREA_W, STORAGE_BAR_H);
    lv_obj_set_style_bg_color(storage_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(storage_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(storage_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(storage_bar, ITEM_PAD_LEFT, LV_PART_MAIN);
    lv_obj_set_style_pad_right(storage_bar, ITEM_PAD_RIGHT, LV_PART_MAIN);
    lv_obj_set_style_pad_top(storage_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(storage_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(storage_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(storage_bar, clr->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(storage_bar, 1, LV_PART_MAIN);
    lv_obj_clear_flag(storage_bar, LV_OBJ_FLAG_SCROLLABLE);

    s_fm.storage_label = lv_label_create(storage_bar);
    lv_label_set_text(s_fm.storage_label, "Free: -- / --");
    lv_label_set_long_mode(s_fm.storage_label, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_font(s_fm.storage_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_fm.storage_label, clr->text_secondary, LV_PART_MAIN);
    lv_obj_align(s_fm.storage_label, LV_ALIGN_LEFT_MID, 0, 0);

    /* Populate list starting at SD card root */
    navigate_to(THISTLE_SDCARD);

    return ESP_OK;
}

void filemgr_ui_show(void)
{
    if (s_fm.root) {
        lv_obj_clear_flag(s_fm.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void filemgr_ui_hide(void)
{
    if (s_fm.root) {
        lv_obj_add_flag(s_fm.root, LV_OBJ_FLAG_HIDDEN);
    }
}
