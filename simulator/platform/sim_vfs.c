/*
 * Simulator VFS — resolves the simulator/sdcard/ directory and creates
 * a symlink at /tmp/thistle_sdcard so THISTLE_SDCARD macro works.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_vfs.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
#include <libgen.h>

#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

static char s_sdcard_path[512] = {0};
static int s_initialized = 0;

void sim_vfs_init(void)
{
    if (s_initialized) return;

    /* Strategy: find the executable's directory, then navigate to ../sdcard
     * (since the exe is in simulator/build/ and sdcard is at simulator/sdcard/) */

    char exe_path[512] = {0};

#ifdef __APPLE__
    uint32_t exe_size = sizeof(exe_path);
    _NSGetExecutablePath(exe_path, &exe_size);
#elif defined(__linux__)
    ssize_t len = readlink("/proc/self/exe", exe_path, sizeof(exe_path) - 1);
    if (len > 0) exe_path[len] = '\0';
#else
    /* Fallback: use cwd */
    getcwd(exe_path, sizeof(exe_path));
    strncat(exe_path, "/thistle_sim", sizeof(exe_path) - strlen(exe_path) - 1);
#endif

    /* Get directory of executable */
    char *exe_dir = dirname(exe_path);

    /* Try: exe_dir/../sdcard (exe is in simulator/build/, sdcard is simulator/sdcard/) */
    snprintf(s_sdcard_path, sizeof(s_sdcard_path), "%s/../sdcard", exe_dir);

    /* Resolve to absolute path */
    char resolved[512];
    if (realpath(s_sdcard_path, resolved)) {
        strncpy(s_sdcard_path, resolved, sizeof(s_sdcard_path) - 1);
    } else {
        /* realpath failed — try other locations */
        snprintf(s_sdcard_path, sizeof(s_sdcard_path), "%s/../../simulator/sdcard", exe_dir);
        if (realpath(s_sdcard_path, resolved)) {
            strncpy(s_sdcard_path, resolved, sizeof(s_sdcard_path) - 1);
        } else {
            fprintf(stderr, "[sim_vfs] WARNING: Cannot find simulator/sdcard directory\n");
            /* Last resort: create it next to the executable */
            snprintf(s_sdcard_path, sizeof(s_sdcard_path), "%s/../sdcard", exe_dir);
            mkdir(s_sdcard_path, 0755);
            if (realpath(s_sdcard_path, resolved)) {
                strncpy(s_sdcard_path, resolved, sizeof(s_sdcard_path) - 1);
            }
        }
    }

    /* Create symlink at /tmp/thistle_sdcard → our sdcard dir */
    unlink("/tmp/thistle_sdcard");
    if (symlink(s_sdcard_path, "/tmp/thistle_sdcard") == 0) {
        fprintf(stderr, "[sim_vfs] SD card: %s\n", s_sdcard_path);
        fprintf(stderr, "[sim_vfs] Symlink: /tmp/thistle_sdcard -> %s\n", s_sdcard_path);
    } else {
        fprintf(stderr, "[sim_vfs] WARNING: Failed to create symlink\n");
        fprintf(stderr, "[sim_vfs] SD card path: %s\n", s_sdcard_path);
    }

    s_initialized = 1;
}

const char *sim_vfs_get_sdcard_path(void)
{
    if (!s_initialized) sim_vfs_init();
    return s_sdcard_path;
}
