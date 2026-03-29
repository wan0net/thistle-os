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
#include "esp_http_client.h"
#include "esp_crt_bundle.h"

static const char *TAG = "assistant_ui";

/* ------------------------------------------------------------------ */
/* Constants                                                            */
/* ------------------------------------------------------------------ */

#define MAX_MESSAGES        30
#define MAX_MSG_TEXT        512
#define CONV_DATA_DIR       THISTLE_SDCARD "/data/assistant"
#define CONV_FILE_PATH      THISTLE_SDCARD "/data/assistant/last_conversation.txt"
#define CONFIG_FILE_PATH    THISTLE_SDCARD "/config/assistant.json"

static int s_app_w = 240;
static int s_app_h = 296;
#define HEADER_H     30
#define INPUT_BAR_H  40
static int s_msg_list_h = 226; /* s_app_h - HEADER_H - INPUT_BAR_H */

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
/* AI API — config, HTTP helpers                                        */
/* ------------------------------------------------------------------ */

/*
 * Read assistant configuration from SD card.
 * File format (JSON):
 * {
 *   "api_url": "https://api.anthropic.com/v1/messages",
 *   "api_key": "sk-ant-...",
 *   "model": "claude-haiku-4-5-20251001",
 *   "system_prompt": "You are a helpful assistant on ThistleOS, a portable ESP32 operating system."
 * }
 */
typedef struct {
    char api_url[256];
    char api_key[128];
    char model[64];
    char system_prompt[256];
} assistant_config_t;

/* Extract a JSON string value for a given key. Returns length copied, 0 on failure. */
static size_t json_extract_str(const char *json, const char *key, char *out, size_t out_len)
{
    char pattern[64];
    snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    const char *p = strstr(json, pattern);
    if (!p) return 0;
    p += strlen(pattern);
    /* Skip whitespace and colon */
    while (*p == ' ' || *p == ':' || *p == '\t' || *p == '\n') p++;
    if (*p != '"') return 0;
    p++; /* skip opening quote */
    const char *end = strchr(p, '"');
    if (!end) return 0;
    size_t len = (size_t)(end - p);
    if (len >= out_len) len = out_len - 1;
    memcpy(out, p, len);
    out[len] = '\0';
    return len;
}

static bool load_assistant_config(assistant_config_t *cfg)
{
    /* Set defaults */
    strncpy(cfg->api_url, "https://api.anthropic.com/v1/messages", sizeof(cfg->api_url) - 1);
    strncpy(cfg->model, "claude-haiku-4-5-20251001", sizeof(cfg->model) - 1);
    strncpy(cfg->system_prompt,
            "You are a helpful assistant on ThistleOS, a portable ESP32 operating system.",
            sizeof(cfg->system_prompt) - 1);
    cfg->api_key[0] = '\0';

    FILE *f = fopen(CONFIG_FILE_PATH, "r");
    if (!f) {
        ESP_LOGW(TAG, "No assistant config at %s", CONFIG_FILE_PATH);
        return false;
    }

    char buf[1024];
    size_t n = fread(buf, 1, sizeof(buf) - 1, f);
    fclose(f);
    buf[n] = '\0';

    json_extract_str(buf, "api_key",     cfg->api_key,      sizeof(cfg->api_key));
    json_extract_str(buf, "api_url",     cfg->api_url,      sizeof(cfg->api_url));
    json_extract_str(buf, "model",       cfg->model,        sizeof(cfg->model));
    json_extract_str(buf, "system_prompt", cfg->system_prompt, sizeof(cfg->system_prompt));

    if (cfg->api_key[0] == '\0') {
        ESP_LOGW(TAG, "No api_key in %s", CONFIG_FILE_PATH);
        return false;
    }

    return true;
}

/* Accumulates HTTP response body chunks into a caller-provided buffer. */
typedef struct {
    char *buf;
    size_t buf_len;
    size_t pos;
} http_resp_t;

static esp_err_t http_event_handler(esp_http_client_event_t *evt)
{
    http_resp_t *resp = (http_resp_t *)evt->user_data;
    if (evt->event_id == HTTP_EVENT_ON_DATA && resp && evt->data_len > 0) {
        size_t copy_len = (size_t)evt->data_len;
        if (resp->pos + copy_len >= resp->buf_len) {
            copy_len = resp->buf_len - resp->pos - 1;
        }
        if (copy_len > 0) {
            memcpy(resp->buf + resp->pos, evt->data, copy_len);
            resp->pos += copy_len;
            resp->buf[resp->pos] = '\0';
        }
    }
    return ESP_OK;
}

/*
 * Call the Claude Messages API and write the response text into resp_buf.
 * Returns ESP_OK on success.
 */
static esp_err_t call_claude_api(const assistant_config_t *cfg,
                                  const char *user_msg,
                                  char *resp_buf, size_t resp_buf_len)
{
    /* ------------------------------------------------------------------
     * Build JSON request body.
     * Include up to the last 6 messages for conversation context.
     * ------------------------------------------------------------------ */
    char body[2048];
    int pos = 0;

    pos += snprintf(body + pos, sizeof(body) - pos,
        "{\"model\":\"%s\",\"max_tokens\":512,\"system\":\"%s\",\"messages\":[",
        cfg->model, cfg->system_prompt);

    int start = (s_ai.msg_count > 6) ? s_ai.msg_count - 6 : 0;
    for (int i = start; i < s_ai.msg_count; i++) {
        int idx = i % MAX_MESSAGES;
        const char *role = s_ai.messages[idx].is_user ? "user" : "assistant";
        if (i > start) pos += snprintf(body + pos, sizeof(body) - pos, ",");

        /* Inline-escape the message text: " → \", \ → \\, newline → \n */
        char escaped[MAX_MSG_TEXT * 2];
        size_t ep = 0;
        for (const char *c = s_ai.messages[idx].text; *c && ep + 3 < sizeof(escaped); c++) {
            if (*c == '"')       { escaped[ep++] = '\\'; escaped[ep++] = '"';  }
            else if (*c == '\\') { escaped[ep++] = '\\'; escaped[ep++] = '\\'; }
            else if (*c == '\n') { escaped[ep++] = '\\'; escaped[ep++] = 'n';  }
            else                 { escaped[ep++] = *c; }
        }
        escaped[ep] = '\0';

        pos += snprintf(body + pos, sizeof(body) - pos,
            "{\"role\":\"%s\",\"content\":\"%s\"}", role, escaped);
    }

    /* Current user message — also escaped */
    char escaped_msg[MAX_MSG_TEXT * 2];
    size_t ep = 0;
    for (const char *c = user_msg; *c && ep + 3 < sizeof(escaped_msg); c++) {
        if (*c == '"')       { escaped_msg[ep++] = '\\'; escaped_msg[ep++] = '"';  }
        else if (*c == '\\') { escaped_msg[ep++] = '\\'; escaped_msg[ep++] = '\\'; }
        else if (*c == '\n') { escaped_msg[ep++] = '\\'; escaped_msg[ep++] = 'n';  }
        else                 { escaped_msg[ep++] = *c; }
    }
    escaped_msg[ep] = '\0';

    if (s_ai.msg_count > start) pos += snprintf(body + pos, sizeof(body) - pos, ",");
    pos += snprintf(body + pos, sizeof(body) - pos,
        "{\"role\":\"user\",\"content\":\"%s\"}]}", escaped_msg);

    /* ------------------------------------------------------------------
     * HTTP POST
     * ------------------------------------------------------------------ */
    char raw_resp[2048] = {0};
    http_resp_t resp_ctx = { .buf = raw_resp, .buf_len = sizeof(raw_resp), .pos = 0 };

    esp_http_client_config_t http_config = {
        .url             = cfg->api_url,
        .timeout_ms      = 30000,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .event_handler   = http_event_handler,
        .user_data       = &resp_ctx,
    };

    esp_http_client_handle_t client = esp_http_client_init(&http_config);
    if (!client) {
        ESP_LOGE(TAG, "Failed to init HTTP client");
        return ESP_FAIL;
    }

    esp_http_client_set_method(client, HTTP_METHOD_POST);
    esp_http_client_set_header(client, "x-api-key",           cfg->api_key);
    esp_http_client_set_header(client, "anthropic-version",   "2023-06-01");
    esp_http_client_set_header(client, "content-type",        "application/json");
    esp_http_client_set_post_field(client, body, (int)strlen(body));

    esp_err_t err = esp_http_client_perform(client);
    int status = esp_http_client_get_status_code(client);
    esp_http_client_cleanup(client);

    if (err != ESP_OK) {
        ESP_LOGE(TAG, "HTTP perform failed: %s", esp_err_to_name(err));
        return ESP_FAIL;
    }
    if (status != 200) {
        ESP_LOGW(TAG, "API returned HTTP %d: %s", status, raw_resp);
        /* Try to surface the API error message to the caller */
        const char *err_key = strstr(raw_resp, "\"message\":\"");
        if (err_key) {
            err_key += 11;
            const char *err_end = strchr(err_key, '"');
            if (err_end) {
                size_t elen = (size_t)(err_end - err_key);
                if (elen >= resp_buf_len) elen = resp_buf_len - 1;
                memcpy(resp_buf, err_key, elen);
                resp_buf[elen] = '\0';
                return ESP_FAIL;
            }
        }
        return ESP_FAIL;
    }

    /* ------------------------------------------------------------------
     * Parse response — extract content[0].text
     * Claude response: {"content":[{"type":"text","text":"..."}],...}
     * ------------------------------------------------------------------ */
    const char *text_key = strstr(raw_resp, "\"text\":\"");
    if (!text_key) {
        ESP_LOGW(TAG, "No 'text' field in response: %s", raw_resp);
        return ESP_FAIL;
    }
    text_key += 8; /* skip "text":" */

    /* Walk forward, handling \" escapes to find the real closing quote */
    size_t out_pos = 0;
    const char *p = text_key;
    while (*p && out_pos + 1 < resp_buf_len) {
        if (*p == '\\' && *(p + 1) != '\0') {
            p++;
            if      (*p == '"')  { resp_buf[out_pos++] = '"';  }
            else if (*p == '\\') { resp_buf[out_pos++] = '\\'; }
            else if (*p == 'n')  { resp_buf[out_pos++] = '\n'; }
            else if (*p == 'r')  { /* skip CR */ }
            else                 { resp_buf[out_pos++] = *p;   }
        } else if (*p == '"') {
            break; /* unescaped closing quote */
        } else {
            resp_buf[out_pos++] = *p;
        }
        p++;
    }
    resp_buf[out_pos] = '\0';

    ESP_LOGI(TAG, "API response (%zu chars)", out_pos);
    return (out_pos > 0) ? ESP_OK : ESP_FAIL;
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

    /* Load API config from SD card */
    assistant_config_t cfg;
    if (!load_assistant_config(&cfg)) {
        add_message("AI",
            "No API key configured. Create " CONFIG_FILE_PATH
            " with your Anthropic API key.",
            false);
        if (s_ai.status_label) {
            lv_label_set_text(s_ai.status_label, "No API key");
        }
        return;
    }

    /* Call Claude API */
    char response[MAX_MSG_TEXT];
    esp_err_t ret = call_claude_api(&cfg, user_msg, response, sizeof(response));
    if (ret == ESP_OK && response[0] != '\0') {
        add_message("AI", response, false);
    } else {
        add_message("AI", "Sorry, I couldn't reach the API. Check your connection and API key.", false);
    }

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

    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;
    if (s_app_h == 0) s_app_h = 296;
    s_msg_list_h = s_app_h - HEADER_H - INPUT_BAR_H;

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
    lv_obj_set_size(header, s_app_w, HEADER_H);
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
    lv_obj_set_size(s_ai.msg_list, s_app_w, s_msg_list_h);
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
    lv_obj_set_size(input_bar, s_app_w, INPUT_BAR_H);
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

void assistant_ui_destroy(void)
{
    if (s_ai.root) {
        lv_obj_delete(s_ai.root);
        s_ai.root = NULL;
    }
}
