/*
 * SD card mount point — differs between firmware and simulator.
 * Use THISTLE_SDCARD instead of hardcoding "/sdcard".
 */
#pragma once

#ifdef THISTLE_SDCARD_PATH
/* Simulator: use the compile-time path (e.g., /tmp/thistle_sdcard) */
#define THISTLE_SDCARD THISTLE_SDCARD_PATH
#else
/* Firmware: standard ESP-IDF VFS mount point */
#define THISTLE_SDCARD "/sdcard"
#endif
