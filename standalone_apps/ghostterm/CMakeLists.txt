cmake_minimum_required(VERSION 3.16)
project(ghostterm C)

set(THISTLE_APP_SDK "${CMAKE_CURRENT_SOURCE_DIR}/../../app_sdk")
include(${THISTLE_APP_SDK}/CMakeLists.txt)

thistle_app(ghostterm SRCS main.c)

# libghostty-vt headers
target_include_directories(ghostterm PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../external/ghostty/include
)

# libghostty-vt library (built separately via Zig)
# For now, link against a pre-built .a if available
set(GHOSTTY_LIB "${CMAKE_CURRENT_SOURCE_DIR}/../../external/ghostty/zig-out/lib/libghostty-vt.a")
if(EXISTS ${GHOSTTY_LIB})
    target_link_libraries(ghostterm PRIVATE ${GHOSTTY_LIB})
else()
    message(WARNING "libghostty-vt.a not found — GhostTerm will not link until built")
endif()
