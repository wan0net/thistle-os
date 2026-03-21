// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Rust app manager
//
// Port of the C app_manager.c. Manages app lifecycle, LRU eviction, and event
// publishing. Holds raw C pointers to statically-owned app_entry_t structs;
// all callback invocations are unsafe.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// ESP-IDF error codes (matching esp_err.h)
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of simultaneously-tracked app slots.
pub const APP_SLOTS_MAX: usize = 20;

/// Free-heap threshold below which LRU eviction is attempted before a launch.
pub const APP_MEMORY_THRESHOLD_BYTES: usize = 50 * 1024;

/// Sentinel handle for "no app".
pub const APP_HANDLE_INVALID: i32 = -1;

// ---------------------------------------------------------------------------
// AppState — repr(u32) to match the C enum layout
// ---------------------------------------------------------------------------

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Unloaded    = 0,
    Loading     = 1,
    Running     = 2,
    Backgrounded = 3,
    Suspended   = 4,
}

impl AppState {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Loading,
            2 => Self::Running,
            3 => Self::Backgrounded,
            4 => Self::Suspended,
            _ => Self::Unloaded,
        }
    }
}

// ---------------------------------------------------------------------------
// Event constants (matching C event_type_t)
// ---------------------------------------------------------------------------

const EVENT_APP_LAUNCHED: u32 = 2;
const EVENT_APP_STOPPED: u32 = 3;
const EVENT_APP_SWITCHED: u32 = 4;

// ---------------------------------------------------------------------------
// Heap caps (from esp_heap_caps.h)
// ---------------------------------------------------------------------------

const MALLOC_CAP_DEFAULT: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// External C functions
// ---------------------------------------------------------------------------

extern "C" {
    /// Returns the number of milliseconds since kernel boot.
    fn kernel_uptime_ms() -> u32;

    /// Returns free heap bytes matching the given capability flags.
    fn heap_caps_get_free_size(caps: u32) -> usize;

    /// Publish an event to the C event bus.
    fn event_publish(event: *const CEvent) -> i32;
}

// ---------------------------------------------------------------------------
// CEvent — repr(C) matching the C event_t layout
// ---------------------------------------------------------------------------

#[repr(C)]
struct CEvent {
    event_type: u32,
    timestamp:  u32,
    data:       *const c_void,
    data_len:   usize,
}

// SAFETY: CEvent is a plain-data carrier. The `data` pointer is managed by
// the caller; we never dereference it inside this module.
unsafe impl Send for CEvent {}
unsafe impl Sync for CEvent {}

// ---------------------------------------------------------------------------
// FFI structs — must match the C side exactly
// ---------------------------------------------------------------------------

/// Mirrors C `app_manifest_t`. Owned by the C side (static storage).
#[repr(C)]
pub struct CAppManifest {
    pub id:               *const c_char,
    pub name:             *const c_char,
    pub version:          *const c_char,
    pub allow_background: bool,
    pub min_memory_kb:    u32,
}

// SAFETY: Pointers are into C static storage; the C side guarantees their
// lifetime exceeds any Rust usage.
unsafe impl Send for CAppManifest {}
unsafe impl Sync for CAppManifest {}

/// Mirrors C `app_entry_t`. Owned by the C side (static storage).
#[repr(C)]
pub struct CAppEntry {
    pub on_create:  Option<unsafe extern "C" fn() -> i32>,
    pub on_start:   Option<unsafe extern "C" fn()>,
    pub on_pause:   Option<unsafe extern "C" fn()>,
    pub on_resume:  Option<unsafe extern "C" fn()>,
    pub on_destroy: Option<unsafe extern "C" fn()>,
    pub manifest:   *const CAppManifest,
}

// SAFETY: Same rationale as CAppManifest — all pointers come from C static
// storage and outlive any Rust usage.
unsafe impl Send for CAppEntry {}
unsafe impl Sync for CAppEntry {}

// ---------------------------------------------------------------------------
// Internal AppSlot
// ---------------------------------------------------------------------------

struct AppSlot {
    /// Null pointer means this slot is empty.
    entry:        *const CAppEntry,
    state:        u32,
    handle:       i32,
    last_used_ms: u32,
}

impl AppSlot {
    const fn empty() -> Self {
        Self {
            entry:        std::ptr::null(),
            state:        AppState::Unloaded as u32,
            handle:       APP_HANDLE_INVALID,
            last_used_ms: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.entry.is_null()
    }
}

// SAFETY: AppSlot contains raw C pointers into static C storage. Access is
// serialised through the AppManager Mutex.
unsafe impl Send for AppSlot {}

// ---------------------------------------------------------------------------
// Helper: read a C string from a slot's manifest->id
// ---------------------------------------------------------------------------

/// Extract a UTF-8 `&str` from `slot.entry->manifest->id`.
///
/// Returns `""` if any pointer in the chain is null or the bytes are not
/// valid UTF-8. All pointer dereferences must be done inside an `unsafe`
/// block by the caller.
///
/// # Safety
/// The caller must ensure that `slot.entry`, `entry.manifest`, and
/// `manifest.id` all point to valid, live memory.
unsafe fn slot_app_id(slot: &AppSlot) -> &str {
    if slot.entry.is_null() {
        return "";
    }
    let entry = &*slot.entry;
    if entry.manifest.is_null() {
        return "";
    }
    let manifest = &*entry.manifest;
    if manifest.id.is_null() {
        return "";
    }
    CStr::from_ptr(manifest.id).to_str().unwrap_or("")
}

// ---------------------------------------------------------------------------
// AppManager
// ---------------------------------------------------------------------------

struct AppManager {
    slots:       [AppSlot; APP_SLOTS_MAX],
    slot_count:  usize,
    foreground:  i32, // handle of the currently-foregrounded app, or -1
    next_handle: i32,
    initialized: bool,
}

impl AppManager {
    const fn new() -> Self {
        // AppSlot is not Copy, so we can't use array initialisation syntax with
        // a non-Copy const. We initialise all slots by hand in a fixed-size
        // array; APP_SLOTS_MAX = 20 so this is fully unrolled at compile time.
        Self {
            slots: [
                AppSlot::empty(), AppSlot::empty(), AppSlot::empty(), AppSlot::empty(),
                AppSlot::empty(), AppSlot::empty(), AppSlot::empty(), AppSlot::empty(),
                AppSlot::empty(), AppSlot::empty(), AppSlot::empty(), AppSlot::empty(),
                AppSlot::empty(), AppSlot::empty(), AppSlot::empty(), AppSlot::empty(),
                AppSlot::empty(), AppSlot::empty(), AppSlot::empty(), AppSlot::empty(),
            ],
            slot_count:  0,
            foreground:  APP_HANDLE_INVALID,
            next_handle: 0,
            initialized: false,
        }
    }

    // -----------------------------------------------------------------------
    // init
    // -----------------------------------------------------------------------

    fn init(&mut self) -> i32 {
        if self.initialized {
            return ESP_OK;
        }
        self.slot_count = 0;
        self.foreground = APP_HANDLE_INVALID;
        self.next_handle = 0;
        for slot in self.slots.iter_mut() {
            *slot = AppSlot::empty();
        }
        self.initialized = true;
        ESP_OK
    }

    // -----------------------------------------------------------------------
    // register
    // -----------------------------------------------------------------------

    fn register(&mut self, entry: *const CAppEntry) -> i32 {
        if entry.is_null() {
            return ESP_ERR_INVALID_ARG;
        }

        // Reject duplicate IDs.
        let app_id = unsafe {
            let e = &*entry;
            if e.manifest.is_null() {
                return ESP_ERR_INVALID_ARG;
            }
            let m = &*e.manifest;
            if m.id.is_null() {
                return ESP_ERR_INVALID_ARG;
            }
            CStr::from_ptr(m.id).to_str().unwrap_or("")
        };

        for i in 0..self.slot_count {
            if !self.slots[i].is_empty() {
                let existing_id = unsafe { slot_app_id(&self.slots[i]) };
                if existing_id == app_id {
                    // Already registered; idempotent success.
                    return ESP_OK;
                }
            }
        }

        if self.slot_count >= APP_SLOTS_MAX {
            return ESP_ERR_NO_MEM;
        }

        let handle = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);

        self.slots[self.slot_count] = AppSlot {
            entry,
            state:        AppState::Unloaded as u32,
            handle,
            last_used_ms: 0,
        };
        self.slot_count += 1;

        ESP_OK
    }

    // -----------------------------------------------------------------------
    // find_slot_by_handle (returns index into self.slots)
    // -----------------------------------------------------------------------

    fn find_slot_by_handle(&self, handle: i32) -> Option<usize> {
        if handle == APP_HANDLE_INVALID {
            return None;
        }
        for i in 0..self.slot_count {
            if !self.slots[i].is_empty() && self.slots[i].handle == handle {
                return Some(i);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // find_slot_by_id (returns index into self.slots)
    // -----------------------------------------------------------------------

    fn find_slot_by_id(&self, app_id: &str) -> Option<usize> {
        for i in 0..self.slot_count {
            if !self.slots[i].is_empty() {
                let id = unsafe { slot_app_id(&self.slots[i]) };
                if id == app_id {
                    return Some(i);
                }
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // evict_lru
    // -----------------------------------------------------------------------

    /// Evict the least-recently-used backgrounded or suspended app that is
    /// neither the foreground app nor the launcher (id == "launcher").
    ///
    /// Returns the evicted handle on success, `APP_HANDLE_INVALID` if nothing
    /// was evictable, or a negative ESP error code on failure.
    fn evict_lru(&mut self) -> i32 {
        let mut oldest_idx: Option<usize> = None;
        let mut oldest_ts: u32 = u32::MAX;

        for i in 0..self.slot_count {
            let slot = &self.slots[i];
            if slot.is_empty() {
                continue;
            }
            // Skip foreground app.
            if slot.handle == self.foreground {
                continue;
            }
            // Skip launcher.
            let id = unsafe { slot_app_id(slot) };
            if id == "launcher" {
                continue;
            }
            // Only evict apps that are not running (backgrounded or suspended).
            let state = AppState::from_u32(slot.state);
            if state != AppState::Backgrounded && state != AppState::Suspended {
                continue;
            }
            if slot.last_used_ms < oldest_ts {
                oldest_ts = slot.last_used_ms;
                oldest_idx = Some(i);
            }
        }

        let idx = match oldest_idx {
            Some(i) => i,
            None    => return APP_HANDLE_INVALID,
        };

        // Call on_destroy if available.
        let entry_ptr = self.slots[idx].entry;
        let handle = self.slots[idx].handle;
        if !entry_ptr.is_null() {
            unsafe {
                let entry = &*entry_ptr;
                if let Some(on_destroy) = entry.on_destroy {
                    on_destroy();
                }
            }
        }

        // Publish APP_STOPPED event.
        Self::publish_event(EVENT_APP_STOPPED, handle);

        // Reset slot (keep it in the array but mark as unloaded/empty).
        self.slots[idx] = AppSlot::empty();

        handle
    }

    // -----------------------------------------------------------------------
    // launch
    // -----------------------------------------------------------------------

    /// Bring the app identified by `app_id` to the foreground.
    ///
    /// Lifecycle:
    ///   1. Pause the current foreground app.
    ///   2. If memory is low, evict LRU.
    ///   3. If the target app is Unloaded, call on_create → LOADING → RUNNING.
    ///   4. If the target app is Backgrounded/Suspended, call on_resume → RUNNING.
    ///   5. Update foreground handle and last_used timestamps.
    ///   6. Publish APP_LAUNCHED or APP_SWITCHED.
    fn launch(&mut self, app_id: &str) -> i32 {
        let target_idx = match self.find_slot_by_id(app_id) {
            Some(i) => i,
            None    => return ESP_ERR_NOT_FOUND,
        };

        let target_handle = self.slots[target_idx].handle;

        // Already foreground — nothing to do.
        if self.foreground == target_handle {
            return ESP_OK;
        }

        // Pause the current foreground app, if any.
        let prev_handle = self.foreground;
        if let Some(fg_idx) = self.find_slot_by_handle(self.foreground) {
            if AppState::from_u32(self.slots[fg_idx].state) == AppState::Running {
                self.slots[fg_idx].state = AppState::Backgrounded as u32;
                let entry_ptr = self.slots[fg_idx].entry;
                if !entry_ptr.is_null() {
                    unsafe {
                        let entry = &*entry_ptr;
                        if let Some(on_pause) = entry.on_pause {
                            on_pause();
                        }
                    }
                }
            }
        }

        // Evict LRU if heap is low.
        #[cfg(not(test))]
        {
            let free = unsafe { heap_caps_get_free_size(MALLOC_CAP_DEFAULT) };
            if free < APP_MEMORY_THRESHOLD_BYTES {
                self.evict_lru();
            }
        }

        // Bring target app to the foreground.
        let state = AppState::from_u32(self.slots[target_idx].state);
        let is_new_launch;

        match state {
            AppState::Unloaded => {
                is_new_launch = true;
                // Transition: UNLOADED → LOADING
                self.slots[target_idx].state = AppState::Loading as u32;
                let entry_ptr = self.slots[target_idx].entry;
                if !entry_ptr.is_null() {
                    let rc = unsafe {
                        let entry = &*entry_ptr;
                        match entry.on_create {
                            Some(on_create) => on_create(),
                            None            => ESP_OK,
                        }
                    };
                    if rc != ESP_OK {
                        // Roll back: on_create failed.
                        self.slots[target_idx].state = AppState::Unloaded as u32;
                        // Restore previous foreground.
                        self.foreground = prev_handle;
                        if let Some(fg_idx) = self.find_slot_by_handle(prev_handle) {
                            self.slots[fg_idx].state = AppState::Running as u32;
                        }
                        return rc;
                    }
                }
                // LOADING → RUNNING via on_start
                self.slots[target_idx].state = AppState::Running as u32;
                let entry_ptr = self.slots[target_idx].entry;
                if !entry_ptr.is_null() {
                    unsafe {
                        let entry = &*entry_ptr;
                        if let Some(on_start) = entry.on_start {
                            on_start();
                        }
                    }
                }
            }
            AppState::Backgrounded | AppState::Suspended => {
                is_new_launch = false;
                self.slots[target_idx].state = AppState::Running as u32;
                let entry_ptr = self.slots[target_idx].entry;
                if !entry_ptr.is_null() {
                    unsafe {
                        let entry = &*entry_ptr;
                        if let Some(on_resume) = entry.on_resume {
                            on_resume();
                        }
                    }
                }
            }
            _ => {
                // Loading or already Running (shouldn't normally reach here).
                self.slots[target_idx].state = AppState::Running as u32;
                is_new_launch = false;
            }
        }

        // Update timestamps and foreground.
        #[cfg(not(test))]
        let now = unsafe { kernel_uptime_ms() };
        #[cfg(test)]
        let now: u32 = 0;

        self.slots[target_idx].last_used_ms = now;
        self.foreground = target_handle;

        let event_type = if is_new_launch {
            EVENT_APP_LAUNCHED
        } else {
            EVENT_APP_SWITCHED
        };
        Self::publish_event(event_type, target_handle);

        ESP_OK
    }

    // -----------------------------------------------------------------------
    // switch_to
    // -----------------------------------------------------------------------

    fn switch_to(&mut self, handle: i32) -> i32 {
        // Resolve the handle to an app_id string, then delegate to launch.
        let app_id_owned: CString = {
            let idx = match self.find_slot_by_handle(handle) {
                Some(i) => i,
                None    => return ESP_ERR_NOT_FOUND,
            };
            let raw = unsafe { slot_app_id(&self.slots[idx]) };
            match CString::new(raw) {
                Ok(s)  => s,
                Err(_) => return ESP_ERR_INVALID_ARG,
            }
        };
        self.launch(app_id_owned.to_str().unwrap_or(""))
    }

    // -----------------------------------------------------------------------
    // get_foreground
    // -----------------------------------------------------------------------

    fn get_foreground(&self) -> i32 {
        self.foreground
    }

    // -----------------------------------------------------------------------
    // get_state
    // -----------------------------------------------------------------------

    fn get_state(&self, handle: i32) -> u32 {
        match self.find_slot_by_handle(handle) {
            Some(i) => self.slots[i].state,
            None    => AppState::Unloaded as u32,
        }
    }

    // -----------------------------------------------------------------------
    // suspend
    // -----------------------------------------------------------------------

    fn suspend(&mut self, handle: i32) -> i32 {
        let idx = match self.find_slot_by_handle(handle) {
            Some(i) => i,
            None    => return ESP_ERR_NOT_FOUND,
        };

        let state = AppState::from_u32(self.slots[idx].state);
        match state {
            AppState::Running | AppState::Backgrounded => {
                let entry_ptr = self.slots[idx].entry;
                if !entry_ptr.is_null() {
                    unsafe {
                        let entry = &*entry_ptr;
                        if let Some(on_pause) = entry.on_pause {
                            on_pause();
                        }
                    }
                }
                self.slots[idx].state = AppState::Suspended as u32;
                // If this app was foreground, clear the foreground handle.
                if self.foreground == handle {
                    self.foreground = APP_HANDLE_INVALID;
                }
                ESP_OK
            }
            AppState::Suspended => ESP_OK, // Already suspended.
            _ => ESP_ERR_INVALID_STATE,
        }
    }

    // -----------------------------------------------------------------------
    // kill
    // -----------------------------------------------------------------------

    fn kill(&mut self, handle: i32) -> i32 {
        let idx = match self.find_slot_by_handle(handle) {
            Some(i) => i,
            None    => return ESP_ERR_NOT_FOUND,
        };

        let state = AppState::from_u32(self.slots[idx].state);
        if state == AppState::Unloaded {
            return ESP_ERR_INVALID_STATE;
        }

        // Call on_destroy.
        let entry_ptr = self.slots[idx].entry;
        if !entry_ptr.is_null() {
            unsafe {
                let entry = &*entry_ptr;
                if let Some(on_destroy) = entry.on_destroy {
                    on_destroy();
                }
            }
        }

        // Publish APP_STOPPED.
        Self::publish_event(EVENT_APP_STOPPED, handle);

        // Clear foreground if needed.
        if self.foreground == handle {
            self.foreground = APP_HANDLE_INVALID;
        }

        // Reset the slot.
        self.slots[idx] = AppSlot::empty();

        ESP_OK
    }

    // -----------------------------------------------------------------------
    // list_apps
    // -----------------------------------------------------------------------

    /// Return pointers to all registered manifests (for internal use).
    pub fn list_apps(&self) -> Vec<*const CAppManifest> {
        let mut out = Vec::with_capacity(self.slot_count);
        for i in 0..self.slot_count {
            let slot = &self.slots[i];
            if slot.is_empty() {
                continue;
            }
            let entry_ptr = slot.entry;
            if entry_ptr.is_null() {
                continue;
            }
            let manifest_ptr = unsafe { (*entry_ptr).manifest };
            if !manifest_ptr.is_null() {
                out.push(manifest_ptr);
            }
        }
        out
    }

    // -----------------------------------------------------------------------
    // publish_event (private helper)
    // -----------------------------------------------------------------------

    /// Build a minimal CEvent and hand it to the C event bus.
    ///
    /// `handle` is passed as the data payload (as a raw pointer-sized integer).
    /// This matches the convention used by the C app_manager.c.
    fn publish_event(event_type: u32, handle: i32) {
        // Use the handle value as an inline data word.  We store it as a
        // pointer-sized value in `data`; `data_len` carries its byte size.
        // The C event bus treats `data` as opaque; subscribers that care about
        // app events cast it back to an int.
        let handle_val = handle as usize;
        let ev = CEvent {
            event_type,
            // We can't call kernel_uptime_ms in tests; supply 0 there.
            #[cfg(not(test))]
            timestamp: unsafe { kernel_uptime_ms() },
            #[cfg(test)]
            timestamp: 0,
            data:     handle_val as *const c_void,
            data_len: std::mem::size_of::<i32>(),
        };
        // SAFETY: `ev` is stack-allocated and fully initialised. The C event
        // bus only reads through the pointer synchronously inside this call.
        #[cfg(not(test))]
        unsafe {
            event_publish(&ev as *const CEvent);
        }
        // In tests we skip the C call — the C bus is not linked.
        let _ = ev;
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static APP_MANAGER: Mutex<AppManager> = Mutex::new(AppManager::new());

// ---------------------------------------------------------------------------
// Public Rust API (thin wrappers over the locked manager)
// ---------------------------------------------------------------------------

/// Initialise the app manager. Idempotent.
pub fn init() -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.init(),
        Err(_)      => ESP_ERR_INVALID_STATE,
    }
}

/// Register a C app entry. The pointer must remain valid for the lifetime of
/// the kernel (i.e. point to static storage on the C side).
///
/// # Safety
/// `entry` must point to a valid, static `CAppEntry`.
pub unsafe fn register(entry: *const CAppEntry) -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.register(entry),
        Err(_)      => ESP_ERR_INVALID_STATE,
    }
}

/// Bring the app with the given ID to the foreground.
pub fn launch(app_id: &str) -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.launch(app_id),
        Err(_)      => ESP_ERR_INVALID_STATE,
    }
}

/// Bring the app identified by `handle` to the foreground.
pub fn switch_to(handle: i32) -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.switch_to(handle),
        Err(_)      => ESP_ERR_INVALID_STATE,
    }
}

/// Return the handle of the currently-foregrounded app, or `APP_HANDLE_INVALID`.
pub fn get_foreground() -> i32 {
    match APP_MANAGER.lock() {
        Ok(mgr) => mgr.get_foreground(),
        Err(_)  => APP_HANDLE_INVALID,
    }
}

/// Return the raw `AppState` value for `handle`.
///
/// Returns `AppState::Unloaded` (0) for invalid or unknown handles.
pub fn get_state(handle: i32) -> u32 {
    match APP_MANAGER.lock() {
        Ok(mgr) => mgr.get_state(handle),
        Err(_)  => AppState::Unloaded as u32,
    }
}

/// Transition `handle` into `AppState::Suspended`.
pub fn suspend(handle: i32) -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.suspend(handle),
        Err(_)      => ESP_ERR_INVALID_STATE,
    }
}

/// Destroy the app identified by `handle` and reclaim its slot.
pub fn kill(handle: i32) -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.kill(handle),
        Err(_)      => ESP_ERR_INVALID_STATE,
    }
}

/// Return manifest pointers for all registered apps (for internal use).
pub fn list_apps() -> Vec<*const CAppManifest> {
    match APP_MANAGER.lock() {
        Ok(mgr) => mgr.list_apps(),
        Err(_)  => Vec::new(),
    }
}

/// Evict the LRU backgrounded/suspended app. Returns the evicted handle or
/// `APP_HANDLE_INVALID` if nothing could be evicted.
pub fn evict_lru() -> i32 {
    match APP_MANAGER.lock() {
        Ok(mut mgr) => mgr.evict_lru(),
        Err(_)      => APP_HANDLE_INVALID,
    }
}

/// Return the current free heap in bytes (MALLOC_CAP_DEFAULT region).
///
/// # Safety
/// Calls the ESP-IDF `heap_caps_get_free_size` C function.
pub unsafe fn get_free_memory() -> usize {
    heap_caps_get_free_size(MALLOC_CAP_DEFAULT)
}

// ---------------------------------------------------------------------------
// FFI exports — C-callable surface
// ---------------------------------------------------------------------------

/// Initialise the Rust app manager. Safe to call multiple times.
#[no_mangle]
pub extern "C" fn app_manager_init() -> i32 {
    init()
}

/// Register a static C app entry with the manager.
///
/// # Safety
/// `app` must be a valid pointer to a static `CAppEntry` whose lifetime
/// encompasses the entire kernel session.
#[no_mangle]
pub unsafe extern "C" fn app_manager_register(app: *const CAppEntry) -> i32 {
    register(app)
}

/// Launch (or resume) the app identified by the null-terminated `app_id`.
///
/// # Safety
/// `app_id` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn app_manager_launch(app_id: *const c_char) -> i32 {
    if app_id.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let id_str = match CStr::from_ptr(app_id).to_str() {
        Ok(s)  => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    launch(id_str)
}

/// Switch the foreground to `handle`.
#[no_mangle]
pub unsafe extern "C" fn app_manager_switch_to(handle: i32) -> i32 {
    switch_to(handle)
}

/// Return the handle of the foreground app, or `APP_HANDLE_INVALID` (-1).
#[no_mangle]
pub extern "C" fn app_manager_get_foreground() -> i32 {
    get_foreground()
}

/// Return the state of `handle` as a `u32` (maps to `app_state_t`).
#[no_mangle]
pub extern "C" fn app_manager_get_state(handle: i32) -> u32 {
    get_state(handle)
}

/// Suspend the app identified by `handle`.
#[no_mangle]
pub unsafe extern "C" fn app_manager_suspend(handle: i32) -> i32 {
    suspend(handle)
}

/// Destroy the app identified by `handle`.
#[no_mangle]
pub unsafe extern "C" fn app_manager_kill(handle: i32) -> i32 {
    kill(handle)
}

/// Evict the LRU backgrounded/suspended app.
#[no_mangle]
pub unsafe extern "C" fn app_manager_evict_lru() -> i32 {
    evict_lru()
}

/// Return free heap bytes (MALLOC_CAP_DEFAULT).
///
/// # Safety
/// Calls the ESP-IDF `heap_caps_get_free_size` C function.
#[no_mangle]
pub unsafe extern "C" fn app_manager_get_free_memory() -> usize {
    get_free_memory()
}

/// List all registered app manifests.
/// Writes up to `max_count` manifest pointers into `out[]`.
/// Returns the number written.
///
/// # Safety
/// `out` must point to an array of at least `max_count` pointers.
#[no_mangle]
pub unsafe extern "C" fn app_manager_list_apps(
    out: *mut *const CAppManifest,
    max_count: i32,
) -> i32 {
    if out.is_null() || max_count <= 0 {
        return 0;
    }
    let mgr = match APP_MANAGER.lock() {
        Ok(m) => m,
        Err(_) => return 0,
    };
    let mut count = 0i32;
    for slot in mgr.slots.iter() {
        if count >= max_count {
            break;
        }
        if !slot.entry.is_null() {
            let entry = &*slot.entry;
            if !entry.manifest.is_null() {
                *out.offset(count as isize) = entry.manifest;
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::sync::Mutex;

    // Guard all tests behind a single mutex so they don't race on the global
    // APP_MANAGER singleton.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    // -----------------------------------------------------------------------
    // Mock helpers
    // -----------------------------------------------------------------------

    fn make_test_manifest(id: &CStr) -> CAppManifest {
        CAppManifest {
            id:               id.as_ptr(),
            name:             id.as_ptr(),
            version:          b"1.0.0\0".as_ptr() as *const c_char,
            allow_background: false,
            min_memory_kb:    0,
        }
    }

    fn make_test_entry(manifest: *const CAppManifest) -> CAppEntry {
        CAppEntry {
            on_create:  None,
            on_start:   None,
            on_pause:   None,
            on_resume:  None,
            on_destroy: None,
            manifest,
        }
    }

    /// Reset the global manager to a pristine state between tests.
    fn reset_manager() {
        let mut mgr = APP_MANAGER.lock().unwrap();
        *mgr = AppManager::new();
        mgr.init();
    }

    // -----------------------------------------------------------------------
    // test_init
    // -----------------------------------------------------------------------

    #[test]
    fn test_init() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_manager();

        let rc = init();
        assert_eq!(rc, ESP_OK, "init() must return ESP_OK");
    }

    // -----------------------------------------------------------------------
    // test_register
    // -----------------------------------------------------------------------

    #[test]
    fn test_register() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_manager();

        let id = CStr::from_bytes_with_nul(b"test_app\0").unwrap();
        let manifest = make_test_manifest(id);
        let entry = make_test_entry(&manifest as *const CAppManifest);

        let rc = unsafe { register(&entry as *const CAppEntry) };
        assert_eq!(rc, ESP_OK, "register() must succeed");

        // Verify slot count increased.
        let mgr = APP_MANAGER.lock().unwrap();
        assert_eq!(mgr.slot_count, 1, "slot_count must be 1 after one registration");
    }

    // -----------------------------------------------------------------------
    // test_get_state
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_state() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_manager();

        let id = CStr::from_bytes_with_nul(b"state_app\0").unwrap();
        let manifest = make_test_manifest(id);
        let entry = make_test_entry(&manifest as *const CAppManifest);

        let rc = unsafe { register(&entry as *const CAppEntry) };
        assert_eq!(rc, ESP_OK);

        // Retrieve the assigned handle from the slot.
        let handle = {
            let mgr = APP_MANAGER.lock().unwrap();
            mgr.slots[0].handle
        };

        let state = get_state(handle);
        assert_eq!(
            state,
            AppState::Unloaded as u32,
            "freshly registered app must be in UNLOADED state"
        );
    }

    // -----------------------------------------------------------------------
    // test_invalid_handle
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_handle() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_manager();

        // APP_HANDLE_INVALID (-1)
        let state = get_state(APP_HANDLE_INVALID);
        assert_eq!(
            state,
            AppState::Unloaded as u32,
            "get_state(-1) must return UNLOADED"
        );

        // Arbitrary out-of-range handle
        let state = get_state(999);
        assert_eq!(
            state,
            AppState::Unloaded as u32,
            "get_state(999) must return UNLOADED for unknown handle"
        );
    }
}
