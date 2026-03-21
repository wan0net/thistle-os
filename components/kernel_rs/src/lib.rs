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
mod ffi;
