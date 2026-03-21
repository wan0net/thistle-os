// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — Rust event bus
//
// Port of the C event bus (pub/sub) system. Dispatches events to registered
// subscribers. Exposes a C-compatible FFI surface for integration with existing
// C drivers and apps.

use std::sync::Mutex;
use std::os::raw::c_void;

// ---------------------------------------------------------------------------
// ESP-IDF error codes (matching esp_err.h)
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_NOT_FOUND: i32 = 0x105;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of distinct event types (matches C EVENT_MAX).
pub const EVENT_MAX: usize = 17;

/// Maximum subscribers per event type.
pub const EVENT_SUBSCRIBERS_MAX: usize = 8;

// ---------------------------------------------------------------------------
// EventType enum — repr(u32) to match the C enum layout
// ---------------------------------------------------------------------------

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Convert a raw u32 from C into an EventType, returning None if out of range.
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0  => Some(Self::SystemBoot),
            1  => Some(Self::SystemShutdown),
            2  => Some(Self::AppLaunched),
            3  => Some(Self::AppStopped),
            4  => Some(Self::AppSwitched),
            5  => Some(Self::InputKey),
            6  => Some(Self::InputTouch),
            7  => Some(Self::RadioRx),
            8  => Some(Self::GpsFix),
            9  => Some(Self::BatteryLow),
            10 => Some(Self::BatteryCharging),
            11 => Some(Self::SdMounted),
            12 => Some(Self::SdUnmounted),
            13 => Some(Self::WifiConnected),
            14 => Some(Self::WifiDisconnected),
            15 => Some(Self::BleConnected),
            16 => Some(Self::BleDisconnected),
            _  => None,
        }
    }
}

// ---------------------------------------------------------------------------
// CEvent — repr(C) struct matching the C event_t layout exactly
// ---------------------------------------------------------------------------

/// Mirrors the C `event_t` struct. Must not be reordered.
///
/// ```c
/// typedef struct {
///     event_type_t type;    // u32
///     uint32_t     timestamp;
///     void        *data;
///     size_t       data_len;
/// } event_t;
/// ```
#[repr(C)]
pub struct CEvent {
    pub event_type: u32,
    pub timestamp:  u32,
    pub data:       *mut c_void,
    pub data_len:   usize,
}

// SAFETY: CEvent is only a plain-data carrier. The raw pointer `data` is
// managed by the caller; we never dereference it inside the bus.
unsafe impl Send for CEvent {}
unsafe impl Sync for CEvent {}

// ---------------------------------------------------------------------------
// Subscriber
// ---------------------------------------------------------------------------

/// A single registered handler plus its opaque user-data pointer.
///
/// Function pointers are `Copy` and inherently `Send` (they hold no state).
/// The `user_data` raw pointer is caller-managed; the bus never dereferences it.
#[derive(Clone, Copy)]
struct Subscriber {
    handler:   extern "C" fn(*const CEvent, *mut c_void),
    user_data: *mut c_void,
}

// SAFETY: The handler is a plain fn pointer. user_data lifetime/threading is
// the caller's responsibility, matching the contract of the equivalent C API.
unsafe impl Send for Subscriber {}

// ---------------------------------------------------------------------------
// EventBus
// ---------------------------------------------------------------------------

struct EventBus {
    /// For each of the EVENT_MAX event types, up to EVENT_SUBSCRIBERS_MAX slots.
    subscribers: [[Option<Subscriber>; EVENT_SUBSCRIBERS_MAX]; EVENT_MAX],
}

impl EventBus {
    const fn new() -> Self {
        Self {
            subscribers: [[None; EVENT_SUBSCRIBERS_MAX]; EVENT_MAX],
        }
    }

    fn subscribe(
        &mut self,
        event_type: usize,
        handler: extern "C" fn(*const CEvent, *mut c_void),
        user_data: *mut c_void,
    ) -> i32 {
        let slots = &mut self.subscribers[event_type];
        for slot in slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(Subscriber { handler, user_data });
                return ESP_OK;
            }
        }
        ESP_ERR_NO_MEM
    }

    fn unsubscribe(
        &mut self,
        event_type: usize,
        handler: extern "C" fn(*const CEvent, *mut c_void),
    ) -> i32 {
        let slots = &mut self.subscribers[event_type];
        for slot in slots.iter_mut() {
            if let Some(sub) = slot {
                if sub.handler as usize == handler as usize {
                    *slot = None;
                    return ESP_OK;
                }
            }
        }
        ESP_ERR_NOT_FOUND
    }

    fn publish(&self, event: *const CEvent) -> i32 {
        // SAFETY: caller guarantees the pointer is valid and points to a
        // correctly-initialised CEvent. We only read it here.
        let ev = unsafe { &*event };
        let idx = ev.event_type as usize;
        if idx >= EVENT_MAX {
            return ESP_ERR_INVALID_ARG;
        }
        for slot in &self.subscribers[idx] {
            if let Some(sub) = slot {
                (sub.handler)(event, sub.user_data);
            }
        }
        ESP_OK
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static EVENT_BUS: Mutex<EventBus> = Mutex::new(EventBus::new());

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the event bus. Idempotent; safe to call multiple times.
#[no_mangle]
pub extern "C" fn event_bus_init() -> i32 {
    // The bus is zero-initialised at static construction time.
    // Nothing to do; exposed for symmetry with the C API.
    ESP_OK
}

/// Register `handler` to receive events of `event_type`.
///
/// Returns `ESP_OK` on success, `ESP_ERR_INVALID_ARG` if the event type is out
/// of range, or `ESP_ERR_NO_MEM` if all subscriber slots are full.
#[no_mangle]
pub extern "C" fn event_subscribe(
    event_type: u32,
    handler: extern "C" fn(*const CEvent, *mut c_void),
    user_data: *mut c_void,
) -> i32 {
    let idx = match EventType::from_u32(event_type) {
        Some(t) => t as usize,
        None    => return ESP_ERR_INVALID_ARG,
    };
    match EVENT_BUS.lock() {
        Ok(mut bus) => bus.subscribe(idx, handler, user_data),
        Err(_)      => ESP_ERR_INVALID_ARG,
    }
}

/// Remove the first matching registration of `handler` for `event_type`.
///
/// Returns `ESP_OK` on success, `ESP_ERR_INVALID_ARG` if the event type is out
/// of range, or `ESP_ERR_NOT_FOUND` if `handler` was never registered.
#[no_mangle]
pub extern "C" fn event_unsubscribe(
    event_type: u32,
    handler: extern "C" fn(*const CEvent, *mut c_void),
) -> i32 {
    let idx = match EventType::from_u32(event_type) {
        Some(t) => t as usize,
        None    => return ESP_ERR_INVALID_ARG,
    };
    match EVENT_BUS.lock() {
        Ok(mut bus) => bus.unsubscribe(idx, handler),
        Err(_)      => ESP_ERR_INVALID_ARG,
    }
}

/// Dispatch `event` to all registered subscribers of `event->type`.
///
/// Handlers are called synchronously in registration order. Returns `ESP_OK`
/// on success, `ESP_ERR_INVALID_ARG` if `event` is null or the type is out of
/// range.
#[no_mangle]
pub extern "C" fn event_publish(event: *const CEvent) -> i32 {
    if event.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match EVENT_BUS.lock() {
        Ok(bus) => bus.publish(event),
        Err(_)  => ESP_ERR_INVALID_ARG,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::os::raw::c_void;

    // Each test manipulates the global bus, so we reset affected slots by
    // unsubscribing after the test rather than sharing mutable state across
    // tests (which would require a test mutex).

    fn make_event(event_type: EventType) -> CEvent {
        CEvent {
            event_type: event_type as u32,
            timestamp:  0,
            data:       std::ptr::null_mut(),
            data_len:   0,
        }
    }

    // -----------------------------------------------------------------------
    // test_subscribe_and_publish
    // -----------------------------------------------------------------------

    static COUNTER_SP: AtomicU32 = AtomicU32::new(0);

    extern "C" fn handler_sp(_event: *const CEvent, _ud: *mut c_void) {
        COUNTER_SP.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn test_subscribe_and_publish() {
        COUNTER_SP.store(0, Ordering::SeqCst);

        let rc = event_subscribe(
            EventType::SystemBoot as u32,
            handler_sp,
            std::ptr::null_mut(),
        );
        assert_eq!(rc, ESP_OK);

        let ev = make_event(EventType::SystemBoot);
        let rc = event_publish(&ev as *const CEvent);
        assert_eq!(rc, ESP_OK);
        assert_eq!(COUNTER_SP.load(Ordering::SeqCst), 1);

        // Clean up
        event_unsubscribe(EventType::SystemBoot as u32, handler_sp);
    }

    // -----------------------------------------------------------------------
    // test_unsubscribe
    // -----------------------------------------------------------------------

    static COUNTER_US: AtomicU32 = AtomicU32::new(0);

    extern "C" fn handler_us(_event: *const CEvent, _ud: *mut c_void) {
        COUNTER_US.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn test_unsubscribe() {
        COUNTER_US.store(0, Ordering::SeqCst);

        let rc = event_subscribe(
            EventType::SystemShutdown as u32,
            handler_us,
            std::ptr::null_mut(),
        );
        assert_eq!(rc, ESP_OK);

        let rc = event_unsubscribe(EventType::SystemShutdown as u32, handler_us);
        assert_eq!(rc, ESP_OK);

        let ev = make_event(EventType::SystemShutdown);
        event_publish(&ev as *const CEvent);

        assert_eq!(
            COUNTER_US.load(Ordering::SeqCst),
            0,
            "handler must not be called after unsubscribe"
        );
    }

    // -----------------------------------------------------------------------
    // test_multiple_subscribers
    // -----------------------------------------------------------------------

    static COUNTER_A: AtomicU32 = AtomicU32::new(0);
    static COUNTER_B: AtomicU32 = AtomicU32::new(0);
    static COUNTER_C: AtomicU32 = AtomicU32::new(0);

    extern "C" fn handler_a(_event: *const CEvent, _ud: *mut c_void) {
        COUNTER_A.fetch_add(1, Ordering::SeqCst);
    }
    extern "C" fn handler_b(_event: *const CEvent, _ud: *mut c_void) {
        COUNTER_B.fetch_add(1, Ordering::SeqCst);
    }
    extern "C" fn handler_c(_event: *const CEvent, _ud: *mut c_void) {
        COUNTER_C.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn test_multiple_subscribers() {
        COUNTER_A.store(0, Ordering::SeqCst);
        COUNTER_B.store(0, Ordering::SeqCst);
        COUNTER_C.store(0, Ordering::SeqCst);

        let etype = EventType::RadioRx as u32;
        assert_eq!(event_subscribe(etype, handler_a, std::ptr::null_mut()), ESP_OK);
        assert_eq!(event_subscribe(etype, handler_b, std::ptr::null_mut()), ESP_OK);
        assert_eq!(event_subscribe(etype, handler_c, std::ptr::null_mut()), ESP_OK);

        let ev = make_event(EventType::RadioRx);
        let rc = event_publish(&ev as *const CEvent);
        assert_eq!(rc, ESP_OK);

        assert_eq!(COUNTER_A.load(Ordering::SeqCst), 1, "handler_a not called");
        assert_eq!(COUNTER_B.load(Ordering::SeqCst), 1, "handler_b not called");
        assert_eq!(COUNTER_C.load(Ordering::SeqCst), 1, "handler_c not called");

        // Clean up
        event_unsubscribe(etype, handler_a);
        event_unsubscribe(etype, handler_b);
        event_unsubscribe(etype, handler_c);
    }

    // -----------------------------------------------------------------------
    // test_invalid_event_type
    // -----------------------------------------------------------------------

    extern "C" fn handler_dummy(_event: *const CEvent, _ud: *mut c_void) {}

    #[test]
    fn test_invalid_event_type() {
        let out_of_range = EVENT_MAX as u32; // == 17 == EVENT_MAX

        let rc = event_subscribe(out_of_range, handler_dummy, std::ptr::null_mut());
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "subscribe with invalid type must fail");

        let rc = event_unsubscribe(out_of_range, handler_dummy);
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "unsubscribe with invalid type must fail");

        // publish: build an event with an out-of-range type directly
        let ev = CEvent {
            event_type: out_of_range,
            timestamp:  0,
            data:       std::ptr::null_mut(),
            data_len:   0,
        };
        let rc = event_publish(&ev as *const CEvent);
        assert_eq!(rc, ESP_ERR_INVALID_ARG, "publish with invalid type must fail");
    }
}
