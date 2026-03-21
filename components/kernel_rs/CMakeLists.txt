# ThistleOS Rust kernel component (REQUIRED)
#
# Provides: all kernel logic (permissions, IPC, events, app manager, signing,
# manifest, display server, board config, drivers, OTA, WiFi, BLE, net, appstore).
# Built via cargo as a static library, linked into the ESP-IDF project.

idf_component_register(
    SRCS "stub.c"
    INCLUDE_DIRS "include"
)

set(RUST_TARGET "xtensa-esp32s3-espidf")
set(RUST_BUILD_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/${RUST_TARGET}/release")
set(RUST_LIB "${RUST_BUILD_DIR}/libthistle_kernel.a")

# Build Rust kernel in a subshell that sources the ESP Rust environment
# without polluting the outer IDF build's PATH/toolchain.
add_custom_command(
    OUTPUT ${RUST_LIB}
    COMMAND bash -c "
        export PATH=\"$ENV{HOME}/.cargo/bin:$$PATH\"
        [ -f $ENV{HOME}/export-esp.sh ] && . $ENV{HOME}/export-esp.sh 2>/dev/null
        cargo +esp build --release --target ${RUST_TARGET} -Zbuild-std=std,panic_abort
    "
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Building Rust kernel (thistle-kernel)"
    VERBATIM
)

add_custom_target(kernel_rs_build DEPENDS ${RUST_LIB})
add_dependencies(${COMPONENT_LIB} kernel_rs_build)
target_link_libraries(${COMPONENT_LIB} INTERFACE ${RUST_LIB})
