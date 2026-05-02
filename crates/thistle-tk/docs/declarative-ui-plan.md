# Declarative UI Compiler Plan

## Goal

Add a host-side UI compiler for `thistle-tk` so app developers can describe screens with a small HTML/CSS-like format, while devices continue to run ordinary compiled Rust and the existing `thistle-tk` widget/runtime code.

The compiler must never ship to ESP32 devices. It runs only during app or OS builds, parses markup and simple CSS on the host, and emits Rust source that constructs a `thistle-tk` widget tree or calls the ThistleOS `thistle_ui_*` facade.

## Placement

The reusable compiler should live with `thistle-tk`, not in the ThistleOS kernel tree.

Recommended split:

- `crates/thistle-tk`: keep as the `no_std` runtime widget, layout, input, and rendering crate.
- `crates/thistle-tk/compiler` or `crates/thistle-tk/tools/thistle-tk-ui`: add a `std` host-only compiler crate.
- ThistleOS app build integration: add thin `build.rs` helpers or app SDK templates in the main `thistle-os` repo that invoke the compiler and include the generated Rust.

Reasoning:

- The markup and CSS subset are a frontend for `thistle-tk`, so they belong with the toolkit semantics.
- Keeping the compiler as a separate `std` crate prevents parser dependencies from entering the device runtime dependency graph.
- ThistleOS can supply OS-specific bindings, callback naming conventions, and app templates without owning the core language.

## Non-goals

- Do not embed an HTML parser, CSS parser, selector engine, DOM, or style resolver in firmware.
- Do not implement browser layout.
- Do not promise full HTML or CSS compatibility.
- Do not support JavaScript.
- Do not make app behavior declarative in v1. App logic remains native Rust.

## Authoring Model

An app may provide:

```text
ui/weather.ui.xml
ui/weather.css
src/main.rs
build.rs
```

Example markup:

```xml
<screen class="screen">
  <row class="header">
    <label text="Weather Station" class="title"/>
  </row>

  <column id="content" class="content">
    <label id="temperature" text="Temperature: -- C"/>
    <divider/>
    <label id="pressure" text="Pressure: -- hPa"/>
    <button id="refresh" text="Refresh" class="primary" on-press="refresh"/>
  </column>
</screen>
```

Example CSS:

```css
.screen {
  layout: column;
  background: theme(bg);
}

.header {
  layout: row;
  height: 30px;
  padding: 0 8px;
  align: start center;
  background: theme(surface);
  border-bottom-width: 1px;
  border-color: theme(text-secondary);
}

.content {
  layout: column;
  flex-grow: 1;
  gap: 4px;
  scrollable: true;
  background: theme(bg);
}

.title {
  font-size: large;
  color: theme(text);
}

.primary {
  height: 26px;
  padding: 2px 10px;
  radius: 4px;
  background: theme(primary);
  color: theme(bg);
}
```

Generated Rust should expose stable widget handles for nodes with `id` attributes:

```rust
pub struct WeatherUi {
    pub root: WidgetId,
    pub content: WidgetId,
    pub temperature: WidgetId,
    pub pressure: WidgetId,
    pub refresh: WidgetId,
}

pub fn build(ui: &mut UiBuilder) -> WeatherUi {
    // generated widget creation and style application
}
```

The app owns behavior:

```rust
fn refresh(ui: &mut WeatherUi) {
    ui.set_text(ui.temperature, "Temperature: N/A");
}
```

## Input Language

Use strict XML-like markup for v1, even if docs describe it as HTML-like. Strict XML makes diagnostics and code generation simpler and avoids browser parsing edge cases.

Initial tags:

- `screen`: root container for a screen
- `column`: container with column layout
- `row`: container with row layout
- `label`: text label
- `button`: pressable button
- `text-input`: editable single-line text field
- `divider`: visual separator
- `spacer`: flex spacer
- `list-item`: semantic row with title/subtitle
- `progress`: progress bar

Initial attributes:

- `id`: exported handle name
- `class`: whitespace-separated classes
- `text`: static text
- `placeholder`: text input placeholder
- `value`: progress value
- `on-press`: generated callback hook name
- `visible`: initial visibility

Optional v2 attributes:

- `repeat`: repeat a node for an app-provided data collection
- `if`: conditionally create a node
- `bind-text`: bind label text to an app-owned field

## CSS Subset

Keep the supported CSS deliberately small and map every property directly to `thistle-tk` concepts.

Selectors in v1:

- tag selectors: `button`
- class selectors: `.primary`
- ID selectors: `#refresh`
- combined tag/class selectors: `button.primary`

Avoid descendant selectors, pseudo-classes, specificity surprises, media queries, and custom property inheritance in v1. Add them only if real app ports need them.

Properties in v1:

- Layout: `layout`, `align`, `gap`, `flex-grow`, `scrollable`
- Size: `width`, `height`, `min-width`, `min-height` if the layout engine supports them
- Spacing: `padding`, `padding-top`, `padding-right`, `padding-bottom`, `padding-left`
- Color: `background`, `color`, `border-color`
- Border: `border-width`, `border-bottom-width`, `radius`
- Text: `font-size`, `max-lines`, `word-wrap`
- Visibility: `display: none` as initial hidden state

Supported units:

- `px`
- `%`
- unitless numbers where the property naturally expects a number
- semantic values such as `auto`, `true`, `false`, `small`, `normal`, `large`
- theme colors via `theme(primary)`, `theme(bg)`, `theme(surface)`, `theme(text)`, `theme(text-secondary)`
- explicit colors via `#rrggbb`

Unsupported CSS should produce clear errors by default. A permissive warning mode can be added later for porting experiments.

## Compiler Architecture

Host-only pipeline:

```text
Markup file -> UI AST
CSS file -> style rules
AST + style rules -> styled widget tree
styled widget tree -> Rust source
Rust source -> normal app build
```

Recommended crates:

- `roxmltree` for strict XML-like markup parsing.
- `cssparser` for a small custom CSS subset.
- `proc-macro2` and `quote` for robust Rust source generation, or plain string generation for the first prototype.
- `miette` or `ariadne` later for nicer file/line diagnostics.

Do not depend on these crates from `thistle-tk` runtime. Put them in the compiler crate only.

## Generated Code Strategy

Prefer source generation for v1.

Advantages:

- App developers can inspect generated Rust.
- The generated code uses the same APIs as handwritten Rust.
- No device-side bytecode interpreter or UI loader is needed.
- Compile errors point back to generated Rust while compiler diagnostics point back to the original markup/CSS.

The compiler should support two emission targets:

- `direct-tree`: generate code against `thistle_tk::{UiTree, Widget, ...}` for pure toolkit tests and non-ThistleOS consumers.
- `thistle-os`: generate code against a small Rust facade over `thistle_ui_*` for loadable apps.

The `thistle-os` facade should be added before large-scale migration so generated code does not need to call raw `extern "C"` APIs directly.

## Build Integration

V1 app integration:

```rust
// build.rs
fn main() {
    thistle_tk_ui_build::compile_ui("ui/weather.ui.xml", "ui/weather.css", "weather_ui.rs");
}
```

```rust
// src/main.rs
mod generated {
    include!(concat!(env!("OUT_DIR"), "/weather_ui.rs"));
}
```

The build helper should:

- Re-run when markup or CSS files change.
- Fail the build on invalid markup, unknown tags, invalid selectors, or unsupported CSS properties.
- Emit generated Rust into `OUT_DIR`.
- Optionally write a pretty copy under `target/generated-ui/` for debugging.

## Migration Strategy

1. Build a launcher-only proof of concept.
   - Convert the Rust `tk_launcher` layout into markup and CSS.
   - Generate equivalent Rust and compare simulator output.

2. Add the Rust UI facade.
   - Wrap `thistle_ui_*` calls in safe-ish Rust builder methods.
   - Keep handles as `WidgetId` or a transparent newtype.
   - Preserve direct hand-written Rust UI as a first-class option.

3. Add a small app template.
   - Demonstrate `build.rs`, generated handle structs, callback stubs, and dynamic `set_text`.

4. Port one simple LVGL app.
   - Good candidates: flashlight or weather.
   - Keep app behavior in Rust.
   - Use this to validate tags, CSS properties, and generated handle ergonomics.

5. Port larger apps incrementally.
   - Split declarative static layout from imperative dynamic list population.
   - Add `repeat` only when at least two real ports need it.
   - Avoid recreating LVGL-specific styling details that do not serve the new toolkit.

## Verification

Compiler tests:

- Parse valid markup.
- Reject malformed markup with useful file/line diagnostics.
- Parse and normalize the supported CSS subset.
- Reject unsupported CSS properties by default.
- Verify selector precedence for tag, class, id, and tag/class selectors.
- Snapshot generated Rust for small fixtures.

Toolkit tests:

- Build generated `direct-tree` output and assert expected widget tree shape.
- Assert layout-relevant properties on generated widgets.

ThistleOS tests:

- Build the simulator with the generated launcher.
- Launch the app and compare key log assertions.
- Add screenshot or framebuffer smoke tests once simulator visual assertions are in place.

## Open Questions

- Should the public format be called `.ui.xml`, `.thistle.xml`, or `.tui`?
- Should generated code target `UiTree` directly for built-in Rust apps, or always go through the ThistleOS widget facade?
- How much selector specificity is worth supporting before it becomes surprising?
- Should app templates include generated callback trait impls, free functions, or an explicit callback table?
- Do we want a `style` attribute for tiny overrides, or should all styling live in CSS files?

## Proposed First Milestone

Implement a host-only compiler crate that can parse:

- `screen`, `row`, `column`, `label`, `button`
- `id`, `class`, `text`, `on-press`
- tag, class, id, and tag/class selectors
- `layout`, `width`, `height`, `flex-grow`, `gap`, `padding`, `background`, `color`, `font-size`, `radius`, `scrollable`

Use it to regenerate the current `tk_launcher` UI shape. Stop there before adding repeaters, text inputs, or richer CSS.
