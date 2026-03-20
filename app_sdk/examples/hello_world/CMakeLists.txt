cmake_minimum_required(VERSION 3.16)
project(hello_world)

set(THISTLE_SDK_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../../")
include(${THISTLE_SDK_PATH}/CMakeLists.txt)

thistle_app(hello_world SRCS main.c)
