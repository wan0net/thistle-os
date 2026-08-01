# ThistleOS Rust kernel component (REQUIRED)

set(KERNEL_RS_REQUIRES
    thistle_hal esp_timer freertos esp_partition fatfs vfs app_update
    esp_wifi esp_netif esp_event mbedtls esp_http_client driver spiffs
    nvs_flash esp_adc sdmmc espressif__elf_loader
)
if(CONFIG_BT_ENABLED)
    list(APPEND KERNEL_RS_REQUIRES bt)
endif()

idf_component_register(
    SRCS "stub.c" "epaper_spi_shim.c" "http_client_shim.c"
    INCLUDE_DIRS "include"
    REQUIRES ${KERNEL_RS_REQUIRES}
    WHOLE_ARCHIVE
)

if(IDF_TARGET STREQUAL "esp32")
    set(RUST_TARGET "xtensa-esp32-espidf")
elseif(IDF_TARGET STREQUAL "esp32s2")
    set(RUST_TARGET "xtensa-esp32s2-espidf")
elseif(IDF_TARGET STREQUAL "esp32s3")
    set(RUST_TARGET "xtensa-esp32s3-espidf")
elseif(IDF_TARGET STREQUAL "esp32c3" OR IDF_TARGET STREQUAL "esp32c6")
    set(RUST_TARGET "riscv32imc-esp-espidf")
else()
    message(FATAL_ERROR "Unsupported ThistleOS target: ${IDF_TARGET}")
endif()

set(RUST_FEATURES "${IDF_TARGET}")
if(CONFIG_BT_ENABLED)
    string(APPEND RUST_FEATURES ",ble-hardware")
endif()
set(RUST_BUILD_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/${RUST_TARGET}/release")
set(RUST_LIB "${RUST_BUILD_DIR}/libthistle_kernel.a")

# Write a build script that sources the ESP Rust env in isolation
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/build_rust.sh"
"#!/bin/bash\nexport PATH=\"$ENV{HOME}/.cargo/bin:$PATH\"\nif [ -f \"$ENV{HOME}/export-esp.sh\" ]; then . \"$ENV{HOME}/export-esp.sh\" 2>/dev/null; fi\ncd \"${CMAKE_CURRENT_SOURCE_DIR}\"\ncargo +esp build --release --target ${RUST_TARGET} --features ${RUST_FEATURES} -Zbuild-std=std,panic_abort\n")

file(GLOB_RECURSE RUST_SRCS "${CMAKE_CURRENT_SOURCE_DIR}/src/*.rs")

add_custom_command(
    OUTPUT ${RUST_LIB}
    DEPENDS ${RUST_SRCS} "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
    COMMAND bash "${CMAKE_CURRENT_BINARY_DIR}/build_rust.sh"
    # Touch stub.c so CMake rebuilds the component lib and triggers the
    # POST_BUILD merge of Rust objects. Without this, the component archive
    # is never updated after an incremental Rust build.
    COMMAND ${CMAKE_COMMAND} -E touch "${CMAKE_CURRENT_SOURCE_DIR}/stub.c"
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Building Rust kernel (thistle-kernel)"
)

add_custom_target(kernel_rs_build DEPENDS ${RUST_LIB})
add_dependencies(${COMPONENT_LIB} kernel_rs_build)
# Merge Rust staticlib objects into this component's .a archive so the
# linker sees them as part of __idf_kernel_rs (inside ESP-IDF's
# --start-group block). This avoids the INTERFACE/PUBLIC link propagation
# placing the Rust lib outside the group.
add_custom_command(TARGET ${COMPONENT_LIB} POST_BUILD
    COMMAND ${CMAKE_AR} x ${RUST_LIB}
    COMMAND ${CMAKE_AR} rcs $<TARGET_FILE:${COMPONENT_LIB}> *.o
    COMMAND ${CMAKE_COMMAND} -E remove -f *.o
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}
    COMMENT "Merging Rust kernel objects into ${COMPONENT_LIB}"
)
