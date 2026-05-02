// SPDX-License-Identifier: BSD-3-Clause
//
// The active TCA8418 driver implementation lives in
// components/kernel_rs/src/drv_kbd_tca8418.rs (Rust). It is the one that
// wins the link, because kernel_rs is built with WHOLE_ARCHIVE so its
// strong `drv_kbd_tca8418_get` symbol overrides any C definition in this
// component. This file is intentionally empty — the original C
// implementation was dead code, and editing it had no runtime effect.
//
// The header `drv_kbd_tca8418.h` is still useful: it defines the
// `kbd_tca8418_config_t` struct that the legacy compiled-in board
// initialisers (board_tdeck.c, board_tdeck_pro.c) and the JSON-driven
// dispatcher (kernel/src/board_builtin_drivers.c) populate before
// calling drv_kbd_tca8418_get(). That header stays.
