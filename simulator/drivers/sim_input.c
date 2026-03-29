#include "sim_input.h"
#include "sim_display.h"
#include <SDL2/SDL.h>
#include <stdio.h>
#include <stdbool.h>
#include <stdlib.h>

static hal_input_cb_t s_cb       = NULL;
static void          *s_cb_data  = NULL;
static bool           s_initialized = false;

static esp_err_t sim_input_init(const void *config)
{
    (void)config;
    s_initialized = true;
    return ESP_OK;
}

static void sim_input_deinit(void)
{
    s_initialized = false;
}

static esp_err_t sim_input_register_cb(hal_input_cb_t cb, void *user_data)
{
    s_cb      = cb;
    s_cb_data = user_data;
    return ESP_OK;
}

static uint16_t sdl_key_to_keycode(SDL_Keycode key)
{
    if (key >= SDLK_a && key <= SDLK_z) return (uint16_t)key;
    if (key >= SDLK_0 && key <= SDLK_9) return (uint16_t)key;
    switch (key) {
        case SDLK_SPACE:     return ' ';
        case SDLK_RETURN:    return '\n';
        case SDLK_BACKSPACE: return '\b';
        case SDLK_ESCAPE:    return 0x1B;
        case SDLK_TAB:       return '\t';
        case SDLK_COMMA:     return ',';
        case SDLK_PERIOD:    return '.';
        case SDLK_SLASH:     return '/';
        case SDLK_MINUS:     return '-';
        case SDLK_EQUALS:    return '=';
        default:             return 0;
    }
}

static esp_err_t sim_input_poll(void)
{
    extern bool sim_is_headless(void);
    if (sim_is_headless()) return ESP_OK;

    if (!s_initialized) return ESP_OK;

    SDL_Event e;
    while (SDL_PollEvent(&e)) {
        if (e.type == SDL_QUIT) {
#ifdef __EMSCRIPTEN__
            /* Don't exit in WASM — there's no window to close */
            continue;
#else
            printf("Window closed — exiting\n");
            exit(0);
#endif
        }

        if ((e.type == SDL_KEYDOWN || e.type == SDL_KEYUP) && s_cb) {
            uint16_t keycode = sdl_key_to_keycode(e.key.keysym.sym);
            if (keycode != 0) {
                hal_input_event_t evt = {
                    .type      = (e.type == SDL_KEYDOWN)
                                     ? HAL_INPUT_EVENT_KEY_DOWN
                                     : HAL_INPUT_EVENT_KEY_UP,
                    .timestamp = SDL_GetTicks(),
                };
                evt.key.keycode = keycode;
                s_cb(&evt, s_cb_data);
            }
        }

        if (e.type == SDL_MOUSEBUTTONDOWN ||
            e.type == SDL_MOUSEBUTTONUP   ||
            e.type == SDL_MOUSEMOTION) {

            /* Only send move if left button is held */
            if (e.type == SDL_MOUSEMOTION &&
                !(SDL_GetMouseState(NULL, NULL) & SDL_BUTTON(1))) {
                continue;
            }

            int x, y;
            SDL_GetMouseState(&x, &y);
            int scale = sim_display_get_scale();
            x /= scale;
            y /= scale;

            hal_input_event_type_t type;
            if (e.type == SDL_MOUSEBUTTONDOWN)     type = HAL_INPUT_EVENT_TOUCH_DOWN;
            else if (e.type == SDL_MOUSEBUTTONUP)  type = HAL_INPUT_EVENT_TOUCH_UP;
            else                                   type = HAL_INPUT_EVENT_TOUCH_MOVE;

            if (s_cb) {
                hal_input_event_t evt = {
                    .type      = type,
                    .timestamp = SDL_GetTicks(),
                };
                evt.touch.x = (uint16_t)x;
                evt.touch.y = (uint16_t)y;
                s_cb(&evt, s_cb_data);
            }
        }
    }

    return ESP_OK;
}

static const hal_input_driver_t sim_input_driver = {
    .init              = sim_input_init,
    .deinit            = sim_input_deinit,
    .register_callback = sim_input_register_cb,
    .poll              = sim_input_poll,
    .name              = "SDL2 Keyboard+Mouse",
    .is_touch          = false,
};

const hal_input_driver_t *sim_input_get(void)
{
    return &sim_input_driver;
}

void sim_input_poll_sdl(void)
{
    sim_input_poll();
}
