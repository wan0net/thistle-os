// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Rust implementation
//
// This crate implements kernel subsystems in Rust, exposing a C-compatible FFI
// for integration with the existing C codebase. Modules are migrated incrementally.

pub mod app_manager;
pub mod event;
pub mod ipc;
pub mod manifest;
pub mod permissions;
pub mod signing;
pub mod version;
pub mod kernel_boot;
pub mod display_server;
pub mod board_config;
pub mod driver_manager;
pub mod driver_loader;
pub mod elf_loader;
pub mod syscall_table;
pub mod ota;
pub mod wifi_manager;
pub mod ble_manager;
pub mod net_manager;
pub mod appstore_client;
pub mod crypto;
pub mod widget;
mod ffi;
