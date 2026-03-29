/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — GhostTerm serial terminal UI
 *
 * Hardware UART terminal for connecting to external devices.
 * Layout identical to terminal_ui.c but talks to UART instead of
 * dispatching built-in commands.
 *
 * Layout (dynamic app area, default 240x296):
 *   +--------------------------------+
 *   |  GhostTerm          115200 8N1 |  30 px header
 *   +--------------------------------+
 *   |  [received UART data]          |  scrollable output (monospace)
 *   |  ...                           |
 *   +--------------------------------+
 *   |  > [input text              ]> |  28 px input bar
 *   +--------------------------------+
 */
#include "ghostterm/ghostterm_app.h"

#include "ui/theme.h"
#include "ui/manager.h"

#include "lvgl.h"
#include "esp_log.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

#include "driver/uart.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char *TAG = "ghostterm_ui";

/* Shell modes */
typedef enum {
    GHOST_MODE_LOCAL,   /* Local shell — commands go to thistle_shell */
    GHOST_MODE_UART,    /* UART serial — commands go to hardware UART */
} ghost_mode_t;

/* Rust shell module FFI */
extern int thistle_shell_exec(const char *input,
                              void (*output_cb)(const char *line, void *ctx),
                              void *user_data);

/* ------------------------------------------------------------------ */
/* Layout constants                                                     */
/* ------------------------------------------------------------------ */

/* App-area dimensions — set from parent in ghostterm_ui_create() */
static int s_app_w = 240;
static int s_app_h = 296;
#define HEADER_H      30
#define INPUT_BAR_H   28

/* Max characters in the output textarea before we trim */
#define OUTPUT_MAX_CHARS  2048

/* ------------------------------------------------------------------ */
/* State                                                                */
/* ------------------------------------------------------------------ */

static struct {
    lv_obj_t    *root;
    lv_obj_t    *output_ta;     /* read-only monospace textarea */
    lv_obj_t    *input_ta;      /* one-line input */
    lv_obj_t    *mode_label;    /* mode display in header */
    ghost_mode_t mode;
    int          uart_num;
    int          baud_rate;
    bool         local_echo;
    bool         uart_running;
    TaskHandle_t rx_task_handle;
} s_ghost;

/* ------------------------------------------------------------------ */
/* Output helpers                                                       */
/* ------------------------------------------------------------------ */

static void ghost_print_raw(const char *text)
{
    if (!s_ghost.output_ta || !text) return;

    /* Trim if oversized */
    const char *cur = lv_textarea_get_text(s_ghost.output_ta);
    if (cur && strlen(cur) > OUTPUT_MAX_CHARS - 128) {
        const char *nl = strchr(cur, '\n');
        if (nl) {
            lv_textarea_set_text(s_ghost.output_ta, nl + 1);
        } else {
            lv_textarea_set_text(s_ghost.output_ta, "");
        }
    }

    lv_textarea_add_text(s_ghost.output_ta, text);

    /* Scroll to end */
    lv_obj_scroll_to_y(s_ghost.output_ta,
                       lv_obj_get_scroll_bottom(s_ghost.output_ta),
                       LV_ANIM_OFF);
}

static void ghost_print_line(const char *line)
{
    if (!s_ghost.output_ta) return;
    ghost_print_raw(line);
    ghost_print_raw("\n");
}

/* Callback for thistle_shell_exec — prints each line to the output textarea */
static void shell_output_cb(const char *line, void *ctx)
{
    (void)ctx;
    ghost_print_line(line);
}

/* ------------------------------------------------------------------ */
/* UART setup                                                           */
/* ------------------------------------------------------------------ */

static void ghostterm_uart_init(void)
{
    uart_config_t cfg = {
        .baud_rate  = s_ghost.baud_rate,
        .data_bits  = UART_DATA_8_BITS,
        .parity     = UART_PARITY_DISABLE,
        .stop_bits  = UART_STOP_BITS_1,
        .flow_ctrl  = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
    };
    uart_param_config(s_ghost.uart_num, &cfg);
    uart_set_pin(s_ghost.uart_num, 1, 2, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE);
    uart_driver_install(s_ghost.uart_num, 1024, 0, 0, NULL, 0);
    s_ghost.uart_running = true;
}

static void ghostterm_uart_reinit(void)
{
    uart_driver_delete(s_ghost.uart_num);
    ghostterm_uart_init();
}

/* ------------------------------------------------------------------ */
/* RX task                                                              */
/* ------------------------------------------------------------------ */

#ifndef SIMULATOR_BUILD
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#else
#define portTICK_PERIOD_MS 1
#endif

static void ghostterm_rx_task(void *arg)
{
    (void)arg;
    uint8_t buf[128];
    while (s_ghost.uart_running) {
        int len = uart_read_bytes(s_ghost.uart_num, buf, sizeof(buf) - 1,
                                  100 / portTICK_PERIOD_MS);
        if (len > 0) {
            buf[len] = '\0';
            ui_manager_lock();
            ghost_print_raw((const char *)buf);
            ui_manager_unlock();
        }
    }
    vTaskDelete(NULL);
}

static void ghostterm_rx_task_start(void)
{
    if (s_ghost.rx_task_handle) return;
    s_ghost.uart_running = true;
    xTaskCreate(ghostterm_rx_task, "ghost_rx", 4096, NULL, 5,
                &s_ghost.rx_task_handle);
}

static void ghostterm_rx_task_stop(void)
{
    s_ghost.uart_running = false;
    /* Task will self-delete on next loop iteration */
    s_ghost.rx_task_handle = NULL;
}

/* ------------------------------------------------------------------ */
/* Built-in slash commands                                              */
/* ------------------------------------------------------------------ */

static void update_mode_label(void)
{
    if (!s_ghost.mode_label) return;
    char buf[32];
    if (s_ghost.mode == GHOST_MODE_LOCAL) {
        snprintf(buf, sizeof(buf), "Local");
    } else {
        snprintf(buf, sizeof(buf), "UART %d", s_ghost.baud_rate);
    }
    lv_label_set_text(s_ghost.mode_label, buf);
}

static void cmd_baud(const char *arg)
{
    if (s_ghost.mode != GHOST_MODE_UART) {
        ghost_print_line("[GhostTerm] /baud only available in UART mode");
        return;
    }
    int rate = atoi(arg);
    if (rate != 9600 && rate != 19200 && rate != 38400 &&
        rate != 57600 && rate != 115200) {
        ghost_print_line("[GhostTerm] Invalid baud rate. Use: 9600, 19200, 38400, 57600, 115200");
        return;
    }
    s_ghost.baud_rate = rate;
    ghostterm_rx_task_stop();
    ghostterm_uart_reinit();
    ghostterm_rx_task_start();
    update_mode_label();
    char buf[48];
    snprintf(buf, sizeof(buf), "[GhostTerm] Baud rate set to %d", rate);
    ghost_print_line(buf);
}

static void cmd_clear(void)
{
    if (s_ghost.output_ta) {
        lv_textarea_set_text(s_ghost.output_ta, "");
    }
}

static void cmd_help(void)
{
    ghost_print_line("[GhostTerm] Commands:");
    ghost_print_line("  /mode local   - switch to local shell");
    ghost_print_line("  /mode uart    - switch to UART serial");
    ghost_print_line("  /baud <rate>  - set baud (9600/19200/38400/57600/115200)");
    ghost_print_line("  /clear        - clear output");
    ghost_print_line("  /echo on|off  - toggle local echo");
    ghost_print_line("  /help         - this list");
}

static void cmd_echo(const char *arg)
{
    if (strcmp(arg, "on") == 0) {
        s_ghost.local_echo = true;
        ghost_print_line("[GhostTerm] Local echo ON");
    } else if (strcmp(arg, "off") == 0) {
        s_ghost.local_echo = false;
        ghost_print_line("[GhostTerm] Local echo OFF");
    } else {
        ghost_print_line("[GhostTerm] Usage: /echo on|off");
    }
}

static void dispatch_slash_command(const char *raw)
{
    /* Skip the leading '/' */
    raw++;
    while (*raw == ' ') raw++;

    if (strncmp(raw, "baud ", 5) == 0) {
        cmd_baud(raw + 5);
    } else if (strcmp(raw, "clear") == 0) {
        cmd_clear();
    } else if (strcmp(raw, "help") == 0) {
        cmd_help();
    } else if (strncmp(raw, "echo ", 5) == 0) {
        cmd_echo(raw + 5);
    } else if (strcmp(raw, "echo") == 0) {
        char buf[48];
        snprintf(buf, sizeof(buf), "[GhostTerm] Local echo: %s",
                 s_ghost.local_echo ? "ON" : "OFF");
        ghost_print_line(buf);
    } else if (strcmp(raw, "mode local") == 0 || strcmp(raw, "mode shell") == 0) {
        if (s_ghost.mode != GHOST_MODE_LOCAL) {
            ghostterm_rx_task_stop();
            s_ghost.mode = GHOST_MODE_LOCAL;
            update_mode_label();
            ghost_print_line("[GhostTerm] Switched to Local shell mode");
        }
    } else if (strncmp(raw, "mode uart", 9) == 0) {
        if (s_ghost.mode != GHOST_MODE_UART) {
            s_ghost.mode = GHOST_MODE_UART;
            ghostterm_uart_init();
            ghostterm_rx_task_start();
            update_mode_label();
            ghost_print_line("[GhostTerm] Switched to UART mode");
        }
    } else if (strcmp(raw, "mode") == 0) {
        char buf[48];
        snprintf(buf, sizeof(buf), "[GhostTerm] Mode: %s",
                 s_ghost.mode == GHOST_MODE_LOCAL ? "Local" : "UART");
        ghost_print_line(buf);
    } else {
        ghost_print_line("[GhostTerm] Unknown command. Type /help");
    }
}

/* ------------------------------------------------------------------ */
/* Input event callback                                                 */
/* ------------------------------------------------------------------ */

static void input_ta_cb(lv_event_t *e)
{
    lv_event_code_t code = lv_event_get_code(e);

    if (code == LV_EVENT_KEY) {
        uint32_t key = lv_event_get_key(e);
        if (key == LV_KEY_ENTER) {
            const char *text = lv_textarea_get_text(s_ghost.input_ta);
            if (text && text[0] != '\0') {
                char cmd[128];
                strncpy(cmd, text, sizeof(cmd) - 1);
                cmd[sizeof(cmd) - 1] = '\0';

                lv_textarea_set_text(s_ghost.input_ta, "");

                if (cmd[0] == '/') {
                    /* Slash command — handle locally */
                    dispatch_slash_command(cmd);
                } else if (s_ghost.mode == GHOST_MODE_LOCAL) {
                    /* Local shell — dispatch to thistle_shell */
                    char prompt[140];
                    snprintf(prompt, sizeof(prompt), "$ %s", cmd);
                    ghost_print_line(prompt);
                    int ret = thistle_shell_exec(cmd, shell_output_cb, NULL);
                    if (ret == -2) {
                        /* Special: clear command */
                        lv_textarea_set_text(s_ghost.output_ta, "");
                    }
                } else {
                    /* UART mode — send over serial with \r\n */
                    if (s_ghost.local_echo) {
                        ghost_print_raw(cmd);
                        ghost_print_raw("\r\n");
                    }
                    uart_write_bytes(s_ghost.uart_num, cmd, strlen(cmd));
                    uart_write_bytes(s_ghost.uart_num, "\r\n", 2);
                }
            }
        }
    }
}

/* ------------------------------------------------------------------ */
/* Public API                                                           */
/* ------------------------------------------------------------------ */

esp_err_t ghostterm_ui_create(lv_obj_t *parent)
{
    ESP_LOGI(TAG, "Creating GhostTerm UI");

    if (parent == NULL) {
        parent = lv_scr_act();
    }

    /* Read actual dimensions from parent */
    lv_obj_update_layout(parent);
    s_app_w = lv_obj_get_width(parent);
    s_app_h = lv_obj_get_height(parent);
    if (s_app_w == 0) s_app_w = 240;  /* fallback */
    if (s_app_h == 0) s_app_h = 296;

    const int output_h = s_app_h - HEADER_H - INPUT_BAR_H;

    memset(&s_ghost, 0, sizeof(s_ghost));
    s_ghost.mode       = GHOST_MODE_LOCAL;
    s_ghost.uart_num   = UART_NUM_2;
    s_ghost.baud_rate  = 115200;
    s_ghost.local_echo = true;

    const theme_colors_t *clr = theme_get_colors();

    /* Root container */
    s_ghost.root = lv_obj_create(parent);
    lv_obj_set_size(s_ghost.root, LV_PCT(100), LV_PCT(100));
    lv_obj_set_pos(s_ghost.root, 0, 0);
    lv_obj_set_style_bg_color(s_ghost.root, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ghost.root, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_ghost.root, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(s_ghost.root, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ghost.root, 0, LV_PART_MAIN);
    lv_obj_clear_flag(s_ghost.root, LV_OBJ_FLAG_SCROLLABLE);

    /* Header */
    lv_obj_t *hdr = lv_obj_create(s_ghost.root);
    lv_obj_set_size(hdr, s_app_w, HEADER_H);
    lv_obj_set_pos(hdr, 0, 0);
    lv_obj_set_style_bg_color(hdr, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(hdr, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(hdr, LV_BORDER_SIDE_BOTTOM, LV_PART_MAIN);
    lv_obj_set_style_border_color(hdr, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(hdr, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_right(hdr, 8, LV_PART_MAIN);
    lv_obj_set_style_pad_top(hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(hdr, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(hdr, 0, LV_PART_MAIN);
    lv_obj_clear_flag(hdr, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(hdr, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(hdr, LV_FLEX_ALIGN_SPACE_BETWEEN, LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);

    lv_obj_t *title = lv_label_create(hdr);
    lv_label_set_text(title, "GhostTerm");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_18, LV_PART_MAIN);
    lv_obj_set_style_text_color(title, clr->text, LV_PART_MAIN);

    /* Mode label (right-aligned) */
    s_ghost.mode_label = lv_label_create(hdr);
    lv_obj_set_style_text_font(s_ghost.mode_label, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ghost.mode_label, clr->text_secondary, LV_PART_MAIN);
    update_mode_label();

    /* Output textarea (read-only, auto-scroll, monospace via montserrat_14) */
    s_ghost.output_ta = lv_textarea_create(s_ghost.root);
    lv_obj_set_pos(s_ghost.output_ta, 0, HEADER_H);
    lv_obj_set_size(s_ghost.output_ta, s_app_w, output_h);
    lv_textarea_set_one_line(s_ghost.output_ta, false);
    lv_textarea_set_cursor_click_pos(s_ghost.output_ta, false);
    lv_obj_clear_flag(s_ghost.output_ta, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_bg_color(s_ghost.output_ta, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ghost.output_ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_ghost.output_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ghost.output_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_ghost.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(s_ghost.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_ghost.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(s_ghost.output_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_text_font(s_ghost.output_ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ghost.output_ta, clr->text, LV_PART_MAIN);
    lv_obj_set_scrollbar_mode(s_ghost.output_ta, LV_SCROLLBAR_MODE_AUTO);
    lv_obj_set_style_bg_color(s_ghost.output_ta, clr->primary, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(s_ghost.output_ta, LV_OPA_COVER, LV_PART_SCROLLBAR);
    lv_obj_set_style_width(s_ghost.output_ta, 2, LV_PART_SCROLLBAR);
    /* Hide cursor */
    lv_obj_set_style_bg_opa(s_ghost.output_ta, LV_OPA_TRANSP, LV_PART_CURSOR);

    /* Input bar */
    lv_obj_t *input_bar = lv_obj_create(s_ghost.root);
    lv_obj_set_pos(input_bar, 0, HEADER_H + output_h);
    lv_obj_set_size(input_bar, s_app_w, INPUT_BAR_H);
    lv_obj_set_style_bg_color(input_bar, clr->surface, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(input_bar, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_side(input_bar, LV_BORDER_SIDE_TOP, LV_PART_MAIN);
    lv_obj_set_style_border_color(input_bar, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_border_width(input_bar, 1, LV_PART_MAIN);
    lv_obj_set_style_pad_left(input_bar, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(input_bar, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(input_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(input_bar, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(input_bar, 0, LV_PART_MAIN);
    lv_obj_clear_flag(input_bar, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_flex_flow(input_bar, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(input_bar, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(input_bar, 4, LV_PART_MAIN);

    /* ">" prompt label */
    lv_obj_t *prompt = lv_label_create(input_bar);
    lv_label_set_text(prompt, ">");
    lv_obj_set_style_text_font(prompt, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(prompt, clr->primary, LV_PART_MAIN);

    /* One-line input textarea */
    s_ghost.input_ta = lv_textarea_create(input_bar);
    lv_textarea_set_one_line(s_ghost.input_ta, true);
    lv_textarea_set_placeholder_text(s_ghost.input_ta, "type here...");
    lv_obj_set_flex_grow(s_ghost.input_ta, 1);
    lv_obj_set_height(s_ghost.input_ta, INPUT_BAR_H - 4);
    lv_obj_set_style_bg_color(s_ghost.input_ta, clr->bg, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_ghost.input_ta, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_ghost.input_ta, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(s_ghost.input_ta, clr->text_secondary, LV_PART_MAIN);
    lv_obj_set_style_radius(s_ghost.input_ta, 3, LV_PART_MAIN);
    lv_obj_set_style_pad_left(s_ghost.input_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_right(s_ghost.input_ta, 4, LV_PART_MAIN);
    lv_obj_set_style_pad_top(s_ghost.input_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_bottom(s_ghost.input_ta, 0, LV_PART_MAIN);
    lv_obj_set_style_text_font(s_ghost.input_ta, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(s_ghost.input_ta, clr->text, LV_PART_MAIN);
    lv_obj_add_event_cb(s_ghost.input_ta, input_ta_cb, LV_EVENT_KEY, NULL);

    /* ">" send indicator label */
    lv_obj_t *send_lbl = lv_label_create(input_bar);
    lv_label_set_text(send_lbl, ">");
    lv_obj_set_style_text_font(send_lbl, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(send_lbl, clr->primary, LV_PART_MAIN);

    /* Initialize UART only if starting in UART mode */
    if (s_ghost.mode == GHOST_MODE_UART) {
        ghostterm_uart_init();
        ghostterm_rx_task_start();
    }

    /* Startup banner */
    ghost_print_line("ThistleOS Shell");
    ghost_print_line("Type 'help' for commands, '/help' for terminal options");
    ghost_print_line("");

    return ESP_OK;
}

void ghostterm_ui_show(void)
{
    if (s_ghost.root) {
        lv_obj_clear_flag(s_ghost.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void ghostterm_ui_hide(void)
{
    if (s_ghost.root) {
        lv_obj_add_flag(s_ghost.root, LV_OBJ_FLAG_HIDDEN);
    }
}

void ghostterm_uart_stop(void)
{
    ghostterm_rx_task_stop();
    if (s_ghost.uart_running) {
        uart_driver_delete(s_ghost.uart_num);
        s_ghost.uart_running = false;
    }
}

void ghostterm_uart_start(void)
{
    if (!s_ghost.uart_running) {
        ghostterm_uart_init();
        ghostterm_rx_task_start();
    }
}
