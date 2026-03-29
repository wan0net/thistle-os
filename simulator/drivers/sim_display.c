#include "sim_display.h"
#include <SDL2/SDL.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <stdbool.h>

/* Runtime-configurable resolution — set via sim_display_set_resolution() before init */
static int s_width  = 320;
static int s_height = 240;
static char s_title[128] = "ThistleOS Simulator";

static SDL_Window   *s_window   = NULL;
static SDL_Renderer *s_renderer = NULL;
static SDL_Texture  *s_texture  = NULL;
static uint16_t     *s_fb       = NULL;  /* RGB565 framebuffer (LVGL output) */
static uint32_t     *s_fb32     = NULL;  /* RGBA8888 for SDL texture (Emscripten) */
static bool          s_initialized = false;
static int           s_scale       = 2;

/* Scale factor: small displays get a bigger scale so the window is usable */
static int calc_scale(int w, int h)
{
    if (w <= 128) return 4;
    if (w <= 240 && h <= 135) return 3;
    return 2;
}

void sim_display_set_resolution(int width, int height)
{
    s_width  = width;
    s_height = height;
}

void sim_display_set_title(const char *device_name)
{
    if (device_name) {
        snprintf(s_title, sizeof(s_title), "ThistleOS Simulator — %s", device_name);
    }
}

static esp_err_t sim_display_init(const void *config)
{
    (void)config;

    if (s_initialized) return ESP_OK;

    int scale = calc_scale(s_width, s_height);
    s_scale = scale;

    /* Allocate framebuffers dynamically based on runtime resolution */
    s_fb   = calloc((size_t)(s_width * s_height), sizeof(uint16_t));
    s_fb32 = calloc((size_t)(s_width * s_height), sizeof(uint32_t));
    if (!s_fb || !s_fb32) {
        printf("sim_display: framebuffer alloc failed\n");
        free(s_fb);
        free(s_fb32);
        s_fb = s_fb32 = NULL;
        return ESP_ERR_NO_MEM;
    }

    extern bool sim_is_headless(void);
    if (sim_is_headless()) {
        /* Headless: framebuffer only, no SDL window */
        s_initialized = true;
        printf("Simulator display initialized (%dx%d, headless, RGB565)\n", s_width, s_height);
        return ESP_OK;
    }

    if (SDL_Init(SDL_INIT_VIDEO) < 0) {
        printf("SDL_Init failed: %s\n", SDL_GetError());
        return ESP_FAIL;
    }

    s_window = SDL_CreateWindow(
        s_title,
        SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED,
        s_width * scale, s_height * scale,
        SDL_WINDOW_SHOWN
    );
    if (!s_window) {
        printf("SDL_CreateWindow failed: %s\n", SDL_GetError());
        return ESP_FAIL;
    }

    /* Emscripten SDL2 works best with software renderer */
#ifdef __EMSCRIPTEN__
    s_renderer = SDL_CreateRenderer(s_window, -1, SDL_RENDERER_SOFTWARE);
#else
    s_renderer = SDL_CreateRenderer(s_window, -1, SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC);
    if (!s_renderer) {
        s_renderer = SDL_CreateRenderer(s_window, -1, SDL_RENDERER_SOFTWARE);
    }
#endif
    if (!s_renderer) {
        printf("SDL_CreateRenderer failed: %s\n", SDL_GetError());
        SDL_DestroyWindow(s_window);
        s_window = NULL;
        return ESP_FAIL;
    }

    /* Emscripten SDL2 doesn't support RGB565 textures — use RGBA8888 and convert */
#ifdef __EMSCRIPTEN__
    s_texture = SDL_CreateTexture(
        s_renderer,
        SDL_PIXELFORMAT_ABGR8888,
        SDL_TEXTUREACCESS_STREAMING,
        s_width, s_height
    );
#else
    s_texture = SDL_CreateTexture(
        s_renderer,
        SDL_PIXELFORMAT_RGB565,
        SDL_TEXTUREACCESS_STREAMING,
        s_width, s_height
    );
#endif
    if (!s_texture) {
        printf("SDL_CreateTexture failed: %s\n", SDL_GetError());
        SDL_DestroyRenderer(s_renderer);
        SDL_DestroyWindow(s_window);
        s_renderer = NULL;
        s_window   = NULL;
        return ESP_FAIL;
    }

    /* White background */
    memset(s_fb, 0xFF, (size_t)(s_width * s_height) * sizeof(uint16_t));

    /* Initial render — white screen */
#ifdef __EMSCRIPTEN__
    memset(s_fb32, 0xFF, (size_t)(s_width * s_height) * sizeof(uint32_t));
    SDL_UpdateTexture(s_texture, NULL, s_fb32, s_width * (int)sizeof(uint32_t));
#else
    SDL_UpdateTexture(s_texture, NULL, s_fb, s_width * (int)sizeof(uint16_t));
#endif
    SDL_RenderClear(s_renderer);
    SDL_RenderCopy(s_renderer, s_texture, NULL, NULL);
    SDL_RenderPresent(s_renderer);

    s_initialized = true;
    printf("Simulator display initialized (%dx%d, %dx scale, RGB565)\n",
           s_width, s_height, scale);
    return ESP_OK;
}

static void sim_display_deinit(void)
{
    extern bool sim_is_headless(void);
    if (!sim_is_headless()) {
        if (s_texture)  SDL_DestroyTexture(s_texture);
        if (s_renderer) SDL_DestroyRenderer(s_renderer);
        if (s_window)   SDL_DestroyWindow(s_window);
        SDL_Quit();
    }
    s_texture     = NULL;
    s_renderer    = NULL;
    s_window      = NULL;
    free(s_fb);
    free(s_fb32);
    s_fb          = NULL;
    s_fb32        = NULL;
    s_initialized = false;
}

static esp_err_t sim_display_flush(const hal_area_t *area, const uint8_t *data)
{
    if (!s_initialized || !area || !data) return ESP_FAIL;

    /* LVGL sends RGB565 pixels (2 bytes per pixel) */
    const uint16_t *pixels = (const uint16_t *)data;
    uint16_t w = area->x2 - area->x1 + 1;

    for (uint16_t y = area->y1; y <= area->y2 && y < (uint16_t)s_height; y++) {
        for (uint16_t x = area->x1; x <= area->x2 && x < (uint16_t)s_width; x++) {
            size_t src_idx = (size_t)(y - area->y1) * w + (x - area->x1);
            s_fb[(size_t)y * (size_t)s_width + x] = pixels[src_idx];
        }
    }

    extern bool sim_is_headless(void);
    if (sim_is_headless()) {
        return ESP_OK;  /* Framebuffer updated, skip SDL render */
    }

    /* Update SDL texture and render */
#ifdef __EMSCRIPTEN__
    /* Convert RGB565 → RGBA8888 for Emscripten */
    int npix = s_width * s_height;
    for (int i = 0; i < npix; i++) {
        uint16_t c = s_fb[i];
        uint8_t r = ((c >> 11) & 0x1F) << 3;
        uint8_t g = ((c >> 5) & 0x3F) << 2;
        uint8_t b = (c & 0x1F) << 3;
        s_fb32[i] = (0xFF << 24) | (b << 16) | (g << 8) | r; /* ABGR8888 */
    }
    SDL_UpdateTexture(s_texture, NULL, s_fb32, s_width * (int)sizeof(uint32_t));
#else
    SDL_UpdateTexture(s_texture, NULL, s_fb, s_width * (int)sizeof(uint16_t));
#endif
    SDL_RenderClear(s_renderer);
    SDL_RenderCopy(s_renderer, s_texture, NULL, NULL);
    SDL_RenderPresent(s_renderer);

    return ESP_OK;
}

static uint16_t sim_display_get_width(void)  { return (uint16_t)s_width;  }
static uint16_t sim_display_get_height(void) { return (uint16_t)s_height; }

static esp_err_t sim_display_brightness(uint8_t pct) { (void)pct; return ESP_OK; }
static esp_err_t sim_display_sleep(bool enter) { (void)enter; return ESP_OK; }
static esp_err_t sim_display_refresh_mode(hal_display_refresh_mode_t mode) { (void)mode; return ESP_OK; }

int sim_display_get_scale(void)
{
    return s_scale;
}

/* Mutable driver struct so width/height can be patched at init time */
static hal_display_driver_t sim_display_driver = {
    .init             = sim_display_init,
    .deinit           = sim_display_deinit,
    .flush            = sim_display_flush,
    .refresh          = NULL,   /* LCD sim: no deferred refresh needed */
    .set_brightness   = sim_display_brightness,
    .sleep            = sim_display_sleep,
    .set_refresh_mode = sim_display_refresh_mode,
    .width            = 320,
    .height           = 240,
    .type             = HAL_DISPLAY_TYPE_LCD,
    .name             = "SDL2 Simulator",
};

const hal_display_driver_t *sim_display_get(void)
{
    /* Patch width/height from runtime config before returning */
    sim_display_driver.width  = (uint16_t)s_width;
    sim_display_driver.height = (uint16_t)s_height;
    return &sim_display_driver;
}
