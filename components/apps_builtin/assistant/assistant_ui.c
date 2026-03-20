/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — AI Assistant chat UI
 *
 * Provides a single-conversation chat interface for conversing with an AI
 * (Claude API or similar) over WiFi/4G. When offline, shows the last
 * conversation loaded from SD card.
 *
 * API configuration is read from THISTLE_SDCARD/config/assistant.json:
 * {
 *   "api_url": "https://api.anthropic.com/v1/messages",
 *   "api_key": "sk-ant-...",
 *   "model": "claude-haiku-4-5-20251001",
 *   "system_prompt": "You are a helpful assistant..."
 * }
 *
 * Conversation is persisted to THISTLE_SDCARD/data/assistant/last_conversation.txt
 * on app pause and loaded on app create.
 */
#include "assistant/assistant_app.h"

#include "lvgl.h"
#include "esp_log.h"
#include "esp_timer.h"
#include <string.h>
#include <stdio.h>
#include <sys/stat.h>

#include "hal/sdcard_path.h"
#include "thistle/wifi_manager.h"
#include "thistle/net_manager.h"
#include "thistle/app_manager.h"
#include "ui/theme.h"

static const char *TAG = "assistant_ui";

/* ------------------------------------------------------------------ */
/* Constants                                                            */
/* ------------------------------------------------------------------ */

#define MAX_MESSAGES        30
#define MAX_MSG_TEXT        512
#define CONV_DATA_DIR       THISTLE_SDCARD "/data/assistant"
#define CONV_FILE_PATH      THISTLE_SDCARD "/data/assistant/last_conversation.txt"
#define CONFIG_FILE_PATH    THISTLE_SDCARD "/config/assistant.json"

#define APP_AREA_W  320
#define APP_AREA_H  216
#define HEADER_H     30
#define INPUT_BAR_H  40
#define MSG_LIST_H  (APP_AREA_H - HEADER_H - INPUT_BAR_H)  /* 146px */

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

typedef struct {
    char sender[8];         /* "You" or "AI" */
    char text[MAX_MSG_TEXT];
    char time_str[8];       /* "HH:MM" */
    bool is_user;
} ai_message_t;

/* ------------------------------------------------------------------ */
/* Module state                                                         */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t *root;
    lv_obj_t *msg_list;
    lv_obj_t *input_ta;
    lv_obj_t *send_btn;
    lv_obj_t *status_label;
    ai_message_t messages[MAX_MESSAGES];
    int msg_count;
} s_ai;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void create_message_bubble(const ai_message_t *msg);
static void add_message(const char *sender, const char *text, bool is_user);
static void send_to_ai(const char *user_msg);

/* ------------------------------------------------------------------ */
/* Persistence                                                          */
/* ------------------------------------------------------------------ */

/* Ensure the data directory exists on the SD card */
static void ensure_data_dir(void)
{
    struct stat st;
    if (stat(CONV_DATA_DIR, &st) != 0) {
        if (mkdir(CONV_DATA_DIR, 0755) != 0) {
            ESP_LOGW(TAG, "mkdir %s failed", CONV_DATA_DIR);
        } else {
            ESP_LOGI(TAG, "Created %s", CONV_DATA_DIR);
        }
    }
}

/*
 * Save conversation to SD card.
 * Format: one message per line — "sender|time_str|text\n"
 * Called from assistant_app.c on_pause.
 */
esp_err_t assistant_ui_save_conversation(void)
{
    ensure_data_dir();

    FILE *f = fopen(CONV_FILE_PATH, "w");
    if (!f) {
        ESP_LOGW(TAG, "Cannot open %s for write", CONV_FILE_PATH);
        return ESP_ERR_NOT_FOUND;
    }

    int count = (s_ai.msg_count < MAX_MESSAGES) ? s_ai.msg_count : MAX_MESSAGES;
    int start = (s_ai.msg_count <= MAX_MESSAGES) ? 0 : (s_ai.msg_count % MAX_MESSAGES);
    for (int i = 0; i < count; i++) {
        int idx = (start + i) % MAX_MESSAGES;
        const ai_message_t *m = &s_ai.messages[idx];
        /* Escape newlines in text as \n literal so one line = one message */
        fputs(m->sender, f);
        fputc('|', f);
        fputs(m->time_str, f);
        fputc('|', f);
        fputs(m->text, f);
        fputc('\n', f);
    }

    fclose(f);
    ESP_LOGI(TAG, "Conversation saved (%d messages)", count);
    return ESP_OK;
}

/*
 * Load last conversation from SD card.
 * Called during assistant_ui_create().
 */
static void load_conversation(void)
{
    FILE *f = fopen(CONV_FILE_PATH, "r");
    if (!f) {
        ESP_LOGI(TAG, "No saved conversation at %s", CONV_FILE_PATH);
        return;
    }

    char line[MAX_MSG_TEXT + 32];
    while (fgets(line, sizeof(line), f) != NULL && s_ai.msg_count < MAX_MESSAGES) {
        /* Strip trailing newline */
        size_t len = strlen(line);
        if (len > 0 && line[len - 1] == '\n') {
            line[len - 1] = '\0';
            len--;
        }

        /* Parse: sender|time_str|text */
        char *p1 = strchr(line, '|');
        if (!p1) continue;
        *p1 = '\0';
        char *p2 = strchr(p1 + 1, '|');
        if (!p2) continue;
        *p2 = '\0';

        const char *sender   = line;
        const char *time_str = p1 + 1;
        const char *text     = p2 + 1;

        bool is_user = (strcmp(sender, "You") == 0);

        int idx = s_ai.msg_count;
        strncpy(s_ai.messages[idx].sender,   sender,   sizeof(s_ai.messages[idx].sender)   - 1);
        strncpy(s_ai.messages[idx].time_str, time_str, sizeof(s_ai.messages[idx].time_str) - 1);
        strncpy(s_ai.messages[idx].text,     text,     sizeof(s_ai.messages[idx].text)     - 1);
        s_ai.messages[idx].is_user = is_user;
        s_ai.msg_count++;

        /* Render the bubble */
        if (s_ai.msg_list) {
            create_message_bubble(&s_ai.messages[idx]);
        }
    }

    fclose(f);
    ESP_LOGI(TAG, "Loaded %d messages from conversation history", s_ai.msg_count);

    /* Scroll to bottom so the latest message is visible */
    if (s_ai.msg_list && s_ai.msg_count > 0) {
        lv_obj_scroll_to_y(s_ai.msg_list, LV_COORD_MAX, LV_ANIM_OFF);
    }
}

/* ------------------------------------------------------------------ */
/* Message bubble creation                                              */
/* ------------------------------------------------------------------ */

static void create_message_bubble(const ai_message_t *msg)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *bubble = lv_obj_create(s_ai.msg_list);
    lv_obj_set_width(bubble, LV_PCT(90));
    lv_obj_set_height(bubble, LV_SIZE_CONTENT);
    lv_obj_set_style_pad_all(bubble, 6, LV_PART_MAIN);
    lv_obj_set_style_radius(bubble, 6, LV_PART_MAIN);
    lv_obj_set_style_border_width(bubble, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(bubble, colors->text_secondary, LV_PART_MAIN);
    lv_obj_clear_flag(bubble, LV_OBJ_FLAG_SCROLLABLE);

    if (msg->is_user) {
        /* User messages: right-aligned, primary/accent background */
        lv_obj_set_style_bg_color(bubble, colors->primary, LV_PART_MAIN);
        lv_obj_set_align(bubble, LV_ALIGN_RIGHT_MID);
    } else {
        /* AI messages: left-aligned, surface background */
        lv_obj_set_style_bg_color(bubble, colors->surface, LV_PART_MAIN);
    }
    lv_obj_set_style_bg_opa(bubble, LV_OPA_COVER, LV_PART_MAIN);

    lv_obj_set_flex_flow(bubble, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(bubble, 2, LV_PART_MAIN);

    /* Header: sender + timestamp */
    lv_obj_t *header = lv_label_create(bubble);
    char hdr[24];
    snprintf(hdr, sizeof(hdr), "[%s] %s", msg->sender, msg->time_str);
    lv_label_set_text(header, hdr);
    lv_obj_set_style_text_font(header, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(header,
        msg->is_user ? lv_color_white() : colors->text_secondary,
        LV_PART_MAIN);

    /* Message body — word-wrap for long AI responses */
    lv_obj_t *body = lv_label_create(bubble);
    lv_label_set_text(body, msg->text);
    lv_label_set_long_mode(body, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(body, LV_PCT(100));
    lv_obj_set_style_text_font(body, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(body,
        msg->is_user ? lv_color_white() : colors->text,
        LV_PART_MAIN);
}

/* ------------------------------------------------------------------ */
/* Add message to history and UI                                        */
/* ------------------------------------------------------------------ */

static void add_message(const char *sender, const char *text, bool is_user)
{
    /* Circular buffer: wrap when full */
    int idx = s_ai.msg_count % MAX_MESSAGES;

    strncpy(s_ai.messages[idx].sender, sender, sizeof(s_ai.messages[idx].sender) - 1);
    s_ai.messages[idx].sender[sizeof(s_ai.messages[idx].sender) - 1] = '\0';

    strncpy(s_ai.messages[idx].text, text, MAX_MSG_TEXT - 1);
    s_ai.messages[idx].text[MAX_MSG_TEXT - 1] = '\0';

    s_ai.messages[idx].is_user = is_user;

    /* Timestamp */
    char time_buf[8];
    wifi_manager_get_time_str(time_buf, sizeof(time_buf));
    strncpy(s_ai.messages[idx].time_str, time_buf, sizeof(s_ai.messages[idx].time_str) - 1);
    s_ai.messages[idx].time_str[sizeof(s_ai.messages[idx].time_str) - 1] = '\0';

    s_ai.msg_count++;

    /* Render bubble */
    if (s_ai.msg_list) {
        create_message_bubble(&s_ai.messages[idx]);
        lv_obj_scroll_to_y(s_ai.msg_list, LV_COORD_MAX, LV_ANIM_ON);
    }
}

/* ------------------------------------------------------------------ */
/* AI message sending                                                   */
/* ------------------------------------------------------------------ */

static void send_to_ai(const char *user_msg)
{
    if (!user_msg || user_msg[0] == '\0') return;

    /* Add the user's message to the chat */
    add_message("You", user_msg, true);

    /* Check network connectivity */
    if (!net_is_connected()) {
        add_message("AI", "No network connection. Connect to WiFi first.", false);
        if (s_ai.status_label) {
            lv_label_set_text(s_ai.status_label, "Offline");
        }
        return;
    }

    /* Show thinking indicator */
    if (s_ai.status_label) {
        lv_label_set_text(s_ai.status_label, "Thinking...");
    }

    /*
     * TODO: Make HTTP POST to AI API
     *
     * When API is implemented:
     * 1. Read api_url, api_key, model, system_prompt from CONFIG_FILE_PATH
     * 2. Build JSON request body with conversation history:
     *    POST https://api.anthropic.com/v1/messages
     *    Headers: x-api-key: <api_key>
     *             anthropic-version: 2023-06-01
     *             content-type: application/json
     *    Body: { "model": "<model>",
     *            "system": "<system_prompt>",
     *            "messages": [ {"role":"user","content":"..."}, ... ],
     *            "max_tokens": 1024 }
     * 3. esp_http_client_perform() to API endpoint
     * 4. Parse JSON response — extract content[0].text
     * 5. Call add_message("AI", extracted_text, false)
     *
     * For now: return a placeholder response describing the setup.
     */
    add_message("AI",
        "I'm running on ThistleOS! API integration coming soon. "
        "Connect to WiFi and configure " THISTLE_SDCARD "/config/assistant.json "
        "with your API key to enable real responses.",
        false);

    if (s_ai.status_label) {
        lv_label_set_text(s_ai.status_label, "Connected");
    }
}

/* ------------------------------------------------------------------ */
/* Event callbacks                                                      */
/* ------------------------------------------------------------------ */

static void send_btn_cb(lv_event_t *e)
{
    (void)e;
    if (!s_ai.input_ta) return;

    const char *text = lv_textarea_get_text(s_ai.input_ta);
    if (!text || text[0] == '\0') return;

    /* Copy text before clearing the field */
    char msg[MAX_MSG_TEXT];
    strncpy(msg, text, MAX_MSG_TEXT - 1);
    msg[MAX_MSG_TEXT - 1] = '\0';

    lv_textarea_set_text(s_ai.input_ta, "");

    send_to_ai(msg);
}

static void ta_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ENTER) {
        send_btn_cb(e);
    } else if (key == LV_KEY_ESC) {
        app_manager_launch("com.thistle.launcher");
    }
}

static void root_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ESC) {
        app_manager_launch("com.thistle.launcher");
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t assistant_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating assistant UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    memset(&s_ai, 0, sizeof(s_ai));

    const theme_colors_t *colors = theme_get_colors();

    /* ----------------------------------------------------------------
     * Root container
     * ---------------------------------------------------------------- */
    s_ai.root = lv_obj_create(parent);
    lv_obj_set_size(s_ai.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_ai.root, 0, 0);
    lv_obj_set_style_bg_color(s_ai.root, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ai.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_ai.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_ai.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ai.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_ai.root, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_ai.root, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_ai.root, root_key_cb, LV_EVENT_KEY, NULL);

    /* ----------------------------------------------------------------
     * Header bar (30px)
     * Contains: "Assistant" title on left, connection status on right
     * ---------------------------------------------------------------- */
    lv_obj_t *header = lv_obj_create(s_ai.root);
    lv_obj_set_size(header, APP_AREA_W, HEADER_H);
    lv_obj_set_pos(header, 0, 0);
    lv_obj_set_style_bg_color(header, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(header, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(header, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(header, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(header, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(header, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(header, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(header, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(header, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(header, 0, LV_PART_MAIN);
    lv_obj_clear_flag(header, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(header, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(header,
                          LV_FLEX_ALIGN_SPACE_BETWEEN,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);

    lv_obj_t *title_label = lv_label_create(header);
    lv_label_set_text(title_label, "Assistant");
    lv_obj_set_style_text_font(title_label, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(title_label, colors->text, LV_PART_MAIN);
    lv_obj_set_flex_grow(title_label, 1);

    s_ai.status_label = lv_label_create(header);
    lv_label_set_text(s_ai.status_label,
                      net_is_connected() ? "Connected" : "Offline");
    lv_obj_set_style_text_font(s_ai.status_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ai.status_label,
                                net_is_connected() ? colors->primary : colors->text_secondary,
                                LV_PART_MAIN);

    /* ----------------------------------------------------------------
     * Message list (scrollable, between header and input bar)
     * Header=30px, input_bar=40px → list height = 216-30-40 = 146px
     * ---------------------------------------------------------------- */
    s_ai.msg_list = lv_obj_create(s_ai.root);
    lv_obj_set_size(s_ai.msg_list, APP_AREA_W, MSG_LIST_H);
    lv_obj_set_pos(s_ai.msg_list, 0, HEADER_H);
    lv_obj_set_style_bg_color(s_ai.msg_list, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ai.msg_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_ai.msg_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_ai.msg_list, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ai.msg_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_row(s_ai.msg_list, 4, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_ai.msg_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_scroll_dir(s_ai.msg_list, LV_DIR_VER);
    lv_obj_set_scrollbar_mode(s_ai.msg_list, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_ai.msg_list, colors->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_ai.msg_list, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_ai.msg_list, 2, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(s_ai.msg_list, 0, LV_PART_SCROLLBAR);

    /* ----------------------------------------------------------------
     * Input bar (40px at bottom)
     * [Ask anything...          ] [>]
     * ---------------------------------------------------------------- */
    lv_obj_t *input_bar = lv_obj_create(s_ai.root);
    lv_obj_set_size(input_bar, APP_AREA_W, INPUT_BAR_H);
    lv_obj_align(input_bar, LV_ALIGN_BOTTOM_MID, 0, 0);
    lv_obj_set_style_bg_color(input_bar, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(input_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(input_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(input_bar, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(input_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_all(input_bar, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(input_bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(input_bar, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(input_bar, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(input_bar,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(input_bar, 4, LV_PART_MAIN);

    /* Text area — grows to fill available width */
    s_ai.input_ta = lv_textarea_create(input_bar);
    lv_textarea_set_one_line(s_ai.input_ta, true);
    lv_textarea_set_placeholder_text(s_ai.input_ta, "Ask anything...");
    lv_obj_set_flex_grow(s_ai.input_ta, 1);
    lv_obj_set_height(s_ai.input_ta, 30);
    lv_obj_set_style_text_font(s_ai.input_ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_add_event_cb(s_ai.input_ta, ta_key_cb, LV_EVENT_KEY, NULL);

    /* Focus textarea so physical keyboard types into it immediately */
    lv_group_t *grp = lv_group_get_default();
    if (grp) {
        lv_group_add_obj(grp, s_ai.input_ta);
        lv_group_focus_obj(s_ai.input_ta);
    }

    /* Send button (">") */
    s_ai.send_btn = lv_button_create(input_bar);
    lv_obj_set_size(s_ai.send_btn, 40, 30);
    lv_obj_set_style_bg_color(s_ai.send_btn, colors->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ai.send_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ai.send_btn, 4, LV_PART_MAIN);
    lv_obj_add_event_cb(s_ai.send_btn, send_btn_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *send_lbl = lv_label_create(s_ai.send_btn);
    lv_label_set_text(send_lbl, ">");
    lv_obj_set_style_text_font(send_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(send_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(send_lbl);

    /* ----------------------------------------------------------------
     * Load persisted conversation from SD card
     * ---------------------------------------------------------------- */
    load_conversation();

    return ESP_OK;
}

void assistant_ui_show(void)
{
    if (s_ai.root) {
        lv_obj_clear_flag(s_ai.root, LV_OBJ_FLAG_HIDDEN);

        /* Re-focus textarea so keyboard input goes here */
        if (s_ai.input_ta) {
            lv_group_t *grp = lv_group_get_default();
            if (grp) {
                lv_group_focus_obj(s_ai.input_ta);
            }
        }

        /* Update status label to reflect current network state */
        if (s_ai.status_label) {
            bool connected = net_is_connected();
            const theme_colors_t *colors = theme_get_colors();
            lv_label_set_text(s_ai.status_label,
                              connected ? "Connected" : "Offline");
            lv_obj_set_style_text_color(s_ai.status_label,
                                        connected ? colors->primary : colors->text_secondary,
                                        LV_PART_MAIN);
        }
    }
}

void assistant_ui_hide(void)
{
    if (s_ai.root) {
        lv_obj_add_flag(s_ai.root, LV_OBJ_FLAG_HIDDEN);
    }
}
