/*
 * Simulator VFS — creates a /sdcard symlink pointing to simulator/sdcard/
 * Call sim_vfs_init() at startup before any file operations.
 */
#pragma once

/* Initialize simulated SD card filesystem.
 * Creates /tmp/thistle_sdcard symlink → simulator/sdcard/ and
 * overrides the SDCARD mount point. */
void sim_vfs_init(void);

/* Returns the base path for the simulated SD card */
const char *sim_vfs_get_sdcard_path(void);
