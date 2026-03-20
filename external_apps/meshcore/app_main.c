/*
 * MeshCore ThistleOS App Entry Point (Phase 5 stub)
 *
 * This wraps MeshCore with the ThistleOS shim layer and provides
 * the standard app lifecycle callbacks.
 */
#include "thistle_app.h"

static int meshcore_create(void)
{
    thistle_log("meshcore", "MeshCore app created (Phase 5 stub)");
    /* TODO: Initialize MeshCore shim, call meshcore_shim_init() */
    return 0;
}

static void meshcore_start(void)
{
    thistle_log("meshcore", "MeshCore starting (Phase 5 stub)");
    /* TODO: Call MeshCore setup(), start loop() task */
}

static void meshcore_pause(void)
{
    /* TODO: Reduce radio polling frequency */
}

static void meshcore_resume(void)
{
    /* TODO: Restore normal operation */
}

static void meshcore_destroy(void)
{
    /* TODO: Cleanup MeshCore resources */
}

static const thistle_app_t meshcore_app = {
    .id               = "com.meshcore.chat",
    .name             = "MeshCore",
    .version          = "1.0.0",
    .allow_background = true,
    .on_create        = meshcore_create,
    .on_start         = meshcore_start,
    .on_pause         = meshcore_pause,
    .on_resume        = meshcore_resume,
    .on_destroy       = meshcore_destroy,
};

THISTLE_APP(meshcore_app);
