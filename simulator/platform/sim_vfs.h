/*
 * Simulator VFS — rewrites /sdcard paths to simulator/sdcard/ on the host.
 * Include this AFTER standard headers but BEFORE any ThistleOS code.
 */
#pragma once

#ifdef SIMULATOR_BUILD

#include <stdio.h>
#include <dirent.h>
#include <sys/stat.h>

/* These functions rewrite "/sdcard/..." paths to the host-local sdcard dir */
FILE *sim_fopen(const char *path, const char *mode);
DIR  *sim_opendir(const char *path);
int   sim_stat(const char *path, struct stat *buf);

/* Override standard POSIX calls */
#define fopen   sim_fopen
#define opendir sim_opendir

/* stat is trickier — macOS has stat as a macro already; override carefully */
#undef stat
#define stat(path, buf) sim_stat(path, buf)

/* Get the rewritten path (for use in other calls like readdir, etc.) */
const char *sim_vfs_rewrite_path(const char *path);

#endif /* SIMULATOR_BUILD */
