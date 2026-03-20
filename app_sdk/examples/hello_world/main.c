/*
 * Hello World -- ThistleOS Example App
 */
#include "thistle_app.h"

static int hello_create(void)
{
    thistle_log("hello", "Hello World app created!");
    return 0;
}

static void hello_start(void)
{
    thistle_log("hello", "Hello from ThistleOS!");
    /* TODO: Create LVGL UI */
}

static void hello_pause(void)   {}
static void hello_resume(void)  {}
static void hello_destroy(void) {}

static const thistle_app_t hello_app = {
    .id               = "com.example.hello",
    .name             = "Hello World",
    .version          = "1.0.0",
    .allow_background = false,
    .on_create        = hello_create,
    .on_start         = hello_start,
    .on_pause         = hello_pause,
    .on_resume        = hello_resume,
    .on_destroy       = hello_destroy,
};

THISTLE_APP(hello_app);
