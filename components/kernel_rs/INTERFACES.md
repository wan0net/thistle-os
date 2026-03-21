# ThistleOS Rust Kernel — Interface Documentation

This document describes the public interfaces of every module in the `kernel_rs`
crate (`components/kernel_rs/`), their C FFI equivalents, current limitations,
and areas that need improvement before the kernel can be considered production-ready.

---

## Overview

ThistleOS splits responsibility at the syscall boundary:

- **Kernelspace (Rust)** — safety-critical subsystems: manifest parsing,
  permissions, IPC, event bus, version negotiation. Compiled as a static
  library (`crate-type = ["staticlib"]`) by `cargo +esp` and linked into the
  firmware image via `CMakeLists.txt`.
- **Userspace / drivers / UI (C)** — LVGL window manager, hardware drivers,
  board definitions, ELF loader, signing. These call into the Rust kernel
  through the `rs_*` C FFI functions declared in `thistle/kernel_rs.h`.

The crate is `thistle-kernel` (version `0.1.0`). The only runtime dependency is
`log = "0.4"` (currently wired up as a no-op — see Cross-cutting concerns).
Build depends on `embuild = "0.33"` for ESP-IDF path discovery.

Modules are migrated from C incrementally. During the dual-existence period both
the C file and the Rust file can coexist; the CMakeLists.txt for each subsystem
determines which implementation is actually linked.

---

## Module: manifest

**Source:** `src/manifest.rs`, FFI glue in `src/ffi.rs`

### Purpose

Unified manifest parser covering all three loadable entity types: apps, drivers,
and firmware packages. Parses JSON without a `serde` dependency (simple string
scanning), performs OS version and architecture compatibility checks, and derives
manifest file paths from ELF paths. The intent is to replace the equivalent
logic in `components/kernel/src/manifest.c`.

### Rust API

```rust
// Parsed, owned manifest. Heap-allocated strings; not repr(C).
pub struct Manifest {
    pub manifest_type: ManifestType,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub min_os: String,
    pub arch: String,
    pub entry: String,
    pub icon: String,
    pub permissions: u32,
    pub background: bool,
    pub min_memory_kb: u32,
    pub hal_interface: String,   // driver-only
    pub changelog: String,       // firmware-only
}

pub enum ManifestType { App = 0, Driver = 1, Firmware = 2 }

pub enum ManifestError {
    NotFound,
    ParseError(String),
    Incompatible(String),
    IoError(std::io::Error),
}

impl Manifest {
    pub fn from_json(json: &str) -> Result<Self, ManifestError>;
    pub fn from_file(path: &Path) -> Result<Self, ManifestError>;
    pub fn path_from_elf(elf_path: &str) -> String;
    pub fn is_compatible(&self, current_arch: &str) -> bool;
    pub fn type_str(&self) -> &'static str;
}
```

Permission flag constants (`manifest::perm` submodule, mirrors `permissions.rs`):

```rust
pub mod perm {
    pub const RADIO:   u32 = 1 << 0;
    pub const GPS:     u32 = 1 << 1;
    pub const STORAGE: u32 = 1 << 2;
    pub const NETWORK: u32 = 1 << 3;
    pub const AUDIO:   u32 = 1 << 4;
    pub const SYSTEM:  u32 = 1 << 5;
    pub const IPC:     u32 = 1 << 6;
    pub const ALL:     u32 = 0x7F;
}
```

### C FFI

All three functions are in `src/ffi.rs`. Include `thistle/kernel_rs.h`.

```c
// Parse a manifest.json file into a C-compatible struct.
// Returns ESP_OK, ESP_ERR_NOT_FOUND, or ESP_ERR_INVALID_ARG.
// Drop-in for manifest_parse_file().
esp_err_t rs_manifest_parse_file(const char *json_path, thistle_manifest_t *out);

// Check whether a parsed manifest is compatible with the running kernel.
// Returns true if arch matches and min_os is satisfied.
// Drop-in for manifest_is_compatible().
bool rs_manifest_is_compatible(const thistle_manifest_t *manifest,
                               const char *current_arch);

// Derive the manifest path from an ELF path.
//   "messenger.app.elf"  →  "messenger.manifest.json"
//   "sx1262.drv.elf"     →  "sx1262.manifest.json"
// Drop-in for manifest_path_from_elf().
void rs_manifest_path_from_elf(const char *elf_path, char *out_path, size_t out_size);
```

### Data Structures

`CManifest` / `thistle_manifest_t` is `repr(C)` with fixed-size byte arrays
(no heap pointers, safe to copy across the FFI boundary):

| Field | Type | Size |
|-------|------|------|
| `manifest_type` | `uint8_t` | 1 |
| `id` | `char[64]` | 64 |
| `name` | `char[32]` | 32 |
| `version` | `char[16]` | 16 |
| `author` | `char[32]` | 32 |
| `description` | `char[128]` | 128 |
| `min_os` | `char[16]` | 16 |
| `arch` | `char[16]` | 16 |
| `entry` | `char[64]` | 64 |
| `icon` | `char[64]` | 64 |
| `permissions` | `uint32_t` | 4 |
| `background` | `bool` | 1 |
| `min_memory_kb` | `uint32_t` | 4 |
| `hal_interface` | `char[16]` | 16 |
| `changelog` | `char[256]` | 256 |

### Known Limitations / TODO

- **No serde** — the JSON parser is a bespoke string scanner. It does not
  handle escaped characters, nested objects, or arrays of objects. Suffices
  for simple manifest files; will silently misparse edge cases.
- **`permissions` field format is ambiguous** — `parse_permissions()` detects
  both `["radio","gps"]` (array) and `"radio,gps"` (comma string) by content
  scanning, but does not correctly handle e.g. `"ipc"` appearing as a
  substring of a longer token in the JSON.
- **`perm` submodule duplicates `permissions.rs` constants** — these should
  share a single source of truth (re-export from `permissions::perm` or a
  common `flags.rs`).
- **`from_file` uses `std::fs`** — on ESP-IDF this works via the VFS layer, but
  requires the SD card to be mounted before the manifest can be read. No
  provision for reading from embedded flash.
- **No schema validation** — unknown fields are silently ignored; required
  fields beyond `type` and `id` are not enforced.
- **Tests run on host only** — the 8 unit tests (`cargo test`) exercise the
  parser on the development machine, not on the target.

---

## Module: version

**Source:** `src/version.rs`

### Purpose

Declares the running kernel's semantic version and provides `satisfies()` for
`min_os` comparisons. Used by `manifest::Manifest::is_compatible()` and
exposed to C via `rs_kernel_version()` in `ffi.rs`.

Current kernel version: **0.1.0**

### Rust API

```rust
pub const VERSION_MAJOR:  u32  = 0;
pub const VERSION_MINOR:  u32  = 1;
pub const VERSION_PATCH:  u32  = 0;
pub const VERSION_STRING: &str = "0.1.0";

/// Returns true when `requirement` (semver string) is <= the running kernel.
/// Major mismatches are handled; prerelease/build metadata are ignored.
pub fn satisfies(requirement: &str) -> bool;
```

### C FFI

```c
// Returns a pointer to the static string "0.1.0\0". Do not free.
const char *rs_kernel_version(void);
```

### Known Limitations / TODO

- **No build-time stamp or git hash** — the version string is a constant.
  There is no mechanism to embed a commit SHA or build date for diagnostic
  output.
- **`satisfies()` ignores prerelease identifiers** — `"1.0.0-alpha"` would
  parse as `1.0.0`. Once the version moves past `0.x`, this may matter.
- **No `rs_kernel_version_parts()` FFI** — C code can only get the string form,
  not the individual major/minor/patch integers, which makes numeric comparison
  from C awkward.

---

## Module: permissions

**Source:** `src/permissions.rs`

### Purpose

Manages per-app permission grants using a statically-allocated slot table
(`MAX_APPS = 16` entries, no heap allocation for the table itself). Permissions
are represented as a bitmask of seven named flags. Provides parse/format helpers
for the string representation used in manifests.

### Rust API

```rust
// Permission flag constants (must match C PERM_* values in permissions.h)
pub const PERM_RADIO:   u32 = 0x01;
pub const PERM_GPS:     u32 = 0x02;
pub const PERM_STORAGE: u32 = 0x04;
pub const PERM_NETWORK: u32 = 0x08;
pub const PERM_AUDIO:   u32 = 0x10;
pub const PERM_SYSTEM:  u32 = 0x20;
pub const PERM_IPC:     u32 = 0x40;
pub const PERM_ALL:     u32 = 0x7F;

pub const MAX_APPS: usize = 16;

/// Clear all slots. Idempotent.
pub fn init() -> i32;

/// Grant `perms` to `app_id`. OR-accumulates if the slot already exists.
/// Returns ESP_ERR_NO_MEM when all 16 slots are occupied.
pub fn grant(app_id: &str, perms: u32) -> i32;

/// Revoke `perms` from `app_id`.
/// Returns ESP_ERR_NOT_FOUND if the app has no slot.
pub fn revoke(app_id: &str, perms: u32) -> i32;

/// Return true if `app_id` holds the single permission bit `perm`.
pub fn check(app_id: &str, perm: u32) -> bool;

/// Return the full bitmask for `app_id`, or 0 if not found.
pub fn get(app_id: &str) -> u32;

/// Parse a comma-separated permission string into a bitmask.
/// Case-insensitive; unknown tokens contribute 0.
/// "radio,gps" → PERM_RADIO | PERM_GPS
pub fn parse(name: &str) -> u32;

/// Format a bitmask as a canonical comma-separated string.
/// PERM_RADIO | PERM_GPS → "radio,gps"
pub fn to_string(perms: u32) -> String;
```

### C FFI

```c
esp_err_t rs_permissions_init(void);

esp_err_t rs_permissions_grant(const char *app_id, uint32_t perms);

esp_err_t rs_permissions_revoke(const char *app_id, uint32_t perms);

// Returns 1 if granted, 0 if not. (bool as int32 for C compat.)
int32_t   rs_permissions_check(const char *app_id, uint32_t perm);

uint32_t  rs_permissions_get(const char *app_id);
```

All `app_id` pointers must be valid, non-null, null-terminated C strings.

### Data Structures

The slot table is internal (`AppPerms`, not exposed). Each slot holds a
null-terminated app ID of up to 63 bytes and a `u32` bitmask. The table is
protected by a `std::sync::Mutex`.

### Improvement Areas

- **Advisory-only — no enforcement at the syscall boundary.** `check()` returns
  a boolean, but nothing in the current kernel actually gates a syscall on it.
  The permissions subsystem records grants but does not prevent a misbehaving
  app from calling a syscall it has no permission for.

- **No FreeRTOS task → app_id mapping.** To enforce permissions per-caller,
  the syscall dispatcher needs to resolve the calling task handle to an app_id
  at call time. This requires the app_manager to maintain a
  `TaskHandle_t → app_id` map and expose a lookup function.

- **Permission strings are hardcoded.** The seven flag names (`radio`, `gps`,
  etc.) are hardcoded in both `parse()` and `to_string()`. There is no
  mechanism for drivers or apps to declare custom permissions. This limits
  extensibility for third-party hardware.

- **No permission request / prompt UI flow.** There is no runtime path for an
  app to request a permission it does not currently hold, and no kernel support
  for surfacing a user-visible approval dialog. All grants are currently done
  at app load time from the manifest.

- **No persistent permission storage.** Grants are held in RAM and reset on
  reboot. There is no flash storage or SD-card database for persisted
  per-app grants (e.g., user-approved overrides).

- **Slot limit is small.** `MAX_APPS = 16` means no more than 16 apps can hold
  permissions simultaneously. If apps are loaded/unloaded frequently, revoked
  slots are not reclaimed (the slot stays allocated with a zero bitmask after
  `revoke()` — freeing requires a separate `remove()` call that does not yet
  exist).

- **Consider: capability-based model vs bitmask.** The current bitmask is
  simple but coarse. A capability model (unforgeable tokens granted at load
  time) would be more composable and easier to audit, at the cost of
  complexity.

---

## Module: ipc

**Source:** `src/ipc.rs`

### Purpose

Bounded message-passing queue with registered type-based handlers. A single
global queue (`IPC_QUEUE_DEPTH = 16` messages) holds `CIpcMessage` values.
Handlers are invoked synchronously on `ipc_send()` before the message is
enqueued. A Condvar is used for blocking receives. Designed to replace
`components/kernel/src/ipc.c`.

### Rust API

```rust
pub const IPC_MSG_MAX_DATA:  usize = 256;
pub const IPC_QUEUE_DEPTH:   usize = 16;
pub const IPC_HANDLER_MAX:   usize = 16;

/// Initialise the IPC subsystem. Idempotent.
pub fn ipc_init() -> i32;

/// Dispatch `msg` to all matching handlers, then enqueue it.
/// Returns ESP_ERR_NO_MEM when the queue is full.
/// Returns ESP_ERR_INVALID_STATE if not initialised.
pub fn ipc_send(msg: CIpcMessage) -> i32;

/// Dequeue the oldest message, blocking up to `timeout_ms`.
/// Returns ESP_ERR_TIMEOUT or ESP_ERR_INVALID_STATE on failure.
pub fn ipc_recv(timeout_ms: u32) -> Result<CIpcMessage, i32>;

/// Register a handler for messages of `msg_type`.
/// Returns ESP_ERR_NO_MEM when IPC_HANDLER_MAX registrations exist.
pub fn ipc_register_handler(
    msg_type: u32,
    handler: extern "C" fn(*const CIpcMessage, *mut c_void),
    user_data: *mut c_void,
) -> i32;
```

### C FFI

```c
esp_err_t rs_ipc_init(void);

// msg must point to a valid, initialised ipc_message_t.
esp_err_t rs_ipc_send(const ipc_message_t *msg);

// Writes received message into *msg on success.
esp_err_t rs_ipc_recv(ipc_message_t *msg, uint32_t timeout_ms);

// handler and user_data must remain valid for the lifetime of the IPC subsystem.
esp_err_t rs_ipc_register_handler(
    uint32_t msg_type,
    void (*handler)(const ipc_message_t *, void *),
    void *user_data
);
```

### Data Structures

`CIpcMessage` / `ipc_message_t` is `repr(C)` and must match the C header
exactly. Do not reorder fields.

| Field | Type | Notes |
|-------|------|-------|
| `src_app` | `uint32_t` | Sender app ID (numeric) |
| `dst_app` | `uint32_t` | Destination app ID; 0 = broadcast |
| `msg_type` | `uint32_t` | Application-defined type discriminant |
| `data` | `uint8_t[256]` | Inline payload |
| `data_len` | `size_t` | Valid bytes in `data` |
| `timestamp` | `uint32_t` | Caller-supplied tick count |

### Improvement Areas

- **Single global queue.** All apps share one queue and one receive path.
  `ipc_recv()` dequeues the oldest message regardless of `dst_app`. Sending
  to a specific app requires the receiver to inspect `dst_app` and discard
  messages not meant for it, which wastes CPU and increases latency. Per-app
  queues (indexed by app_id) would eliminate this.

- **`dst_app` field exists but is not used for filtering.** The intent of
  the field is visible in the struct, but `ipc_send()` makes no use of it
  during enqueue and `ipc_recv()` makes no use of it during dequeue.

- **No message priority.** All messages are FIFO. A high-priority event (e.g.,
  battery critical) can be queued behind a backlog of routine UI messages.

- **Handler dispatch runs under the queue lock.** In `ipc_send()`, registered
  handlers are called while `st.state` is locked. A handler that itself calls
  `ipc_send()` or `ipc_recv()` will deadlock. Handlers are documented as
  "expected to be short and non-blocking" but this is not enforced.

- **No handler unregister.** `ipc_register_handler()` adds entries; there is
  no `ipc_unregister_handler()`. When an app exits, its registered handler
  function pointer becomes dangling. The kernel must zero out handlers at
  app teardown time, which requires cross-module cooperation not yet
  implemented.

- **No message acknowledgment / reply mechanism.** Fire-and-forget only.
  Request-response patterns require the caller to register a response handler
  keyed on a correlation ID they invent themselves.

- **`OnceLock` state cannot be reset in tests.** Unit tests work around this
  by constructing local `IpcGlobal` instances rather than using the singleton,
  meaning the test coverage does not exercise the global initialisation path.

- **Consider: typed channels vs generic message passing.** Typed channels
  (producer and consumer agree on a Rust type) would eliminate the `data` byte
  array and the `data_len` field, and allow the compiler to catch message
  type/payload mismatches. Requires a more invasive FFI design.

---

## Module: event

**Source:** `src/event.rs`

### Purpose

Publish-subscribe event bus. Maintains a statically-allocated subscriber table
of `EVENT_MAX = 17` event types, each holding up to `EVENT_SUBSCRIBERS_MAX = 8`
handler slots. Handlers are invoked synchronously in registration order on
`rs_event_publish()`. Designed to replace `components/kernel/src/event.c`.

### Rust API

```rust
pub const EVENT_MAX:             usize = 17;
pub const EVENT_SUBSCRIBERS_MAX: usize = 8;

#[repr(u32)]
pub enum EventType {
    SystemBoot        = 0,
    SystemShutdown    = 1,
    AppLaunched       = 2,
    AppStopped        = 3,
    AppSwitched       = 4,
    InputKey          = 5,
    InputTouch        = 6,
    RadioRx           = 7,
    GpsFix            = 8,
    BatteryLow        = 9,
    BatteryCharging   = 10,
    SdMounted         = 11,
    SdUnmounted       = 12,
    WifiConnected     = 13,
    WifiDisconnected  = 14,
    BleConnected      = 15,
    BleDisconnected   = 16,
}

impl EventType {
    pub fn from_u32(v: u32) -> Option<Self>;
}
```

The FFI functions below are the primary Rust API surface (there are no separate
internal Rust functions wrapping them — the `EventBus` struct methods are
private).

### C FFI

```c
// Idempotent; bus is zero-initialised at static construction.
esp_err_t rs_event_bus_init(void);

// Register handler for event_type.
// Returns ESP_ERR_INVALID_ARG (out-of-range type) or ESP_ERR_NO_MEM (slots full).
esp_err_t rs_event_subscribe(
    uint32_t  event_type,
    void    (*handler)(const event_t *, void *),
    void     *user_data
);

// Remove the first registration matching (event_type, handler).
// Returns ESP_ERR_NOT_FOUND if the handler was never registered.
esp_err_t rs_event_unsubscribe(
    uint32_t  event_type,
    void    (*handler)(const event_t *, void *)
);

// Dispatch event to all subscribers of event->type.
// Handlers are called synchronously in registration order.
esp_err_t rs_event_publish(const event_t *event);
```

### Data Structures

`CEvent` / `event_t` is `repr(C)` and must match the C header exactly:

```c
typedef struct {
    uint32_t  event_type;   // EventType enum value
    uint32_t  timestamp;
    void     *data;         // caller-managed; bus never dereferences
    size_t    data_len;
} event_t;
```

The bus itself stores no event data — it only holds subscriber function pointers
and `user_data` opaque pointers. All storage is in a `static Mutex<EventBus>`.

### Improvement Areas

- **Fixed 17 event types — not extensible by apps or drivers.** A driver that
  wants to publish a custom event (e.g., `LoRaSendComplete`) must repurpose an
  existing type or the protocol must be changed. Options: reserve a range of
  dynamic type IDs, or use a string-keyed registry.

- **Subscriber limit per event (8) is small.** With 8 slots per event type,
  a popular event like `InputKey` could be exhausted by the shell, the active
  app, the status bar, and a few background services. `ESP_ERR_NO_MEM` from
  `rs_event_subscribe` is easy to miss.

- **No event filtering or priority.** All subscribers receive all events of a
  given type. There is no way to say "only deliver `InputKey` to the foreground
  app" without every subscriber filtering manually.

- **Synchronous dispatch — a slow subscriber blocks the publisher.** Because
  `rs_event_publish()` calls each handler inline while holding the bus mutex,
  a slow handler stalls the entire dispatch chain, and the mutex prevents
  concurrent publishes from any other task. For events published from ISR or
  radio RX contexts this is a latency hazard.

- **No `unsubscribe_all(app_id)` call.** When an app exits, all of its
  subscriptions must be individually removed. The kernel must track which
  handlers belong to which app to do this reliably.

- **`unsubscribe` matches on raw function pointer.** If an app registers the
  same handler function for two different `user_data` values (e.g., two
  different window IDs), `rs_event_unsubscribe` will remove the first match
  regardless of `user_data`. There is no way to unsubscribe a specific
  registration if the same function pointer was reused.

- **Consider: async dispatch / per-subscriber event queue.** Rather than
  calling subscribers inline, the bus could enqueue events into per-subscriber
  ring buffers and wake the subscriber's task. This decouples publisher latency
  from subscriber processing speed and removes the need to hold the bus mutex
  during handler execution.

---

## Module: app_manager (being ported)

**Source:** `src/app_manager.rs` — **file does not yet exist**. This module is
the next planned port from `components/kernel/src/app_manager.c`.

### Purpose

Tracks the lifecycle of loaded apps (slots: created, running, paused, stopped),
enforces the maximum simultaneous app limit, drives lifecycle callbacks
(`on_create`, `on_resume`, `on_pause`, `on_destroy`), and implements LRU
eviction when the system runs low on free slots.

### Rust API (planned)

The Rust API will mirror the existing C interface:

```rust
pub const MAX_APP_SLOTS: usize = 8;  // matches current C definition

pub enum AppState { Empty, Created, Running, Paused, Stopped }

pub struct AppSlot {
    pub app_id: [u8; 64],
    pub state: AppState,
    pub last_active_tick: u32,
    // ... vtable pointers for lifecycle callbacks
}

pub fn app_manager_init() -> i32;
pub fn app_launch(manifest: &CManifest) -> i32;   // returns slot index or error
pub fn app_resume(slot: usize) -> i32;
pub fn app_pause(slot: usize) -> i32;
pub fn app_destroy(slot: usize) -> i32;
pub fn app_find(app_id: &str) -> Option<usize>;
pub fn app_evict_lru() -> i32;
```

### C FFI (planned)

```c
esp_err_t rs_app_manager_init(void);
esp_err_t rs_app_launch(const thistle_manifest_t *manifest);
esp_err_t rs_app_resume(uint32_t slot);
esp_err_t rs_app_pause(uint32_t slot);
esp_err_t rs_app_destroy(uint32_t slot);
int32_t   rs_app_find(const char *app_id);    // returns slot index or -1
```

### Improvement Areas

- **LRU eviction is basic.** The current C implementation tracks a
  `last_active_tick` timestamp and evicts the oldest paused app. It does not
  respond to memory pressure signals from FreeRTOS heap stats. Consider
  integrating `esp_get_free_heap_size()` thresholds to trigger earlier
  eviction.

- **No app isolation.** All apps share the same address space (the ESP32-S3
  has no MMU). A misbehaving app can corrupt kernel state. Mitigation options:
  stack canaries per app task, guard regions via MPU (ESP32-S3 has a PMS
  peripheral), and/or a watchdog task per app.

- **Lifecycle callbacks are synchronous.** `on_create` and `on_destroy` are
  called from the kernel task context. A slow or blocking `on_create` stalls
  app startup and blocks the kernel from processing other events.

- **No crash recovery.** If `on_create` panics (Rust panic in a C callback,
  or an unhandled exception in the app's init function), the slot is left in
  an inconsistent state. There is no watchdog, no restart policy, and no
  dead-slot GC.

- **No task → app_id map.** As noted in the permissions section, enforcing
  per-caller permissions requires knowing which app_id corresponds to the
  calling FreeRTOS task. The app_manager is the natural owner of this mapping
  (`xTaskGetCurrentTaskHandle()` → app_id lookup table), but it does not
  exist yet.

- **Consider: watchdog per app, restart policy.** A per-app FreeRTOS software
  timer that fires if the app has not yielded or responded to a heartbeat
  message within a deadline. On expiry: attempt `on_destroy`, free the slot,
  optionally relaunch. Requires cooperation with the event bus and IPC.

---

## Cross-cutting Concerns

### Thread Safety

All modules use `std::sync::Mutex` for shared mutable state:

| Module | Lock granularity |
|--------|-----------------|
| `permissions` | Single `Mutex<[AppPerms; 16]>` covering all operations |
| `ipc` | Single `Mutex<IpcState>` + `Condvar` for blocking recv |
| `event` | Single `Mutex<EventBus>` covering subscribe, unsubscribe, publish |

No module uses `unsafe` interior mutability outside of `OnceLock` in `ipc`.
All globals are initialised at compile time (const constructors) or lazily on
first call. There are no `static mut` items.

**Lock-order hazard:** `ipc::ipc_send()` calls registered handlers while holding
the IPC mutex. If any handler attempts to call `ipc_send()` or `ipc_recv()`,
it will deadlock. This must be documented in the C header and enforced by
convention or caught at runtime with `try_lock`.

### Error Handling

All public functions return `i32` at the FFI boundary using the ESP-IDF error
code conventions:

| Constant | Value | Meaning |
|----------|-------|---------|
| `ESP_OK` | `0x000` | Success |
| `ESP_ERR_NO_MEM` | `0x101` | Slot/queue/table full |
| `ESP_ERR_INVALID_ARG` | `0x102` | Null pointer or empty string |
| `ESP_ERR_INVALID_STATE` | `0x103` | Subsystem not initialised |
| `ESP_ERR_NOT_FOUND` | `0x105` | App ID not in table |
| `ESP_ERR_TIMEOUT` | `0x107` | `ipc_recv` deadline expired |

Internally, functions return `i32` directly rather than `Result<_, esp_err_t>`.
This is a deliberate choice for FFI symmetry but it loses the Rust type system's
ability to enforce error handling at call sites within the crate. Internal
helpers could use `Result` and convert at the FFI boundary, which would improve
clarity without affecting the C-visible API.

### Logging

The `log` crate (`version 0.4`) is listed as a dependency but is not currently
wired to any backend. No `log::info!()` or `log::error!()` calls exist in any
module. To enable logging:

1. Add an ESP-IDF-compatible log backend (e.g., `esp-idf-svc::log::EspLogger`)
   to `Cargo.toml`.
2. Call `EspLogger::initialize_default()` during kernel init.
3. Add `log::info!` / `log::warn!` / `log::error!` calls at key decision points
   (manifest parse errors, permission denials, IPC queue full, etc.).

Until this is done, silent failures (e.g., a manifest parse error returns
`ESP_ERR_INVALID_ARG` with no diagnostic) make embedded debugging difficult.

### Testing

Unit tests are in `#[cfg(test)]` blocks within each module source file. They
run on the **host** target (`cargo test`) and do not require an ESP32 or
ESP-IDF.

| Module | Test count | Notes |
|--------|-----------|-------|
| `manifest` | 8 | Covers all three manifest types, error cases, path derivation, compat check |
| `permissions` | 8 | Covers grant/revoke/check, accumulation, slot exhaustion, parse/format |
| `ipc` | 4 | send/recv, handler dispatch, queue full, recv timeout |
| `event` | 4 | subscribe/publish, unsubscribe, multi-subscriber, invalid type |
| `version` | — | Tested indirectly via `manifest` compatibility tests |

Total: 24 unit tests. All run on the host target.

**ESP-IDF integration testing** (running on target hardware or QEMU) does not
exist yet. Key gaps: VFS-based manifest file reading, FreeRTOS Condvar
behaviour, actual IRQ/task interaction.

### Build

```toml
# Cargo.toml
[lib]
crate-type = ["staticlib"]

[dependencies]
log = "0.4"

[build-dependencies]
embuild = "0.33"
```

```
# Host tests:
cargo test

# ESP32-S3 target:
cargo +esp build --release --target xtensa-esp32s3-espidf

# CMakeLists.txt links the resulting libthistle_kernel.a:
# target_link_libraries(${COMPONENT_LIB} INTERFACE kernel_rs)
```

The `embuild` crate auto-generates the `build.rs` bindings needed to find the
ESP-IDF sysroots. The `.cargo/config.toml` in this component pins the target
triple and linker flags.

---

## Migration Status Table

| Module | C file | Rust file | Status | Host tests |
|--------|--------|-----------|--------|-----------|
| manifest | `manifest.c` | `manifest.rs` + `ffi.rs` | Dual (both exist) | 8 |
| version | `kernel.h` (constant) | `version.rs` + `ffi.rs` | Rust-only | via manifest |
| permissions | `permissions.c` | `permissions.rs` | Dual (both exist) | 8 |
| ipc | `ipc.c` | `ipc.rs` | Dual (both exist) | 4 |
| event | `event.c` | `event.rs` | Dual (both exist) | 4 |
| app_manager | `app_manager.c` | — (planned) | C-only | — |
| signing | `signing.c` | — | C-only (Monocypher/Ed25519) | via C tests |
| elf_loader | `elf_loader.c` | — | C-only | — |
| driver_loader | `driver_loader.c` | — | C-only | — |
| kernel | `kernel.c` | — | C-only (boot sequence) | — |

"Dual" means both the C and Rust implementations exist on disk. The active
implementation is determined by whether the C source is listed in
`components/kernel/CMakeLists.txt SRCS` and whether `kernel_rs` is in
`REQUIRES`. During transition, only one should be linked.

---

## Switching a Module from C to Rust

The following procedure applies to any module in the "Dual" state. Using
`permissions` as the concrete example:

1. **Add the Rust header include in the C caller:**
   ```c
   #include "thistle/kernel_rs.h"
   ```

2. **Replace every call to the C function with its `rs_` equivalent:**
   ```c
   // Before:
   permissions_init();
   permissions_grant(app_id, PERM_RADIO);

   // After:
   rs_permissions_init();
   rs_permissions_grant(app_id, PERM_RADIO);
   ```

3. **Remove the C source from `components/kernel/CMakeLists.txt`:**
   ```cmake
   # Remove this line from SRCS:
   # src/permissions.c
   ```

4. **Ensure `kernel_rs` is in `REQUIRES`:**
   ```cmake
   idf_component_register(
       ...
       REQUIRES kernel_rs ...
   )
   ```

5. **Build and verify:**
   ```sh
   idf.py build
   ```
   A linker error about an undefined `rs_permissions_*` symbol means either
   the Rust crate was not compiled or the header path is wrong. A duplicate
   symbol error means the C source was not removed from `SRCS`.

6. **Run host tests to confirm Rust behaviour has not regressed:**
   ```sh
   cd components/kernel_rs && cargo test
   ```
