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
    # HAL (registry moved to Rust — hal_registry.rs in kernel_rs)
    ${THISTLE_DIR}/components/thistle_hal/src/stub.c
    # Widget API dispatch (Rust widget.rs → WM vtable bridge)
    ${THISTLE_DIR}/components/ui/src/widget_shims.c
    # thistle-tk WM C vtable + HAL bridge
    ${THISTLE_DIR}/components/kernel/src/tk_wm_shims.c
    # UI
    ${THISTLE_DIR}/components/ui/src/manager.c
    ${THISTLE_DIR}/components/ui/src/lvgl_wm.c
    ${THISTLE_DIR}/components/ui/src/lvgl_wm_epaper.c
    ${THISTLE_DIR}/components/ui/src/lvgl_wm_lcd.c
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
    platform/sim_http.c
    platform/sim_i2c_bus.c
    platform/sim_spi_bus.c
    devices/dev_pcf8563.c
    devices/dev_qmi8658c.c
    devices/dev_tca8418.c
    devices/dev_cst328.c
    devices/dev_ltr553.c
    drivers/sim_display.c
    drivers/sim_input.c
    drivers/sim_storage.c
    drivers/sim_network.c
    drivers/sim_power.c
    drivers/sim_gps.c
    drivers/sim_imu.c
    drivers/sim_rtc.c
    drivers/sim_audio.c
    drivers/sim_radio.c
    drivers/sim_scenario.c
    # sim_wifi.c and sim_ble.c removed — Rust kernel provides wifi_manager + ble_manager
    board_simulator.c
    sim_assert.c
)

add_executable(thistle_sim ${SIM_SRCS} ${THISTLE_SRCS} ${LVGL_SRCS})

# Include paths — simulator platform stubs MUST come before any ThistleOS includes
# so that #include "esp_log.h" finds our stub, not a real ESP-IDF header
target_include_directories(thistle_sim PRIVATE
    # Simulator stubs FIRST (shadow ESP-IDF headers)
    ${CMAKE_CURRENT_SOURCE_DIR}/platform
    ${CMAKE_CURRENT_SOURCE_DIR}/drivers
    ${CMAKE_CURRENT_SOURCE_DIR}/devices
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

# Build Rust kernel for host target
set(KERNEL_RS_DIR "${THISTLE_DIR}/components/kernel_rs")
execute_process(
    COMMAND uname -m
    OUTPUT_VARIABLE HOST_ARCH
    OUTPUT_STRIP_TRAILING_WHITESPACE
)
# Rust uses "aarch64" not "arm64"
if(HOST_ARCH STREQUAL "arm64")
    set(HOST_ARCH "aarch64")
endif()
if(APPLE)
    set(RUST_HOST_TARGET "${HOST_ARCH}-apple-darwin")
else()
    set(RUST_HOST_TARGET "${HOST_ARCH}-unknown-linux-gnu")
endif()
set(RUST_HOST_LIB "${KERNEL_RS_DIR}/target/${RUST_HOST_TARGET}/release/libthistle_kernel.a")

add_custom_command(
    OUTPUT ${RUST_HOST_LIB}
    COMMAND ${CMAKE_COMMAND} -E env "PATH=$ENV{HOME}/.cargo/bin:$ENV{PATH}"
        cargo build --release --target ${RUST_HOST_TARGET} --features sim-bus
    WORKING_DIRECTORY ${KERNEL_RS_DIR}
    COMMENT "Building Rust kernel for simulator (${RUST_HOST_TARGET})"
    VERBATIM
)
add_custom_target(kernel_rs_host_build DEPENDS ${RUST_HOST_LIB})
add_dependencies(thistle_sim kernel_rs_host_build)

target_include_directories(thistle_sim PRIVATE
    ${KERNEL_RS_DIR}/include
)

target_link_directories(thistle_sim PRIVATE ${SDL2_LIBRARY_DIRS})
target_link_libraries(thistle_sim PRIVATE ${SDL2_LIBRARIES} ${RUST_HOST_LIB} pthread m curl dl)

# Export symbols so dlopen'd modules can resolve syscall functions
if(NOT APPLE)
    target_link_options(thistle_sim PRIVATE -rdynamic)
else()
    target_link_options(thistle_sim PRIVATE -Wl,-export_dynamic)
endif()

# Suppress warnings from LVGL and external code
target_compile_options(thistle_sim PRIVATE
    -Wno-unused-parameter
    -Wno-sign-compare
    -Wno-missing-field-initializers
    -Wno-unused-function
    -Wno-unused-variable
)

# --- Simulator unit tests ---
add_executable(thistle_sim_tests
    tests/test_sim_main.c
    tests/test_sim_assert.c
    tests/test_i2c_bus.c
    sim_assert.c
    platform/sim_i2c_bus.c
    platform/sim_spi_bus.c
    platform/platform_stubs.c
    devices/dev_pcf8563.c
    devices/dev_qmi8658c.c
    devices/dev_tca8418.c
    devices/dev_cst328.c
    devices/dev_ltr553.c
)

target_include_directories(thistle_sim_tests PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/platform
    ${CMAKE_CURRENT_SOURCE_DIR}/drivers
    ${CMAKE_CURRENT_SOURCE_DIR}/devices
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/tests
    ${THISTLE_DIR}/components/thistle_hal/include
)

target_compile_definitions(thistle_sim_tests PRIVATE
    SIMULATOR_BUILD=1
)

target_compile_options(thistle_sim_tests PRIVATE
    -Wno-unused-parameter
    -Wno-sign-compare
    -Wno-missing-field-initializers
    -Wno-unused-function
    -Wno-unused-variable
)

target_link_libraries(thistle_sim_tests PRIVATE pthread m)
