// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Rust implementation
//
// This crate implements kernel subsystems in Rust, exposing a C-compatible FFI
// for integration with the existing C codebase. Modules are migrated incrementally.

pub mod manifest;
pub mod version;
mod ffi;
