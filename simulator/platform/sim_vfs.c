/*
 * Simulator VFS — rewrites /sdcard paths to simulator/sdcard/ on the host.
 * SPDX-License-Identifier: BSD-3-Clause
 */

/* Must undef our macros before including system headers */
#undef fopen
#undef opendir
#undef stat

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>
#include <unistd.h>
#include <libgen.h>

/* Path to the simulator sdcard directory — set at init or computed */
static char s_sdcard_base[512] = {0};
static int  s_initialized = 0;

static void sim_vfs_init(void)
{
    if (s_initialized) return;

    /* Try environment variable first */
    const char *env = getenv("THISTLE_SIM_SDCARD");
    if (env && env[0]) {
        strncpy(s_sdcard_base, env, sizeof(s_sdcard_base) - 1);
        s_initialized = 1;
        return;
    }

    /* Default: find simulator/sdcard relative to the executable's parent.
     * The executable is in simulator/build/, so go up two levels to project root,
     * then into simulator/sdcard/. */
    char cwd[512];
    if (getcwd(cwd, sizeof(cwd))) {
        /* Try: cwd/../sdcard (if running from simulator/build/) */
        snprintf(s_sdcard_base, sizeof(s_sdcard_base), "%s/../sdcard", cwd);

        /* Check if it exists */
        struct stat st;
        if (stat(s_sdcard_base, &st) != 0 || !S_ISDIR(st.st_mode)) {
            /* Try: cwd/../../simulator/sdcard (if running from project root) */
            snprintf(s_sdcard_base, sizeof(s_sdcard_base), "%s/../../simulator/sdcard", cwd);
            if (stat(s_sdcard_base, &st) != 0 || !S_ISDIR(st.st_mode)) {
                /* Fallback: just use simulator/sdcard relative to cwd */
                snprintf(s_sdcard_base, sizeof(s_sdcard_base), "%s/simulator/sdcard", cwd);
            }
        }
    }

    s_initialized = 1;
    fprintf(stderr, "[sim_vfs] SD card root: %s\n", s_sdcard_base);
}

const char *sim_vfs_rewrite_path(const char *path)
{
    static char rewritten[1024];

    if (!path) return path;
    sim_vfs_init();

    /* Only rewrite paths starting with /sdcard */
    if (strncmp(path, "/sdcard", 7) == 0) {
        const char *suffix = path + 7; /* everything after "/sdcard" */
        if (*suffix == '\0' || *suffix == '/') {
            snprintf(rewritten, sizeof(rewritten), "%s%s", s_sdcard_base, suffix);
            return rewritten;
        }
    }

    return path;
}

FILE *sim_fopen(const char *path, const char *mode)
{
    const char *real_path = sim_vfs_rewrite_path(path);
    return fopen(real_path, mode);
}

DIR *sim_opendir(const char *path)
{
    const char *real_path = sim_vfs_rewrite_path(path);
    return opendir(real_path);
}

int sim_stat(const char *path, struct stat *buf)
{
    const char *real_path = sim_vfs_rewrite_path(path);
    return stat(real_path, buf);
}
