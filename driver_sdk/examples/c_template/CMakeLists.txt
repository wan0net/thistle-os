cmake_minimum_required(VERSION 3.16)
project(example_driver C)

set(THISTLE_DRIVER_SDK "${CMAKE_CURRENT_SOURCE_DIR}/../..")
include(${THISTLE_DRIVER_SDK}/CMakeLists.txt)

thistle_driver(example_driver SRCS main.c)
