# ThistleOS Rust kernel component
#
# Builds the Rust staticlib via cargo and links it into the ESP-IDF project.
# The Rust code exposes C-compatible FFI functions (rs_manifest_*, rs_kernel_*).
#
# The Rust build is OPTIONAL — if cargo/esp toolchain is not available,
# the component registers as a no-op and the C implementations are used instead.

idf_component_register(
    SRCS "stub.c"
    INCLUDE_DIRS "include"
)

# Check if Rust ESP toolchain is available
find_program(CARGO_BIN cargo HINTS "$ENV{HOME}/.cargo/bin")

if(CARGO_BIN)
    # Verify the esp toolchain exists
    execute_process(
        COMMAND ${CARGO_BIN} +esp --version
        RESULT_VARIABLE CARGO_ESP_RESULT
        OUTPUT_QUIET ERROR_QUIET
    )

    if(CARGO_ESP_RESULT EQUAL 0)
        message(STATUS "Rust ESP toolchain found — building kernel_rs")

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
    else()
        message(STATUS "Rust ESP toolchain not found — skipping kernel_rs (using C fallback)")
    endif()
else()
    message(STATUS "cargo not found — skipping kernel_rs Rust build (using C fallback)")
endif()
