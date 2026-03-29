#include "hal/sdcard_path.h"
/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Notes app UI
 *
 * Two modes:
 *   File list — browse /sdcard/notes/ and select or create a .txt file
 *   Editor    — full-screen lv_textarea for writing/editing notes
 */
#include "notes/notes_app.h"

#include "ui/theme.h"
#include "ui/toast.h"
#include "thistle/app_manager.h"

#include "lvgl.h"
#include "esp_log.h"

#include <dirent.h>
#include <sys/stat.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static const char *TAG = "notes_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

static int s_app_w = 240;
static int s_app_h = 296;
#define HEADER_H      30
#define ITEM_H        30
#define NOTES_PATH   THISTLE_SDCARD "/notes"
#define MAX_NOTE_FILES 64

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t *root;

    /* File list mode */
    lv_obj_t *list_screen;
    lv_obj_t *list_container;

    /* Editor mode */
    lv_obj_t *editor_screen;
    lv_obj_t *textarea;
    lv_obj_t *title_label;

    char current_file[256];
    bool has_unsaved_changes;
} s_notes;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void switch_to_list(void);
static void open_note(const char *path);

/* ------------------------------------------------------------------ */
/* File helpers                                                         */
/* ------------------------------------------------------------------ */

static void format_size(off_t size, char *buf, size_t buf_len)
{
    if (size < 1024)
        snprintf(buf, buf_len, "%ld B", (long)size);
    else
        snprintf(buf, buf_len, "%.1f KB", size / 1024.0);
}

/* Ensure /sdcard/notes/ directory exists */
static void ensure_notes_dir(void)
{
    struct stat st;
    if (stat(NOTES_PATH, &st) != 0) {
        if (mkdir(NOTES_PATH, 0755) != 0) {
            ESP_LOGW(TAG, "mkdir %s failed", NOTES_PATH);
        } else {
            ESP_LOGI(TAG, "Created %s", NOTES_PATH);
        }
    }
}

/* ------------------------------------------------------------------ */
/* File I/O                                                             */
/* ------------------------------------------------------------------ */

static esp_err_t load_note(const char *path)
{
    FILE *f = fopen(path, "r");
    if (!f) {
        ESP_LOGW(TAG, "Cannot open: %s", path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size < 0) {
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    char *content = (char *)malloc((size_t)size + 1);
    if (!content) {
        fclose(f);
        ESP_LOGE(TAG, "OOM: load_note malloc(%ld)", size);
        return ESP_ERR_NO_MEM;
    }

    size_t read_bytes = fread(content, 1, (size_t)size, f);
    fclose(f);
    if (read_bytes != (size_t)size) {
        free(content);
        return ESP_ERR_INVALID_SIZE;
    }
    content[read_bytes] = '\0';

    lv_textarea_set_text(s_notes.textarea, content);
    free(content);

    strncpy(s_notes.current_file, path, sizeof(s_notes.current_file) - 1);
    s_notes.current_file[sizeof(s_notes.current_file) - 1] = '\0';
    s_notes.has_unsaved_changes = false;

    return ESP_OK;
}

static esp_err_t save_note(void)
{
    if (s_notes.current_file[0] == '\0') return ESP_ERR_INVALID_STATE;

    const char *text = lv_textarea_get_text(s_notes.textarea);
    FILE *f = fopen(s_notes.current_file, "w");
    if (!f) {
        ESP_LOGW(TAG, "Cannot write: %s", s_notes.current_file);
        return ESP_ERR_NOT_FOUND;
    }

    fwrite(text, 1, strlen(text), f);
    fclose(f);
    s_notes.has_unsaved_changes = false;

    ESP_LOGI(TAG, "Saved: %s", s_notes.current_file);
    return ESP_OK;
}

/* Public: called by notes_app.c on_pause for auto-save */
esp_err_t notes_ui_save_if_needed(void)
{
    if (s_notes.has_unsaved_changes && s_notes.current_file[0] != '\0') {
        return save_note();
    }
    return ESP_OK;
}

/* ------------------------------------------------------------------ */
/* Event callbacks                                                      */
/* ------------------------------------------------------------------ */

static void textarea_changed_cb(lv_event_t *e)
{
    (void)e;
    s_notes.has_unsaved_changes = true;
}

static void save_btn_cb(lv_event_t *e)
{
    (void)e;
    esp_err_t err = save_note();
    if (err == ESP_OK) {
        toast_show("Saved", TOAST_SUCCESS, 1500);
    } else {
        toast_warn("Save failed");
    }
}

static void back_from_editor_cb(lv_event_t *e)
{
    (void)e;
    /* Auto-save before returning to list */
    if (s_notes.has_unsaved_changes) {
        esp_err_t err = save_note();
        if (err == ESP_OK) {
            toast_show("Saved", TOAST_SUCCESS, 1200);
        }
    }
    switch_to_list();
}

static void new_note_btn_cb(lv_event_t *e)
{
    (void)e;

    ensure_notes_dir();

    /* Generate filename: note-YYYYMMDD-HHMMSS.txt */
    time_t now = time(NULL);
    struct tm *tm_info = localtime(&now);
    char filename[64];
    if (tm_info) {
        snprintf(filename, sizeof(filename),
                 "note-%04d%02d%02d-%02d%02d%02d.txt",
                 tm_info->tm_year + 1900,
                 tm_info->tm_mon + 1,
                 tm_info->tm_mday,
                 tm_info->tm_hour,
                 tm_info->tm_min,
                 tm_info->tm_sec);
    } else {
        /* Fallback if time is unavailable */
        snprintf(filename, sizeof(filename), "note-new.txt");
    }

    char path[512];
    snprintf(path, sizeof(path), "%s/%s", NOTES_PATH, filename);

    /* Create the empty file on disk */
    FILE *f = fopen(path, "w");
    if (f) {
        fclose(f);
    } else {
        ESP_LOGW(TAG, "Could not create: %s", path);
        toast_warn("SD card error");
        return;
    }

    open_note(path);
}

/* Row click: open the note for editing */
static void note_row_clicked_cb(lv_event_t *e)
{
    lv_obj_t   *row  = lv_event_get_target(e);
    const char *path = (const char *)lv_obj_get_user_data(row);
    if (path) {
        char path_copy[512];
        strncpy(path_copy, path, sizeof(path_copy) - 1);
        path_copy[sizeof(path_copy) - 1] = '\0';
        open_note(path_copy);
    }
}

/* Free the path string stored as user_data when the row is deleted */
static void note_row_delete_cb(lv_event_t *e)
{
    void *ud = lv_obj_get_user_data(lv_event_get_target(e));
    if (ud) {
        free(ud);
        lv_obj_set_user_data(lv_event_get_target(e), NULL);
    }
}

/* ESC on list → back to launcher */
static void list_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC || key == 'q' || key == 'Q') {
        app_manager_launch("com.thistle.launcher");
    }
}

/* ESC in editor → back to list (with auto-save) */
static void editor_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC) {
        if (s_notes.has_unsaved_changes) {
            esp_err_t err = save_note();
            if (err == ESP_OK) {
                toast_show("Saved", TOAST_SUCCESS, 1200);
            }
        }
        switch_to_list();
    }
}

/* ------------------------------------------------------------------ */
/* File list population                                                 */
/* ------------------------------------------------------------------ */

static void populate_list(void)
{
    lv_obj_clean(s_notes.list_container);
    lv_obj_scroll_to_y(s_notes.list_container, 0, LV_ANIM_OFF);

    const theme_colors_t *clr = theme_get_colors();

    ensure_notes_dir();

    DIR *dir = opendir(NOTES_PATH);
    if (!dir) {
        lv_obj_t *lbl = lv_label_create(s_notes.list_container);
        lv_label_set_text(lbl, "(no notes yet)");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_width(lbl, LV_PCT(100));
        lv_obj_set_style_pad_top(lbl, 20, LV_PART_MAIN);
        ESP_LOGW(TAG, "Cannot open %s", NOTES_PATH);
        return;
    }

    /* Collect .txt entries */
    typedef struct { char name[256]; off_t size; } note_entry_t;
    note_entry_t *entries = (note_entry_t *)malloc(MAX_NOTE_FILES * sizeof(note_entry_t));
    if (!entries) {
        closedir(dir);
        return;
    }
    int count = 0;

    struct dirent *de;
    while ((de = readdir(dir)) != NULL && count < MAX_NOTE_FILES) {
        if (de->d_name[0] == '.') continue;
        const char *dot = strrchr(de->d_name, '.');
        if (!dot || strcmp(dot, ".txt") != 0) continue;

        strncpy(entries[count].name, de->d_name, sizeof(entries[count].name) - 1);
        entries[count].name[sizeof(entries[count].name) - 1] = '\0';

        char full[512];
        snprintf(full, sizeof(full), "%s/%s", NOTES_PATH, de->d_name);
        struct stat st;
        entries[count].size = (stat(full, &st) == 0) ? st.st_size : 0;
        count++;
    }
    closedir(dir);

    if (count == 0) {
        free(entries);
        lv_obj_t *lbl = lv_label_create(s_notes.list_container);
        lv_label_set_text(lbl, "(no notes yet)");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_width(lbl, LV_PCT(100));
        lv_obj_set_style_pad_top(lbl, 20, LV_PART_MAIN);
        return;
    }

    for (int i = 0; i < count; i++) {
        char *full_path = (char *)malloc(512);
        if (!full_path) continue;
        snprintf(full_path, 512, "%s/%s", NOTES_PATH, entries[i].name);

        lv_obj_t *row = lv_obj_create(s_notes.list_container);
        lv_obj_set_size(row, LV_PCT(100), ITEM_H);
        lv_obj_set_style_bg_color(row, clr->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(row, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_left(row, 8, LV_PART_MAIN);
        lv_obj_set_style_pad_right(row, 8, LV_PART_MAIN);
        lv_obj_set_style_pad_top(row, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_bottom(row, 0, LV_PART_MAIN);
        lv_obj_set_style_border_side(row, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(row, clr->text_secondary, LV_PART_MAIN);
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
        lv_obj_set_user_data(row, full_path);
        lv_obj_add_event_cb(row, note_row_clicked_cb, LV_EVENT_CLICKED, NULL);
        lv_obj_add_event_cb(row, note_row_delete_cb,  LV_EVENT_DELETE,  NULL);

        /* Filename — grows to fill available space */
        lv_obj_t *lbl_name = lv_label_create(row);
        lv_label_set_text(lbl_name, entries[i].name);
        lv_label_set_long_mode(lbl_name, LV_LABEL_LONG_CLIP);
        lv_obj_set_style_text_font(lbl_name, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_name, clr->text, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_name, clr->bg, LV_STATE_PRESSED);
        lv_obj_set_flex_grow(lbl_name, 1);

        /* Size label on the right */
        char size_str[16];
        format_size(entries[i].size, size_str, sizeof(size_str));
        lv_obj_t *lbl_size = lv_label_create(row);
        lv_label_set_text(lbl_size, size_str);
        lv_obj_set_style_text_font(lbl_size, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_size, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl_size, clr->bg, LV_STATE_PRESSED);
    }

    free(entries);
}

/* ------------------------------------------------------------------ */
/* Mode switching                                                       */
/* ------------------------------------------------------------------ */

static void switch_to_list(void)
{
    if (s_notes.editor_screen) {
        lv_obj_add_flag(s_notes.editor_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_notes.list_screen) {
        lv_obj_clear_flag(s_notes.list_screen, LV_OBJ_FLAG_HIDDEN);
    }

    /* Refresh list so updated file sizes are shown */
    populate_list();

    /* Clear editor state */
    s_notes.current_file[0] = '\0';
    s_notes.has_unsaved_changes = false;
}

static void open_note(const char *path)
{
    /* Determine the base filename for the title */
    const char *basename = strrchr(path, '/');
    basename = basename ? basename + 1 : path;

    /* Show editor, hide list */
    lv_obj_add_flag(s_notes.list_screen, LV_OBJ_FLAG_HIDDEN);
    lv_obj_clear_flag(s_notes.editor_screen, LV_OBJ_FLAG_HIDDEN);

    /* Update header title */
    lv_label_set_text(s_notes.title_label, basename);

    /* Focus textarea so keyboard input is captured */
    lv_obj_clear_flag(s_notes.textarea, LV_OBJ_FLAG_HIDDEN);
    lv_group_t *grp = lv_group_get_default();
    if (grp) {
        lv_group_add_obj(grp, s_notes.textarea);
        lv_group_focus_obj(s_notes.textarea);
    }

    /* Load content (empty string for a brand-new file of size 0) */
    esp_err_t err = load_note(path);
    if (err == ESP_ERR_NOT_FOUND) {
        /* File was just created and is empty — clear textarea */
        lv_textarea_set_text(s_notes.textarea, "");
        strncpy(s_notes.current_file, path, sizeof(s_notes.current_file) - 1);
        s_notes.current_file[sizeof(s_notes.current_file) - 1] = '\0';
        s_notes.has_unsaved_changes = false;
    } else if (err != ESP_OK) {
        toast_warn("Cannot open note");
        switch_to_list();
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t notes_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating notes UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    memset(&s_notes, 0, sizeof(s_notes));

    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;
    if (s_app_h == 0) s_app_h = 296;

    ensure_notes_dir();

    const theme_colors_t *clr = theme_get_colors();

    /* ----------------------------------------------------------------
     * Root container
     * ---------------------------------------------------------------- */
    s_notes.root = lv_obj_create(parent);
    lv_obj_set_size(s_notes.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_notes.root, 0, 0);
    lv_obj_set_style_bg_opa(s_notes.root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_notes.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_notes.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_notes.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_notes.root, LV_OBJ_FLAG_SCROLLABLE);

    /* ================================================================
     * FILE LIST SCREEN
     * ================================================================ */
    s_notes.list_screen = lv_obj_create(s_notes.root);
    lv_obj_set_size(s_notes.list_screen, s_app_w, s_app_h);
    lv_obj_set_pos(s_notes.list_screen, 0, 0);
    lv_obj_set_style_bg_color(s_notes.list_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_notes.list_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_notes.list_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_notes.list_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_notes.list_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_notes.list_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_notes.list_screen, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_notes.list_screen, list_key_cb, LV_EVENT_KEY, NULL);

    /* List header bar */
    lv_obj_t *list_header = lv_obj_create(s_notes.list_screen);
    lv_obj_set_size(list_header, s_app_w, HEADER_H);
    lv_obj_set_pos(list_header, 0, 0);
    lv_obj_set_style_bg_color(list_header, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(list_header, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(list_header, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(list_header, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(list_header, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(list_header, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(list_header, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(list_header, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(list_header, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(list_header, 0, LV_PART_MAIN);
    lv_obj_clear_flag(list_header, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(list_header, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(list_header,
                          LV_FLEX_ALIGN_SPACE_BETWEEN,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);

    lv_obj_t *list_title = lv_label_create(list_header);
    lv_label_set_text(list_title, "Notes");
    lv_obj_set_style_text_font(list_title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(list_title, clr->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(list_title, 1);

    /* "+ New" button */
    lv_obj_t *new_btn = lv_button_create(list_header);
    lv_obj_set_size(new_btn, 44, 22);
    lv_obj_set_style_bg_color(new_btn, clr->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(new_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(new_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(new_btn, 2, LV_PART_MAIN);
    lv_obj_add_event_cb(new_btn, new_note_btn_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *new_lbl = lv_label_create(new_btn);
    lv_label_set_text(new_lbl, "+ New");
    lv_obj_set_style_text_font(new_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(new_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(new_lbl);

    /* Scrollable note list */
    s_notes.list_container = lv_obj_create(s_notes.list_screen);
    lv_obj_set_pos(s_notes.list_container, 0, HEADER_H);
    lv_obj_set_size(s_notes.list_container, s_app_w, s_app_h - HEADER_H);
    lv_obj_set_style_bg_color(s_notes.list_container, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_notes.list_container, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_notes.list_container, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_notes.list_container, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_notes.list_container, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_notes.list_container, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_notes.list_container,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(s_notes.list_container, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_notes.list_container, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_notes.list_container, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_notes.list_container, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_notes.list_container, 0, LV_PART_SCROLLBAR);

    populate_list();

    /* ================================================================
     * EDITOR SCREEN
     * ================================================================ */
    s_notes.editor_screen = lv_obj_create(s_notes.root);
    lv_obj_set_size(s_notes.editor_screen, s_app_w, s_app_h);
    lv_obj_set_pos(s_notes.editor_screen, 0, 0);
    lv_obj_set_style_bg_color(s_notes.editor_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_notes.editor_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_notes.editor_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_notes.editor_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_notes.editor_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_notes.editor_screen, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_notes.editor_screen, LV_OBJ_FLAG_HIDDEN);

    /* Editor header bar */
    lv_obj_t *editor_header = lv_obj_create(s_notes.editor_screen);
    lv_obj_set_size(editor_header, s_app_w, HEADER_H);
    lv_obj_set_pos(editor_header, 0, 0);
    lv_obj_set_style_bg_color(editor_header, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(editor_header, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(editor_header, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(editor_header, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(editor_header, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(editor_header, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(editor_header, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(editor_header, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(editor_header, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(editor_header, 0, LV_PART_MAIN);
    lv_obj_clear_flag(editor_header, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(editor_header, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(editor_header,
                          LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(editor_header, 6, LV_PART_MAIN);

    /* "< Back" button */
    lv_obj_t *back_btn = lv_button_create(editor_header);
    lv_obj_set_size(back_btn, 38, 22);
    lv_obj_set_style_bg_color(back_btn, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(back_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(back_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(back_btn, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_radius(back_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(back_btn, 2, LV_PART_MAIN);
    lv_obj_add_event_cb(back_btn, back_from_editor_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *back_lbl = lv_label_create(back_btn);
    lv_label_set_text(back_lbl, "< Back");
    lv_obj_set_style_text_font(back_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(back_lbl, clr->text, LV_PART_MAIN);
    lv_obj_center(back_lbl);

    /* "Save" button */
    lv_obj_t *save_btn = lv_button_create(editor_header);
    lv_obj_set_size(save_btn, 36, 22);
    lv_obj_set_style_bg_color(save_btn, clr->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(save_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(save_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(save_btn, 2, LV_PART_MAIN);
    lv_obj_add_event_cb(save_btn, save_btn_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *save_lbl = lv_label_create(save_btn);
    lv_label_set_text(save_lbl, "Save");
    lv_obj_set_style_text_font(save_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(save_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(save_lbl);

    /* Note filename title — fills remaining space */
    s_notes.title_label = lv_label_create(editor_header);
    lv_label_set_text(s_notes.title_label, "");
    lv_label_set_long_mode(s_notes.title_label, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_font(s_notes.title_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_notes.title_label, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_flex_grow(s_notes.title_label, 1);

    /* Textarea — full area below header */
    s_notes.textarea = lv_textarea_create(s_notes.editor_screen);
    lv_obj_set_size(s_notes.textarea, s_app_w, s_app_h - HEADER_H);
    lv_obj_set_pos(s_notes.textarea, 0, HEADER_H);
    lv_textarea_set_one_line(s_notes.textarea, false);  /* multiline */
    lv_textarea_set_placeholder_text(s_notes.textarea, "Start typing...");
    lv_obj_set_style_text_font(s_notes.textarea, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_notes.textarea, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_notes.textarea, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_notes.textarea, 6, LV_PART_MAIN);
    lv_obj_set_style_bg_color(s_notes.textarea, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_notes.textarea, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_notes.textarea, clr->text, LV_PART_MAIN);

    /* Track unsaved changes */
    lv_obj_add_event_cb(s_notes.textarea, textarea_changed_cb,
                        LV_EVENT_VALUE_CHANGED, NULL);

    /* ESC to go back from editor */
    lv_obj_add_event_cb(s_notes.editor_screen, editor_key_cb,
                        LV_EVENT_KEY, NULL);
    lv_obj_add_flag(s_notes.editor_screen, LV_OBJ_FLAG_CLICKABLE);

    return ESP_OK;
}

void notes_ui_show(void)
{
    if (s_notes.root) {
        lv_obj_clear_flag(s_notes.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void notes_ui_hide(void)
{
    if (s_notes.root) {
        lv_obj_add_flag(s_notes.root, LV_OBJ_FLAG_HIDDEN);
    }
}
