# MeshCore as ThistleOS App (Phase 5)
#
# Builds MeshCore firmware source + ThistleOS shim as a .app.elf
# Prerequisites:
#   - MeshCore source cloned as git submodule at external_apps/meshcore/MeshCore/
#   - ThistleOS App SDK at app_sdk/
#
# cmake_minimum_required(VERSION 3.16)
# project(meshcore_app)
#
# set(THISTLE_SDK_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../../app_sdk")
# include(${THISTLE_SDK_PATH}/CMakeLists.txt)
#
# thistle_app(meshcore
#     SRCS
#         app_main.c
#         # MeshCore sources would be listed here
# )
#
# target_compile_definitions(meshcore PRIVATE
#     BOARD_CLASS=ThistleMeshBoard
#     RADIO_CLASS=ThistleMeshRadio
#     DISPLAY_CLASS=ThistleMeshDisplay
# )

message(STATUS "MeshCore app build -- Phase 5, not yet configured")
