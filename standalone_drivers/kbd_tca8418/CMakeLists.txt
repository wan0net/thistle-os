cmake_minimum_required(VERSION 3.16)
project(kbd_tca8418 C)

set(THISTLE_DRIVER_SDK "${CMAKE_CURRENT_SOURCE_DIR}/../../driver_sdk")
include(${THISTLE_DRIVER_SDK}/CMakeLists.txt)

thistle_driver(kbd_tca8418 SRCS main.c)

# HAL headers for struct definitions + ESP-IDF type stubs
target_include_directories(kbd_tca8418 PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../components/thistle_hal/include
    ${CMAKE_CURRENT_SOURCE_DIR}/../../simulator/platform
)
