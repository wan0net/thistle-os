# ThistleOS Rust kernel component (REQUIRED)
#
# Provides: permissions, IPC, event bus, manifest parser, version.
# Built via cargo as a static library, linked into the ESP-IDF project.

idf_component_register(
    SRCS "stub.c"
    INCLUDE_DIRS "include"
)

set(RUST_TARGET "xtensa-esp32s3-espidf")
set(RUST_BUILD_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/${RUST_TARGET}/release")
set(RUST_LIB "${RUST_BUILD_DIR}/libthistle_kernel.a")

add_custom_command(
    OUTPUT ${RUST_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
        "PATH=$ENV{HOME}/.cargo/bin:$ENV{PATH}"
        cargo +esp build --release --target ${RUST_TARGET}
            -Zbuild-std=std,panic_abort
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Building Rust kernel (thistle-kernel)"
    VERBATIM
)

add_custom_target(kernel_rs_build DEPENDS ${RUST_LIB})
add_dependencies(${COMPONENT_LIB} kernel_rs_build)
target_link_libraries(${COMPONENT_LIB} INTERFACE ${RUST_LIB})
