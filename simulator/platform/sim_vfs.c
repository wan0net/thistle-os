/*
 * Simulator VFS — sets up a path prefix so /sdcard resolves correctly.
 *
 * Strategy: we can't rewrite libc calls portably, so instead we define
 * SDCARD_MOUNT_POINT as a compile-time constant that points to the
 * simulator's local sdcard directory. ThistleOS code that uses
 * "/sdcard" literally still won't work, but the file manager, theme
 * engine, and reader all get the path from a central place.
 *
 * For the most transparent fix: at simulator startup, create a symlink
 *   /tmp/thistle_sdcard → <project>/simulator/sdcard
 * and set SDCARD_MOUNT_POINT to "/tmp/thistle_sdcard".
 *
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "sim_vfs.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>

static char s_sdcard_path[512] = {0};
static int s_initialized = 0;

void sim_vfs_init(void)
{
    if (s_initialized) return;

    /* Find the simulator/sdcard directory */
    char cwd[512];
    if (!getcwd(cwd, sizeof(cwd))) {
        fprintf(stderr, "[sim_vfs] ERROR: getcwd failed\n");
        s_initialized = 1;
        return;
    }

    /* Try various relative paths from cwd */
    const char *candidates[] = {
        "%s/../sdcard",               /* running from simulator/build/ */
        "%s/simulator/sdcard",        /* running from project root */
        "%s/../../simulator/sdcard",  /* running from deep build dir */
    };

    struct stat st;
    for (int i = 0; i < 3; i++) {
        snprintf(s_sdcard_path, sizeof(s_sdcard_path), candidates[i], cwd);
        if (stat(s_sdcard_path, &st) == 0 && S_ISDIR(st.st_mode)) {
            break;
        }
        s_sdcard_path[0] = '\0';
    }

    if (s_sdcard_path[0] == '\0') {
        fprintf(stderr, "[sim_vfs] WARNING: Could not find simulator/sdcard directory\n");
        /* Create a fallback */
        snprintf(s_sdcard_path, sizeof(s_sdcard_path), "%s/../sdcard", cwd);
        mkdir(s_sdcard_path, 0755);
    }

    /* Create /tmp/thistle_sdcard symlink → our sdcard dir.
     * This lets code that hardcodes "/sdcard" paths work if we
     * also create a /sdcard → /tmp/thistle_sdcard link (needs root).
     * More practically, we just print the path for debugging. */
    char real_path[512];
    if (realpath(s_sdcard_path, real_path)) {
        strncpy(s_sdcard_path, real_path, sizeof(s_sdcard_path) - 1);
    }

    /* Create symlink at /tmp/thistle_sdcard for convenience */
    unlink("/tmp/thistle_sdcard");
    symlink(s_sdcard_path, "/tmp/thistle_sdcard");

    fprintf(stderr, "[sim_vfs] SD card path: %s\n", s_sdcard_path);
    fprintf(stderr, "[sim_vfs] Symlink: /tmp/thistle_sdcard -> %s\n", s_sdcard_path);

    s_initialized = 1;
}

const char *sim_vfs_get_sdcard_path(void)
{
    if (!s_initialized) sim_vfs_init();
    return s_sdcard_path;
}
