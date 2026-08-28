// SPDX-License-Identifier: BSD-3-Clause
// Notes — toolkit-neutral standalone ThistleOS app.

#include "thistle_app.h"

#include <stdio.h>
#include <string.h>

#define TAG "notes"
#define NOTES_DIR "notes"
#define MAX_NOTES 24
#define MAX_NOTE_BYTES 4096

static thistle_widget_t s_root = THISTLE_WIDGET_NONE;
static thistle_widget_t s_list_screen = THISTLE_WIDGET_NONE;
static thistle_widget_t s_list = THISTLE_WIDGET_NONE;
static thistle_widget_t s_empty = THISTLE_WIDGET_NONE;
static thistle_widget_t s_editor_screen = THISTLE_WIDGET_NONE;
static thistle_widget_t s_title = THISTLE_WIDGET_NONE;
static thistle_widget_t s_editor = THISTLE_WIDGET_NONE;
static thistle_widget_t s_status = THISTLE_WIDGET_NONE;
static thistle_widget_t s_note_buttons[MAX_NOTES];
static thistle_fs_entry_t s_entries[MAX_NOTES];
static char s_current_path[THISTLE_FS_NAME_MAX + sizeof(NOTES_DIR) + 2];
static char s_text[MAX_NOTE_BYTES];
static int s_note_count;

static void show_list(void);

static void set_status(const char *message)
{
    thistle_ui_set_text(s_status, message);
}

static int has_txt_suffix(const char *name)
{
    size_t length = strlen(name);
    return length > 4 && strcmp(name + length - 4, ".txt") == 0;
}

static void clear_note_buttons(void)
{
    for (int i = 0; i < s_note_count; i++) {
        if (s_note_buttons[i] != THISTLE_WIDGET_NONE) {
            thistle_ui_destroy(s_note_buttons[i]);
            s_note_buttons[i] = THISTLE_WIDGET_NONE;
        }
    }
    s_note_count = 0;
}

static int load_current_note(void)
{
    void *file = thistle_fs_open(s_current_path, "rb");
    if (!file) return -1;
    int read = thistle_fs_read(s_text, 1, sizeof(s_text) - 1, file);
    int close_result = thistle_fs_close(file);
    if (read < 0 || close_result != 0) return -1;
    s_text[read] = '\0';
    thistle_ui_set_text(s_editor, s_text);
    return 0;
}

static int save_current_note(void)
{
    const char *text = thistle_ui_get_text(s_editor);
    if (!text || s_current_path[0] == '\0') return -1;

    char temp_path[sizeof(s_current_path) + 8];
    snprintf(temp_path, sizeof(temp_path), "%s.tmp", s_current_path);
    void *file = thistle_fs_open(temp_path, "wb");
    if (!file) return -1;

    size_t length = strlen(text);
    if (length >= MAX_NOTE_BYTES) length = MAX_NOTE_BYTES - 1;
    int written = thistle_fs_write(text, 1, length, file);
    int close_result = thistle_fs_close(file);
    if (written != (int)length || close_result != 0) {
        thistle_fs_remove(temp_path);
        return -1;
    }
    if (thistle_fs_replace(temp_path, s_current_path) != 0) {
        thistle_fs_remove(temp_path);
        return -1;
    }
    return 0;
}

static void show_editor(const char *name)
{
    snprintf(s_current_path, sizeof(s_current_path), "%s/%s", NOTES_DIR, name);
    thistle_ui_set_visible(s_list_screen, false);
    thistle_ui_set_visible(s_editor_screen, true);
    if (load_current_note() == 0) {
        thistle_ui_set_text(s_title, "Note");
        set_status("Saved locally");
    }
    else {
        thistle_ui_set_text(s_title, "New note");
        thistle_ui_set_text(s_editor, "");
        set_status("New note");
    }
}

static void note_clicked(thistle_widget_t widget, int event, void *user_data)
{
    (void)widget;
    (void)event;
    const char *name = (const char *)user_data;
    if (name) show_editor(name);
}

static void refresh_list(void)
{
    clear_note_buttons();
    int found = thistle_fs_list(NOTES_DIR, s_entries, MAX_NOTES);
    if (found < 0) found = 0;

    for (int i = 0; i < found && s_note_count < MAX_NOTES; i++) {
        if (s_entries[i].entry_type != THISTLE_FS_TYPE_FILE ||
            !has_txt_suffix(s_entries[i].name)) continue;
        int slot = s_note_count++;
        s_note_buttons[slot] = thistle_ui_create_button(s_list, s_entries[i].name);
        thistle_ui_set_size(s_note_buttons[slot], -1, 38);
        thistle_ui_set_bg_color(s_note_buttons[slot], thistle_ui_theme_surface());
        thistle_ui_set_text_color(s_note_buttons[slot], thistle_ui_theme_text());
        thistle_ui_set_font_size(s_note_buttons[slot], 14);
        thistle_ui_set_radius(s_note_buttons[slot], 6);
        thistle_ui_on_event(s_note_buttons[slot], THISTLE_EVENT_CLICK,
                            note_clicked, s_entries[i].name);
    }
    thistle_ui_set_visible(s_empty, s_note_count == 0);
}

static void new_note_clicked(thistle_widget_t widget, int event, void *user_data)
{
    (void)widget;
    (void)event;
    (void)user_data;
    thistle_log(TAG, "new note");
    char name[THISTLE_FS_NAME_MAX];
    snprintf(name, sizeof(name), "note-%010lu.txt",
             (unsigned long)thistle_millis());
    show_editor(name);
}

static void save_clicked(thistle_widget_t widget, int event, void *user_data)
{
    (void)widget;
    (void)event;
    (void)user_data;
    set_status(save_current_note() == 0 ? "Saved locally" : "Save failed");
}

static void back_clicked(thistle_widget_t widget, int event, void *user_data)
{
    (void)widget;
    (void)event;
    (void)user_data;
    show_list();
}

static void show_list(void)
{
    thistle_ui_set_visible(s_editor_screen, false);
    thistle_ui_set_visible(s_list_screen, true);
    refresh_list();
}

static int notes_on_create(void)
{
    thistle_log(TAG, "on_create");
    s_root = thistle_ui_get_app_root();

    s_list_screen = thistle_ui_create_container(s_root);
    thistle_ui_set_size(s_list_screen, -1, -1);
    thistle_ui_set_layout(s_list_screen, THISTLE_LAYOUT_FLEX_COLUMN);
    thistle_ui_set_gap(s_list_screen, 8);
    thistle_ui_set_padding(s_list_screen, 10, 10, 10, 10);
    thistle_ui_set_bg_color(s_list_screen, thistle_ui_theme_bg());

    thistle_widget_t header = thistle_ui_create_container(s_list_screen);
    thistle_ui_set_size(header, -1, 42);
    thistle_ui_set_layout(header, THISTLE_LAYOUT_FLEX_ROW);
    thistle_ui_set_align(header, THISTLE_ALIGN_SPACE_BETWEEN,
                         THISTLE_ALIGN_CENTER);

    thistle_widget_t heading = thistle_ui_create_label(header, "Notes");
    thistle_ui_set_font_size(heading, 20);
    thistle_ui_set_text_color(heading, thistle_ui_theme_text());

    thistle_widget_t new_button = thistle_ui_create_button(header, "+ New");
    thistle_ui_set_size(new_button, 76, 34);
    thistle_ui_set_bg_color(new_button, thistle_ui_theme_primary());
    thistle_ui_set_text_color(new_button, 0xFFFFFF);
    thistle_ui_set_radius(new_button, 8);
    thistle_ui_on_event(new_button, THISTLE_EVENT_CLICK, new_note_clicked, 0);

    s_list = thistle_ui_create_container(s_list_screen);
    thistle_ui_set_size(s_list, -1, -1);
    thistle_ui_set_flex_grow(s_list, 1);
    thistle_ui_set_layout(s_list, THISTLE_LAYOUT_FLEX_COLUMN);
    thistle_ui_set_gap(s_list, 6);
    thistle_ui_set_scrollable(s_list, true);

    s_empty = thistle_ui_create_label(s_list, "No notes yet\nTap + New to begin");
    thistle_ui_set_font_size(s_empty, 14);
    thistle_ui_set_text_color(s_empty, thistle_ui_theme_text_secondary());

    s_editor_screen = thistle_ui_create_container(s_root);
    thistle_ui_set_size(s_editor_screen, -1, -1);
    thistle_ui_set_layout(s_editor_screen, THISTLE_LAYOUT_FLEX_COLUMN);
    thistle_ui_set_gap(s_editor_screen, 8);
    thistle_ui_set_padding(s_editor_screen, 10, 10, 10, 10);
    thistle_ui_set_bg_color(s_editor_screen, thistle_ui_theme_bg());

    thistle_widget_t editor_header = thistle_ui_create_container(s_editor_screen);
    thistle_ui_set_size(editor_header, -1, 38);
    thistle_ui_set_layout(editor_header, THISTLE_LAYOUT_FLEX_ROW);
    thistle_ui_set_gap(editor_header, 8);

    thistle_widget_t back = thistle_ui_create_button(editor_header, "Back");
    thistle_ui_set_size(back, 62, 32);
    thistle_ui_on_event(back, THISTLE_EVENT_CLICK, back_clicked, 0);

    s_title = thistle_ui_create_label(editor_header, "Note");
    thistle_ui_set_flex_grow(s_title, 1);
    thistle_ui_set_font_size(s_title, 14);
    thistle_ui_set_text_color(s_title, thistle_ui_theme_text());

    thistle_widget_t save = thistle_ui_create_button(editor_header, "Save");
    thistle_ui_set_size(save, 62, 32);
    thistle_ui_set_bg_color(save, thistle_ui_theme_primary());
    thistle_ui_set_text_color(save, 0xFFFFFF);
    thistle_ui_on_event(save, THISTLE_EVENT_CLICK, save_clicked, 0);

    s_editor = thistle_ui_create_text_input(s_editor_screen, "Write something...");
    thistle_ui_set_size(s_editor, -1, -1);
    thistle_ui_set_flex_grow(s_editor, 1);
    thistle_ui_set_one_line(s_editor, false);
    thistle_ui_set_bg_color(s_editor, thistle_ui_theme_surface());
    thistle_ui_set_text_color(s_editor, thistle_ui_theme_text());
    thistle_ui_set_font_size(s_editor, 14);
    thistle_ui_set_radius(s_editor, 6);

    s_status = thistle_ui_create_label(s_editor_screen, "Saved locally");
    thistle_ui_set_font_size(s_status, 12);
    thistle_ui_set_text_color(s_status, thistle_ui_theme_text_secondary());

    show_list();
    return 0;
}

static void notes_on_start(void) { thistle_log(TAG, "on_start"); }
static void notes_on_pause(void) { thistle_log(TAG, "on_pause"); }
static void notes_on_resume(void) { thistle_log(TAG, "on_resume"); }
static void notes_on_destroy(void) { thistle_log(TAG, "on_destroy"); }

static const thistle_app_t notes_app = {
    .id = "com.thistle.notes",
    .name = "Notes",
    .version = "1.0.0",
    .allow_background = false,
    .on_create = notes_on_create,
    .on_start = notes_on_start,
    .on_pause = notes_on_pause,
    .on_resume = notes_on_resume,
    .on_destroy = notes_on_destroy,
};

THISTLE_APP(notes_app);
