# ThistleOS Apps

Standalone apps that ship with ThistleOS but are separate from the core OS firmware.

Each app is an independent ESP-IDF component today, on a path to becoming a signed
`.app.elf` loadable at runtime from SPIFFS or SD card.

## Apps

| Directory      | App ID                     | Description                        |
|----------------|----------------------------|------------------------------------|
| `notes/`       | com.thistle.notes          | Text note editor with SD persistence |
| `reader/`      | com.thistle.reader         | Document/ebook reader               |
| `vault/`       | com.thistle.vault          | Encrypted file vault                |
| `file_manager/`| com.thistle.filemgr        | SD card file browser                |
| `wifiscanner/` | com.thistle.wifiscanner    | WiFi network scanner                |
| `navigator/`   | com.thistle.navigator      | GPS map navigator                   |
| `flashlight/`  | com.thistle.flashlight     | LED flashlight                      |
| `weather/`     | com.thistle.weather        | Weather with GPS/IMU integration    |
| `appstore/`    | com.thistle.appstore       | App Store browser and installer     |

## Structure

Each app directory contains:
- `CMakeLists.txt` — ESP-IDF component definition
- `manifest.json` — app metadata (id, name, version, permissions)
- `{name}/` — source files (`*_app.c`, `*_app.h`, `*_ui.c`)

## Building

These apps are built as part of the main ThistleOS firmware. The root
`CMakeLists.txt` includes `apps/` in `EXTRA_COMPONENT_DIRS`.

## Migrating to .app.elf

To build an app as a standalone signed ELF (the target state):

1. Replace `idf_component_register(...)` in `CMakeLists.txt` with:
   ```cmake
   set(THISTLE_SDK_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../../app_sdk")
   include(${THISTLE_SDK_PATH}/CMakeLists.txt)
   thistle_app(app_name SRCS ...)
   ```
2. Replace direct LVGL/HAL includes with `thistle_app.h` API calls.
3. Sign the output ELF with `tools/sign_elf.py`.
4. Drop onto SD card or SPIFFS under `apps/`.
