cmake_minimum_required(VERSION 3.16)
project(thistle_sim C CXX)

set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 17)

# Find SDL2
find_package(PkgConfig REQUIRED)
pkg_check_modules(SDL2 REQUIRED sdl2)

# LVGL source — use the managed component downloaded by ESP-IDF build
set(LVGL_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../managed_components/lvgl__lvgl")

# Collect LVGL sources (core + widgets + draw + display + misc)
file(GLOB_RECURSE LVGL_SRCS
    "${LVGL_DIR}/src/*.c"
)

# ThistleOS source files (kernel, UI, HAL, apps)
set(THISTLE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/..")
set(THISTLE_SRCS
    # HAL
    ${THISTLE_DIR}/components/thistle_hal/src/hal_registry.c
    # Kernel (except elf_loader which needs ESP-IDF elf component)
    ${THISTLE_DIR}/components/kernel/src/kernel.c
    ${THISTLE_DIR}/components/kernel/src/app_manager.c
    ${THISTLE_DIR}/components/kernel/src/driver_manager.c
    ${THISTLE_DIR}/components/kernel/src/syscall_table.c
    ${THISTLE_DIR}/components/kernel/src/ipc.c
    ${THISTLE_DIR}/components/kernel/src/event.c
    # UI
    ${THISTLE_DIR}/components/ui/src/manager.c
    ${THISTLE_DIR}/components/ui/src/theme.c
    ${THISTLE_DIR}/components/ui/src/statusbar.c
    ${THISTLE_DIR}/components/ui/src/app_switcher.c
    ${THISTLE_DIR}/components/ui/src/epaper_refresh.c
    ${THISTLE_DIR}/components/ui/src/toast.c
    # Built-in apps
    ${THISTLE_DIR}/components/apps_builtin/launcher/launcher_app.c
    ${THISTLE_DIR}/components/apps_builtin/launcher/launcher_ui.c
    ${THISTLE_DIR}/components/apps_builtin/settings/settings_app.c
    ${THISTLE_DIR}/components/apps_builtin/settings/settings_ui.c
    ${THISTLE_DIR}/components/apps_builtin/file_manager/filemgr_app.c
    ${THISTLE_DIR}/components/apps_builtin/file_manager/filemgr_ui.c
    ${THISTLE_DIR}/components/apps_builtin/reader/reader_app.c
    ${THISTLE_DIR}/components/apps_builtin/reader/reader_ui.c
    ${THISTLE_DIR}/components/apps_builtin/messenger/messenger_app.c
    ${THISTLE_DIR}/components/apps_builtin/messenger/messenger_ui.c
    ${THISTLE_DIR}/components/apps_builtin/messenger/messenger_transport.c
    ${THISTLE_DIR}/components/apps_builtin/navigator/navigator_app.c
    ${THISTLE_DIR}/components/apps_builtin/navigator/navigator_ui.c
    ${THISTLE_DIR}/components/apps_builtin/notes/notes_app.c
    ${THISTLE_DIR}/components/apps_builtin/notes/notes_ui.c
    ${THISTLE_DIR}/components/apps_builtin/appstore/appstore_app.c
    ${THISTLE_DIR}/components/apps_builtin/appstore/appstore_ui.c
    ${THISTLE_DIR}/components/apps_builtin/assistant/assistant_app.c
    ${THISTLE_DIR}/components/apps_builtin/assistant/assistant_ui.c
    ${THISTLE_DIR}/components/apps_builtin/wifiscanner/wifiscanner_app.c
    ${THISTLE_DIR}/components/apps_builtin/wifiscanner/wifiscanner_ui.c
    ${THISTLE_DIR}/components/apps_builtin/flashlight/flashlight_app.c
    ${THISTLE_DIR}/components/apps_builtin/flashlight/flashlight_ui.c
    ${THISTLE_DIR}/components/apps_builtin/weather/weather_app.c
    ${THISTLE_DIR}/components/apps_builtin/weather/weather_ui.c
    ${THISTLE_DIR}/components/apps_builtin/terminal/terminal_app.c
    ${THISTLE_DIR}/components/apps_builtin/terminal/terminal_ui.c
    ${THISTLE_DIR}/components/apps_builtin/vault/vault_app.c
    ${THISTLE_DIR}/components/apps_builtin/vault/vault_ui.c
)

# Simulator sources
set(SIM_SRCS
    main.c
    platform/platform_stubs.c
    platform/sim_vfs.c
    drivers/sim_display.c
    drivers/sim_input.c
    drivers/sim_storage.c
    drivers/sim_wifi.c
    drivers/sim_ble.c
    board_simulator.c
)

add_executable(thistle_sim ${SIM_SRCS} ${THISTLE_SRCS} ${LVGL_SRCS})

# Include paths — simulator platform stubs MUST come before any ThistleOS includes
# so that #include "esp_log.h" finds our stub, not a real ESP-IDF header
target_include_directories(thistle_sim PRIVATE
    # Simulator stubs FIRST (shadow ESP-IDF headers)
    ${CMAKE_CURRENT_SOURCE_DIR}/platform
    ${CMAKE_CURRENT_SOURCE_DIR}/drivers
    ${CMAKE_CURRENT_SOURCE_DIR}
    # LVGL
    ${LVGL_DIR}
    ${LVGL_DIR}/src
    # ThistleOS component includes
    ${THISTLE_DIR}/components/thistle_hal/include
    ${THISTLE_DIR}/components/kernel/include
    ${THISTLE_DIR}/components/ui/include
    ${THISTLE_DIR}/components/apps_builtin
    # SDL2
    ${SDL2_INCLUDE_DIRS}
)

# LVGL configuration for simulator
target_compile_definitions(thistle_sim PRIVATE
    LV_CONF_INCLUDE_SIMPLE=1
    LV_USE_SDL=1
    LV_USE_OS=LV_OS_NONE
    LV_FONT_MONTSERRAT_14=1
    LV_FONT_MONTSERRAT_18=1
    LV_FONT_MONTSERRAT_22=1
    LV_COLOR_DEPTH=16
    # Disable ESP-IDF specific features
    SIMULATOR_BUILD=1
    CONFIG_LOG_DEFAULT_LEVEL=3
    # SD card path for simulator — points to local sdcard directory
    THISTLE_SDCARD_PATH="/tmp/thistle_sdcard"
)

target_link_directories(thistle_sim PRIVATE ${SDL2_LIBRARY_DIRS})
target_link_libraries(thistle_sim PRIVATE ${SDL2_LIBRARIES} pthread m)

# Suppress warnings from LVGL and external code
target_compile_options(thistle_sim PRIVATE
    -Wno-unused-parameter
    -Wno-sign-compare
    -Wno-missing-field-initializers
    -Wno-unused-function
    -Wno-unused-variable
)
