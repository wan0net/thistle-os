# SPDX-License-Identifier: BSD-3-Clause
# ThistleOS — MeshCore mesh protocol integration
#
# Builds the MeshCore C++ library as an ESP-IDF component.
# We compile only the core protocol files (not examples or UI helpers).

set(MESHCORE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/MeshCore/src)

set(MESHCORE_SRCS
    ${MESHCORE_DIR}/Dispatcher.cpp
    ${MESHCORE_DIR}/Identity.cpp
    ${MESHCORE_DIR}/Mesh.cpp
    ${MESHCORE_DIR}/Packet.cpp
    ${MESHCORE_DIR}/Utils.cpp
    ${MESHCORE_DIR}/helpers/BaseChatMesh.cpp
    ${MESHCORE_DIR}/helpers/AdvertDataHelpers.cpp
    ${MESHCORE_DIR}/helpers/TxtDataHelpers.cpp
    # IdentityStore.cpp and TransportKeyStore.cpp omitted — they use Arduino
    # filesystem APIs (File, FILESYSTEM). We handle identity persistence in
    # our Rust mesh_manager module instead.
    ${MESHCORE_DIR}/helpers/StaticPoolPacketManager.cpp
    # Our shim files
    src/meshcore_shim.cpp
)

idf_component_register(
    SRCS ${MESHCORE_SRCS}
    INCLUDE_DIRS
        "include"
        "${MESHCORE_DIR}"
        "${MESHCORE_DIR}/helpers"
    REQUIRES thistle_hal kernel driver esp_timer mbedtls
)

# Suppress warnings in third-party MeshCore code
target_compile_options(${COMPONENT_LIB} PRIVATE
    -Wno-unused-parameter -Wno-sign-compare -Wno-reorder
    -Wno-missing-field-initializers -Wno-type-limits -Wno-format
)
