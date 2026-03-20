# ThistleOS Development Guide

## Project Overview
ThistleOS is an ESP32-S3 operating system for the LilyGo T-Deck Pro. It provides a kernel with loadable drivers and apps, LVGL-based UI, and a shim layer for existing firmware.

## Build System
- ESP-IDF v5.3+ project with CMake
- Target: ESP32-S3 (LilyGo T-Deck Pro)
- Build: `idf.py build`
- Flash: `idf.py -p /dev/ttyACM0 flash monitor`

## Architecture
- **HAL layer** (`components/thistle_hal/`): Pure interfaces (vtable structs). No implementations.
- **Drivers** (`components/drv_*/`): Each driver is its own ESP-IDF component implementing a HAL interface.
- **Board definitions** (`components/board_*/`): Wire drivers to HAL. Pin configs, I2C addresses, SPI buses.
- **Kernel** (`components/kernel/`): App manager, driver manager, IPC, event bus, syscall table.
- **UI** (`components/ui/`): LVGL 9 window manager, theme engine, status bar.

## Key Conventions
- License: BSD 3-Clause for all ThistleOS code. No GPL dependencies.
- Drivers are fully modular — swap by changing the board definition, not the kernel.
- HAL interfaces use C structs of function pointers (vtables).
- Apps communicate with kernel via exported syscall table.
- All public headers use `#pragma once` and are in `include/` subdirectories.
- Error handling uses `esp_err_t` return codes.
- Logging uses ESP-IDF `ESP_LOG*` macros with component-specific tags.
- FreeRTOS tasks for concurrent operations; message queues for IPC.

## Adding a New Driver
1. Create `components/drv_<name>/` with include/, src/, CMakeLists.txt
2. Implement the relevant HAL vtable interface
3. Add to a board definition to wire it up

## Adding a New Board
1. Create `components/board_<name>/`
2. Define pins, I2C addresses, SPI buses in header
3. In source, create driver instances and register with HAL
4. Add driver components as REQUIRES in CMakeLists.txt
