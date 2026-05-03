// SPDX-License-Identifier: BSD-3-Clause
// Terminal — ThistleOS terminal emulator powered by libghostty-vt
//
// Uses libghostty-vt for VT100/xterm escape sequence parsing and terminal
// state management. Renders via the ThistleOS widget API (any WM).
// Keyboard input is encoded via ghostty's key encoder.
//
// On ESP32: the "subprocess" is ThistleOS's built-in command shell.
// In WASM: keyboard input is echoed with basic line editing.

#include "thistle_app.h"
#include <stdio.h>
#include <string.h>

#define TAG "terminal"

#ifdef THISTLE_TERMINAL_NO_GHOSTTY

static thistle_widget_t s_root;
static thistle_widget_t s_output;
static thistle_widget_t s_input_line;
static char s_output_buf[1024];
static char s_cmd_buf[256];
static int s_cmd_len;

static void append_output(const char *text)
{
    size_t used = strlen(s_output_buf);
    size_t incoming = strlen(text);

    if (incoming >= sizeof(s_output_buf)) {
        text += incoming - sizeof(s_output_buf) + 1;
        incoming = strlen(text);
    }

    if (used + incoming >= sizeof(s_output_buf)) {
        size_t drop = used + incoming - sizeof(s_output_buf) + 1;
        memmove(s_output_buf, s_output_buf + drop, used - drop + 1);
        used = strlen(s_output_buf);
    }

    memcpy(s_output_buf + used, text, incoming + 1);
    thistle_ui_set_text(s_output, s_output_buf);
}

static void shell_execute(const char *cmd)
{
    char line[256];

    if (strcmp(cmd, "help") == 0) {
        append_output("Commands: help, uname, uptime, free, clear\r\n");
    } else if (strcmp(cmd, "uname") == 0) {
        append_output("ThistleOS v0.1.0 (Rust kernel) ESP32-S3\r\n");
    } else if (strcmp(cmd, "uptime") == 0) {
        uint32_t ms = thistle_millis();
        snprintf(line, sizeof(line), "up %lu.%03lus\r\n",
                 (unsigned long)(ms / 1000), (unsigned long)(ms % 1000));
        append_output(line);
    } else if (strcmp(cmd, "free") == 0) {
        append_output("PSRAM: 8192 KB total\r\n");
    } else if (strcmp(cmd, "clear") == 0) {
        s_output_buf[0] = '\0';
        thistle_ui_set_text(s_output, "");
    } else if (cmd[0] != '\0') {
        snprintf(line, sizeof(line), "terminal: %s: command not found\r\n", cmd);
        append_output(line);
    }

    append_output("thistle$ ");
}

static void on_key_event(thistle_widget_t widget, int event, void *ud)
{
    (void)widget; (void)event; (void)ud;

    const char *text = thistle_ui_get_text(s_input_line);
    if (!text || text[0] == '\0') return;

    size_t len = strlen(text);
    char last_char = text[len - 1];

    if (last_char == '\n' || last_char == '\r') {
        append_output("\r\n");
        shell_execute(s_cmd_buf);
        s_cmd_len = 0;
        s_cmd_buf[0] = '\0';
        thistle_ui_set_text(s_input_line, "");
    } else if (s_cmd_len < (int)sizeof(s_cmd_buf) - 1) {
        char echo[2] = { last_char, '\0' };
        s_cmd_buf[s_cmd_len++] = last_char;
        s_cmd_buf[s_cmd_len] = '\0';
        append_output(echo);
    }
}

static int terminal_on_create(void)
{
    thistle_log(TAG, "Creating Terminal fallback");

    s_root = thistle_ui_get_app_root();
    s_output = thistle_ui_create_label(s_root, "");
    thistle_ui_set_size(s_output, -1, -1);
    thistle_ui_set_bg_color(s_output, 0x1A1A19);
    thistle_ui_set_text_color(s_output, 0x86EFAC);
    thistle_ui_set_font_size(s_output, 14);

    s_input_line = thistle_ui_create_text_input(s_root, "");
    thistle_ui_set_size(s_input_line, -1, 24);
    thistle_ui_set_bg_color(s_input_line, 0x232322);
    thistle_ui_set_text_color(s_input_line, 0x86EFAC);
    thistle_ui_set_font_size(s_input_line, 14);
    thistle_ui_on_event(s_input_line, THISTLE_EVENT_VALUE_CHANGED, on_key_event, 0);

    append_output("ThistleOS Terminal v0.1.0\r\nlibghostty-vt unavailable\r\n\r\n");
    shell_execute("");
    return 0;
}

static void terminal_on_start(void) { thistle_log(TAG, "on_start"); }
static void terminal_on_pause(void) { thistle_log(TAG, "on_pause"); }
static void terminal_on_resume(void) { thistle_log(TAG, "on_resume"); }
static void terminal_on_destroy(void) { thistle_log(TAG, "on_destroy"); }

#else

#include <ghostty/vt.h>

// Terminal dimensions (characters)
#define TERM_COLS  40
#define TERM_ROWS  12
#define CELL_W     8    // pixels per cell (monospace)
#define CELL_H     16   // pixels per cell

// State
static GhosttyTerminal s_terminal;
static GhosttyRenderState s_render_state;
static GhosttyKeyEncoder s_key_encoder;
static GhosttyKeyEvent s_key_event;
static thistle_widget_t s_root;
static thistle_widget_t s_grid_container;
static thistle_widget_t s_cell_labels[TERM_ROWS][TERM_COLS];
static thistle_widget_t s_input_line;
static _Bool s_initialized;

// Simple command buffer (no real PTY — built-in shell)
static char s_cmd_buf[256];
static int s_cmd_len;

// ── Shell commands (built-in, no subprocess) ────────────────────────

static void shell_execute(const char *cmd)
{
    char output[256];

    if (strcmp(cmd, "help") == 0) {
        const char *help = "Commands: help, uname, uptime, free, ls, clear\r\n";
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)help, strlen(help));
    } else if (strcmp(cmd, "uname") == 0) {
        snprintf(output, sizeof(output), "ThistleOS v0.1.0 (Rust kernel) ESP32-S3\r\n");
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)output, strlen(output));
    } else if (strcmp(cmd, "uptime") == 0) {
        uint32_t ms = thistle_millis();
        snprintf(output, sizeof(output), "up %lu.%03lus\r\n",
                 (unsigned long)(ms / 1000), (unsigned long)(ms % 1000));
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)output, strlen(output));
    } else if (strcmp(cmd, "free") == 0) {
        snprintf(output, sizeof(output), "PSRAM: 8192 KB total\r\n");
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)output, strlen(output));
    } else if (strcmp(cmd, "clear") == 0) {
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)"\033[2J\033[H", 7);
    } else if (cmd[0] != '\0') {
        snprintf(output, sizeof(output), "terminal: %s: command not found\r\n", cmd);
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)output, strlen(output));
    }

    // Print prompt
    const char *prompt = "thistle$ ";
    ghostty_terminal_vt_write(s_terminal, (const uint8_t *)prompt, strlen(prompt));
}

// ── Rendering ───────────────────────────────────────────────────────

static void render_terminal(void)
{
    if (!s_initialized) return;

    // Update render state from terminal
    if (ghostty_render_state_update(s_render_state, s_terminal) != GHOSTTY_SUCCESS) return;

    // Iterate grid and update cell labels
    GhosttyRenderStateRowIterator row_iter = 0;
    GhosttyRenderStateRowCells cells = 0;
    if (ghostty_render_state_row_iterator_new(0, &row_iter) != GHOSTTY_SUCCESS) return;
    if (ghostty_render_state_row_cells_new(0, &cells) != GHOSTTY_SUCCESS) {
        ghostty_render_state_row_iterator_free(row_iter);
        return;
    }

    ghostty_render_state_get(s_render_state,
        GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR, &row_iter);

    int row = 0;
    while (row_iter && ghostty_render_state_row_iterator_next(row_iter) && row < TERM_ROWS) {
        ghostty_render_state_row_get(row_iter,
            GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &cells);

        int col = 0;
        while (cells && ghostty_render_state_row_cells_next(cells) && col < TERM_COLS) {
            uint32_t grapheme_len = 0;
            ghostty_render_state_row_cells_get(cells,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN, &grapheme_len);

            if (grapheme_len > 0 && s_cell_labels[row][col]) {
                uint32_t graphemes[8] = {0};
                uint32_t codepoint = 0;
                ghostty_render_state_row_cells_get(cells,
                    GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF, graphemes);
                codepoint = graphemes[0];

                char ch[5] = {0};
                if (codepoint < 128) {
                    ch[0] = (char)codepoint;
                }
                thistle_ui_set_text(s_cell_labels[row][col], ch);
            } else if (s_cell_labels[row][col]) {
                thistle_ui_set_text(s_cell_labels[row][col], " ");
            }
            col++;
        }

        // Clear remaining cells in row
        for (; col < TERM_COLS; col++) {
            if (s_cell_labels[row][col]) {
                thistle_ui_set_text(s_cell_labels[row][col], " ");
            }
        }
        row++;
    }

    ghostty_render_state_row_cells_free(cells);
    ghostty_render_state_row_iterator_free(row_iter);
}

// ── Input handling ──────────────────────────────────────────────────

static void on_key_event(thistle_widget_t widget, int event, void *ud)
{
    (void)widget; (void)event; (void)ud;

    // Get the text from the input line
    const char *text = thistle_ui_get_text(s_input_line);
    if (!text || text[0] == '\0') return;

    // Echo the character to the terminal
    size_t len = strlen(text);
    char last_char = text[len - 1];

    if (last_char == '\n' || last_char == '\r') {
        // Enter pressed — execute command
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)"\r\n", 2);
        shell_execute(s_cmd_buf);
        s_cmd_len = 0;
        s_cmd_buf[0] = '\0';
        thistle_ui_set_text(s_input_line, "");
    } else {
        // Regular character — echo and buffer
        if (s_cmd_len < (int)sizeof(s_cmd_buf) - 1) {
            s_cmd_buf[s_cmd_len++] = last_char;
            s_cmd_buf[s_cmd_len] = '\0';
        }
        char echo[2] = { last_char, '\0' };
        ghostty_terminal_vt_write(s_terminal, (const uint8_t *)echo, 1);
    }

    render_terminal();
}

// ── App lifecycle ───────────────────────────────────────────────────

static int terminal_on_create(void)
{
    thistle_log(TAG, "Creating Terminal");

    // Initialize libghostty terminal
    GhosttyTerminalOptions opts;
    memset(&opts, 0, sizeof(opts));
    opts.cols = TERM_COLS;
    opts.rows = TERM_ROWS;
    opts.max_scrollback = 500;

    GhosttyResult err = ghostty_terminal_new(0, &s_terminal, opts);
    if (err != GHOSTTY_SUCCESS) {
        thistle_log(TAG, "ghostty_terminal_new failed");
        return -1;
    }

    // Create render state
    err = ghostty_render_state_new(0, &s_render_state);
    if (err != GHOSTTY_SUCCESS) {
        thistle_log(TAG, "ghostty_render_state_new failed");
        ghostty_terminal_free(s_terminal);
        return -1;
    }

    // Build UI
    s_root = thistle_ui_get_app_root();

    // Terminal grid container
    s_grid_container = thistle_ui_create_container(s_root);
    thistle_ui_set_size(s_grid_container, -1, -1);
    thistle_ui_set_layout(s_grid_container, THISTLE_LAYOUT_FLEX_COLUMN);
    thistle_ui_set_bg_color(s_grid_container, 0x1A1A19); // Dark terminal bg
    thistle_ui_set_padding(s_grid_container, 4, 4, 4, 4);
    thistle_ui_set_gap(s_grid_container, 0);

    // Create cell label grid
    for (int r = 0; r < TERM_ROWS; r++) {
        thistle_widget_t row_container = thistle_ui_create_container(s_grid_container);
        thistle_ui_set_size(row_container, -1, CELL_H);
        thistle_ui_set_layout(row_container, THISTLE_LAYOUT_FLEX_ROW);
        thistle_ui_set_gap(row_container, 0);

        for (int c = 0; c < TERM_COLS; c++) {
            s_cell_labels[r][c] = thistle_ui_create_label(row_container, " ");
            thistle_ui_set_size(s_cell_labels[r][c], CELL_W, CELL_H);
            thistle_ui_set_text_color(s_cell_labels[r][c], 0x86EFAC); // Green terminal text
            thistle_ui_set_font_size(s_cell_labels[r][c], 14);
        }
    }

    // Input line at bottom
    s_input_line = thistle_ui_create_text_input(s_root, "");
    thistle_ui_set_size(s_input_line, -1, 24);
    thistle_ui_set_bg_color(s_input_line, 0x232322);
    thistle_ui_set_text_color(s_input_line, 0x86EFAC);
    thistle_ui_set_font_size(s_input_line, 14);
    thistle_ui_on_event(s_input_line, THISTLE_EVENT_VALUE_CHANGED, on_key_event, 0);

    // Write initial prompt
    const char *banner = "ThistleOS Terminal v0.1.0\r\npowered by libghostty-vt\r\n\r\n";
    ghostty_terminal_vt_write(s_terminal, (const uint8_t *)banner, strlen(banner));
    shell_execute("");

    s_initialized = 1;
    render_terminal();

    char ready_msg[64];
    snprintf(ready_msg, sizeof(ready_msg), "Terminal ready (%dx%d)", TERM_COLS, TERM_ROWS);
    thistle_log(TAG, ready_msg);
    return 0;
}

static void terminal_on_start(void)
{
    thistle_log(TAG, "on_start");
}

static void terminal_on_pause(void)
{
    thistle_log(TAG, "on_pause");
}

static void terminal_on_resume(void)
{
    thistle_log(TAG, "on_resume");
    render_terminal();
}

static void terminal_on_destroy(void)
{
    thistle_log(TAG, "on_destroy");
    if (s_initialized) {
        ghostty_render_state_free(s_render_state);
        ghostty_terminal_free(s_terminal);
        s_initialized = 0;
    }
}

#endif

static const thistle_app_t terminal_app = {
    .id               = "com.thistle.terminal",
    .name             = "Terminal",
    .version          = "0.1.0",
    .allow_background = false,
    .on_create        = terminal_on_create,
    .on_start         = terminal_on_start,
    .on_pause         = terminal_on_pause,
    .on_resume        = terminal_on_resume,
    .on_destroy       = terminal_on_destroy,
};

THISTLE_APP(terminal_app);
