/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger chat UI + LoRa radio integration
 */
#include "messenger/messenger_app.h"

#include "lvgl.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "string.h"

#include "thistle/wifi_manager.h"
#include "hal/board.h"
#include "ui/theme.h"

static const char *TAG = "messenger_ui";

/* ------------------------------------------------------------------ */
/* Constants                                                            */
/* ------------------------------------------------------------------ */

#define MSG_MAX_TEXT  200
#define MSG_HISTORY   50

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

typedef struct {
    char sender[16];        /* "You" or "Node-XXXX" */
    char text[MSG_MAX_TEXT];
    char time_str[8];       /* "HH:MM" */
    bool is_self;
} chat_message_t;

/* ------------------------------------------------------------------ */
/* Module state                                                         */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t *root;
    lv_obj_t *msg_list;      /* scrollable container for messages */
    lv_obj_t *input_ta;      /* lv_textarea for typing */
    lv_obj_t *send_btn;
    lv_obj_t *header_label;
    /* Message history */
    chat_message_t messages[MSG_HISTORY];
    int msg_count;
    /* Device identity */
    uint32_t device_id;      /* random 4-byte ID, shown as hex */
    char device_id_str[12];  /* "Node-XXXX" */
    /* Radio availability */
    bool radio_available;
} s_msg;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void create_message_bubble(const chat_message_t *msg);
static void add_message(const char *sender, const char *text, bool is_self);
static void send_message(void);

/* ------------------------------------------------------------------ */
/* Radio RX callback (called from radio driver context)                */
/* ------------------------------------------------------------------ */

static void radio_rx_callback(const uint8_t *data, size_t len, int rssi, void *user_data)
{
    (void)user_data;
    (void)rssi;

    if (len < 7) return; /* minimum: 4 id + 2 len + 1 char */

    uint32_t sender_id;
    uint16_t msg_len;
    memcpy(&sender_id, data, 4);
    memcpy(&msg_len, data + 4, 2);

    if (msg_len > len - 6) msg_len = (uint16_t)(len - 6);
    if (msg_len > MSG_MAX_TEXT - 1) msg_len = MSG_MAX_TEXT - 1;

    /* Ignore our own re-transmitted packets */
    if (sender_id == s_msg.device_id) return;

    char sender[16];
    snprintf(sender, sizeof(sender), "Node-%04X", (unsigned)(sender_id & 0xFFFF));

    char text[MSG_MAX_TEXT];
    memcpy(text, data + 6, msg_len);
    text[msg_len] = '\0';

    add_message(sender, text, false);
}

/* ------------------------------------------------------------------ */
/* Sending                                                              */
/* ------------------------------------------------------------------ */

static void send_message(void)
{
    if (!s_msg.input_ta) return;

    const char *text = lv_textarea_get_text(s_msg.input_ta);
    if (!text || text[0] == '\0') return;

    size_t text_len = strlen(text);
    if (text_len > 249) text_len = 249;

    if (s_msg.radio_available) {
        /* Build and transmit packet */
        uint8_t packet[255];
        memcpy(packet, &s_msg.device_id, 4);
        uint16_t len16 = (uint16_t)text_len;
        memcpy(packet + 4, &len16, 2);
        memcpy(packet + 6, text, text_len);

        const hal_registry_t *reg = hal_get_registry();
        if (reg && reg->radio && reg->radio->send) {
            esp_err_t err = reg->radio->send(packet, 6 + text_len);
            if (err != ESP_OK) {
                ESP_LOGW(TAG, "radio send failed: %s", esp_err_to_name(err));
            }
        }
    }

    /* Always show the message locally (even if no radio) */
    add_message("You", text, true);

    /* Clear input field */
    lv_textarea_set_text(s_msg.input_ta, "");
}

/* ------------------------------------------------------------------ */
/* Message bubble creation                                              */
/* ------------------------------------------------------------------ */

static void create_message_bubble(const chat_message_t *msg)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *bubble = lv_obj_create(s_msg.msg_list);
    lv_obj_set_width(bubble, LV_PCT(90));
    lv_obj_set_height(bubble, LV_SIZE_CONTENT);
    lv_obj_set_style_pad_all(bubble, 6, LV_PART_MAIN);
    lv_obj_set_style_radius(bubble, 6, LV_PART_MAIN);
    lv_obj_set_style_border_width(bubble, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(bubble, colors->text_secondary, LV_PART_MAIN);
    lv_obj_clear_flag(bubble, LV_OBJ_FLAG_SCROLLABLE);

    if (msg->is_self) {
        /* Self messages: right-aligned, accent bg */
        lv_obj_set_style_bg_color(bubble, colors->primary, LV_PART_MAIN);
        lv_obj_set_align(bubble, LV_ALIGN_RIGHT_MID);
    } else {
        /* Others: left-aligned, surface bg */
        lv_obj_set_style_bg_color(bubble, colors->surface, LV_PART_MAIN);
    }
    lv_obj_set_style_bg_opa(bubble, LV_OPA_COVER, LV_PART_MAIN);

    lv_obj_set_flex_flow(bubble, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(bubble, 2, LV_PART_MAIN);

    /* Header: sender + time */
    lv_obj_t *header = lv_label_create(bubble);
    char hdr[32];
    snprintf(hdr, sizeof(hdr), "[%s] %s", msg->sender, msg->time_str);
    lv_label_set_text(header, hdr);
    lv_obj_set_style_text_font(header, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(header,
        msg->is_self ? lv_color_white() : colors->text_secondary,
        LV_PART_MAIN);

    /* Message text */
    lv_obj_t *body = lv_label_create(bubble);
    lv_label_set_text(body, msg->text);
    lv_label_set_long_mode(body, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(body, LV_PCT(100));
    lv_obj_set_style_text_font(body, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(body,
        msg->is_self ? lv_color_white() : colors->text,
        LV_PART_MAIN);
}

/* ------------------------------------------------------------------ */
/* Add message to history and UI                                        */
/* ------------------------------------------------------------------ */

static void add_message(const char *sender, const char *text, bool is_self)
{
    /* Store in circular history buffer */
    int idx = s_msg.msg_count % MSG_HISTORY;
    strncpy(s_msg.messages[idx].sender, sender, 15);
    s_msg.messages[idx].sender[15] = '\0';
    strncpy(s_msg.messages[idx].text, text, MSG_MAX_TEXT - 1);
    s_msg.messages[idx].text[MSG_MAX_TEXT - 1] = '\0';
    s_msg.messages[idx].is_self = is_self;

    /* Get current time */
    char time_buf[8];
    wifi_manager_get_time_str(time_buf, sizeof(time_buf));
    strncpy(s_msg.messages[idx].time_str, time_buf, 7);
    s_msg.messages[idx].time_str[7] = '\0';

    s_msg.msg_count++;

    /* Add bubble to LVGL list */
    if (s_msg.msg_list) {
        create_message_bubble(&s_msg.messages[idx]);

        /* Scroll to bottom */
        lv_obj_scroll_to_y(s_msg.msg_list, LV_COORD_MAX, LV_ANIM_ON);
    }
}

/* ------------------------------------------------------------------ */
/* Event callbacks                                                      */
/* ------------------------------------------------------------------ */

static void send_btn_cb(lv_event_t *e)
{
    (void)e;
    send_message();
}

static void ta_key_cb(lv_event_t *e)
{
    uint32_t key = lv_event_get_key(e);
    if (key == LV_KEY_ENTER) {
        send_message();
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t messenger_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "creating messenger UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Generate device ID from timer + salt */
    s_msg.device_id = (uint32_t)esp_timer_get_time() ^ 0xDEADBEEF;
    snprintf(s_msg.device_id_str, sizeof(s_msg.device_id_str),
             "Node-%04X", (unsigned)(s_msg.device_id & 0xFFFF));

    /* Check radio availability once at creation time */
    const hal_registry_t *reg = hal_get_registry();
    s_msg.radio_available = (reg && reg->radio && reg->radio->send &&
                             reg->radio->start_receive);

    const theme_colors_t *colors = theme_get_colors();

    /* Root container */
    s_msg.root = lv_obj_create(parent);
    lv_obj_set_size(s_msg.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_msg.root, 0, 0);
    lv_obj_set_style_bg_color(s_msg.root, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_msg.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_msg.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_msg.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_msg.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_msg.root, LV_OBJ_FLAG_SCROLLABLE);

    /* ----------------------------------------------------------------
     * Header bar (30px)
     * ---------------------------------------------------------------- */
    lv_obj_t *header = lv_obj_create(s_msg.root);
    lv_obj_set_size(header, 320, 30);
    lv_obj_set_pos(header, 0, 0);
    lv_obj_set_style_bg_color(header, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(header, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(header, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(header, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(header, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_all(header, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(header, 0, LV_PART_MAIN);
    lv_obj_clear_flag(header, LV_OBJ_FLAG_SCROLLABLE);

    s_msg.header_label = lv_label_create(header);
    if (s_msg.radio_available) {
        lv_label_set_text(s_msg.header_label, "Messenger");
    } else {
        lv_label_set_text(s_msg.header_label, "Messenger  [No radio available]");
    }
    lv_obj_set_style_text_font(s_msg.header_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_msg.header_label, colors->text, LV_PART_MAIN);
    lv_obj_align(s_msg.header_label, LV_ALIGN_LEFT_MID, 0, 0);

    /* ----------------------------------------------------------------
     * Message list (scrollable, between header and input bar)
     * Header=30px, input_bar=40px → list height = 216-30-40 = 146px
     * ---------------------------------------------------------------- */
    s_msg.msg_list = lv_obj_create(s_msg.root);
    lv_obj_set_size(s_msg.msg_list, 320, 146);
    lv_obj_set_pos(s_msg.msg_list, 0, 30);
    lv_obj_set_style_bg_color(s_msg.msg_list, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_msg.msg_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_msg.msg_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_msg.msg_list, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(s_msg.msg_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_row(s_msg.msg_list, 4, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_msg.msg_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_scroll_dir(s_msg.msg_list, LV_DIR_VER);

    /* ----------------------------------------------------------------
     * Input bar (40px at bottom)
     * ---------------------------------------------------------------- */
    lv_obj_t *input_bar = lv_obj_create(s_msg.root);
    lv_obj_set_size(input_bar, 320, 40);
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

    /* Text area */
    s_msg.input_ta = lv_textarea_create(input_bar);
    lv_textarea_set_one_line(s_msg.input_ta, true);
    lv_textarea_set_placeholder_text(s_msg.input_ta, "Type a message...");
    lv_obj_set_flex_grow(s_msg.input_ta, 1);
    lv_obj_set_height(s_msg.input_ta, 30);
    lv_obj_add_event_cb(s_msg.input_ta, ta_key_cb, LV_EVENT_KEY, NULL);

    if (!s_msg.radio_available) {
        /* Visually dim when no radio — still allows typing but won't transmit */
        lv_obj_set_style_text_color(s_msg.input_ta, colors->text_secondary, LV_PART_MAIN);
    }

    /* Send button */
    s_msg.send_btn = lv_button_create(input_bar);
    lv_obj_set_size(s_msg.send_btn, 40, 30);
    lv_obj_set_style_bg_color(s_msg.send_btn, colors->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_msg.send_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(s_msg.send_btn, 4, LV_PART_MAIN);

    lv_obj_t *send_lbl = lv_label_create(s_msg.send_btn);
    lv_label_set_text(send_lbl, ">");
    lv_obj_set_style_text_color(send_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(send_lbl);
    lv_obj_add_event_cb(s_msg.send_btn, send_btn_cb, LV_EVENT_CLICKED, NULL);

    if (!s_msg.radio_available) {
        /* Disable send when no radio — button still works to show local echo */
        lv_obj_set_style_bg_color(s_msg.send_btn, colors->text_secondary, LV_PART_MAIN);
    }

    /* ----------------------------------------------------------------
     * Register radio RX callback (if radio is available)
     * ---------------------------------------------------------------- */
    if (s_msg.radio_available) {
        esp_err_t err = reg->radio->start_receive(radio_rx_callback, NULL);
        if (err != ESP_OK) {
            ESP_LOGW(TAG, "start_receive failed: %s", esp_err_to_name(err));
            s_msg.radio_available = false;
        } else {
            ESP_LOGI(TAG, "radio RX active, node ID: %s", s_msg.device_id_str);
        }
    } else {
        ESP_LOGW(TAG, "no radio driver registered — running in display-only mode");
    }

    return ESP_OK;
}

void messenger_ui_show(void)
{
    if (s_msg.root) {
        lv_obj_clear_flag(s_msg.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void messenger_ui_hide(void)
{
    if (s_msg.root) {
        lv_obj_add_flag(s_msg.root, LV_OBJ_FLAG_HIDDEN);
    }
}
