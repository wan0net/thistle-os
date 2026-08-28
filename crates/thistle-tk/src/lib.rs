// SPDX-License-Identifier: BSD-3-Clause
//! thistle-ui — Embedded widget toolkit for e-paper and LCD displays
//!
//! Built on `embedded-graphics`. Apps create widgets using a high-level API.
//! A layout engine positions them. A renderer draws them to any `DrawTarget`.
//! Display-specific backends handle the pixel format (1-bit mono, RGB565, etc).
//!
//! ```text
//! App → Widget Tree → Layout → Renderer → DrawTarget (e-paper / LCD)
//! ```

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod color;
pub mod input;
pub mod layout;
pub mod render;
pub mod theme;
pub mod tree;
pub mod widget;

pub use color::Color;
pub use render::{render, render_dirty, ColorMapper, MonoMapper, RgbMapper};
pub use theme::Theme;
pub use tree::UiTree;
pub use widget::{Widget, WidgetId};
