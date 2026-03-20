#include "sim_display.h"
#include <SDL2/SDL.h>
#include <stdio.h>
#include <string.h>
#include <stdbool.h>

#define SIM_WIDTH  320
#define SIM_HEIGHT 240
#define SIM_SCALE  2

static SDL_Window   *s_window   = NULL;
static SDL_Renderer *s_renderer = NULL;
static SDL_Texture  *s_texture  = NULL;
static uint16_t      s_fb[SIM_WIDTH * SIM_HEIGHT];  /* RGB565 framebuffer */
static bool          s_initialized = false;

static esp_err_t sim_display_init(const void *config)
{
    (void)config;

    if (s_initialized) return ESP_OK;

    if (SDL_Init(SDL_INIT_VIDEO) < 0) {
        printf("SDL_Init failed: %s\n", SDL_GetError());
        return ESP_FAIL;
    }

    s_window = SDL_CreateWindow(
        "ThistleOS Simulator",
        SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED,
        SIM_WIDTH * SIM_SCALE, SIM_HEIGHT * SIM_SCALE,
        SDL_WINDOW_SHOWN
    );
    if (!s_window) {
        printf("SDL_CreateWindow failed: %s\n", SDL_GetError());
        return ESP_FAIL;
    }

    s_renderer = SDL_CreateRenderer(s_window, -1, SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC);
    if (!s_renderer) {
        s_renderer = SDL_CreateRenderer(s_window, -1, SDL_RENDERER_SOFTWARE);
    }
    if (!s_renderer) {
        printf("SDL_CreateRenderer failed: %s\n", SDL_GetError());
        SDL_DestroyWindow(s_window);
        s_window = NULL;
        return ESP_FAIL;
    }

    /* RGB565 texture matching LVGL's 16-bit color output */
    s_texture = SDL_CreateTexture(
        s_renderer,
        SDL_PIXELFORMAT_RGB565,
        SDL_TEXTUREACCESS_STREAMING,
        SIM_WIDTH, SIM_HEIGHT
    );
    if (!s_texture) {
        printf("SDL_CreateTexture failed: %s\n", SDL_GetError());
        SDL_DestroyRenderer(s_renderer);
        SDL_DestroyWindow(s_window);
        s_renderer = NULL;
        s_window   = NULL;
        return ESP_FAIL;
    }

    /* White background */
    memset(s_fb, 0xFF, sizeof(s_fb));

    /* Initial render — white screen */
    SDL_UpdateTexture(s_texture, NULL, s_fb, SIM_WIDTH * sizeof(uint16_t));
    SDL_RenderClear(s_renderer);
    SDL_RenderCopy(s_renderer, s_texture, NULL, NULL);
    SDL_RenderPresent(s_renderer);

    s_initialized = true;
    printf("Simulator display initialized (%dx%d, %dx scale, RGB565)\n",
           SIM_WIDTH, SIM_HEIGHT, SIM_SCALE);
    return ESP_OK;
}

static void sim_display_deinit(void)
{
    if (s_texture)  SDL_DestroyTexture(s_texture);
    if (s_renderer) SDL_DestroyRenderer(s_renderer);
    if (s_window)   SDL_DestroyWindow(s_window);
    SDL_Quit();
    s_texture     = NULL;
    s_renderer    = NULL;
    s_window      = NULL;
    s_initialized = false;
}

static esp_err_t sim_display_flush(const hal_area_t *area, const uint8_t *data)
{
    if (!s_initialized || !area || !data) return ESP_FAIL;

    /* LVGL sends RGB565 pixels (2 bytes per pixel) */
    const uint16_t *pixels = (const uint16_t *)data;
    uint16_t w = area->x2 - area->x1 + 1;

    for (uint16_t y = area->y1; y <= area->y2 && y < SIM_HEIGHT; y++) {
        for (uint16_t x = area->x1; x <= area->x2 && x < SIM_WIDTH; x++) {
            size_t src_idx = (size_t)(y - area->y1) * w + (x - area->x1);
            s_fb[y * SIM_WIDTH + x] = pixels[src_idx];
        }
    }

    /* Update SDL texture and render */
    SDL_UpdateTexture(s_texture, NULL, s_fb, SIM_WIDTH * sizeof(uint16_t));
    SDL_RenderClear(s_renderer);
    SDL_RenderCopy(s_renderer, s_texture, NULL, NULL);
    SDL_RenderPresent(s_renderer);

    return ESP_OK;
}

static esp_err_t sim_display_brightness(uint8_t pct) { (void)pct; return ESP_OK; }
static esp_err_t sim_display_sleep(bool enter) { (void)enter; return ESP_OK; }
static esp_err_t sim_display_refresh_mode(hal_display_refresh_mode_t mode) { (void)mode; return ESP_OK; }

static const hal_display_driver_t sim_display_driver = {
    .init             = sim_display_init,
    .deinit           = sim_display_deinit,
    .flush            = sim_display_flush,
    .set_brightness   = sim_display_brightness,
    .sleep            = sim_display_sleep,
    .set_refresh_mode = sim_display_refresh_mode,
    .width            = SIM_WIDTH,
    .height           = SIM_HEIGHT,
    .type             = HAL_DISPLAY_TYPE_LCD,
    .name             = "SDL2 Simulator",
};

const hal_display_driver_t *sim_display_get(void)
{
    return &sim_display_driver;
}
