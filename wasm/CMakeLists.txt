cmake_minimum_required(VERSION 3.16)
project(thistle_wasm C)

# Emscripten compiles C to WASM and auto-shims SDL2 to HTML5 Canvas.
# The Rust kernel is compiled separately and linked as a static lib.

set(CMAKE_C_STANDARD 11)

# LVGL source — use the managed component downloaded by ESP-IDF build
set(LVGL_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../managed_components/lvgl__lvgl")

file(GLOB_RECURSE LVGL_SRCS "${LVGL_DIR}/src/*.c")

# ThistleOS source files (UI + apps — kernel is Rust)
set(THISTLE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/..")
set(THISTLE_SRCS
    # HAL (registry moved to Rust — hal_registry.rs in kernel_rs)
    ${THISTLE_DIR}/components/thistle_hal/src/stub.c
    # Widget shims (Rust widget.rs → WM vtable bridge)
    ${THISTLE_DIR}/components/ui/src/widget_shims.c
    # thistle-tk WM C vtable + HAL bridge
    ${THISTLE_DIR}/components/kernel/src/tk_wm_shims.c
    # UI (C — stays C, this is the LVGL WM)
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
    ${THISTLE_DIR}/components/apps_builtin/assistant/assistant_request.c
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

# Built-in apps are being migrated from compiled-in C to signed Rust ELFs.
# Keep the browser simulator buildable throughout that migration: only compile
# source files that still exist and expose availability flags to main.c.
function(thistle_set_optional_app_flag flag source)
    if(EXISTS "${THISTLE_DIR}/${source}")
        set(${flag} 1 PARENT_SCOPE)
    else()
        set(${flag} 0 PARENT_SCOPE)
    endif()
endfunction()

thistle_set_optional_app_flag(THISTLE_HAVE_LAUNCHER "components/apps_builtin/launcher/launcher_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_SETTINGS "components/apps_builtin/settings/settings_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_FILEMGR "components/apps_builtin/file_manager/filemgr_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_READER "components/apps_builtin/reader/reader_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_MESSENGER "components/apps_builtin/messenger/messenger_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_NAVIGATOR "components/apps_builtin/navigator/navigator_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_NOTES "components/apps_builtin/notes/notes_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_APPSTORE "components/apps_builtin/appstore/appstore_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_ASSISTANT "components/apps_builtin/assistant/assistant_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_WIFISCANNER "components/apps_builtin/wifiscanner/wifiscanner_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_FLASHLIGHT "components/apps_builtin/flashlight/flashlight_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_WEATHER "components/apps_builtin/weather/weather_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_TERMINAL "components/apps_builtin/terminal/terminal_app.c")
thistle_set_optional_app_flag(THISTLE_HAVE_VAULT "components/apps_builtin/vault/vault_app.c")

set(THISTLE_WASM_SRCS)
foreach(source IN LISTS THISTLE_SRCS)
    if(EXISTS "${source}")
        list(APPEND THISTLE_WASM_SRCS "${source}")
    else()
        message(STATUS "Skipping migrated WASM app source: ${source}")
    endif()
endforeach()

# WASM-specific sources
set(SIM_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../simulator")
set(WASM_SRCS
    main.c
    platform_stubs.c
    # Simulator drivers (SDL2 display/input, board init)
    ${SIM_DIR}/drivers/sim_display.c
    ${SIM_DIR}/drivers/sim_input.c
    ${SIM_DIR}/drivers/sim_storage.c
    ${SIM_DIR}/drivers/sim_network.c
    ${SIM_DIR}/drivers/sim_power.c
    ${SIM_DIR}/drivers/sim_gps.c
    ${SIM_DIR}/drivers/sim_imu.c
    ${SIM_DIR}/drivers/sim_rtc.c
    ${SIM_DIR}/drivers/sim_audio.c
    ${SIM_DIR}/drivers/sim_radio.c
    ${SIM_DIR}/drivers/sim_scenario.c
    ${SIM_DIR}/platform/sim_i2c_bus.c
    ${SIM_DIR}/platform/sim_spi_bus.c
    ${SIM_DIR}/devices/dev_pcf8563.c
    ${SIM_DIR}/devices/dev_qmi8658c.c
    ${SIM_DIR}/devices/dev_tca8418.c
    ${SIM_DIR}/devices/dev_cst328.c
    ${SIM_DIR}/devices/dev_ltr553.c
    ${SIM_DIR}/sim_assert.c
    ${SIM_DIR}/board_simulator.c
    ${SIM_DIR}/platform/sim_vfs.c
)

add_executable(thistle_wasm ${WASM_SRCS} ${THISTLE_WASM_SRCS} ${LVGL_SRCS})
set_target_properties(thistle_wasm PROPERTIES SUFFIX ".html")

# Emscripten SDL2 — auto-provided, no system install needed
set(USE_FLAGS "-sUSE_SDL=2")

target_include_directories(thistle_wasm PRIVATE
    # WASM platform stubs (shadow ESP-IDF headers)
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/../simulator/platform
    ${CMAKE_CURRENT_SOURCE_DIR}/../simulator/drivers
    ${CMAKE_CURRENT_SOURCE_DIR}/../simulator/devices
    ${CMAKE_CURRENT_SOURCE_DIR}/../simulator
    # LVGL
    ${LVGL_DIR}
    ${LVGL_DIR}/src
    # ThistleOS component includes
    ${THISTLE_DIR}/components/thistle_hal/include
    ${THISTLE_DIR}/components/kernel/include
    ${THISTLE_DIR}/components/kernel_rs/include
    ${THISTLE_DIR}/components/ui/include
    ${THISTLE_DIR}/components/apps_builtin
)

target_compile_definitions(thistle_wasm PRIVATE
    LV_CONF_INCLUDE_SIMPLE=1
    LV_USE_SDL=1
    LV_USE_OS=LV_OS_NONE
    LV_FONT_MONTSERRAT_14=1
    LV_FONT_MONTSERRAT_18=1
    LV_FONT_MONTSERRAT_22=1
    LV_COLOR_DEPTH=16
    SIMULATOR_BUILD=1
    WASM_BUILD=1
    THISTLE_HAVE_LAUNCHER=${THISTLE_HAVE_LAUNCHER}
    THISTLE_HAVE_SETTINGS=${THISTLE_HAVE_SETTINGS}
    THISTLE_HAVE_FILEMGR=${THISTLE_HAVE_FILEMGR}
    THISTLE_HAVE_READER=${THISTLE_HAVE_READER}
    THISTLE_HAVE_MESSENGER=${THISTLE_HAVE_MESSENGER}
    THISTLE_HAVE_NAVIGATOR=${THISTLE_HAVE_NAVIGATOR}
    THISTLE_HAVE_NOTES=${THISTLE_HAVE_NOTES}
    THISTLE_HAVE_APPSTORE=${THISTLE_HAVE_APPSTORE}
    THISTLE_HAVE_ASSISTANT=${THISTLE_HAVE_ASSISTANT}
    THISTLE_HAVE_WIFISCANNER=${THISTLE_HAVE_WIFISCANNER}
    THISTLE_HAVE_FLASHLIGHT=${THISTLE_HAVE_FLASHLIGHT}
    THISTLE_HAVE_WEATHER=${THISTLE_HAVE_WEATHER}
    THISTLE_HAVE_TERMINAL=${THISTLE_HAVE_TERMINAL}
    THISTLE_HAVE_VAULT=${THISTLE_HAVE_VAULT}
    CONFIG_LOG_DEFAULT_LEVEL=3
    THISTLE_SDCARD_PATH="/thistle_sdcard"
)

target_compile_options(thistle_wasm PRIVATE
    ${USE_FLAGS}
    -Wno-unused-parameter
    -Wno-sign-compare
    -Wno-missing-field-initializers
    -Wno-unused-function
    -Wno-unused-variable
)

# Link Rust kernel (compiled to wasm32-unknown-emscripten)
set(KERNEL_RS_DIR "${THISTLE_DIR}/components/kernel_rs")
set(RUST_WASM_TARGET "wasm32-unknown-emscripten")
set(RUST_WASM_LIB "${KERNEL_RS_DIR}/target/${RUST_WASM_TARGET}/release/libthistle_kernel.a")

# Build Rust kernel for WASM via script
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/build_rust_wasm.sh"
"#!/bin/bash\nexport PATH=\"$ENV{HOME}/.cargo/bin:$PATH\"\ncd \"${KERNEL_RS_DIR}\"\ncargo build --release --target ${RUST_WASM_TARGET}\n")

add_custom_command(
    OUTPUT ${RUST_WASM_LIB}
    COMMAND bash "${CMAKE_CURRENT_BINARY_DIR}/build_rust_wasm.sh"
    WORKING_DIRECTORY ${KERNEL_RS_DIR}
    COMMENT "Building Rust kernel for WASM (${RUST_WASM_TARGET})"
)
add_custom_target(kernel_rs_wasm_build DEPENDS ${RUST_WASM_LIB})
add_dependencies(thistle_wasm kernel_rs_wasm_build)

target_link_options(thistle_wasm PRIVATE
    ${USE_FLAGS}
    -sALLOW_MEMORY_GROWTH=1
    -sINITIAL_MEMORY=33554432
    -sSTACK_SIZE=1048576
    -sEXPORTED_FUNCTIONS=['_main','_wasm_set_device','_app_manager_launch','_free']
    -sEXPORTED_RUNTIME_METHODS=['ccall','cwrap','allocateUTF8']
    --preload-file ${THISTLE_DIR}/simulator/sdcard@/thistle_sdcard
    --shell-file ${CMAKE_CURRENT_SOURCE_DIR}/shell.html
)

target_link_libraries(thistle_wasm PRIVATE ${RUST_WASM_LIB})
