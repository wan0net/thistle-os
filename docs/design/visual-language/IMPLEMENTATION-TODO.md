# Phone-style launcher implementation TODO

Status: implementation in progress
Visual source: [ThistleOS visual language](README.md)
Concepts: [portrait](phone-home-portrait-concept.png) and
[320 x 240 landscape](phone-home-landscape-concept.png)
Last updated: 2026-08-28

## Implementation checkpoint — 2026-08-28

Implemented and host/simulator verified in this pass:

- `thistle-tk` now has semantic vector launcher icons, icon-and-label tiles,
  working border widths, spatial arrow-key focus, hidden-view focus exclusion,
  and strict-monochrome icon coverage.
- Both launchers now use an intentionally empty Home canvas beneath the system
  bar. A persistent bottom dock contains installed favourites plus a centred
  Apps button; pressing Apps opens the full app grid. The redundant
  THISTLE/date/large-clock panel and Home app grid have been removed. E-paper
  renders the same structure in strict monochrome. Escape returns from All
  Apps to Home and view changes move focus to a visible dock or grid button.
- The LVGL colour launcher now presents a responsive 4 x 2 landscape or
  three-column portrait Home grid, semantic LVGL symbols, a centred Tools
  folder over Home, a scrollable All Apps view, and a four-item dock containing
  installed apps only.
- The LCD WM loads `night-instrument.json`; the e-paper WM uses the same visual
  structure with a refresh-conscious monochrome treatment.
- Verified with 17 `thistle-tk` unit tests plus its doc test, all 1,345 kernel
  host tests, a complete simulator build, 17 simulator unit tests, and a live
  320 x 240 T-Deck simulator boot through launcher creation.

Still required before calling the full programme complete: persistent launcher
schema and Arrange mode, standalone-app icon metadata/assets, e-paper All Apps
pagination, automated framebuffer assertions, and physical e-paper/LCD
interaction and refresh verification.

## Outcome

Ship a familiar phone-style ThistleOS home screen on e-paper and colour WMs:

- compact status bar;
- clock and date;
- fixed-position app and folder grid;
- four-item favourites dock;
- centred folder panel over home;
- full all-apps view;
- touch and spatial keyboard navigation;
- persistent layout and folder membership.

The two WMs share launcher data and interaction semantics, but render native
layouts. E-paper remains strictly 1-bit and refresh-aware. LCD/AMOLED uses the
selected dark palette, flat tinted icon tiles, wallpaper mark, and short optional
transitions.

## Scope boundaries

### In scope

- T-Deck Pro 240 x 320 portrait e-paper home.
- T-Deck/CYD 320 x 240 landscape colour home.
- Waveshare 410 x 502 portrait AMOLED layout support, simulator-first until its
  board pin configuration is complete.
- Built-in and standalone app icons.
- Folders, dock favourites, all apps, focus, touch, keyboard, persistence, and
  screenshot/framebuffer tests.

### Not in the first release

- Home-screen widgets.
- Animated or live wallpaper.
- Notification badges beyond a later semantic count hook.
- Nested folders.
- Cloud launcher-layout sync.
- A desktop/taskbar or multi-window shell.
- A compressed phone grid on 128 x 64 displays. Those need a separate compact
  launcher layout after the primary targets are complete.

## Current baseline and gaps

- [x] Treat the existing uncommitted `tk_launcher.rs` and `layout.rs` work as
  the baseline; reconcile it before implementation rather than replacing it.
- [ ] Merge or otherwise resolve GitHub PR #128 so the selected thistle mark has
  one repository-owned GitHub asset before deriving embedded versions.
- [ ] Record a baseline framebuffer and interaction log for the current launcher
  on T-Deck Pro before changing visuals.
- [ ] Record a baseline simulator screenshot for the current 320 x 240 LVGL
  launcher.

Known implementation gaps:

- `components/kernel_rs/src/tk_launcher.rs` and
  `components/apps_builtin/launcher/launcher_ui.c` use text abbreviations instead
  of icon assets.
- The lightweight `app_manifest_t` / Rust `CAppManifest` exposes no icon field,
  although the full standalone manifest already has `icon[64]`.
- `thistle-tk::ImageWidget` exists, but the display-server WM ABI has no image or
  semantic-icon creation function.
- `tk_wm_widget_set_border_width()` is a no-op even though `CommonProps` and the
  renderer support borders.
- `tk_wm_widget_set_scrollable()` is a no-op, so a production all-apps view
  cannot rely on the current hook.
- The generic C callback adapter in `tk_wm_widget_on_event()` remains incomplete;
  the Rust launcher currently wires Rust button callbacks directly.
- The colour launcher has a drawer and dock, but not the selected clock + home
  grid + folders model.
- App registration is capped at 20 entries and ELF registration has separate
  capacity constraints. Pagination and capacity behaviour must be explicit.

## Recommended product defaults

These defaults remove avoidable ambiguity from the first implementation:

- [ ] One curated home page on first boot; do not place every installed app on
  home automatically.
- [ ] A protected `Apps` tile occupies the last home-grid slot. It can move in
  Arrange mode but cannot be deleted.
- [ ] Default dock contains the first installed matches from: Messages,
  Navigator/Maps, Notes/Reader, and Files. Missing apps leave an explicit empty
  slot rather than silently substituting an unrelated app.
- [ ] All apps is paged on e-paper and vertically scrollable on colour.
- [ ] E-paper folders are limited to one six-item page in v1. Colour folders may
  page when they exceed six items.
- [ ] Layout editing uses an explicit `Arrange` mode. Drag-and-drop is deferred;
  keyboard Move and touch select-then-place must both work.
- [ ] New installations appear in All Apps and do not alter home or dock.
- [ ] Uninstalling an app leaves a recoverable empty slot and removes it from
  folders/dock only after the uninstall transaction succeeds.

## Critical path

```text
Launcher schema and app icon metadata
        |
        v
Deterministic icon pipeline and semantic WM primitives
        |
        +--------------------+
        |                    |
        v                    v
thistle-tk e-paper home   LVGL colour home
        |                    |
        +----------+---------+
                   v
      persistence, arrange mode, integration tests
                   |
                   v
       simulator and physical-device verification
```

## WP0 — Lock the visual assets and measurements

Owner scope: design assets and build tooling. No launcher behaviour changes.

- [ ] Redraw the selected generated thistle mark as a deterministic SVG in
  `docs/branding/`; keep the transparent PNG as a visual reference and GitHub
  asset, not the source for firmware rasterisation.
- [ ] Decide whether the new mark replaces `docs/thistle-logo.svg` across the
  documentation site or remains the launcher/GitHub mark. Do not silently mix
  two canonical logos.
- [ ] Define an icon construction sheet:
  - 24 x 24 logical glyph grid;
  - 2 px reference stroke at 44 px output;
  - square terminals;
  - 2 px optical safe area;
  - filled fallback for details that disappear at 1-bit resolution.
- [ ] Create initial source SVGs for `apps`, `folder`, `messages`, `maps`,
  `radio`, `notes`, `files`, `settings`, `terminal`, `assistant`, `store`,
  `reader`, and unknown-app fallback.
- [ ] Select repo-compatible fonts and record licence, glyph coverage, flash
  size, and rendered metrics for 10, 12, 14, 18, 32, and 40 px targets.
- [ ] Add native-resolution static layout proofs for:
  - 240 x 320 portrait, 3 x 3 grid;
  - 320 x 240 landscape, 4 x 2 grid;
  - 410 x 502 portrait, 3 x 3 grid.
- [ ] Verify every label in the real built-in/catalog inventory at maximum and
  minimum lengths; specify ellipsis and two-line-label policy.

Acceptance:

- [ ] Every built-in app has a reviewed source glyph or documented fallback.
- [ ] Generated 1-bit icons remain identifiable at 32, 40, and 44 px.
- [ ] RGB565 previews retain the selected violet/blue/graphite separation.
- [ ] Font and icon generation is deterministic from checked-in sources.

## WP1 — Define the launcher data model and storage

Owner scope: new launcher-model module shared by both launcher implementations.

Proposed files:

- `components/kernel_rs/src/launcher_model.rs`
- `components/kernel_rs/src/launcher_store.rs`
- `components/kernel_rs/tests/launcher_model_tests.rs` or in-module tests
- `sdcard_layout/config/launcher.json`
- `simulator/sdcard/config/launcher.json`

- [ ] Define a versioned schema with:
  - `schema_version`;
  - ordered home pages and slots;
  - `App`, `Folder`, `Apps`, and `Empty` slot types;
  - stable folder IDs, names, and ordered contents;
  - exactly four dock slots;
  - optional wallpaper ID, not an arbitrary unchecked path;
  - per-layout positions only if one logical order cannot reflow safely.
- [ ] Use app IDs as durable references; never persist raw widget IDs, pointers,
  display coordinates, or app-list indices.
- [ ] Define deterministic first-boot defaults from the recommended product
  defaults above.
- [ ] Reconcile stored references against installed apps without rearranging
  surviving entries.
- [ ] Make writes restart-safe: validate to a temporary file, flush, atomically
  replace, and retain one last-known-good copy.
- [ ] Reject oversized files, excessive pages/folders, duplicate reserved slots,
  invalid UTF-8, traversal-like wallpaper/icon IDs, and unsupported schema
  versions.
- [ ] Add a pure migration entry point before schema v2 is needed.
- [ ] Expose read-only snapshots to both launchers so rendering never holds the
  storage lock.

Acceptance:

- [ ] Corrupt, truncated, missing, and future-version configs fall back safely.
- [ ] Reboot preserves pages, folders, dock, and empty slots exactly.
- [ ] Install/uninstall reconciliation is deterministic and unit-tested.
- [ ] Failed writes leave the previous valid layout recoverable.

## WP2 — Carry icon metadata through app registration

Owner scope: manifest/app-manager ABI. This is a pre-stable ABI change and must
be made deliberately across C, Rust, built-in apps, and ELF registration.

Affected areas:

- `components/kernel/include/thistle/app_manager.h`
- `components/kernel_rs/src/app_manager.rs`
- `components/kernel/include/thistle/manifest.h`
- manifest parsers and ELF registration paths
- built-in `app_manifest_t` declarations

- [ ] Add a stable icon identifier or path to lightweight `app_manifest_t` and
  Rust `CAppManifest`; document ownership and lifetime.
- [ ] Prefer semantic icon IDs for built-ins and a validated adjacent asset name
  for standalone apps.
- [ ] Preserve the existing full-manifest `icon[64]` field through registration
  rather than discarding it.
- [ ] Validate external icon names as filenames, not paths: no `/`, `..`, NUL,
  absolute path, alternate separator, or overlong value.
- [ ] Define maximum decoded dimensions and byte size before loading an external
  icon.
- [ ] Specify fallback order: declared icon -> built-in app-ID mapping -> unknown
  glyph. Missing icons must never prevent app launch.
- [ ] Update ABI size/offset assertions and add C/Rust layout parity tests.

Acceptance:

- [ ] Built-in and standalone apps expose the same launcher icon lookup API.
- [ ] Invalid or missing external icons render the fallback without crashing.
- [ ] App-list enumeration retains ID, display name, and icon metadata with
  stable lifetime.

## WP3 — Build the deterministic icon pipeline

Owner scope: source artwork and generated embedded assets.

Proposed layout:

```text
assets/icons/src/*.svg              canonical glyphs
assets/icons/generated/mono1/*.rs   1-bit packed masks
assets/icons/generated/lvgl/*       LVGL alpha-mask descriptors
assets/icons/generated/preview/*    review-only PNG sheets
tools/build_icons.*                 deterministic generator
```

- [ ] Choose one permissively licensed renderer/generator compatible with the
  BSD-3-Clause/no-GPL policy and pin its version.
- [ ] Generate MSB-first 1-bit masks matching `thistle-tk::ImageWidget`.
- [ ] Generate colour-independent LVGL alpha masks so the WM can tint icons by
  theme/state instead of storing many coloured copies.
- [ ] Generate miniature four-glyph folder previews from the same sources.
- [ ] Generate the low-contrast wallpaper mark separately from app icons.
- [ ] Fail generation on non-square view boxes, out-of-bounds geometry,
  unexpected colours, output-size drift, or duplicate IDs.
- [ ] Add a check mode to CI that proves checked-in generated files are current.
- [ ] Record flash/rodata cost for the initial icon pack.

Acceptance:

- [ ] Re-running the generator produces a clean Git diff.
- [ ] Mono masks and LVGL masks share icon IDs and visual geometry.
- [ ] The full initial icon pack fits the agreed firmware/storage budget.

## WP4 — Add semantic icon primitives to the WM boundary

Owner scope: display-server vtable, shims, `thistle-tk`, and LVGL implementations.

Do not make the launcher manually paint pixels through display HAL calls.

- [ ] Add a versioned `thistle_icon_desc_t` or equivalent semantic icon API to
  `display_server_wm_t`, including:
  - stable icon ID or validated mask reference;
  - logical width/height;
  - tint role (`primary`, `secondary`, `text`, `focus`, `status`);
  - optional accessible/fallback label;
  - explicit data ownership/lifetime.
- [ ] Add `widget_create_icon()` and necessary setters to:
  - `components/kernel/include/thistle/display_server.h`;
  - `components/ui/src/widget_shims.c`;
  - `components/ui/src/lvgl_wm.c`;
  - `components/ui/src/lvgl_wm_lcd.c`;
  - `components/ui/src/lvgl_wm_epaper.c` if retained;
  - `components/kernel_rs/src/tk_wm.rs`.
- [x] Support an icon and label inside one focusable tile without duplicating the
  press target.
- [x] Implement `tk_wm_widget_set_border_width()` using
  `CommonProps.border_width` and dirty the exact widget.
- [ ] Decide whether radius lives only in `CommonProps`; remove the current
  button-only mismatch if possible.
- [ ] Complete the C event callback adapter or explicitly keep built-in launchers
  on typed native callbacks with a documented boundary.
- [x] Add hit testing for the complete tile, not only the glyph.
- [ ] Add visible `focused`, `pressed`, `disabled`, and `selected` states in both
  WMs.

Acceptance:

- [ ] The same icon ID renders as a 1-bit mask in `thistle-tk` and a tinted alpha
  mask in LVGL.
- [ ] Focus, pressed, and disabled states are visually distinct without changing
  launcher code by display type.
- [ ] All new vtable entries are populated or explicitly capability-gated.
- [ ] Old/mismatched external WMs fail version checks cleanly rather than calling
  the wrong vtable offsets.

## WP5 — Implement reusable launcher controller logic

Owner scope: shared state transitions; no display-specific geometry.

- [ ] Define launcher states: `Home`, `FolderOpen(folder_id)`, `AllApps(page)`,
  and `Arrange`.
- [ ] Replace the single pending action slot with a small bounded action queue or
  prove that coalescing is correct for rapid input.
- [ ] Define transitions for tap/click, Enter, Escape/Back, arrows, page keys,
  and function keys.
- [x] Close a folder before launching an app.
- [ ] Return to the same home page and focus slot when an app pauses/resumes.
- [ ] Handle launch failure without losing the folder/home state and show one
  bounded error message.
- [ ] Make the `Apps` tile protected and recoverable.
- [x] Map spatial keyboard movement with deterministic edge behaviour:
  clamp, wrap, or change page—choose and test one rule.
- [ ] Expose render-neutral view models to both launcher implementations.

Acceptance:

- [ ] State-transition tests cover every input in every launcher state.
- [ ] Rapid double activation cannot launch two apps or corrupt state.
- [ ] App failure, uninstall, and resume preserve a valid focus target.

## WP6 — Implement the T-Deck Pro e-paper home

Primary files:

- `components/kernel_rs/src/tk_launcher.rs`
- `components/kernel_rs/src/tk_wm.rs`
- `crates/thistle-tk/src/{widget,layout,render,input,tree}.rs`
- `main/main.c` only where render/refresh scheduling genuinely changes

- [x] Build the 240 x 320 layout:
  - compact split identity/clock header matching the LCD hierarchy;
  - 3 x 3 grid;
  - four-item dock;
  - no wallpaper by default.
- [x] Replace text abbreviations with generated 1-bit icons and real labels.
- [x] Implement app tile, folder tile with four mini-glyphs, `Apps` tile, empty
  slot, and dock tile.
- [x] Implement centred opaque folder panel with 2 x 3 contents, title, and close
  affordance.
- [ ] Implement paged All Apps without depending on the current scrolling no-op.
- [x] Invert only the focused tile; ensure glyph and label remain legible.
- [x] Preserve minute-only clock updates in `tk_launcher_tick()`.
- [ ] Dirty the old and new focus tiles rather than the complete screen where the
  compositor/driver path permits it.
- [ ] Make a full refresh an explicit page/folder policy decision, not an
  accidental result of reconstructing the full tree.
- [ ] Restore focus and home page after app resume without forcing a needless
  full refresh.

Acceptance:

- [x] Home, folder, all apps, and dock match the approved structure at native
  resolution.
- [ ] An unchanged minute produces no refresh request.
- [ ] One focus move updates only the required regions or documents why the
  panel requires more.
- [ ] Folder open/close and page change leave no unacceptable ghosting after the
  agreed partial/full refresh sequence.
- [x] Touch targets are at least 40 x 40 px where the layout permits.

## WP7 — Implement colour homes in LVGL

Primary files:

- `components/apps_builtin/launcher/launcher_ui.c`
- `components/ui/src/lvgl_wm.c`
- `components/ui/src/lvgl_wm_lcd.c`
- `components/ui/src/theme.c`
- `components/ui/include/ui/theme.h`

- [ ] Refactor the current launcher into reusable `launcher_controller` and
  LVGL view code instead of adding folders to the existing global-state block.
- [x] Implement responsive breakpoints:
  - 320 x 240: 4 x 2 grid, compact centred clock, four-item dock;
  - 410 x 502: 3 x 3 grid, larger clock, four-item dock.
- [x] Use the Night Instrument theme tokens already added to the SD/simulator
  layouts, extending the theme struct for accent, focus, healthy, error, and
  wallpaper tokens rather than using raw colours in launcher code.
- [ ] Add the faint new thistle mark as a tintable wallpaper asset with a hard
  maximum contrast behind labels.
- [ ] Replace letter icons with generated tintable icon masks.
- [ ] Implement folder tiles, centred 3 x 2 folder panel, close/outside-tap
  behaviour, and page indicator.
- [ ] Implement vertical All Apps scrolling with keyboard focus auto-scroll.
- [ ] Add optional `F1`–`F4` dock hints only when the board/input capabilities
  report corresponding keys.
- [ ] Use 4–6 px radii, flat fills, 1 px keylines, no gradients/blur/shadows.
- [ ] Keep animations optional, short, and disabled in screenshot tests.

Acceptance:

- [x] Both colour breakpoints match the approved phone mental model.
- [ ] RGB565 captures retain readable labels and unmistakable focus.
- [ ] Keyboard navigation and touch activate the same tiles.
- [ ] Disabling animations does not hide state or break layout.

## WP8 — Implement Arrange mode and persistence integration

- [ ] Enter Arrange mode through a long press, menu action, or keyboard shortcut;
  document the chosen discoverable path.
- [ ] Touch flow: select an item, then select destination. Avoid drag-only
  behaviour on e-paper.
- [ ] Keyboard flow: arrows choose destination; Enter commits; Escape cancels.
- [ ] Support move app, move folder, swap slots, create folder, rename folder,
  add/remove folder entry, and set/clear dock favourite.
- [ ] Protect launcher, `Apps`, and required recovery/system actions.
- [ ] Persist only after a complete valid operation; cancelled operations do not
  write.
- [ ] Show explicit feedback for a full folder, full page, missing app, or failed
  write.
- [ ] Add migration-safe reset-to-default that does not remove installed apps.

Acceptance:

- [ ] Every Arrange action works with touch and keyboard.
- [ ] Power loss during a layout save preserves either the old or new valid
  layout, never a partial file.
- [ ] Reset restores curated defaults while All Apps still shows installed apps.

## WP9 — Automated verification

### Unit tests

- [ ] Launcher schema parse, validation, migration, and atomic-save recovery.
- [ ] Default layout generation with missing and extra apps.
- [ ] Folder capacity, duplicate handling, protected `Apps` tile, and dock size.
- [ ] Controller transition table for touch, keyboard, failure, pause, and resume.
- [ ] Icon metadata validation and fallback.
- [ ] Icon generator determinism and output bounds.
- [ ] `thistle-tk` icon rendering, border width, hit testing, layout, and dirty
  rectangle calculation.

### Visual/framebuffer tests

- [ ] Add deterministic captures for each supported layout:
  - home idle;
  - every focus state;
  - folder open;
  - all apps first and later page;
  - Arrange mode;
  - missing icon;
  - long/truncated label;
  - app launch error;
  - corrupt config fallback.
- [ ] Compare structural regions or approved golden frames at native resolution.
- [ ] Test both light/1-bit e-paper and RGB565 colour conversion.
- [ ] Assert that e-paper output contains only two pixel values.

### Host/simulator checks

- [x] Run `cargo test --manifest-path crates/thistle-tk/Cargo.toml`.
- [x] Run the kernel host suite with its explicit manifest and single-threaded
  setting where required.
- [ ] Run simulator unit and integration suites across T-Deck Pro, T-Deck, CYD,
  and AMOLED model where available.
- [ ] Add a simulator screenshot/export facility if the current SDL path cannot
  produce deterministic frames in CI.
- [ ] Run `cargo fmt` with explicit manifests; do not run it at repository root.

Acceptance:

- [ ] CI fails on stale generated icon assets or changed golden frames.
- [ ] Golden updates require an explicit review command, not automatic rewrite.
- [ ] Tests distinguish render success from refresh behaviour.

## WP10 — Physical-device verification

Static tests and simulator captures do not complete this work.

### T-Deck Pro e-paper

- [ ] Flash from an isolated build using the known-good quad-PSRAM defaults.
- [ ] Verify boot home and record serial/render logs.
- [ ] Move focus at least 20 times across grid, dock, and boundaries.
- [ ] Open and close a folder through touch and keyboard.
- [ ] Change All Apps pages in both directions.
- [ ] Launch each available app and return home; verify focus/page restoration.
- [ ] Observe a real minute transition and prove unchanged minutes do not refresh.
- [ ] Exercise Arrange mode and reboot to prove persistence.
- [ ] Inspect ghosting after repeated focus, folder, and page operations.
- [ ] Record which actions use partial versus full refresh and their latency.

### T-Deck/CYD colour LCD

- [ ] Verify RGB565 palette, label contrast, icon tint, and focus edge.
- [ ] Verify touch coordinates and spatial keyboard navigation.
- [ ] Verify folder open/close and optional animation at real frame rate.
- [ ] Verify function-key dock shortcuts only appear and work on capable boards.

### AMOLED

- [ ] Complete board pin configuration before claiming hardware support.
- [ ] Verify portrait breakpoint, touch, contrast, and static-chrome burn-in
  policy on hardware.

Acceptance:

- [ ] Required physical actions have captured evidence, not inferred results.
- [ ] A skipped minute-transition or interaction refresh gate is `INCOMPLETE`,
  not `PASS`.
- [ ] Any device-specific deviation is recorded as a layout capability, not an
  app-level hardware check.

## Suggested independently scoped tickets

These are proposed ticket boundaries, not evidence that GitHub issues already
exist. Check for duplicates before filing.

1. **Define versioned launcher layout schema and restart-safe storage**
   - WP1 only; no UI.
2. **Carry icon metadata through app-manager registration ABI**
   - WP2 only; include ABI tests and external-name validation.
3. **Create deterministic ThistleOS icon asset pipeline**
   - WP0/WP3 source glyphs, generated masks, and CI check.
4. **Add semantic icon widgets to all WM implementations**
   - WP4 vtable, shims, `thistle-tk`, LVGL, and version gating.
5. **Implement shared launcher controller state machine**
   - WP5, pure state tests, no final layout.
6. **Build 240 x 320 phone-style e-paper home in thistle-tk**
   - WP6 including folders, dock, all apps, and framebuffer tests.
7. **Build responsive phone-style colour home in LVGL**
   - WP7 for 320 x 240 and 410 x 502 layouts.
8. **Add launcher Arrange mode and persistent user customisation**
   - WP8 after storage/controller foundations.
9. **Add deterministic launcher screenshot and framebuffer CI**
   - WP9 visual test tooling and goldens.
10. **Complete launcher interaction and refresh hardware V&V**
    - WP10 with separate e-paper and colour evidence checklists.

## Definition of done

- [x] The selected phone-style launcher is implemented in both primary WMs.
- [x] Real icons replace all launcher text abbreviations.
- [ ] Home, folders, dock, and all-apps content persist and recover safely.
- [ ] Touch and keyboard navigation have equivalent complete paths.
- [ ] E-paper output is strictly 1-bit and its interaction/timer refresh behaviour
  is physically verified.
- [ ] Colour layouts pass RGB565 and native-device contrast checks.
- [ ] Built-in and standalone apps share validated icon metadata and fallback.
- [ ] Unit, integration, simulator, framebuffer, and required hardware gates pass.
- [x] Documentation distinguishes implemented, simulator-verified, and
  hardware-verified support per display family.
