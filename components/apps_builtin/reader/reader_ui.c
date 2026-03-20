#include "hal/sdcard_path.h"
/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Reader app UI
 *
 * Two modes:
 *   Library  — browse /sdcard/books/ and select a .txt/.md file
 *   Reading  — paginated text display with keyboard navigation
 */
#include "reader/reader_app.h"

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

static const char *TAG = "reader_ui";

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

#define APP_AREA_W      320
#define APP_AREA_H      216
#define TITLE_BAR_H      30
#define FOOTER_H         24
#define TEXT_MARGIN_H     8   /* top/bottom margin inside text area */
#define TEXT_MARGIN_W     8   /* left/right margin */

/* Pagination geometry (static estimate for monospaced 14px font) */
#define TEXT_W          (APP_AREA_W - TEXT_MARGIN_W * 2)        /* 304 px */
#define TEXT_H          (APP_AREA_H - FOOTER_H - TEXT_MARGIN_H) /* 184 px */
#define CHARS_PER_LINE   38   /* ~304 / 8 px per glyph */
#define LINES_PER_PAGE   10   /* ~184 / 18 px per line  */
#define CHARS_PER_PAGE  (CHARS_PER_LINE * LINES_PER_PAGE)       /* 380   */

#define BOOKS_PATH      THISTLE_SDCARD "/books"
#define MAX_BOOK_FILES   64
#define ITEM_H           30

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t *root;

    /* Library mode */
    lv_obj_t *library_screen;
    lv_obj_t *lib_list;

    /* Reading mode */
    lv_obj_t *reader_screen;
    lv_obj_t *text_label;
    lv_obj_t *footer_label;

    /* Book data */
    char     *book_text;
    size_t    book_size;
    char      book_path[256];

    /* Pagination */
    int       current_page;
    int       total_pages;
    int      *page_offsets;   /* array of char offsets, length = total_pages + 1 */
    int       page_offset_count;
} s_reader;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void switch_to_library(void);
static void open_book(const char *path);
static void show_page(int page);

/* ------------------------------------------------------------------ */
/* Helpers                                                              */
/* ------------------------------------------------------------------ */

static void format_size(off_t size, char *buf, size_t buf_len)
{
    if (size < 1024)
        snprintf(buf, buf_len, "%ld B", (long)size);
    else if (size < 1024 * 1024)
        snprintf(buf, buf_len, "%.1f KB", size / 1024.0);
    else
        snprintf(buf, buf_len, "%.1f MB", size / (1024.0 * 1024.0));
}

static bool is_readable_ext(const char *name)
{
    const char *dot = strrchr(name, '.');
    if (!dot) return false;
    return (strcmp(dot, ".txt") == 0 || strcmp(dot, ".md") == 0);
}

/* ------------------------------------------------------------------ */
/* Pagination                                                           */
/* ------------------------------------------------------------------ */

static void calculate_pagination(void)
{
    if (!s_reader.book_text) return;

    int text_len = (int)strlen(s_reader.book_text);
    s_reader.total_pages = (text_len + CHARS_PER_PAGE - 1) / CHARS_PER_PAGE;
    if (s_reader.total_pages < 1) s_reader.total_pages = 1;

    /* Rebuild page offset array */
    if (s_reader.page_offsets) {
        free(s_reader.page_offsets);
        s_reader.page_offsets = NULL;
    }
    s_reader.page_offsets = (int *)calloc(s_reader.total_pages + 1, sizeof(int));
    if (!s_reader.page_offsets) {
        ESP_LOGE(TAG, "OOM: page_offsets");
        s_reader.total_pages = 0;
        return;
    }
    s_reader.page_offset_count = s_reader.total_pages + 1;

    s_reader.page_offsets[0] = 0;

    for (int i = 1; i <= s_reader.total_pages; i++) {
        int raw = i * CHARS_PER_PAGE;
        if (raw >= text_len) {
            s_reader.page_offsets[i] = text_len;
            continue;
        }

        /* Walk back to the nearest word boundary so we don't break mid-word */
        int start_of_page = (i - 1) * CHARS_PER_PAGE;
        int offset = raw;
        while (offset > start_of_page &&
               s_reader.book_text[offset] != ' ' &&
               s_reader.book_text[offset] != '\n' &&
               s_reader.book_text[offset] != '\t') {
            offset--;
        }
        /* Skip past the whitespace character itself */
        if (offset > start_of_page &&
            (s_reader.book_text[offset] == ' ' ||
             s_reader.book_text[offset] == '\n' ||
             s_reader.book_text[offset] == '\t')) {
            offset++;
        }
        s_reader.page_offsets[i] = offset;
    }
}

/* ------------------------------------------------------------------ */
/* Book loading                                                         */
/* ------------------------------------------------------------------ */

static esp_err_t load_book(const char *path)
{
    FILE *f = fopen(path, "r");
    if (!f) {
        ESP_LOGW(TAG, "Cannot open: %s", path);
        return ESP_ERR_NOT_FOUND;
    }

    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (size <= 0) {
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    if (s_reader.book_text) {
        free(s_reader.book_text);
        s_reader.book_text = NULL;
    }

    s_reader.book_text = (char *)malloc((size_t)size + 1);
    if (!s_reader.book_text) {
        fclose(f);
        ESP_LOGE(TAG, "OOM: book_text malloc(%ld)", size);
        return ESP_ERR_NO_MEM;
    }

    size_t read_bytes = fread(s_reader.book_text, 1, (size_t)size, f);
    s_reader.book_text[read_bytes] = '\0';
    s_reader.book_size = read_bytes;
    fclose(f);

    strncpy(s_reader.book_path, path, sizeof(s_reader.book_path) - 1);
    s_reader.book_path[sizeof(s_reader.book_path) - 1] = '\0';

    calculate_pagination();
    s_reader.current_page = 0;

    ESP_LOGI(TAG, "Loaded %s: %zu bytes, %d pages", path, s_reader.book_size, s_reader.total_pages);
    return ESP_OK;
}

/* ------------------------------------------------------------------ */
/* Page display                                                         */
/* ------------------------------------------------------------------ */

static void show_page(int page)
{
    if (!s_reader.book_text || s_reader.total_pages <= 0) return;

    if (page < 0) page = 0;
    if (page >= s_reader.total_pages) page = s_reader.total_pages - 1;
    s_reader.current_page = page;

    int start = s_reader.page_offsets[page];
    int end   = (page + 1 <= s_reader.total_pages)
                    ? s_reader.page_offsets[page + 1]
                    : (int)s_reader.book_size;

    if (end < start) end = start;

    /* Extract page slice into a temp buffer */
    int len = end - start;
    char *page_text = (char *)malloc((size_t)len + 1);
    if (!page_text) {
        lv_label_set_text(s_reader.text_label, "(out of memory)");
        return;
    }
    memcpy(page_text, s_reader.book_text + start, (size_t)len);
    page_text[len] = '\0';

    lv_label_set_text(s_reader.text_label, page_text);
    free(page_text);

    /* Update footer: "Page N / T    PP%" */
    char footer[64];
    int pct = (s_reader.total_pages > 0)
                  ? ((page + 1) * 100 / s_reader.total_pages)
                  : 0;
    snprintf(footer, sizeof(footer), "Page %d / %d    %d%%",
             page + 1, s_reader.total_pages, pct);
    lv_label_set_text(s_reader.footer_label, footer);
}

/* ------------------------------------------------------------------ */
/* Keyboard navigation (reading mode)                                   */
/* ------------------------------------------------------------------ */

static void reader_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);

    switch (key) {
        case ' ':
        case LV_KEY_RIGHT:
        case LV_KEY_DOWN:
            show_page(s_reader.current_page + 1);
            break;

        case LV_KEY_BACKSPACE:
        case LV_KEY_LEFT:
        case LV_KEY_UP:
            show_page(s_reader.current_page - 1);
            break;

        case LV_KEY_ESC:
        case 'q':
        case 'Q':
            switch_to_library();
            break;

        default:
            break;
    }
}

/* ------------------------------------------------------------------ */
/* Reading screen creation / display                                    */
/* ------------------------------------------------------------------ */

static void create_reader_screen(void)
{
    const theme_colors_t *clr = theme_get_colors();

    s_reader.reader_screen = lv_obj_create(s_reader.root);
    lv_obj_set_size(s_reader.reader_screen, APP_AREA_W, APP_AREA_H);
    lv_obj_set_pos(s_reader.reader_screen, 0, 0);
    lv_obj_set_style_bg_color(s_reader.reader_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_reader.reader_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_reader.reader_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_reader.reader_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_reader.reader_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_reader.reader_screen, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Text area — fills the space above the footer
     * ---------------------------------------------------------------- */
    lv_obj_t *text_area = lv_obj_create(s_reader.reader_screen);
    lv_obj_set_pos(text_area, 0, 0);
    lv_obj_set_size(text_area, APP_AREA_W, APP_AREA_H - FOOTER_H);
    lv_obj_set_style_bg_color(text_area, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(text_area, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(text_area, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(text_area, TEXT_MARGIN_W, LV_PART_MAIN);
    lv_obj_set_style_pad_right(text_area, TEXT_MARGIN_W, LV_PART_MAIN);
    lv_obj_set_style_pad_top(text_area, TEXT_MARGIN_H, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(text_area, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(text_area, 0, LV_PART_MAIN);
    lv_obj_clear_flag(text_area, LV_OBJ_FLAG_SCROLLABLE);

    /* Text label — clipped to the area, no scroll */
    s_reader.text_label = lv_label_create(text_area);
    lv_label_set_long_mode(s_reader.text_label, LV_LABEL_LONG_CLIP);
    lv_obj_set_size(s_reader.text_label, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_text_font(s_reader.text_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_reader.text_label, clr->text, LV_PART_MAIN);
    lv_obj_set_style_text_align(s_reader.text_label, LV_TEXT_ALIGN_LEFT, LV_PART_MAIN);
    lv_label_set_text(s_reader.text_label, "");

    /* ----------------------------------------------------------------
     * Footer bar (24 px)
     * ---------------------------------------------------------------- */
    lv_obj_t *footer_bar = lv_obj_create(s_reader.reader_screen);
    lv_obj_set_pos(footer_bar, 0, APP_AREA_H - FOOTER_H);
    lv_obj_set_size(footer_bar, APP_AREA_W, FOOTER_H);
    lv_obj_set_style_bg_color(footer_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(footer_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(footer_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(footer_bar, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(footer_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(footer_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(footer_bar, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(footer_bar, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(footer_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(footer_bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(footer_bar, LV_OBJ_FLAG_SCROLLABLE);

    s_reader.footer_label = lv_label_create(footer_bar);
    lv_label_set_long_mode(s_reader.footer_label, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_font(s_reader.footer_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_reader.footer_label, clr->text_secondary, LV_PART_MAIN);
    lv_obj_align(s_reader.footer_label, LV_ALIGN_LEFT_MID, 0, 0);
    lv_label_set_text(s_reader.footer_label, "Page -- / --");

    /* Register keyboard events on the reading screen */
    lv_obj_add_flag(s_reader.reader_screen, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_reader.reader_screen, reader_key_cb, LV_EVENT_KEY, NULL);
}

static void open_book(const char *path)
{
    if (load_book(path) != ESP_OK) {
        toast_warn("Cannot open book");
        return;
    }

    /* Hide library, reveal reading screen */
    lv_obj_add_flag(s_reader.library_screen, LV_OBJ_FLAG_HIDDEN);

    if (!s_reader.reader_screen) {
        create_reader_screen();
    }
    lv_obj_clear_flag(s_reader.reader_screen, LV_OBJ_FLAG_HIDDEN);

    show_page(0);
}

/* ------------------------------------------------------------------ */
/* Library mode                                                         */
/* ------------------------------------------------------------------ */

/* user_data on each book row: malloc'd full path string */
static void book_row_delete_cb(lv_event_t *e)
{
    void *ud = lv_obj_get_user_data(lv_event_get_target(e));
    if (ud) {
        free(ud);
        lv_obj_set_user_data(lv_event_get_target(e), NULL);
    }
}

static void book_row_clicked_cb(lv_event_t *e)
{
    lv_obj_t   *row  = lv_event_get_target(e);
    const char *path = (const char *)lv_obj_get_user_data(row);
    if (path) {
        open_book(path);
    }
}

static void populate_library(void)
{
    lv_obj_clean(s_reader.lib_list);
    lv_obj_scroll_to_y(s_reader.lib_list, 0, LV_ANIM_OFF);

    const theme_colors_t *clr = theme_get_colors();

    DIR *dir = opendir(BOOKS_PATH);
    if (!dir) {
        /* Directory missing — show hint */
        lv_obj_t *lbl = lv_label_create(s_reader.lib_list);
        lv_label_set_text(lbl, "No books found.\nAdd .txt files to\n/sdcard/books/");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_width(lbl, LV_PCT(100));
        lv_obj_set_style_pad_top(lbl, 20, LV_PART_MAIN);
        ESP_LOGW(TAG, "Cannot open %s", BOOKS_PATH);
        return;
    }

    /* Collect entries */
    typedef struct { char name[256]; off_t size; } book_entry_t;
    book_entry_t *entries = (book_entry_t *)malloc(MAX_BOOK_FILES * sizeof(book_entry_t));
    if (!entries) { closedir(dir); return; }
    int count = 0;

    struct dirent *de;
    while ((de = readdir(dir)) != NULL && count < MAX_BOOK_FILES) {
        if (de->d_name[0] == '.') continue;
        if (!is_readable_ext(de->d_name)) continue;

        strncpy(entries[count].name, de->d_name, sizeof(entries[count].name) - 1);
        entries[count].name[sizeof(entries[count].name) - 1] = '\0';

        char full[512];
        snprintf(full, sizeof(full), "%s/%s", BOOKS_PATH, de->d_name);
        struct stat st;
        entries[count].size = (stat(full, &st) == 0) ? st.st_size : 0;
        count++;
    }
    closedir(dir);

    if (count == 0) {
        free(entries);
        lv_obj_t *lbl = lv_label_create(s_reader.lib_list);
        lv_label_set_text(lbl, "No books found.\nAdd .txt files to\n/sdcard/books/");
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl, clr->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, LV_PART_MAIN);
        lv_obj_set_width(lbl, LV_PCT(100));
        lv_obj_set_style_pad_top(lbl, 20, LV_PART_MAIN);
        return;
    }

    /* Render one row per book */
    for (int i = 0; i < count; i++) {
        /* Full path stored as user_data */
        char *full_path = (char *)malloc(512);
        if (!full_path) continue;
        snprintf(full_path, 512, "%s/%s", BOOKS_PATH, entries[i].name);

        lv_obj_t *row = lv_obj_create(s_reader.lib_list);
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
        lv_obj_set_flex_align(row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(row, 6, LV_PART_MAIN);
        lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
        lv_obj_set_user_data(row, full_path);
        lv_obj_add_event_cb(row, book_row_clicked_cb, LV_EVENT_CLICKED, NULL);
        lv_obj_add_event_cb(row, book_row_delete_cb, LV_EVENT_DELETE, NULL);

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

static void switch_to_library(void)
{
    if (s_reader.reader_screen) {
        lv_obj_add_flag(s_reader.reader_screen, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_reader.library_screen) {
        lv_obj_clear_flag(s_reader.library_screen, LV_OBJ_FLAG_HIDDEN);
    }

    /* Free book data to reclaim heap */
    if (s_reader.book_text) {
        free(s_reader.book_text);
        s_reader.book_text = NULL;
        s_reader.book_size = 0;
    }
    if (s_reader.page_offsets) {
        free(s_reader.page_offsets);
        s_reader.page_offsets = NULL;
        s_reader.page_offset_count = 0;
    }
    s_reader.total_pages   = 0;
    s_reader.current_page  = 0;
}

/* ------------------------------------------------------------------ */
/* Library keyboard: ESC → back to launcher                            */
/* ------------------------------------------------------------------ */

static void library_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC || key == 'q' || key == 'Q') {
        app_manager_launch("com.thistle.launcher");
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t reader_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating reader UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Zero out state */
    memset(&s_reader, 0, sizeof(s_reader));

    const theme_colors_t *clr = theme_get_colors();

    /* Root container */
    s_reader.root = lv_obj_create(parent);
    lv_obj_set_size(s_reader.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_reader.root, 0, 0);
    lv_obj_set_style_bg_opa(s_reader.root, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_reader.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_reader.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_reader.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_reader.root, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Library screen
     * ---------------------------------------------------------------- */
    s_reader.library_screen = lv_obj_create(s_reader.root);
    lv_obj_set_size(s_reader.library_screen, APP_AREA_W, APP_AREA_H);
    lv_obj_set_pos(s_reader.library_screen, 0, 0);
    lv_obj_set_style_bg_color(s_reader.library_screen, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_reader.library_screen, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_reader.library_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_reader.library_screen, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_reader.library_screen, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_reader.library_screen, LV_OBJ_FLAG_SCROLLABLE);

    /* Library keyboard events (ESC to quit to launcher) */
    lv_obj_add_flag(s_reader.library_screen, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_reader.library_screen, library_key_cb, LV_EVENT_KEY, NULL);

    /* Title bar */
    lv_obj_t *title_bar = lv_obj_create(s_reader.library_screen);
    lv_obj_set_pos(title_bar, 0, 0);
    lv_obj_set_size(title_bar, APP_AREA_W, TITLE_BAR_H);
    lv_obj_set_style_bg_color(title_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(title_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(title_bar, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(title_bar, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(title_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_border_side(title_bar, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(title_bar, clr->text, LV_PART_MAIN);
    lv_obj_set_style_border_width(title_bar, 1, LV_PART_MAIN);
    lv_obj_clear_flag(title_bar, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *title_lbl = lv_label_create(title_bar);
    lv_label_set_text(title_lbl, "< Library");
    lv_label_set_long_mode(title_lbl, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_font(title_lbl, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(title_lbl, clr->text, LV_PART_MAIN);
    lv_obj_align(title_lbl, LV_ALIGN_LEFT_MID, 0, 0);

    /* Scrollable book list */
    s_reader.lib_list = lv_obj_create(s_reader.library_screen);
    lv_obj_set_pos(s_reader.lib_list, 0, TITLE_BAR_H);
    lv_obj_set_size(s_reader.lib_list, APP_AREA_W, APP_AREA_H - TITLE_BAR_H);
    lv_obj_set_style_bg_color(s_reader.lib_list, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_reader.lib_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_reader.lib_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_reader.lib_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_reader.lib_list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_reader.lib_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_reader.lib_list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
    lv_obj_set_scrollbar_mode(s_reader.lib_list, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_reader.lib_list, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_reader.lib_list, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_reader.lib_list, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_reader.lib_list, 0, LV_PART_SCROLLBAR);

    /* Populate immediately */
    populate_library();

    /* Reader screen is created lazily when a book is opened */
    s_reader.reader_screen = NULL;

    return ESP_OK;
}

void reader_ui_show(void)
{
    if (s_reader.root) {
        lv_obj_clear_flag(s_reader.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void reader_ui_hide(void)
{
    if (s_reader.root) {
        lv_obj_add_flag(s_reader.root, LV_OBJ_FLAG_HIDDEN);
    }
}
