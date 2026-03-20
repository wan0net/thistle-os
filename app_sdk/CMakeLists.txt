# ThistleOS App SDK -- CMake helpers for building .app.elf
#
# Usage in app CMakeLists.txt:
#   set(THISTLE_SDK_PATH "/path/to/app_sdk")
#   include(${THISTLE_SDK_PATH}/CMakeLists.txt)
#   thistle_app(my_app SRCS main.c)

cmake_minimum_required(VERSION 3.16)

function(thistle_app APP_NAME)
    cmake_parse_arguments(ARG "" "" "SRCS" ${ARGN})

    add_executable(${APP_NAME} ${ARG_SRCS})

    target_include_directories(${APP_NAME} PRIVATE
        ${CMAKE_CURRENT_FUNCTION_LIST_DIR}/include
    )

    # Build as relocatable ELF (no standard libs, position independent)
    target_compile_options(${APP_NAME} PRIVATE
        -fPIC
        -nostdlib
        -fno-exceptions
    )

    target_link_options(${APP_NAME} PRIVATE
        -nostdlib
        -Wl,-relocatable
    )

    set_target_properties(${APP_NAME} PROPERTIES
        SUFFIX ".app.elf"
    )
endfunction()
