#pragma once

/* Discover host-architecture TAP app generations installed on the simulator
 * SD card and register their exported thistle_app_t lifecycle. */
int sim_app_loader_scan_and_register(void);
