#pragma once

#define LV_CONF_H

/* Display */
#define LV_HOR_RES_MAX 320
#define LV_VER_RES_MAX 240
#define LV_COLOR_DEPTH 16

/* Fonts */
#define LV_FONT_MONTSERRAT_14 1
#define LV_FONT_MONTSERRAT_18 1
#define LV_FONT_MONTSERRAT_22 1
#define LV_FONT_DEFAULT &lv_font_montserrat_14

/* Features */
#define LV_USE_LOG 1
#define LV_LOG_LEVEL LV_LOG_LEVEL_WARN
#define LV_USE_ASSERT_NULL 1
#define LV_USE_ASSERT_MALLOC 1

/* SDL display driver */
#define LV_USE_SDL 1
#define LV_SDL_WINDOW_TITLE "ThistleOS Simulator"
#define LV_SDL_INCLUDE_PATH <SDL2/SDL.h>
#define LV_SDL_RENDER_MODE LV_DISPLAY_RENDER_MODE_DIRECT
#define LV_SDL_BUF_COUNT 1

/* Stdlib */
#define LV_USE_STDLIB_MALLOC LV_STDLIB_BUILTIN
#define LV_USE_STDLIB_STRING LV_STDLIB_BUILTIN
#define LV_USE_STDLIB_SPRINTF LV_STDLIB_BUILTIN

/* OS */
#define LV_USE_OS LV_OS_NONE

/* Layout */
#define LV_USE_FLEX 1
#define LV_USE_GRID 1

/* Widgets — use all defaults (all enabled) */

/* Theme */
#define LV_USE_THEME_DEFAULT 1
#define LV_USE_THEME_SIMPLE 1
