/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger UI (multi-transport)
 *
 * Screen flow:
 *
 *   SCREEN_CONV_LIST
 *     Scrollable list of conversations, one per registered transport.
 *     Each entry shows the transport icon, name, and last message preview.
 *     A [+] button in the header opens SCREEN_TRANSPORT_SELECT.
 *
 *   SCREEN_TRANSPORT_SELECT
 *     Picker showing all registered (not necessarily available) transports.
 *     Tapping one opens SCREEN_CHAT for that transport.
 *
 *   SCREEN_CHAT
 *     Classic message-bubble chat view.  The header shows the transport
 *     name in brackets: "Messenger [LoRa]".
 *     Input bar + send button at the bottom.
 *
 * Backward compatibility: LoRa broadcast works exactly as before.
 * SMS, BLE, and Internet backends are registered stubs; their
 * is_available() returns false until real drivers are wired up.
 */

#include "messenger/messenger_app.h"
#include "messenger/messenger_transport.h"

#include "lvgl.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "string.h"
#include "stdio.h"
#include "stdlib.h"

#include "thistle/wifi_manager.h"
#include "ui/theme.h"

static const char *TAG = "messenger_ui";

static volatile int s_pending_rx = 0;
#define MAX_PENDING_RX 10

/* ------------------------------------------------------------------ */
/* Constants                                                            */
/* ------------------------------------------------------------------ */

#define MSG_MAX_TEXT     200
#define MSG_HISTORY      50
#define MAX_CONVS        MSG_TRANSPORT_COUNT  /* one conversation per transport */

/* Fixed pixel dimensions for 240×296 display */
#define DISP_W           240
#define DISP_H           296
#define HEADER_H         30
#define INPUT_BAR_H      40
#define CHAT_LIST_H      (DISP_H - HEADER_H - INPUT_BAR_H)   /* 146 */
#define CONV_ITEM_H      48

/* ------------------------------------------------------------------ */
/* Data types                                                           */
/* ------------------------------------------------------------------ */

typedef enum {
    SCREEN_CONV_LIST,
    SCREEN_TRANSPORT_SELECT,
    SCREEN_CHAT,
} app_screen_t;

typedef struct {
    char sender[16];
    char text[MSG_MAX_TEXT];
    char time_str[8];   /* "HH:MM" */
    bool is_self;
} chat_message_t;

typedef struct {
    msg_transport_t      transport;
    char                 dest[32];          /* empty = broadcast */
    chat_message_t       messages[MSG_HISTORY];
    int                  msg_count;
    char                 last_preview[48];  /* shown in conversation list */
    char                 last_time[8];
} conversation_t;

/* ------------------------------------------------------------------ */
/* Module state                                                         */
/* ------------------------------------------------------------------ */

static struct {
    /* LVGL root */
    lv_obj_t   *root;

    /* Active screen */
    app_screen_t screen;

    /* --- Conversation list screen --- */
    lv_obj_t   *conv_list_screen;
    lv_obj_t   *conv_list;          /* scrollable column of items */

    /* --- Transport select screen --- */
    lv_obj_t   *transport_select_screen;

    /* --- Chat screen --- */
    lv_obj_t   *chat_screen;
    lv_obj_t   *chat_header_label;
    lv_obj_t   *msg_list;
    lv_obj_t   *input_ta;
    lv_obj_t   *send_btn;

    /* Conversations (one per transport) */
    conversation_t  convs[MAX_CONVS];
    int             conv_count;          /* initialised convs */
    int             active_conv;         /* index into convs[] */

    /* Device identity (used by LoRa) */
    uint32_t        device_id;
    char            device_id_str[12];   /* "Node-XXXX" */
} s_msg;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void show_screen(app_screen_t screen);
static void build_conv_list_screen(void);
static void build_transport_select_screen(void);
static void build_chat_screen(void);
static void refresh_conv_list(void);
static void open_conversation(int conv_idx);
static void create_message_bubble(const chat_message_t *msg);
static void add_message_to_conv(int conv_idx, const char *sender,
                                const char *text, bool is_self);
static void send_message(void);

/* ------------------------------------------------------------------ */
/* Transport RX callback (may be called from any task context)         */
/* ------------------------------------------------------------------ */

/*
 * All transport backends call this when a message arrives.  Because
 * LVGL is not thread-safe we marshal updates through lv_async_call so
 * the actual UI work runs on the LVGL task.
 */

typedef struct {
    msg_transport_t transport;
    char            sender[16];
    char            text[MSG_MAX_TEXT];
} rx_async_arg_t;

static void rx_async_handler(void *arg)
{
    rx_async_arg_t *rx = (rx_async_arg_t *)arg;
    if (!rx) return;
    s_pending_rx--;

    /* Find the matching conversation by transport type */
    int ci = -1;
    for (int i = 0; i < s_msg.conv_count; i++) {
        if (s_msg.convs[i].transport == rx->transport) {
            ci = i;
            break;
        }
    }

    if (ci >= 0) {
        add_message_to_conv(ci, rx->sender, rx->text, false);

        /* If this conversation is currently open, bubbles already rendered */
        /* If we are on the conversation list, refresh the preview row */
        if (s_msg.screen == SCREEN_CONV_LIST) {
            refresh_conv_list();
        }
    }

    /* lv_async_call does NOT free the arg — we must do it ourselves.
     * The arg was heap-allocated in transport_rx_cb below. */
    free(rx);
}

static void transport_rx_cb(msg_transport_t transport,
                            const char *sender,
                            const char *text)
{
    if (s_pending_rx >= MAX_PENDING_RX) {
        ESP_LOGW(TAG, "RX queue full, dropping message");
        return;
    }
    s_pending_rx++;
    rx_async_arg_t *arg = malloc(sizeof(rx_async_arg_t));
    if (!arg) { s_pending_rx--; return; }

    arg->transport = transport;
    strncpy(arg->sender, sender ? sender : "?", sizeof(arg->sender) - 1);
    arg->sender[sizeof(arg->sender) - 1] = '\0';
    strncpy(arg->text, text ? text : "", sizeof(arg->text) - 1);
    arg->text[sizeof(arg->text) - 1] = '\0';

    lv_async_call(rx_async_handler, arg);
}

/* ------------------------------------------------------------------ */
/* Screen switching                                                     */
/* ------------------------------------------------------------------ */

static void show_screen(app_screen_t screen)
{
    /* Hide all sub-screens first */
    if (s_msg.conv_list_screen)
        lv_obj_add_flag(s_msg.conv_list_screen, LV_OBJ_FLAG_HIDDEN);
    if (s_msg.transport_select_screen)
        lv_obj_add_flag(s_msg.transport_select_screen, LV_OBJ_FLAG_HIDDEN);
    if (s_msg.chat_screen)
        lv_obj_add_flag(s_msg.chat_screen, LV_OBJ_FLAG_HIDDEN);

    switch (screen) {
        case SCREEN_CONV_LIST:
            if (s_msg.conv_list_screen) {
                lv_obj_clear_flag(s_msg.conv_list_screen, LV_OBJ_FLAG_HIDDEN);
                refresh_conv_list();
            }
            break;
        case SCREEN_TRANSPORT_SELECT:
            if (s_msg.transport_select_screen)
                lv_obj_clear_flag(s_msg.transport_select_screen, LV_OBJ_FLAG_HIDDEN);
            break;
        case SCREEN_CHAT:
            if (s_msg.chat_screen)
                lv_obj_clear_flag(s_msg.chat_screen, LV_OBJ_FLAG_HIDDEN);
            break;
    }
    s_msg.screen = screen;
}

/* ------------------------------------------------------------------ */
/* Conversation list screen                                             */
/* ------------------------------------------------------------------ */

static void conv_item_cb(lv_event_t *e)
{
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    open_conversation(idx);
}

static void new_conv_btn_cb(lv_event_t *e)
{
    (void)e;
    show_screen(SCREEN_TRANSPORT_SELECT);
}

static void build_conv_list_screen(void)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *scr = lv_obj_create(s_msg.root);
    lv_obj_set_size(scr, DISP_W, DISP_H);
    lv_obj_set_pos(scr, 0, 0);
    lv_obj_set_style_bg_color(scr, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(scr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(scr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(scr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(scr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);
    s_msg.conv_list_screen = scr;

    /* Header bar */
    lv_obj_t *header = lv_obj_create(scr);
    lv_obj_set_size(header, DISP_W, HEADER_H);
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
    lv_label_set_text(title, "Messenger");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(title, colors->text, LV_PART_MAIN);
    lv_obj_align(title, LV_ALIGN_LEFT_MID, 0, 0);

    /* [+] new conversation button */
    lv_obj_t *new_btn = lv_button_create(header);
    lv_obj_set_size(new_btn, 22, 22);
    lv_obj_align(new_btn, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_set_style_bg_color(new_btn, colors->primary, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(new_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_radius(new_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(new_btn, 2, LV_PART_MAIN);
    lv_obj_t *new_lbl = lv_label_create(new_btn);
    lv_label_set_text(new_lbl, "+");
    lv_obj_set_style_text_color(new_lbl, lv_color_white(), LV_PART_MAIN);
    lv_obj_center(new_lbl);
    lv_obj_add_event_cb(new_btn, new_conv_btn_cb, LV_EVENT_CLICKED, NULL);

    /* Scrollable list body */
    s_msg.conv_list = lv_obj_create(scr);
    lv_obj_set_size(s_msg.conv_list, DISP_W, DISP_H - HEADER_H);
    lv_obj_set_pos(s_msg.conv_list, 0, HEADER_H);
    lv_obj_set_style_bg_color(s_msg.conv_list, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_msg.conv_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_msg.conv_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_msg.conv_list, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_msg.conv_list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_msg.conv_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_scroll_dir(s_msg.conv_list, LV_DIR_VER);
}

/*
 * refresh_conv_list — rebuild the conversation list items.
 * Called after init and whenever a new message arrives while the list is shown.
 */
static void refresh_conv_list(void)
{
    if (!s_msg.conv_list) return;
    const theme_colors_t *colors = theme_get_colors();

    /* Remove all existing items */
    lv_obj_clean(s_msg.conv_list);

    for (int i = 0; i < s_msg.conv_count; i++) {
        conversation_t *cv = &s_msg.convs[i];
        const msg_transport_driver_t *drv = messenger_get_transport(cv->transport);
        if (!drv) continue;

        bool avail = drv->is_available();

        /* Item row */
        lv_obj_t *item = lv_obj_create(s_msg.conv_list);
        lv_obj_set_size(item, DISP_W, CONV_ITEM_H);
        lv_obj_set_style_bg_color(item, colors->surface, LV_PART_MAIN);
        lv_obj_set_style_bg_opa(item, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_border_side(item, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
        lv_obj_set_style_border_color(item, colors->text_secondary, LV_PART_MAIN);
        lv_obj_set_style_border_width(item, 1, LV_PART_MAIN);
        lv_obj_set_style_radius(item, 0, LV_PART_MAIN);
        lv_obj_set_style_pad_hor(item, 8, LV_PART_MAIN);
        lv_obj_set_style_pad_ver(item, 4, LV_PART_MAIN);
        lv_obj_clear_flag(item, LV_OBJ_FLAG_SCROLLABLE);
        lv_obj_add_flag(item, LV_OBJ_FLAG_CLICKABLE);
        lv_obj_add_event_cb(item, conv_item_cb, LV_EVENT_CLICKED,
                            (void *)(intptr_t)i);

        /* First line: icon + transport name */
        char top_str[40];
        snprintf(top_str, sizeof(top_str), "%s %s%s",
                 drv->icon, drv->name,
                 avail ? "" : " (unavailable)");

        lv_obj_t *name_lbl = lv_label_create(item);
        lv_label_set_text(name_lbl, top_str);
        lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(name_lbl,
            avail ? colors->text : colors->text_secondary,
            LV_PART_MAIN);
        lv_obj_set_pos(name_lbl, 0, 0);

        /* Second line: last message preview */
        if (cv->last_preview[0] != '\0') {
            char preview[64];
            snprintf(preview, sizeof(preview), "%s (%s)",
                     cv->last_preview, cv->last_time);

            lv_obj_t *prev_lbl = lv_label_create(item);
            lv_label_set_text(prev_lbl, preview);
            lv_label_set_long_mode(prev_lbl, LV_LABEL_LONG_DOT);
            lv_obj_set_width(prev_lbl, DISP_W - 16);
            lv_obj_set_style_text_font(prev_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
            lv_obj_set_style_text_color(prev_lbl, colors->text_secondary, LV_PART_MAIN);
            lv_obj_set_pos(prev_lbl, 0, 18);
        }
    }
}

/* ------------------------------------------------------------------ */
/* Transport select screen                                              */
/* ------------------------------------------------------------------ */

static void transport_pick_cb(lv_event_t *e)
{
    msg_transport_t type = (msg_transport_t)(intptr_t)lv_event_get_user_data(e);

    /* Find or create a conversation for this transport */
    int ci = -1;
    for (int i = 0; i < s_msg.conv_count; i++) {
        if (s_msg.convs[i].transport == type) {
            ci = i;
            break;
        }
    }
    if (ci < 0 && s_msg.conv_count < MAX_CONVS) {
        ci = s_msg.conv_count++;
        memset(&s_msg.convs[ci], 0, sizeof(conversation_t));
        s_msg.convs[ci].transport = type;
    }
    if (ci >= 0) {
        open_conversation(ci);
    }
}

static void transport_back_cb(lv_event_t *e)
{
    (void)e;
    show_screen(SCREEN_CONV_LIST);
}

static void build_transport_select_screen(void)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *scr = lv_obj_create(s_msg.root);
    lv_obj_set_size(scr, DISP_W, DISP_H);
    lv_obj_set_pos(scr, 0, 0);
    lv_obj_set_style_bg_color(scr, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(scr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(scr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(scr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(scr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);
    s_msg.transport_select_screen = scr;

    /* Header */
    lv_obj_t *header = lv_obj_create(scr);
    lv_obj_set_size(header, DISP_W, HEADER_H);
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
    lv_label_set_text(title, "< New Conversation");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(title, colors->text, LV_PART_MAIN);
    lv_obj_align(title, LV_ALIGN_LEFT_MID, 0, 0);
    lv_obj_add_flag(title, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(title, transport_back_cb, LV_EVENT_CLICKED, NULL);

    /* Transport buttons */
    lv_obj_t *list = lv_obj_create(scr);
    lv_obj_set_size(list, DISP_W, DISP_H - HEADER_H);
    lv_obj_set_pos(list, 0, HEADER_H);
    lv_obj_set_style_bg_color(list, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(list, 8, LV_PART_MAIN);
    lv_obj_set_style_radius(list, 0, LV_PART_MAIN);
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(list, 8, LV_PART_MAIN);
    lv_obj_clear_flag(list, LV_OBJ_FLAG_SCROLLABLE);

    for (int t = 0; t < MSG_TRANSPORT_COUNT; t++) {
        const msg_transport_driver_t *drv = messenger_get_transport((msg_transport_t)t);
        if (!drv) continue;

        bool avail = drv->is_available();

        lv_obj_t *btn = lv_button_create(list);
        lv_obj_set_size(btn, DISP_W - 16, 36);
        lv_obj_set_style_bg_color(btn,
            avail ? colors->primary : colors->surface,
            LV_PART_MAIN);
        lv_obj_set_style_bg_opa(btn, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_radius(btn, 4, LV_PART_MAIN);
        lv_obj_set_style_border_width(btn,
            avail ? 0 : 1,
            LV_PART_MAIN);
        lv_obj_set_style_border_color(btn, colors->text_secondary, LV_PART_MAIN);

        char btn_label[48];
        snprintf(btn_label, sizeof(btn_label), "%s %s%s",
                 drv->icon, drv->name,
                 avail ? "" : " — unavailable");

        lv_obj_t *lbl = lv_label_create(btn);
        lv_label_set_text(lbl, btn_label);
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, LV_PART_MAIN);
        lv_obj_set_style_text_color(lbl,
            avail ? lv_color_white() : colors->text_secondary,
            LV_PART_MAIN);
        lv_obj_center(lbl);

        lv_obj_add_event_cb(btn, transport_pick_cb, LV_EVENT_CLICKED,
                            (void *)(intptr_t)t);
    }
}

/* ------------------------------------------------------------------ */
/* Chat screen                                                          */
/* ------------------------------------------------------------------ */

static void chat_back_cb(lv_event_t *e)
{
    (void)e;

    /* Stop receiving on the current transport */
    if (s_msg.active_conv >= 0) {
        const msg_transport_driver_t *drv =
            messenger_get_transport(s_msg.convs[s_msg.active_conv].transport);
        if (drv && drv->stop_receive) drv->stop_receive();
    }
    s_msg.active_conv = -1;
    show_screen(SCREEN_CONV_LIST);
}

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

static void build_chat_screen(void)
{
    const theme_colors_t *colors = theme_get_colors();

    lv_obj_t *scr = lv_obj_create(s_msg.root);
    lv_obj_set_size(scr, DISP_W, DISP_H);
    lv_obj_set_pos(scr, 0, 0);
    lv_obj_set_style_bg_color(scr, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(scr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(scr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(scr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(scr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);
    s_msg.chat_screen = scr;

    /* Header bar */
    lv_obj_t *header = lv_obj_create(scr);
    lv_obj_set_size(header, DISP_W, HEADER_H);
    lv_obj_set_pos(header, 0, 0);
    lv_obj_set_style_bg_color(header, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(header, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(header, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(header, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(header, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_all(header, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(header, 0, LV_PART_MAIN);
    lv_obj_clear_flag(header, LV_OBJ_FLAG_SCROLLABLE);

    s_msg.chat_header_label = lv_label_create(header);
    lv_label_set_text(s_msg.chat_header_label, "Messenger");
    lv_obj_set_style_text_font(s_msg.chat_header_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_msg.chat_header_label, colors->text, LV_PART_MAIN);
    lv_obj_align(s_msg.chat_header_label, LV_ALIGN_LEFT_MID, 0, 0);

    /* Back button */
    lv_obj_t *back_btn = lv_button_create(header);
    lv_obj_set_size(back_btn, 28, 22);
    lv_obj_align(back_btn, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_set_style_bg_color(back_btn, colors->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(back_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(back_btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(back_btn, colors->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_radius(back_btn, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_all(back_btn, 2, LV_PART_MAIN);
    lv_obj_t *back_lbl = lv_label_create(back_btn);
    lv_label_set_text(back_lbl, "<<");
    lv_obj_set_style_text_color(back_lbl, colors->text, LV_PART_MAIN);
    lv_obj_center(back_lbl);
    lv_obj_add_event_cb(back_btn, chat_back_cb, LV_EVENT_CLICKED, NULL);

    /* Message list */
    s_msg.msg_list = lv_obj_create(scr);
    lv_obj_set_size(s_msg.msg_list, DISP_W, CHAT_LIST_H);
    lv_obj_set_pos(s_msg.msg_list, 0, HEADER_H);
    lv_obj_set_style_bg_color(s_msg.msg_list, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_msg.msg_list, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_msg.msg_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_msg.msg_list, 4, LV_PART_MAIN);
    lv_obj_set_style_radius(s_msg.msg_list, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_row(s_msg.msg_list, 4, LV_PART_MAIN);
    lv_obj_set_flex_flow(s_msg.msg_list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_scroll_dir(s_msg.msg_list, LV_DIR_VER);

    /* Input bar */
    lv_obj_t *input_bar = lv_obj_create(scr);
    lv_obj_set_size(input_bar, DISP_W, INPUT_BAR_H);
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

    s_msg.input_ta = lv_textarea_create(input_bar);
    lv_textarea_set_one_line(s_msg.input_ta, true);
    lv_textarea_set_placeholder_text(s_msg.input_ta, "Type a message...");
    lv_obj_set_flex_grow(s_msg.input_ta, 1);
    lv_obj_set_height(s_msg.input_ta, 30);
    lv_obj_add_event_cb(s_msg.input_ta, ta_key_cb, LV_EVENT_KEY, NULL);

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
}

/* ------------------------------------------------------------------ */
/* Open a conversation (switch to chat screen and start receiving)     */
/* ------------------------------------------------------------------ */

static void open_conversation(int conv_idx)
{
    if (conv_idx < 0 || conv_idx >= s_msg.conv_count) return;

    s_msg.active_conv = conv_idx;
    conversation_t *cv = &s_msg.convs[conv_idx];
    const msg_transport_driver_t *drv = messenger_get_transport(cv->transport);

    /* Update header label: "Messenger [LoRa]" */
    if (s_msg.chat_header_label && drv) {
        char hdr[48];
        snprintf(hdr, sizeof(hdr), "Messenger [%s]", drv->name);
        lv_label_set_text(s_msg.chat_header_label, hdr);
    }

    /* Dim send controls if transport unavailable */
    const theme_colors_t *colors = theme_get_colors();
    bool avail = (drv && drv->is_available());

    if (s_msg.send_btn) {
        lv_obj_set_style_bg_color(s_msg.send_btn,
            avail ? colors->primary : colors->text_secondary,
            LV_PART_MAIN);
    }
    if (s_msg.input_ta) {
        lv_obj_set_style_text_color(s_msg.input_ta,
            avail ? colors->text : colors->text_secondary,
            LV_PART_MAIN);
    }

    /* Rebuild message bubbles from history */
    if (s_msg.msg_list) {
        lv_obj_clean(s_msg.msg_list);
        int count = cv->msg_count;
        int start = (count > MSG_HISTORY) ? count - MSG_HISTORY : 0;
        for (int i = start; i < count; i++) {
            create_message_bubble(&cv->messages[i % MSG_HISTORY]);
        }
        lv_obj_scroll_to_y(s_msg.msg_list, LV_COORD_MAX, LV_ANIM_OFF);
    }

    /* Start receiving on this transport */
    if (drv && drv->start_receive) {
        drv->start_receive(transport_rx_cb);
    }

    show_screen(SCREEN_CHAT);
}

/* ------------------------------------------------------------------ */
/* Message bubble creation                                              */
/* ------------------------------------------------------------------ */

static void create_message_bubble(const chat_message_t *msg)
{
    if (!s_msg.msg_list) return;
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
        lv_obj_set_style_bg_color(bubble, colors->primary, LV_PART_MAIN);
        lv_obj_set_align(bubble, LV_ALIGN_RIGHT_MID);
    } else {
        lv_obj_set_style_bg_color(bubble, colors->surface, LV_PART_MAIN);
    }
    lv_obj_set_style_bg_opa(bubble, LV_OPA_COVER, LV_PART_MAIN);

    lv_obj_set_flex_flow(bubble, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(bubble, 2, LV_PART_MAIN);

    /* Sender + timestamp */
    lv_obj_t *hdr_lbl = lv_label_create(bubble);
    char hdr[32];
    snprintf(hdr, sizeof(hdr), "[%s] %s", msg->sender, msg->time_str);
    lv_label_set_text(hdr_lbl, hdr);
    lv_obj_set_style_text_font(hdr_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(hdr_lbl,
        msg->is_self ? lv_color_white() : colors->text_secondary,
        LV_PART_MAIN);

    /* Message body */
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
/* Add message to a conversation's history and (optionally) the UI     */
/* ------------------------------------------------------------------ */

static void add_message_to_conv(int conv_idx, const char *sender,
                                const char *text, bool is_self)
{
    if (conv_idx < 0 || conv_idx >= s_msg.conv_count) return;

    conversation_t *cv = &s_msg.convs[conv_idx];
    int idx = cv->msg_count % MSG_HISTORY;

    strncpy(cv->messages[idx].sender, sender, 15);
    cv->messages[idx].sender[15] = '\0';
    strncpy(cv->messages[idx].text, text, MSG_MAX_TEXT - 1);
    cv->messages[idx].text[MSG_MAX_TEXT - 1] = '\0';
    cv->messages[idx].is_self = is_self;

    char time_buf[8];
    wifi_manager_get_time_str(time_buf, sizeof(time_buf));
    strncpy(cv->messages[idx].time_str, time_buf, 7);
    cv->messages[idx].time_str[7] = '\0';

    cv->msg_count++;

    /* Update conversation list preview */
    strncpy(cv->last_preview, text, sizeof(cv->last_preview) - 1);
    cv->last_preview[sizeof(cv->last_preview) - 1] = '\0';
    strncpy(cv->last_time, time_buf, sizeof(cv->last_time) - 1);
    cv->last_time[sizeof(cv->last_time) - 1] = '\0';

    /* If this conversation is the active chat, add a bubble immediately */
    if (s_msg.screen == SCREEN_CHAT && s_msg.active_conv == conv_idx) {
        create_message_bubble(&cv->messages[idx]);
        if (s_msg.msg_list) {
            lv_obj_scroll_to_y(s_msg.msg_list, LV_COORD_MAX, LV_ANIM_ON);
        }
    }
}

/* ------------------------------------------------------------------ */
/* Send a message on the active conversation's transport               */
/* ------------------------------------------------------------------ */

static void send_message(void)
{
    if (!s_msg.input_ta) return;
    if (s_msg.active_conv < 0) return;

    const char *text = lv_textarea_get_text(s_msg.input_ta);
    if (!text || text[0] == '\0') return;

    conversation_t *cv = &s_msg.convs[s_msg.active_conv];
    const msg_transport_driver_t *drv = messenger_get_transport(cv->transport);

    if (drv && drv->send && drv->is_available()) {
        esp_err_t ret = drv->send(cv->dest[0] ? cv->dest : NULL, text);
        if (ret != ESP_OK) {
            ESP_LOGW(TAG, "send failed on %s: %s",
                     drv->name, esp_err_to_name(ret));
            /* Still echo locally so the user sees their own message */
        }
    } else {
        ESP_LOGW(TAG, "transport %s unavailable — local echo only",
                 drv ? drv->name : "?");
    }

    /* Always show the message locally */
    add_message_to_conv(s_msg.active_conv, "You", text, true);

    lv_textarea_set_text(s_msg.input_ta, "");
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t messenger_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "creating messenger UI (multi-transport)");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Reset state */
    memset(&s_msg, 0, sizeof(s_msg));
    s_msg.active_conv = -1;

    /* Generate device identity (used by LoRa backend) */
    s_msg.device_id = (uint32_t)esp_timer_get_time() ^ 0xDEADBEEF;
    snprintf(s_msg.device_id_str, sizeof(s_msg.device_id_str),
             "Node-%04X", (unsigned)(s_msg.device_id & 0xFFFF));

    /* Initialise transport registry */
    messenger_transport_init();

    /* Root container — fills the app panel */
    const theme_colors_t *colors = theme_get_colors();
    s_msg.root = lv_obj_create(parent);
    lv_obj_set_size(s_msg.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_msg.root, 0, 0);
    lv_obj_set_style_bg_color(s_msg.root, colors->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_msg.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_msg.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_msg.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_msg.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_msg.root, LV_OBJ_FLAG_SCROLLABLE);

    /* Build all sub-screens (each starts hidden) */
    build_conv_list_screen();
    build_transport_select_screen();
    build_chat_screen();

    /*
     * Pre-populate one conversation per transport so the list is
     * immediately meaningful.  Only LoRa will show as available on
     * typical hardware; others show as "(unavailable)" until the
     * corresponding drivers are implemented.
     */
    for (int t = 0; t < MSG_TRANSPORT_COUNT; t++) {
        const msg_transport_driver_t *drv = messenger_get_transport((msg_transport_t)t);
        if (!drv) continue;

        int ci = s_msg.conv_count++;
        memset(&s_msg.convs[ci], 0, sizeof(conversation_t));
        s_msg.convs[ci].transport = (msg_transport_t)t;
    }

    /* Start on the conversation list */
    show_screen(SCREEN_CONV_LIST);

    ESP_LOGI(TAG, "messenger ready — node %s", s_msg.device_id_str);
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

        /* Stop any active receive when hidden */
        if (s_msg.active_conv >= 0) {
            const msg_transport_driver_t *drv =
                messenger_get_transport(s_msg.convs[s_msg.active_conv].transport);
            if (drv && drv->stop_receive) drv->stop_receive();
        }
    }
}
