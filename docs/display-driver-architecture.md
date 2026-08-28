# Rust display stack based on `decaday/display-driver`

Status: accepted direction; implementation pending

Last reviewed: 2026-08-02

This decision record captures the intended replacement of ThistleOS's current
display HAL with a Rust-first display stack built around
[`decaday/display-driver`](https://github.com/decaday/display-driver). It is a
design target, not a description of the currently implemented system.

The wider decision history and the device-by-device Rust reuse audit are in
[`architecture-conversation-record.md`](architecture-conversation-record.md)
and [`rust-driver-inventory.md`](rust-driver-inventory.md).

## Decision

ThistleOS will use `display-driver` as the primary Rust framework for colour LCD
and AMOLED drivers. We will replace the current colour-display HAL rather than
preserve it as a compatibility layer.

The framework will be compiled into each standalone display `.drv.elf`, along
with the selected panel implementation. A small, versioned, C-compatible
Thistle display ABI will remain at the dynamic ELF boundary because Rust traits,
Rust layouts, and async futures are not a stable ABI between independently
compiled binaries.

This is not a decision to force all display technologies through one model:

- colour LCD and AMOLED panels use `display-driver`;
- e-paper retains a separate staged-update and explicit-refresh interface;
- monochrome buffered displays such as SSD1306 remain separate initially and
  may gain their own small interface later.

The existing `hal_display_driver_t` API is not considered stable and need not be
kept source- or binary-compatible.

## Why change the current interface

The existing interface was useful for bringing up several panels, but it hides
information that a safe standalone driver needs:

```c
esp_err_t (*flush)(const hal_area_t *area, const uint8_t *color_data);
```

The call does not state:

- the buffer length;
- pixel format or byte order;
- row stride;
- whether the source buffer remains valid after the call;
- whether transfer completion is synchronous or asynchronous;
- orientation and coordinate-space rules;
- whether the operation is a complete frame, partial update, or continuation;
- alignment requirements imposed by the panel or transport.

It also combines immediate-mode colour panels and e-paper panels even though
their commit and refresh lifecycles are materially different. Preserving this
interface would make the new framework conform to old assumptions and would
discard much of the value of adopting it.

## Why `display-driver`

The project is a good match for the intended Rust driver model:

- `no_std`, async-native Rust;
- Apache-2.0 licensed;
- separates panel-controller behaviour from the physical bus;
- uses atomic command-plus-parameter operations;
- represents update regions and frame metadata explicitly;
- supports partial-region writes without requiring a full framebuffer;
- has optional hardware-fill, readback, reset, orientation, brightness, and
  other capability interfaces;
- currently includes CO5300, ST7789, ST7735, and GC9A01 panel crates;
- includes a reusable `mipidcs` layer that should reduce the work required to
  add other MIPI DCS panels such as ILI9341.

The CO5300 crate includes the 410 x 502 `AM196Q410502LK_196` panel specification,
making it directly relevant to the T-Watch Ultra work.

### Maturity caveat

As reviewed on 2026-08-02, the project is young:

- the crates are at `0.1.x`;
- the repository has no tagged release;
- no automated tests were found in the repository;
- its hardware examples target RP2040 and STM32H7, not ESP32-S3;
- an ESP32-S3 example is still an
  [open request](https://github.com/decaday/display-driver/issues/8);
- complete QSPI transport support is still
  [open work](https://github.com/decaday/display-driver/issues/1);
- the generic panel lifecycle does not yet cover all sleep, wake, display-power,
  and tearing-effect operations ThistleOS will need.

We should therefore pin a reviewed Git revision or exact crate versions. We can
upstream generally useful additions, while retaining a small Thistle adapter or
temporary fork when required. The snapshot above must be rechecked before
implementation because the upstream project is actively changing.

## Target architecture

```text
Kernel display server / compositor
               |
               | versioned Thistle display ABI
               v
Signed standalone display .drv.elf
  +-----------------------------------------------+
  | Thistle ABI entry points                      |
  | display-driver core                          |
  | selected panel Spec (CO5300, ST7789, ...)     |
  | Thistle DisplayBus transport adapter          |
  +-----------------------------------------------+
               |
               | versioned bus syscalls
               v
Kernel-owned SPI/QSPI/DMA service
               |
               v
Physical panel
```

The kernel and display server know about frames, regions, formats, capabilities,
and completion. They do not know the panel controller's command set. The loaded
driver knows the controller and compiles the upstream panel crate into its own
ELF, but it does not directly take ownership of shared ESP32 peripherals.

### Why the framework lives inside each driver ELF

`display-driver` exposes Rust traits such as `DisplayBus` and `Panel`. Passing
those trait objects, or the futures produced by their async methods, across the
dynamic loader boundary would couple the kernel and driver to Rust compiler
implementation details. Updating the toolchain or compiling the artifacts with
different settings could then break the ABI.

Instead, the Rust types remain private to the driver ELF. Only explicitly
versioned `#[repr(C)]` values, integer handles, byte slices expressed as
pointer-plus-length, and C-callable functions cross the boundary.

## Proposed dynamic display ABI

The exact names and layouts are still subject to an implementation spike. The
ABI must carry at least the following concepts.

```rust
#[repr(C)]
pub struct DisplayArea {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[repr(C)]
pub struct DisplayFrame {
    pub area: DisplayArea,
    pub data: *const u8,
    pub len: usize,
    pub stride_bytes: u32,
    pub pixel_format: u32,
    pub flags: u32,
}
```

The final function table should be versioned and contain operations equivalent
to:

- query ABI version and driver metadata;
- initialize and deinitialize;
- query geometry, native orientation, alignment, and supported pixel formats;
- submit a complete or partial frame;
- learn when an asynchronous transfer has completed;
- cancel or drain outstanding work during shutdown;
- set orientation where supported;
- enter and leave sleep;
- turn the panel on and off;
- set brightness where the panel owns brightness;
- perform hardware fill or readback where advertised;
- expose tearing-effect synchronization where available;
- report structured driver errors and diagnostics.

Every optional operation is gated by an advertised capability bit. Unsupported
operations return a defined `NOT_SUPPORTED` result rather than relying on null
function pointers or undocumented behaviour.

### Transfer completion

The upstream framework is async, while a dynamic C ABI cannot expose a Rust
future. The implementation spike should compare two safe ABI patterns:

1. `submit_frame` returns a transfer token; the kernel polls or waits for that
   token through another ABI call.
2. `submit_frame` accepts a kernel-provided completion callback and opaque
   context pointer.

The choice must define callback task context, ISR restrictions, buffer lifetime,
ordering, cancellation, timeout, and unload behaviour. A driver ELF must not be
unloaded while it owns an in-flight DMA transaction or while the kernel can
still call one of its completion functions.

### Pixel and area rules

The new ABI uses origin-plus-size rectangles. The current inclusive
`x1, y1, x2, y2` representation can be converted during migration with:

```text
x      = x1
y      = y1
width  = x2 - x1 + 1
height = y2 - y1 + 1
```

An empty rectangle is invalid. Bounds and `len` are validated before entering
the loaded driver. The expected minimum buffer length is derived using checked
arithmetic from the format, dimensions, and stride. The driver then enforces
panel-specific alignment through the upstream `Panel` specification.

RGB565 byte order must be explicit rather than inferred. Future formats may
include RGB666, RGB888, indexed colour, and monochrome formats, but a driver
advertises only formats it can consume safely.

## ESP32 transport model

The upstream `display-driver-spi` crate uses `embedded-hal-async` SPI plus a DC
pin. Its `QspiFlashBus` formats QSPI-style command prefixes but is not itself an
ESP32 QSPI/DMA implementation.

ThistleOS therefore needs transport adapters that implement the upstream
`DisplayBus` contract over kernel-provided bus services:

- SPI with command/data GPIO support;
- QSPI command, address, and pixel phases for panels such as CO5300;
- DMA-capable writes with explicit lifetime and completion;
- optional reads for controller ID and diagnostics;
- reset and tearing-effect GPIO operations;
- bus locking and arbitration for shared peripherals.

Board configuration supplies bus identifiers and pins. It must not hand raw
ESP-IDF peripheral structures to a standalone driver. The kernel remains the
owner of buses so it can arbitrate multiple devices, validate requested modes,
and prevent one driver from reconfiguring hardware used by another.

Whether the first adapter directly implements the required embedded-hal traits
or uses a narrower Thistle-specific raw transport under `DisplayBus` is an open
implementation choice. It must not require ESP-IDF hardware calls in the
hardware-independent kernel logic.

## Panel lifecycle and neighbouring services

Not every display-related operation belongs to the panel controller:

- panel brightness may be a controller command;
- LCD backlight brightness may instead use PWM, a PMU, or an I/O expander;
- panel reset and tearing-effect signals are GPIO resources;
- panel power rails may be owned by the board's power driver;
- orientation can affect both display coordinates and touch-coordinate mapping.

The display driver advertises what it owns. Board orchestration coordinates
power, reset, brightness, display, and touch services without embedding
board-specific knowledge in the kernel. A composite board service may be needed
where one user operation spans several drivers.

Sleep and wake sequencing must be explicit. If the upstream `Panel` trait does
not provide the required lifecycle, ThistleOS should add a local extension trait
inside the driver or propose a compatible upstream extension. These extension
traits still remain private to the driver ELF; only the versioned operations
cross the ABI.

## Display-family boundaries

### Colour LCD and AMOLED

Use `display-driver` for immediate-mode colour controllers. Initial mappings:

| Thistle target | Controller | Intended implementation |
| --- | --- | --- |
| T-Watch Ultra | CO5300 | upstream CO5300 panel and new ESP32-S3 QSPI transport |
| T-Deck / T-Display family | ST7789 | upstream ST7789 panel and SPI transport |
| Existing ILI9341 boards | ILI9341 | add a panel implementation, preferably using `mipidcs` |

Other upstream-supported panels can be packaged when a board needs them, but
support is not implied merely because the panel crate exists.

### E-paper

E-paper remains separate because it needs concepts that colour-panel frame
writes do not model well:

- staged framebuffer changes followed by an explicit physical refresh;
- full, partial, and fast refresh modes;
- busy-pin waits measured in hundreds or thousands of milliseconds;
- waveform/LUT selection and temperature-dependent behaviour;
- ghosting policy and periodic full-refresh scheduling;
- possibly separate current and previous image buffers.

Common geometry and pixel-buffer validation types may be shared, but the
operational ABI should not pretend e-paper is an immediate-mode LCD.

### Monochrome OLED

SSD1306-class displays remain on the existing implementation during the colour
stack migration. Afterward we can decide whether to implement a small buffered
monochrome ABI, use another pure-Rust framework, or add suitable support to the
new shared types. It is not a prerequisite for adopting `display-driver`.

## Buffering and memory policy

The framework does not require `embedded-graphics` or a full framebuffer.
ThistleOS can submit compositor-owned regions directly. This matters on small
ESP32 targets and when PSRAM is unavailable.

A full 410 x 502 RGB565 buffer consumes 411,640 bytes before alignment and
allocator overhead. The default policy should therefore support:

- direct partial-region transfer from compositor surfaces;
- line or tile buffers in internal DMA-capable memory;
- larger framebuffers in PSRAM only when the transport can use them safely;
- copying into a bounded DMA buffer when the source memory is unsuitable;
- double buffering only when memory and latency measurements justify it.

The ABI's explicit length and stride allow these choices without changing panel
drivers. DMA source constraints must be advertised or handled inside the
transport rather than assumed by callers.

## Packaging and recovery relationship

Recovery installs an architecture-specific kernel; it does not need to contain
the display framework or a board-specific panel driver. Once running, the
kernel identifies or confirms the board, mounts the system volume, and obtains
the signed display `.drv.elf` and its manifest as part of that board's driver
generation. The complete installer and handoff design is recorded in
[`recovery-first-installation.md`](recovery-first-installation.md).

Each packaged display driver records at least:

- Thistle display ABI version range;
- CPU architecture and minimum kernel version;
- panel/controller identity and compatible board profiles;
- required SPI/QSPI/GPIO/power services;
- pixel formats and relevant capabilities;
- upstream `display-driver` revision or crate versions;
- license notices and source provenance;
- artifact digest and Ed25519 signature.

This keeps the kernel architecture-specific but board-independent and lets the
driver catalog update panel support independently of Recovery and kernel OTA.

## Migration plan

### Phase 0: upstream and ABI spike

1. Pin and vendor, or lock, a reviewed upstream revision.
2. Build a host-side fake `DisplayBus` and verify command ordering, regions,
   alignment failures, and error propagation for CO5300 and ST7789.
3. Prototype the versioned C ABI and transfer-completion model.
4. Define unload and cancellation behaviour before enabling DMA.
5. Record required upstream changes, especially lifecycle, TE, QSPI, and ESP32
   integration.

### Phase 1: ST7789 reference path

Use ST7789 as the simpler reference implementation:

1. implement the ESP32 SPI transport adapter;
2. package the driver as a standalone Rust `.drv.elf`;
3. connect it to the new display-server ABI;
4. verify full and partial updates on hardware;
5. compare output, throughput, memory use, and sleep/wake with the current
   driver.

This phase proves loading, ABI safety, and bus ownership without making QSPI a
prerequisite for the first end-to-end result.

### Phase 2: CO5300 and T-Watch Ultra

1. implement the ESP32-S3 QSPI/DMA transport;
2. integrate the upstream CO5300 panel specification;
3. implement panel power, reset, orientation, TE, sleep, and wake sequencing;
4. coordinate brightness and power ownership with the T-Watch Ultra PMU and
   expander drivers;
5. verify partial-region alignment and the 410 x 502 geometry;
6. package, sign, download, load, unload, and reload the standalone driver.

### Phase 3: remaining colour panels

1. add ILI9341 through `mipidcs` or a dedicated upstream panel crate;
2. migrate other supported colour displays where the new stack is a clear fit;
3. remove compiled-in colour-controller implementations after hardware parity;
4. update board manifests and documentation to use catalogued driver artifacts.

### Phase 4: cleanup and separate display families

1. remove the obsolete colour portions of `hal_display_driver_t` and their
   compatibility code;
2. formalize the e-paper ABI from measured requirements;
3. decide the SSD1306/monochrome direction;
4. update simulator transports and public driver-development documentation.

## Acceptance criteria

The replacement is ready to become the default colour-display stack when:

- a signed standalone ST7789 driver loads and renders correctly on at least one
  supported board;
- a signed standalone CO5300 driver renders correctly on T-Watch Ultra using
  the ESP32-S3 QSPI path;
- full-frame, clipped, partial, misaligned, empty, and out-of-bounds requests
  have defined and tested behaviour;
- malformed lengths and overflowing stride calculations are rejected before
  accessing driver memory;
- pixel byte order, orientation, and touch/display coordinate agreement are
  verified with visual test patterns;
- buffer ownership remains correct under asynchronous DMA, timeouts, errors,
  driver shutdown, and attempted unload;
- shared-bus arbitration works with another device active on the same bus;
- sleep/wake and power-cycle recovery work repeatedly;
- a driver crash or initialization failure leaves the kernel able to log the
  failure and enter a recoverable provisioning state;
- host tests validate panel command sequences without hardware;
- hardware tests measure frame time, partial-update time, CPU use, internal RAM,
  PSRAM, and DMA buffer use;
- manifests carry ABI compatibility, provenance, license, digest, and signature
  metadata;
- the simulator has a transport that exercises the same frame and completion
  semantics used by hardware drivers.

## Risks and mitigations

| Risk | Consequence | Mitigation |
| --- | --- | --- |
| Upstream `0.1.x` API changes | Frequent integration breakage | Pin reviewed versions; upgrade deliberately; keep the ELF ABI independent of upstream Rust types |
| No upstream ESP32 transport | More local work than the panel crates suggest | Implement one reusable Thistle SPI/QSPI transport and upstream generic improvements |
| Rust async model crosses ELF boundary accidentally | Toolchain-dependent ABI and unsafe lifetimes | Keep all traits/futures inside the driver; expose tokens or C callbacks only |
| DMA completes after unload | Use-after-free or jump into unloaded code | Reference-count in-flight work; drain/cancel before unload; kernel owns completion routing |
| Full framebuffer pressure | Internal RAM exhaustion or poor PSRAM/DMA behaviour | Prefer regions/tiles; declare memory requirements; measure per board |
| Panel lifecycle gaps | Failed resume, blank screen, or excess power use | Add tested extension traits and explicit ABI operations; contribute upstream where useful |
| Brightness/power spans several devices | Competing ownership and incorrect sequencing | Advertise ownership and coordinate through board services |
| One abstraction is stretched over e-paper | Leaky APIs and incorrect refresh behaviour | Keep separate operational ABIs while sharing safe value types |
| Upstream project becomes unavailable | Builds or maintenance stall | Lock source and checksums; preserve license/provenance; maintain a buildable reviewed copy |

## Alternatives considered

### Preserve the current HAL and wrap `display-driver`

Rejected as the target architecture. It minimizes initial edits but keeps an
underspecified `flush(area, pointer)` contract and makes the new framework adapt
to old assumptions. A short-lived migration shim is acceptable only while
moving individual boards; it must not become the permanent API.

### Expose upstream Rust traits directly from driver ELFs

Rejected. This would provide an elegant source-level API but no stable dynamic
ABI across compiler versions and independently built artifacts.

### Put `display-driver` in the kernel and load only panel data

Rejected for now. It would enlarge and couple the immutable kernel, constrain
driver updates to the kernel's upstream version, and make controller-specific
extensions harder to package independently.

### Build and maintain all colour panel drivers ourselves

Not preferred. It avoids upstream change risk but duplicates reusable panel
command, alignment, and transport abstractions. Thistle-specific work is better
focused on the ELF ABI, ESP32 transports, resource ownership, packaging, and
hardware verification.

### Use one universal ABI for colour, monochrome, and e-paper

Rejected. Shared value types are useful, but forcing their operational models
together produces optional operations and ambiguous refresh semantics.

## Open questions

- Should frame completion use transfer tokens, callbacks, or a kernel event
  queue?
- Does the loaded-driver SDK expose embedded-hal traits directly, or a narrower
  Thistle transport that the driver adapts to embedded-hal?
- Which operations belong in the base ABI versus versioned capability
  extensions?
- How should TE synchronization integrate with the compositor's frame pacing?
- Does orientation live solely in the compositor, solely in the panel, or in a
  coordinated display-and-input transform service?
- Which memory kinds may be submitted without copying on each ESP32 family?
- How is an in-flight transfer cancelled when a driver faults or a device is
  removed?
- Should brightness be a display capability, a power capability, or a composite
  board-level service?
- Should we consume upstream crates, a locked Git dependency, or a vendored
  source snapshot during the first implementation?
- Which upstream changes should be proposed before maintaining local extension
  traits?
- What compatibility policy will apply once the new display ABI reaches its
  first released version?

These questions do not block accepting the overall direction. Phase 0 exists to
resolve them with small prototypes and measured ESP32 behaviour before the ABI
is frozen.

## Upstream references

- Repository and overview: <https://github.com/decaday/display-driver>
- Core bus interface: <https://github.com/decaday/display-driver/blob/master/display-driver/src/bus/mod.rs>
- Core panel interface: <https://github.com/decaday/display-driver/blob/master/display-driver/src/panel/mod.rs>
- CO5300 implementation: <https://github.com/decaday/display-driver/tree/master/panels/co5300>
- QSPI support discussion: <https://github.com/decaday/display-driver/issues/1>
- ESP32-S3 example request: <https://github.com/decaday/display-driver/issues/8>
