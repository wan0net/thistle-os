# ThistleOS Driver SDK — CMake helpers for building .drv.elf
#
# Usage:
#   set(THISTLE_DRIVER_SDK "/path/to/driver_sdk")
#   include(${THISTLE_DRIVER_SDK}/CMakeLists.txt)
#   thistle_driver(my_driver SRCS my_driver.c)

cmake_minimum_required(VERSION 3.16)

function(thistle_driver DRV_NAME)
    cmake_parse_arguments(ARG "" "" "SRCS" ${ARGN})

    add_executable(${DRV_NAME} ${ARG_SRCS})

    target_include_directories(${DRV_NAME} PRIVATE
        ${CMAKE_CURRENT_FUNCTION_LIST_DIR}/include
    )

    # Build as relocatable ELF (no standard libs, position independent)
    target_compile_options(${DRV_NAME} PRIVATE
        -fPIC
        -nostdlib
        -fno-exceptions
        -Os
    )

    target_link_options(${DRV_NAME} PRIVATE
        -nostdlib
        -Wl,-relocatable
    )

    set_target_properties(${DRV_NAME} PROPERTIES
        SUFFIX ".drv.elf"
    )
endfunction()
