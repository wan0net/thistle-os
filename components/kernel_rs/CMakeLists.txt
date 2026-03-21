# ThistleOS Rust kernel component (REQUIRED)

idf_component_register(
    SRCS "stub.c"
    INCLUDE_DIRS "include"
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
target_link_libraries(${COMPONENT_LIB} INTERFACE ${RUST_LIB})
