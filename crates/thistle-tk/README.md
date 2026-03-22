# thistle-tk

A `no_std` Rust widget toolkit for embedded displays, built on [embedded-graphics](https://github.com/embedded-graphics/embedded-graphics).

One widget tree. Any display. E-paper or LCD — same code.

> **Alpha** — under active development as part of [ThistleOS](https://github.com/wan0net/thistle-os).

## What it does

Apps build UI using semantic widgets and theme colors. A layout engine positions them. A renderer draws them to any `embedded-graphics` `DrawTarget`. Display-specific backends handle the pixel format.

```text
App → Widget Tree → Layout Engine → Renderer → DrawTarget
                                        │
                                ┌───────┴───────┐
                                │               │
                           MonoMapper       RgbMapper
                           (BinaryColor)    (Rgb565)
                           e-paper          LCD/OLED
```

## Features

- **Semantic colors** — apps use `Color::Primary`, `Color::Text`, etc. The theme resolves them to actual pixel values. On e-paper, everything thresholds to black/white automatically.
- **Widget types** — Container (flex row/column), Label, Button, TextInput, Image
- **Flexbox layout** — direction, alignment, gap, padding, flex-grow
- **Theme system** — built-in monochrome and dark themes, or define your own
- **Input dispatch** — touch hit-testing and keyboard focus routing
- **Arena-based widget tree** — stable `WidgetId` handles, subtree removal, dirty tracking
- **Display-agnostic** — renders to any `DrawTarget` via the `ColorMapper` trait

## Usage

```rust
use thistle_tk::*;
use thistle_tk::widget::*;
use thistle_tk::layout::Direction;

// Create the UI tree
let mut tree = UiTree::new();
let root = tree.root();

// Add widgets
let label = tree.add_child(root, Widget::label("Hello, world!"));
let button = tree.add_child(root, Widget::button("Press me", Some(on_click)));

// Set container layout
if let Some(w) = tree.get_mut(root) {
    if let Widget::Container(c) = w {
        c.direction = Direction::Column;
        c.gap = 8;
    }
}

// Layout and render
let theme = Theme::monochrome();
layout::layout(&mut tree, Rect { x: 0, y: 0, w: 240, h: 320 });
render::render(&tree, &theme, &MonoMapper, &mut display);
```

## Display backends

The `ColorMapper` trait maps semantic colors to display-specific pixel types:

| Mapper | Target | Use case |
|--------|--------|----------|
| `MonoMapper` | `BinaryColor` | E-paper (1-bit, black/white) |
| `RgbMapper` | `Rgb565` | LCD, OLED (16-bit color) |

Write your own `ColorMapper` for other pixel formats.

## Modules

| Module | What it does |
|--------|-------------|
| `color` | `Color` enum — semantic (Primary, Text, ...) or explicit (Rgb, Black, White) |
| `theme` | `Theme` struct — color palette + font sizes. Built-in: `monochrome()`, `dark()` |
| `widget` | Widget types: Container, Label, Button, TextInput, Image |
| `layout` | Flexbox-like layout engine (row/column, alignment, gap, flex-grow) |
| `tree` | Arena-based `UiTree` with hit testing, focus management, dirty tracking |
| `render` | Draws widget tree to any `DrawTarget` via `ColorMapper` |
| `input` | Routes touch/key events to widgets (hit test, focus, callbacks) |

## Requirements

- Rust (stable)
- `no_std` + `alloc` (needs a global allocator)
- `embedded-graphics 0.8`

## License

BSD 3-Clause. See [LICENSE](LICENSE).

---

*Part of the [ThistleOS](https://github.com/wan0net/thistle-os) project — a portable OS for ESP32 devices.*
