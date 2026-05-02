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
pub mod hal_registry;
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
pub mod mesh_manager;
pub mod appstore_client;
pub mod crypto;
pub mod widget;
pub mod tk_wm;
pub mod tk_launcher;
pub mod tk_flashlight;
pub mod tk_appstore;
pub mod tk_meshchat;
pub mod drv_kbd_tca8418;
pub mod drv_kbd_cardkb;
pub mod drv_touch_cst328;
pub mod drv_touch_cst816;
pub mod drv_touch_xpt2046;
pub mod drv_oled_ssd1306;
pub mod drv_epaper_gdeq031t10;
pub mod drv_gps_mia_m10q;
pub mod drv_power_tp4065b;
pub mod drv_sdcard;
pub mod drv_audio_pcm5102a;
pub mod drv_imu_bhi260ap;
pub mod drv_light_ltr553;
pub mod drv_lcd_st7789;
pub mod drv_lcd_ili9341;
pub mod drv_display_co5300;
pub mod drv_touch_ft3x68;
pub mod drv_touch_cst9217;
pub mod drv_rtc_pcf8563;
pub mod drv_accel_qmi8658;
pub mod gps_track;
pub mod data_logger;
pub mod sos_beacon;
pub mod secure_wipe;
pub mod notification;
pub mod contact_manager;
pub mod ble_scanner;
pub mod burn_timer;
pub mod msg_queue;
pub mod msg_crypto;
pub mod driver_reload;
pub mod thistle_shell;
mod ffi;
