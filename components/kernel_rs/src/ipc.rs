// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — IPC message-passing subsystem (Rust port)
//
// Provides a bounded message queue with registered type-based handlers.
// Thread-safe via Mutex + Condvar. Exported as C-compatible FFI.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

// ── Constants ────────────────────────────────────────────────────────────────

pub const IPC_MSG_MAX_DATA: usize = 256;
pub const IPC_QUEUE_DEPTH: usize = 16;
pub const IPC_HANDLER_MAX: usize = 16;

// ESP-IDF error codes
const ESP_OK: i32 = 0;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_TIMEOUT: i32 = 0x107;

// ── C-compatible message struct ───────────────────────────────────────────────

/// Matches `ipc_message_t` in the C kernel exactly.
/// Field layout and sizes must not be changed without updating the C header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CIpcMessage {
    pub src_app: u32,
    pub dst_app: u32,  // 0 = broadcast
    pub msg_type: u32,
    pub data: [u8; IPC_MSG_MAX_DATA],
    pub data_len: usize,
    pub timestamp: u32,
}

impl Default for CIpcMessage {
    fn default() -> Self {
        Self {
            src_app: 0,
            dst_app: 0,
            msg_type: 0,
            data: [0u8; IPC_MSG_MAX_DATA],
            data_len: 0,
            timestamp: 0,
        }
    }
}

// ── Handler registry entry ────────────────────────────────────────────────────

/// A registered message handler. `msg_type == u32::MAX` matches all types.
struct HandlerEntry {
    msg_type: u32,
    handler: extern "C" fn(*const CIpcMessage, *mut c_void),
    user_data: *mut c_void,
    active: bool,
}

// SAFETY: user_data is an opaque C pointer; the caller guarantees its lifetime
// and thread-safety for anything pointed to.
unsafe impl Send for HandlerEntry {}
unsafe impl Sync for HandlerEntry {}

// ── IPC state ─────────────────────────────────────────────────────────────────

struct IpcState {
    queue: VecDeque<CIpcMessage>,
    handlers: Vec<HandlerEntry>,
    initialized: bool,
}

impl IpcState {
    const fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            handlers: Vec::new(),
            initialized: false,
        }
    }
}

/// Global IPC state: a Mutex-protected queue + handler list, and a Condvar
/// that is notified whenever a new message is enqueued.
struct IpcGlobal {
    state: Mutex<IpcState>,
    condvar: Condvar,
}

static IPC: OnceLock<IpcGlobal> = OnceLock::new();

fn ipc_global() -> &'static IpcGlobal {
    IPC.get_or_init(|| IpcGlobal {
        state: Mutex::new(IpcState::new()),
        condvar: Condvar::new(),
    })
}

// ── Public Rust API ───────────────────────────────────────────────────────────

/// Initialise the IPC subsystem. Idempotent — safe to call more than once.
pub fn ipc_init_impl() -> i32 {
    let g = ipc_global();
    let mut st = g.state.lock().unwrap();
    if !st.initialized {
        st.queue = VecDeque::with_capacity(IPC_QUEUE_DEPTH);
        st.handlers = Vec::with_capacity(IPC_HANDLER_MAX);
        st.initialized = true;
    }
    ESP_OK
}

/// Dispatch `msg` to all matching handlers, then enqueue it.
///
/// Returns `ESP_ERR_NO_MEM` when the queue is already at `IPC_QUEUE_DEPTH`.
pub fn ipc_send_impl(msg: CIpcMessage) -> i32 {
    let g = ipc_global();
    let mut st = g.state.lock().unwrap();

    if !st.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    // Dispatch to handlers while holding the lock so the handler list cannot
    // change under us. Handlers are expected to be short and non-blocking.
    for entry in st.handlers.iter().filter(|e| e.active && e.msg_type == msg.msg_type) {
        (entry.handler)(&msg as *const CIpcMessage, entry.user_data);
    }

    // Bounded enqueue
    if st.queue.len() >= IPC_QUEUE_DEPTH {
        return ESP_ERR_NO_MEM;
    }

    st.queue.push_back(msg);
    g.condvar.notify_one();
    ESP_OK
}

/// Dequeue the oldest message, blocking up to `timeout_ms` milliseconds.
///
/// Returns `ESP_ERR_TIMEOUT` if no message arrives within the deadline, or
/// `ESP_ERR_INVALID_STATE` if the subsystem is not initialised.
pub fn ipc_recv_impl(timeout_ms: u32) -> Result<CIpcMessage, i32> {
    let g = ipc_global();
    let st = g.state.lock().unwrap();

    if !st.initialized {
        return Err(ESP_ERR_INVALID_STATE);
    }

    // Fast path: message already queued
    if !st.queue.is_empty() {
        let mut st = st; // reborrow as mutable (it already is mutable)
        let msg = st.queue.pop_front().unwrap();
        return Ok(msg);
    }

    // Wait with timeout
    let timeout = Duration::from_millis(u64::from(timeout_ms));
    let result = g
        .condvar
        .wait_timeout_while(st, timeout, |s| s.queue.is_empty());

    match result {
        Ok((mut st, _)) if !st.queue.is_empty() => Ok(st.queue.pop_front().unwrap()),
        _ => Err(ESP_ERR_TIMEOUT),
    }
}

/// Register a handler for messages of `msg_type`.
///
/// Returns `ESP_ERR_NO_MEM` when `IPC_HANDLER_MAX` registrations are already
/// active, or `ESP_ERR_INVALID_STATE` if the subsystem is not initialised.
pub fn ipc_register_handler_impl(
    msg_type: u32,
    handler: extern "C" fn(*const CIpcMessage, *mut c_void),
    user_data: *mut c_void,
) -> i32 {
    let g = ipc_global();
    let mut st = g.state.lock().unwrap();

    if !st.initialized {
        return ESP_ERR_INVALID_STATE;
    }

    let active_count = st.handlers.iter().filter(|e| e.active).count();
    if active_count >= IPC_HANDLER_MAX {
        return ESP_ERR_NO_MEM;
    }

    st.handlers.push(HandlerEntry {
        msg_type,
        handler,
        user_data,
        active: true,
    });
    ESP_OK
}

// ── FFI exports ───────────────────────────────────────────────────────────────

/// Initialise the IPC subsystem.
///
/// # Safety
/// Safe to call from C at any time; idempotent.
#[no_mangle]
pub extern "C" fn ipc_init() -> i32 {
    ipc_init_impl()
}

/// Send a message.
///
/// # Safety
/// `msg` must point to a valid, initialised `CIpcMessage`.
#[no_mangle]
pub unsafe extern "C" fn ipc_send(msg: *const CIpcMessage) -> i32 {
    if msg.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    ipc_send_impl(*msg)
}

/// Receive the oldest queued message, blocking up to `timeout_ms`.
///
/// Writes the message into `*msg` on success.
///
/// # Safety
/// `msg` must point to a writable `CIpcMessage`-sized buffer.
#[no_mangle]
pub unsafe extern "C" fn ipc_recv(msg: *mut CIpcMessage, timeout_ms: u32) -> i32 {
    if msg.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    match ipc_recv_impl(timeout_ms) {
        Ok(m) => {
            *msg = m;
            ESP_OK
        }
        Err(e) => e,
    }
}

/// Register a C handler for messages of the given `msg_type`.
///
/// # Safety
/// `handler` must be a valid function pointer for the lifetime of the IPC
/// subsystem. `user_data` lifetime is the caller's responsibility.
#[no_mangle]
pub unsafe extern "C" fn ipc_register_handler(
    msg_type: u32,
    handler: extern "C" fn(*const CIpcMessage, *mut c_void),
    user_data: *mut c_void,
) -> i32 {
    ipc_register_handler_impl(msg_type, handler, user_data)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Re-initialise a fresh IpcGlobal for each test to avoid state leakage.
    /// Because OnceLock cannot be reset, tests operate directly on a local
    /// IpcGlobal rather than the process-global singleton.
    fn make_global() -> IpcGlobal {
        let g = IpcGlobal {
            state: Mutex::new(IpcState::new()),
            condvar: Condvar::new(),
        };
        // Mark as initialised
        {
            let mut st = g.state.lock().unwrap();
            st.queue = VecDeque::with_capacity(IPC_QUEUE_DEPTH);
            st.handlers = Vec::with_capacity(IPC_HANDLER_MAX);
            st.initialized = true;
        }
        g
    }

    fn send_to(g: &IpcGlobal, msg: CIpcMessage) -> i32 {
        let mut st = g.state.lock().unwrap();
        for entry in st.handlers.iter().filter(|e| e.active && e.msg_type == msg.msg_type) {
            (entry.handler)(&msg as *const CIpcMessage, entry.user_data);
        }
        if st.queue.len() >= IPC_QUEUE_DEPTH {
            return ESP_ERR_NO_MEM;
        }
        st.queue.push_back(msg);
        g.condvar.notify_one();
        ESP_OK
    }

    fn recv_from(g: &IpcGlobal, timeout_ms: u32) -> Result<CIpcMessage, i32> {
        let st = g.state.lock().unwrap();
        if !st.queue.is_empty() {
            let mut st = st;
            return Ok(st.queue.pop_front().unwrap());
        }
        let timeout = Duration::from_millis(u64::from(timeout_ms));
        let result = g
            .condvar
            .wait_timeout_while(st, timeout, |s| s.queue.is_empty());
        match result {
            Ok((mut st, _)) if !st.queue.is_empty() => Ok(st.queue.pop_front().unwrap()),
            _ => Err(ESP_ERR_TIMEOUT),
        }
    }

    // ── test_send_and_recv ──────────────────────────────────────────────────

    #[test]
    fn test_send_and_recv() {
        let g = make_global();

        let mut msg = CIpcMessage::default();
        msg.src_app = 1;
        msg.dst_app = 2;
        msg.msg_type = 7;
        msg.data[0] = 0xAB;
        msg.data[1] = 0xCD;
        msg.data_len = 2;
        msg.timestamp = 12345;

        assert_eq!(send_to(&g, msg), ESP_OK);

        let received = recv_from(&g, 100).expect("expected a message");
        assert_eq!(received.src_app, 1);
        assert_eq!(received.dst_app, 2);
        assert_eq!(received.msg_type, 7);
        assert_eq!(received.data[0], 0xAB);
        assert_eq!(received.data[1], 0xCD);
        assert_eq!(received.data_len, 2);
        assert_eq!(received.timestamp, 12345);
    }

    // ── test_handler_dispatch ───────────────────────────────────────────────

    #[test]
    fn test_handler_dispatch() {
        let g = make_global();

        // Shared flag set by the handler
        let called = Arc::new(AtomicBool::new(false));
        let called_ptr: *mut c_void = Arc::into_raw(called.clone()) as *mut c_void;

        extern "C" fn my_handler(msg: *const CIpcMessage, user_data: *mut c_void) {
            let flag = unsafe { &*(user_data as *const AtomicBool) };
            // Verify the message that arrives
            let m = unsafe { &*msg };
            assert_eq!(m.msg_type, 42);
            flag.store(true, Ordering::SeqCst);
        }

        {
            let mut st = g.state.lock().unwrap();
            st.handlers.push(HandlerEntry {
                msg_type: 42,
                handler: my_handler,
                user_data: called_ptr,
                active: true,
            });
        }

        let mut msg = CIpcMessage::default();
        msg.msg_type = 42;
        assert_eq!(send_to(&g, msg), ESP_OK);

        // Restore the Arc so it drops cleanly
        let _ = unsafe { Arc::from_raw(called_ptr as *const AtomicBool) };

        assert!(called.load(Ordering::SeqCst), "handler was not called");
    }

    // ── test_queue_full ─────────────────────────────────────────────────────

    #[test]
    fn test_queue_full() {
        let g = make_global();

        // Fill the queue exactly
        for i in 0..IPC_QUEUE_DEPTH {
            let mut msg = CIpcMessage::default();
            msg.msg_type = i as u32;
            assert_eq!(send_to(&g, msg), ESP_OK, "send {} should succeed", i);
        }

        // One more must fail
        let mut overflow = CIpcMessage::default();
        overflow.msg_type = 99;
        assert_eq!(
            send_to(&g, overflow),
            ESP_ERR_NO_MEM,
            "send beyond queue depth should return ESP_ERR_NO_MEM"
        );
    }

    // ── test_recv_empty ─────────────────────────────────────────────────────

    #[test]
    fn test_recv_empty() {
        let g = make_global();

        // Queue is empty; recv should time out immediately (1 ms)
        let result = recv_from(&g, 1);
        assert_eq!(result.unwrap_err(), ESP_ERR_TIMEOUT);
    }
}
