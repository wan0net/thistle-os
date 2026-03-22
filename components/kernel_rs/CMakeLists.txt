# ThistleOS Rust kernel component (REQUIRED)

idf_component_register(
    SRCS "stub.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal esp_timer freertos esp_partition fatfs vfs app_update
             esp_wifi esp_netif esp_event bt mbedtls esp_http_client
             driver spiffs nvs_flash esp_adc sdmmc espressif__elf_loader
    WHOLE_ARCHIVE
)

set(RUST_TARGET "xtensa-esp32s3-espidf")
set(RUST_BUILD_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/${RUST_TARGET}/release")
set(RUST_LIB "${RUST_BUILD_DIR}/libthistle_kernel.a")

# Write a build script that sources the ESP Rust env in isolation
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/build_rust.sh"
"#!/bin/bash\nexport PATH=\"$ENV{HOME}/.cargo/bin:$PATH\"\nif [ -f \"$ENV{HOME}/export-esp.sh\" ]; then . \"$ENV{HOME}/export-esp.sh\" 2>/dev/null; fi\ncd \"${CMAKE_CURRENT_SOURCE_DIR}\"\ncargo +esp build --release --target ${RUST_TARGET} -Zbuild-std=std,panic_abort\n")

add_custom_command(
    OUTPUT ${RUST_LIB}
    COMMAND bash "${CMAKE_CURRENT_BINARY_DIR}/build_rust.sh"
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
