// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Secure wipe manager
//
// Manages emergency data destruction for sensitive environments.
// This module handles wipe planning, overwrite pattern generation,
// progress tracking, and the state machine for panic wipe sequences.
//
// Does NOT perform actual filesystem I/O — that happens via the storage
// HAL or syscalls at runtime. This module provides the protocol layer.

use std::ffi::CStr;
use std::os::raw::c_char;

// ── ESP error codes ─────────────────────────────────────────────────────────

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ── Xorshift64 PRNG ────────────────────────────────────────────────────────

/// Simple xorshift64 PRNG for generating random overwrite data.
/// Not cryptographically secure — sufficient for overwrite patterns.
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    pub fn new(seed: u64) -> Self {
        // Avoid zero state (xorshift64 has a fixed point at 0)
        let state = if seed == 0 { 0x5A5A_5A5A_5A5A_5A5A } else { seed };
        Self { state }
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }
}

// ── Gutmann patterns ───────────────────────────────────────────────────────
//
// The 35-pass Gutmann method. Passes 1-4 and 32-35 use random data (None).
// Passes 5-31 use specific 3-byte repeating patterns (Some([a, b, c])).

const GUTMANN_PATTERNS: [Option<[u8; 3]>; 35] = [
    None,                          // Pass  1: random
    None,                          // Pass  2: random
    None,                          // Pass  3: random
    None,                          // Pass  4: random
    Some([0x55, 0x55, 0x55]),      // Pass  5
    Some([0xAA, 0xAA, 0xAA]),      // Pass  6
    Some([0x92, 0x49, 0x24]),      // Pass  7
    Some([0x49, 0x24, 0x92]),      // Pass  8
    Some([0x24, 0x92, 0x49]),      // Pass  9
    Some([0x00, 0x00, 0x00]),      // Pass 10
    Some([0x11, 0x11, 0x11]),      // Pass 11
    Some([0x22, 0x22, 0x22]),      // Pass 12
    Some([0x33, 0x33, 0x33]),      // Pass 13
    Some([0x44, 0x44, 0x44]),      // Pass 14
    Some([0x55, 0x55, 0x55]),      // Pass 15
    Some([0x66, 0x66, 0x66]),      // Pass 16
    Some([0x77, 0x77, 0x77]),      // Pass 17
    Some([0x88, 0x88, 0x88]),      // Pass 18
    Some([0x99, 0x99, 0x99]),      // Pass 19
    Some([0xAA, 0xAA, 0xAA]),      // Pass 20
    Some([0xBB, 0xBB, 0xBB]),      // Pass 21
    Some([0xCC, 0xCC, 0xCC]),      // Pass 22
    Some([0xDD, 0xDD, 0xDD]),      // Pass 23
    Some([0xEE, 0xEE, 0xEE]),      // Pass 24
    Some([0xFF, 0xFF, 0xFF]),       // Pass 25
    Some([0x92, 0x49, 0x24]),      // Pass 26
    Some([0x49, 0x24, 0x92]),      // Pass 27
    Some([0x24, 0x92, 0x49]),      // Pass 28
    Some([0x6D, 0xB6, 0xDB]),      // Pass 29
    Some([0xB6, 0xDB, 0x6D]),      // Pass 30
    Some([0xDB, 0x6D, 0xB6]),      // Pass 31
    None,                          // Pass 32: random
    None,                          // Pass 33: random
    None,                          // Pass 34: random
    None,                          // Pass 35: random
];

// ── WipePattern ────────────────────────────────────────────────────────────

/// Overwrite strategy for secure deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WipePattern {
    /// Single pass of 0x00
    Zeros,
    /// Single pass of 0xFF
    Ones,
    /// Single pass of pseudo-random data
    Random,
    /// US DoD 5220.22-M: pass 0 = 0x00, pass 1 = 0xFF, pass 2 = random
    DoD3Pass,
    /// 35-pass Gutmann method
    Gutmann,
}

impl WipePattern {
    /// Create from integer representation (for FFI).
    fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Zeros),
            1 => Some(Self::Ones),
            2 => Some(Self::Random),
            3 => Some(Self::DoD3Pass),
            4 => Some(Self::Gutmann),
            _ => None,
        }
    }
}

// ── WipePriority ───────────────────────────────────────────────────────────

/// Priority for wipe targets. Lower numeric value = wiped first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum WipePriority {
    /// Wipe first: encryption keys, credentials
    Critical = 0,
    /// Wipe second: documents, messages
    High = 1,
    /// Wipe third: app data, logs
    Normal = 2,
    /// Wipe last: cached data, preferences
    Low = 3,
}

impl WipePriority {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Critical),
            1 => Some(Self::High),
            2 => Some(Self::Normal),
            3 => Some(Self::Low),
            _ => None,
        }
    }
}

// ── WipeStatus ─────────────────────────────────────────────────────────────

/// Status of a wipe target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WipeStatus {
    /// Not yet wiped
    Pending,
    /// Currently being overwritten
    InProgress,
    /// Successfully wiped and verified
    Completed,
    /// Wipe failed with reason
    Failed(String),
    /// Skipped (file not found, etc.)
    Skipped,
}

// ── WipeTarget ─────────────────────────────────────────────────────────────

/// A file or directory to be securely erased.
#[derive(Debug, Clone)]
pub struct WipeTarget {
    pub path: String,
    pub size_bytes: u64,
    pub is_directory: bool,
    pub priority: WipePriority,
    pub status: WipeStatus,
}

// ── WipeSummary ────────────────────────────────────────────────────────────

/// Summary of wipe plan status.
#[derive(Debug, Clone)]
pub struct WipeSummary {
    pub total_targets: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub pending: usize,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub pattern: WipePattern,
}

// ── WipePlan ───────────────────────────────────────────────────────────────

/// Manages the secure wipe sequence: target list, pattern, progress.
pub struct WipePlan {
    targets: Vec<WipeTarget>,
    pattern: WipePattern,
    prng: Xorshift64,
}

impl WipePlan {
    /// Create a new wipe plan with the specified overwrite pattern.
    pub fn new(pattern: WipePattern) -> Self {
        Self {
            targets: Vec::new(),
            pattern,
            prng: Xorshift64::new(0xDEAD_BEEF_CAFE_BABE),
        }
    }

    /// Add a file or directory to the wipe plan.
    pub fn add_target(
        &mut self,
        path: &str,
        size_bytes: u64,
        is_directory: bool,
        priority: WipePriority,
    ) {
        self.targets.push(WipeTarget {
            path: path.to_string(),
            size_bytes,
            is_directory,
            priority,
            status: WipeStatus::Pending,
        });
        // Keep targets sorted by priority (stable sort preserves insertion order within priority)
        self.targets.sort_by_key(|t| t.priority);
    }

    /// Add standard ThistleOS sensitive paths.
    pub fn add_default_targets(&mut self) {
        // Critical: encryption keys, credentials
        self.add_target("/sdcard/vault/", 0, true, WipePriority::Critical);
        self.add_target("/sdcard/config/keys/", 0, true, WipePriority::Critical);

        // High: documents, messages, notes
        self.add_target("/sdcard/messages/", 0, true, WipePriority::High);
        self.add_target("/sdcard/notes/", 0, true, WipePriority::High);
        self.add_target("/sdcard/documents/", 0, true, WipePriority::High);

        // Normal: app data, logs
        self.add_target("/sdcard/apps/", 0, true, WipePriority::Normal);
        self.add_target("/sdcard/logs/", 0, true, WipePriority::Normal);

        // Low: cached data, thumbnails
        self.add_target("/sdcard/cache/", 0, true, WipePriority::Low);
        self.add_target("/sdcard/thumbnails/", 0, true, WipePriority::Low);
    }

    /// All targets sorted by priority.
    pub fn targets(&self) -> &[WipeTarget] {
        &self.targets
    }

    /// Targets filtered by a specific priority.
    pub fn targets_by_priority(&self, priority: WipePriority) -> Vec<&WipeTarget> {
        self.targets.iter().filter(|t| t.priority == priority).collect()
    }

    /// Number of targets still pending.
    pub fn pending_count(&self) -> usize {
        self.targets.iter().filter(|t| t.status == WipeStatus::Pending).count()
    }

    /// Number of targets that completed successfully.
    pub fn completed_count(&self) -> usize {
        self.targets.iter().filter(|t| t.status == WipeStatus::Completed).count()
    }

    /// Number of targets that failed.
    pub fn failed_count(&self) -> usize {
        self.targets.iter().filter(|t| matches!(t.status, WipeStatus::Failed(_))).count()
    }

    /// Total number of targets in the plan.
    pub fn total_count(&self) -> usize {
        self.targets.len()
    }

    /// Total bytes across all targets.
    pub fn total_bytes(&self) -> u64 {
        self.targets.iter().map(|t| t.size_bytes).sum()
    }

    /// Bytes from completed targets.
    pub fn completed_bytes(&self) -> u64 {
        self.targets
            .iter()
            .filter(|t| t.status == WipeStatus::Completed)
            .map(|t| t.size_bytes)
            .sum()
    }

    /// Progress as a percentage (0-100) based on bytes.
    /// Returns 100 if total_bytes is 0 and all targets are done.
    pub fn progress_percent(&self) -> u8 {
        let total = self.total_bytes();
        if total == 0 {
            // When sizes are unknown, use target count
            if self.targets.is_empty() {
                return 100;
            }
            let done = self.targets.iter().filter(|t| {
                matches!(t.status, WipeStatus::Completed | WipeStatus::Skipped)
            }).count();
            let pct = (done as u64 * 100) / self.targets.len() as u64;
            pct.min(100) as u8
        } else {
            let completed = self.completed_bytes();
            let pct = (completed * 100) / total;
            pct.min(100) as u8
        }
    }

    /// Whether all targets have been completed or skipped.
    pub fn is_complete(&self) -> bool {
        !self.targets.is_empty()
            && self.targets.iter().all(|t| {
                matches!(t.status, WipeStatus::Completed | WipeStatus::Skipped | WipeStatus::Failed(_))
            })
    }

    /// Mark a target as in-progress.
    pub fn mark_in_progress(&mut self, index: usize) -> Result<(), i32> {
        let target = self.targets.get_mut(index).ok_or(ESP_ERR_INVALID_ARG)?;
        if target.status != WipeStatus::Pending {
            return Err(ESP_ERR_INVALID_STATE);
        }
        target.status = WipeStatus::InProgress;
        Ok(())
    }

    /// Mark a target as completed.
    pub fn mark_completed(&mut self, index: usize) -> Result<(), i32> {
        let target = self.targets.get_mut(index).ok_or(ESP_ERR_INVALID_ARG)?;
        if !matches!(target.status, WipeStatus::InProgress | WipeStatus::Pending) {
            return Err(ESP_ERR_INVALID_STATE);
        }
        target.status = WipeStatus::Completed;
        Ok(())
    }

    /// Mark a target as failed with a reason.
    pub fn mark_failed(&mut self, index: usize, reason: &str) -> Result<(), i32> {
        let target = self.targets.get_mut(index).ok_or(ESP_ERR_INVALID_ARG)?;
        if !matches!(target.status, WipeStatus::InProgress | WipeStatus::Pending) {
            return Err(ESP_ERR_INVALID_STATE);
        }
        target.status = WipeStatus::Failed(reason.to_string());
        Ok(())
    }

    /// Mark a target as skipped.
    pub fn mark_skipped(&mut self, index: usize) -> Result<(), i32> {
        let target = self.targets.get_mut(index).ok_or(ESP_ERR_INVALID_ARG)?;
        if !matches!(target.status, WipeStatus::Pending | WipeStatus::InProgress) {
            return Err(ESP_ERR_INVALID_STATE);
        }
        target.status = WipeStatus::Skipped;
        Ok(())
    }

    /// Index of the next pending target (by priority order).
    pub fn next_pending(&self) -> Option<usize> {
        // Targets are kept sorted by priority, so first pending is highest priority
        self.targets.iter().position(|t| t.status == WipeStatus::Pending)
    }

    /// The overwrite pattern for this plan.
    pub fn pattern(&self) -> &WipePattern {
        &self.pattern
    }

    /// Number of overwrite passes required by the pattern.
    pub fn passes_required(&self) -> usize {
        match self.pattern {
            WipePattern::Zeros => 1,
            WipePattern::Ones => 1,
            WipePattern::Random => 1,
            WipePattern::DoD3Pass => 3,
            WipePattern::Gutmann => 35,
        }
    }

    /// Generate a block of overwrite data for the given pass number.
    ///
    /// - Zeros: all 0x00
    /// - Ones: all 0xFF
    /// - Random: xorshift64 pseudo-random bytes
    /// - DoD3Pass: pass 0 = zeros, pass 1 = ones, pass 2 = random
    /// - Gutmann: appropriate pattern for the pass number (0-34)
    pub fn generate_overwrite_block(&mut self, pass: usize, block_size: usize) -> Vec<u8> {
        match self.pattern {
            WipePattern::Zeros => vec![0x00; block_size],
            WipePattern::Ones => vec![0xFF; block_size],
            WipePattern::Random => self.generate_random_block(block_size),
            WipePattern::DoD3Pass => {
                match pass {
                    0 => vec![0x00; block_size],
                    1 => vec![0xFF; block_size],
                    _ => self.generate_random_block(block_size),
                }
            }
            WipePattern::Gutmann => {
                let pass_idx = pass % 35;
                match GUTMANN_PATTERNS[pass_idx] {
                    Some(pattern) => {
                        let mut block = vec![0u8; block_size];
                        for i in 0..block_size {
                            block[i] = pattern[i % 3];
                        }
                        block
                    }
                    None => self.generate_random_block(block_size),
                }
            }
        }
    }

    /// Generate a summary of the current wipe plan status.
    pub fn summary(&self) -> WipeSummary {
        WipeSummary {
            total_targets: self.total_count(),
            completed: self.completed_count(),
            failed: self.failed_count(),
            skipped: self.targets.iter().filter(|t| t.status == WipeStatus::Skipped).count(),
            pending: self.pending_count(),
            total_bytes: self.total_bytes(),
            completed_bytes: self.completed_bytes(),
            pattern: self.pattern,
        }
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn generate_random_block(&mut self, block_size: usize) -> Vec<u8> {
        let mut block = vec![0u8; block_size];
        let mut i = 0;
        while i < block_size {
            let val = self.prng.next();
            let bytes = val.to_le_bytes();
            let remaining = block_size - i;
            let copy_len = remaining.min(8);
            block[i..i + copy_len].copy_from_slice(&bytes[..copy_len]);
            i += copy_len;
        }
        block
    }
}

// ── C FFI exports ──────────────────────────────────────────────────────────

/// Create a new wipe plan.
/// pattern: 0=Zeros, 1=Ones, 2=Random, 3=DoD3Pass, 4=Gutmann
/// Returns null on invalid pattern.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_create(pattern: i32) -> *mut WipePlan {
    match WipePattern::from_i32(pattern) {
        Some(p) => Box::into_raw(Box::new(WipePlan::new(p))),
        None => std::ptr::null_mut(),
    }
}

/// Destroy a wipe plan and free its memory.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_destroy(plan: *mut WipePlan) {
    if !plan.is_null() {
        drop(Box::from_raw(plan));
    }
}

/// Add a target to the wipe plan.
/// Returns ESP_OK on success, ESP_ERR_INVALID_ARG on null pointers or invalid priority.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_add_target(
    plan: *mut WipePlan,
    path: *const c_char,
    size: u64,
    is_dir: i32,
    priority: u8,
) -> i32 {
    if plan.is_null() || path.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let plan = &mut *plan;
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return ESP_ERR_INVALID_ARG,
    };
    let prio = match WipePriority::from_u8(priority) {
        Some(p) => p,
        None => return ESP_ERR_INVALID_ARG,
    };
    plan.add_target(path_str, size, is_dir != 0, prio);
    ESP_OK
}

/// Add default ThistleOS sensitive paths to the wipe plan.
/// Returns ESP_OK on success, ESP_ERR_INVALID_ARG on null pointer.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_add_defaults(plan: *mut WipePlan) -> i32 {
    if plan.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let plan = &mut *plan;
    plan.add_default_targets();
    ESP_OK
}

/// Get the index of the next pending target, or -1 if none remain.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_next_pending(plan: *const WipePlan) -> i32 {
    if plan.is_null() {
        return -1;
    }
    let plan = &*plan;
    match plan.next_pending() {
        Some(idx) => idx as i32,
        None => -1,
    }
}

/// Mark a target as completed.
/// Returns ESP_OK on success, or an error code.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_mark_completed(plan: *mut WipePlan, index: i32) -> i32 {
    if plan.is_null() || index < 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let plan = &mut *plan;
    match plan.mark_completed(index as usize) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

/// Mark a target as failed with a reason string.
/// Returns ESP_OK on success, or an error code.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_mark_failed(
    plan: *mut WipePlan,
    index: i32,
    reason: *const c_char,
) -> i32 {
    if plan.is_null() || index < 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let plan = &mut *plan;
    let reason_str = if reason.is_null() {
        "unknown"
    } else {
        match CStr::from_ptr(reason).to_str() {
            Ok(s) => s,
            Err(_) => "invalid reason string",
        }
    };
    match plan.mark_failed(index as usize, reason_str) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

/// Get progress as a percentage (0-100).
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_progress(plan: *const WipePlan) -> u8 {
    if plan.is_null() {
        return 0;
    }
    let plan = &*plan;
    plan.progress_percent()
}

/// Check if the wipe plan is complete. Returns 1 if yes, 0 if no.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_is_complete(plan: *const WipePlan) -> i32 {
    if plan.is_null() {
        return 0;
    }
    let plan = &*plan;
    if plan.is_complete() { 1 } else { 0 }
}

/// Generate a block of overwrite data for the given pass number.
/// Writes up to buf_len bytes into buf.
/// Returns ESP_OK on success, ESP_ERR_INVALID_ARG on null pointers.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_generate_block(
    plan: *mut WipePlan,
    pass: u32,
    buf: *mut u8,
    buf_len: usize,
) -> i32 {
    if plan.is_null() || buf.is_null() || buf_len == 0 {
        return ESP_ERR_INVALID_ARG;
    }
    let plan = &mut *plan;
    let block = plan.generate_overwrite_block(pass as usize, buf_len);
    std::ptr::copy_nonoverlapping(block.as_ptr(), buf, buf_len);
    ESP_OK
}

/// Get the number of overwrite passes required by the plan's pattern.
#[no_mangle]
pub unsafe extern "C" fn rs_wipe_plan_passes_required(plan: *const WipePlan) -> i32 {
    if plan.is_null() {
        return 0;
    }
    let plan = &*plan;
    plan.passes_required() as i32
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    // -----------------------------------------------------------------------
    // test_create_plan_zeros
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_plan_zeros() {
        let plan = WipePlan::new(WipePattern::Zeros);
        assert_eq!(*plan.pattern(), WipePattern::Zeros);
        assert_eq!(plan.total_count(), 0);
    }

    // -----------------------------------------------------------------------
    // test_create_plan_ones
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_plan_ones() {
        let plan = WipePlan::new(WipePattern::Ones);
        assert_eq!(*plan.pattern(), WipePattern::Ones);
    }

    // -----------------------------------------------------------------------
    // test_create_plan_random
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_plan_random() {
        let plan = WipePlan::new(WipePattern::Random);
        assert_eq!(*plan.pattern(), WipePattern::Random);
    }

    // -----------------------------------------------------------------------
    // test_create_plan_dod3pass
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_plan_dod3pass() {
        let plan = WipePlan::new(WipePattern::DoD3Pass);
        assert_eq!(*plan.pattern(), WipePattern::DoD3Pass);
    }

    // -----------------------------------------------------------------------
    // test_create_plan_gutmann
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_plan_gutmann() {
        let plan = WipePlan::new(WipePattern::Gutmann);
        assert_eq!(*plan.pattern(), WipePattern::Gutmann);
    }

    // -----------------------------------------------------------------------
    // test_add_targets_and_verify_count
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_targets_and_verify_count() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/test1.txt", 1024, false, WipePriority::Normal);
        plan.add_target("/sdcard/test2.txt", 2048, false, WipePriority::High);
        assert_eq!(plan.total_count(), 2);
    }

    // -----------------------------------------------------------------------
    // test_add_default_targets_paths_and_priorities
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_default_targets_paths_and_priorities() {
        let mut plan = WipePlan::new(WipePattern::DoD3Pass);
        plan.add_default_targets();

        assert_eq!(plan.total_count(), 9);

        // Critical targets (first 2)
        let critical = plan.targets_by_priority(WipePriority::Critical);
        assert_eq!(critical.len(), 2);
        let critical_paths: Vec<&str> = critical.iter().map(|t| t.path.as_str()).collect();
        assert!(critical_paths.contains(&"/sdcard/vault/"));
        assert!(critical_paths.contains(&"/sdcard/config/keys/"));

        // High targets (next 3)
        let high = plan.targets_by_priority(WipePriority::High);
        assert_eq!(high.len(), 3);
        let high_paths: Vec<&str> = high.iter().map(|t| t.path.as_str()).collect();
        assert!(high_paths.contains(&"/sdcard/messages/"));
        assert!(high_paths.contains(&"/sdcard/notes/"));
        assert!(high_paths.contains(&"/sdcard/documents/"));

        // Normal targets (next 2)
        let normal = plan.targets_by_priority(WipePriority::Normal);
        assert_eq!(normal.len(), 2);

        // Low targets (last 2)
        let low = plan.targets_by_priority(WipePriority::Low);
        assert_eq!(low.len(), 2);
    }

    // -----------------------------------------------------------------------
    // test_targets_sorted_by_priority
    // -----------------------------------------------------------------------

    #[test]
    fn test_targets_sorted_by_priority() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        // Add in reverse priority order
        plan.add_target("/sdcard/cache/", 100, true, WipePriority::Low);
        plan.add_target("/sdcard/vault/", 500, true, WipePriority::Critical);
        plan.add_target("/sdcard/logs/", 200, true, WipePriority::Normal);
        plan.add_target("/sdcard/messages/", 300, true, WipePriority::High);

        let targets = plan.targets();
        assert_eq!(targets[0].priority, WipePriority::Critical);
        assert_eq!(targets[1].priority, WipePriority::High);
        assert_eq!(targets[2].priority, WipePriority::Normal);
        assert_eq!(targets[3].priority, WipePriority::Low);
    }

    // -----------------------------------------------------------------------
    // test_filter_by_priority
    // -----------------------------------------------------------------------

    #[test]
    fn test_filter_by_priority() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/vault/", 100, true, WipePriority::Critical);
        plan.add_target("/sdcard/keys/", 200, true, WipePriority::Critical);
        plan.add_target("/sdcard/logs/", 300, true, WipePriority::Normal);

        let critical = plan.targets_by_priority(WipePriority::Critical);
        assert_eq!(critical.len(), 2);

        let normal = plan.targets_by_priority(WipePriority::Normal);
        assert_eq!(normal.len(), 1);

        let low = plan.targets_by_priority(WipePriority::Low);
        assert_eq!(low.len(), 0);
    }

    // -----------------------------------------------------------------------
    // test_mark_lifecycle_pending_to_in_progress_to_completed
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_lifecycle_pending_to_in_progress_to_completed() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/test.txt", 1024, false, WipePriority::Normal);

        assert_eq!(plan.targets()[0].status, WipeStatus::Pending);

        plan.mark_in_progress(0).unwrap();
        assert_eq!(plan.targets()[0].status, WipeStatus::InProgress);

        plan.mark_completed(0).unwrap();
        assert_eq!(plan.targets()[0].status, WipeStatus::Completed);
    }

    // -----------------------------------------------------------------------
    // test_mark_failed_with_reason
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_failed_with_reason() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/test.txt", 1024, false, WipePriority::Normal);

        plan.mark_in_progress(0).unwrap();
        plan.mark_failed(0, "disk error").unwrap();

        assert_eq!(plan.targets()[0].status, WipeStatus::Failed("disk error".to_string()));
    }

    // -----------------------------------------------------------------------
    // test_mark_skipped
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_skipped() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/missing.txt", 0, false, WipePriority::Low);

        plan.mark_skipped(0).unwrap();
        assert_eq!(plan.targets()[0].status, WipeStatus::Skipped);
    }

    // -----------------------------------------------------------------------
    // test_invalid_index_errors
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_index_errors() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/test.txt", 1024, false, WipePriority::Normal);

        assert_eq!(plan.mark_in_progress(99), Err(ESP_ERR_INVALID_ARG));
        assert_eq!(plan.mark_completed(99), Err(ESP_ERR_INVALID_ARG));
        assert_eq!(plan.mark_failed(99, "bad"), Err(ESP_ERR_INVALID_ARG));
        assert_eq!(plan.mark_skipped(99), Err(ESP_ERR_INVALID_ARG));
    }

    // -----------------------------------------------------------------------
    // test_invalid_state_transitions
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_state_transitions() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/test.txt", 1024, false, WipePriority::Normal);

        // Complete it first
        plan.mark_in_progress(0).unwrap();
        plan.mark_completed(0).unwrap();

        // Cannot transition from Completed to anything
        assert_eq!(plan.mark_in_progress(0), Err(ESP_ERR_INVALID_STATE));
        assert_eq!(plan.mark_completed(0), Err(ESP_ERR_INVALID_STATE));
        assert_eq!(plan.mark_failed(0, "x"), Err(ESP_ERR_INVALID_STATE));
        assert_eq!(plan.mark_skipped(0), Err(ESP_ERR_INVALID_STATE));
    }

    // -----------------------------------------------------------------------
    // test_progress_calculation_bytes
    // -----------------------------------------------------------------------

    #[test]
    fn test_progress_calculation_bytes() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/a.txt", 1000, false, WipePriority::Normal);
        plan.add_target("/sdcard/b.txt", 3000, false, WipePriority::Normal);

        assert_eq!(plan.progress_percent(), 0);

        plan.mark_completed(0).unwrap();
        // 1000 / 4000 = 25%
        assert_eq!(plan.progress_percent(), 25);
    }

    // -----------------------------------------------------------------------
    // test_progress_100_when_all_complete
    // -----------------------------------------------------------------------

    #[test]
    fn test_progress_100_when_all_complete() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/a.txt", 1000, false, WipePriority::Normal);
        plan.add_target("/sdcard/b.txt", 3000, false, WipePriority::Normal);

        plan.mark_completed(0).unwrap();
        plan.mark_completed(1).unwrap();

        assert_eq!(plan.progress_percent(), 100);
    }

    // -----------------------------------------------------------------------
    // test_progress_with_mixed_completed_skipped
    // -----------------------------------------------------------------------

    #[test]
    fn test_progress_with_mixed_completed_skipped() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        // All zero-size (default targets)
        plan.add_target("/sdcard/a/", 0, true, WipePriority::Normal);
        plan.add_target("/sdcard/b/", 0, true, WipePriority::Normal);
        plan.add_target("/sdcard/c/", 0, true, WipePriority::Low);

        plan.mark_completed(0).unwrap();
        plan.mark_skipped(1).unwrap();
        // 2 out of 3 done = 66%
        assert_eq!(plan.progress_percent(), 66);
    }

    // -----------------------------------------------------------------------
    // test_next_pending_respects_priority
    // -----------------------------------------------------------------------

    #[test]
    fn test_next_pending_respects_priority() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/cache/", 100, true, WipePriority::Low);
        plan.add_target("/sdcard/vault/", 500, true, WipePriority::Critical);
        plan.add_target("/sdcard/logs/", 200, true, WipePriority::Normal);

        // Should return the Critical target first (index 0 after sort)
        let next = plan.next_pending().unwrap();
        assert_eq!(plan.targets()[next].priority, WipePriority::Critical);

        // Complete it, next should be Normal
        plan.mark_completed(next).unwrap();
        let next = plan.next_pending().unwrap();
        assert_eq!(plan.targets()[next].priority, WipePriority::Normal);

        // Complete it, next should be Low
        plan.mark_completed(next).unwrap();
        let next = plan.next_pending().unwrap();
        assert_eq!(plan.targets()[next].priority, WipePriority::Low);
    }

    // -----------------------------------------------------------------------
    // test_next_pending_returns_none_when_all_done
    // -----------------------------------------------------------------------

    #[test]
    fn test_next_pending_returns_none_when_all_done() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/test.txt", 100, false, WipePriority::Normal);

        plan.mark_completed(0).unwrap();
        assert_eq!(plan.next_pending(), None);
    }

    // -----------------------------------------------------------------------
    // test_is_complete
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_complete() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/a.txt", 100, false, WipePriority::Normal);
        plan.add_target("/sdcard/b.txt", 200, false, WipePriority::Normal);

        assert!(!plan.is_complete());

        plan.mark_completed(0).unwrap();
        assert!(!plan.is_complete());

        plan.mark_skipped(1).unwrap();
        assert!(plan.is_complete());
    }

    // -----------------------------------------------------------------------
    // test_is_complete_with_failed
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_complete_with_failed() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/a.txt", 100, false, WipePriority::Normal);

        plan.mark_failed(0, "error").unwrap();
        // Failed counts as "done" for is_complete
        assert!(plan.is_complete());
    }

    // -----------------------------------------------------------------------
    // test_generate_zeros_block
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_zeros_block() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        let block = plan.generate_overwrite_block(0, 256);
        assert_eq!(block.len(), 256);
        assert!(block.iter().all(|&b| b == 0x00));
    }

    // -----------------------------------------------------------------------
    // test_generate_ones_block
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_ones_block() {
        let mut plan = WipePlan::new(WipePattern::Ones);
        let block = plan.generate_overwrite_block(0, 256);
        assert_eq!(block.len(), 256);
        assert!(block.iter().all(|&b| b == 0xFF));
    }

    // -----------------------------------------------------------------------
    // test_generate_random_block
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_random_block() {
        let mut plan = WipePlan::new(WipePattern::Random);
        let block = plan.generate_overwrite_block(0, 256);
        assert_eq!(block.len(), 256);

        // Random block should not be all zeros
        assert!(!block.iter().all(|&b| b == 0x00), "random block should not be all zeros");

        // Random block should not be all the same value (statistically near impossible)
        let first = block[0];
        assert!(!block.iter().all(|&b| b == first), "random block should not be uniform");
    }

    // -----------------------------------------------------------------------
    // test_generate_dod3pass_blocks
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_dod3pass_blocks() {
        let mut plan = WipePlan::new(WipePattern::DoD3Pass);

        // Pass 0: zeros
        let block0 = plan.generate_overwrite_block(0, 64);
        assert!(block0.iter().all(|&b| b == 0x00));

        // Pass 1: ones
        let block1 = plan.generate_overwrite_block(1, 64);
        assert!(block1.iter().all(|&b| b == 0xFF));

        // Pass 2: random (should not be all zeros or all ones)
        let block2 = plan.generate_overwrite_block(2, 64);
        assert!(!block2.iter().all(|&b| b == 0x00));
        assert!(!block2.iter().all(|&b| b == 0xFF));
    }

    // -----------------------------------------------------------------------
    // test_gutmann_35_passes
    // -----------------------------------------------------------------------

    #[test]
    fn test_gutmann_35_passes() {
        let mut plan = WipePlan::new(WipePattern::Gutmann);

        for pass in 0..35 {
            let block = plan.generate_overwrite_block(pass, 64);
            assert_eq!(block.len(), 64);

            // Verify deterministic patterns for known passes
            match GUTMANN_PATTERNS[pass] {
                Some(pattern) => {
                    // Check first few bytes match the repeating pattern
                    for i in 0..6 {
                        assert_eq!(
                            block[i], pattern[i % 3],
                            "Gutmann pass {} byte {} mismatch", pass, i
                        );
                    }
                }
                None => {
                    // Random passes — just verify non-empty
                    assert_eq!(block.len(), 64);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // test_passes_required
    // -----------------------------------------------------------------------

    #[test]
    fn test_passes_required() {
        assert_eq!(WipePlan::new(WipePattern::Zeros).passes_required(), 1);
        assert_eq!(WipePlan::new(WipePattern::Ones).passes_required(), 1);
        assert_eq!(WipePlan::new(WipePattern::Random).passes_required(), 1);
        assert_eq!(WipePlan::new(WipePattern::DoD3Pass).passes_required(), 3);
        assert_eq!(WipePlan::new(WipePattern::Gutmann).passes_required(), 35);
    }

    // -----------------------------------------------------------------------
    // test_xorshift64_produces_expected_sequence
    // -----------------------------------------------------------------------

    #[test]
    fn test_xorshift64_produces_expected_sequence() {
        let mut rng = Xorshift64::new(1);
        let v1 = rng.next();
        let v2 = rng.next();
        let v3 = rng.next();

        // Values should be deterministic for the same seed
        let mut rng2 = Xorshift64::new(1);
        assert_eq!(rng2.next(), v1);
        assert_eq!(rng2.next(), v2);
        assert_eq!(rng2.next(), v3);

        // Values should be non-zero and different from each other
        assert_ne!(v1, 0);
        assert_ne!(v2, 0);
        assert_ne!(v1, v2);
        assert_ne!(v2, v3);
    }

    // -----------------------------------------------------------------------
    // test_xorshift64_different_seeds
    // -----------------------------------------------------------------------

    #[test]
    fn test_xorshift64_different_seeds() {
        let mut rng_a = Xorshift64::new(42);
        let mut rng_b = Xorshift64::new(99);

        let a1 = rng_a.next();
        let b1 = rng_b.next();
        assert_ne!(a1, b1, "different seeds must produce different sequences");
    }

    // -----------------------------------------------------------------------
    // test_xorshift64_zero_seed_handled
    // -----------------------------------------------------------------------

    #[test]
    fn test_xorshift64_zero_seed_handled() {
        let mut rng = Xorshift64::new(0);
        // Should not be stuck at zero
        let v = rng.next();
        assert_ne!(v, 0, "xorshift64 with seed 0 must not produce 0");
    }

    // -----------------------------------------------------------------------
    // test_summary_struct
    // -----------------------------------------------------------------------

    #[test]
    fn test_summary_struct() {
        let mut plan = WipePlan::new(WipePattern::DoD3Pass);
        plan.add_target("/sdcard/a.txt", 1000, false, WipePriority::Critical);
        plan.add_target("/sdcard/b.txt", 2000, false, WipePriority::High);
        plan.add_target("/sdcard/c.txt", 3000, false, WipePriority::Normal);

        plan.mark_completed(0).unwrap();
        plan.mark_failed(1, "error").unwrap();

        let s = plan.summary();
        assert_eq!(s.total_targets, 3);
        assert_eq!(s.completed, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.skipped, 0);
        assert_eq!(s.pending, 1);
        assert_eq!(s.total_bytes, 6000);
        assert_eq!(s.completed_bytes, 1000);
        assert_eq!(s.pattern, WipePattern::DoD3Pass);
    }

    // -----------------------------------------------------------------------
    // test_total_and_completed_bytes
    // -----------------------------------------------------------------------

    #[test]
    fn test_total_and_completed_bytes() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/a.txt", 500, false, WipePriority::Normal);
        plan.add_target("/sdcard/b.txt", 1500, false, WipePriority::Normal);

        assert_eq!(plan.total_bytes(), 2000);
        assert_eq!(plan.completed_bytes(), 0);

        plan.mark_completed(0).unwrap();
        assert_eq!(plan.completed_bytes(), 500);

        plan.mark_completed(1).unwrap();
        assert_eq!(plan.completed_bytes(), 2000);
    }

    // -----------------------------------------------------------------------
    // test_ffi_create_and_destroy
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_create_and_destroy() {
        unsafe {
            // Valid patterns
            for pattern in 0..5 {
                let plan = rs_wipe_plan_create(pattern);
                assert!(!plan.is_null(), "pattern {} should create valid plan", pattern);
                rs_wipe_plan_destroy(plan);
            }

            // Invalid pattern
            let plan = rs_wipe_plan_create(99);
            assert!(plan.is_null(), "invalid pattern should return null");
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_null_pointer_safety
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_null_pointer_safety() {
        unsafe {
            // All FFI functions should handle null gracefully
            rs_wipe_plan_destroy(std::ptr::null_mut());

            let path = CString::new("/test").unwrap();
            assert_eq!(
                rs_wipe_plan_add_target(std::ptr::null_mut(), path.as_ptr(), 100, 0, 0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_wipe_plan_add_defaults(std::ptr::null_mut()),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(rs_wipe_plan_next_pending(std::ptr::null()), -1);
            assert_eq!(
                rs_wipe_plan_mark_completed(std::ptr::null_mut(), 0),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(
                rs_wipe_plan_mark_failed(std::ptr::null_mut(), 0, std::ptr::null()),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(rs_wipe_plan_progress(std::ptr::null()), 0);
            assert_eq!(rs_wipe_plan_is_complete(std::ptr::null()), 0);

            let mut buf = [0u8; 64];
            assert_eq!(
                rs_wipe_plan_generate_block(std::ptr::null_mut(), 0, buf.as_mut_ptr(), 64),
                ESP_ERR_INVALID_ARG
            );
            assert_eq!(rs_wipe_plan_passes_required(std::ptr::null()), 0);
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_add_target_and_defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_add_target_and_defaults() {
        unsafe {
            let plan = rs_wipe_plan_create(0); // Zeros
            assert!(!plan.is_null());

            let path = CString::new("/sdcard/test.txt").unwrap();
            let rc = rs_wipe_plan_add_target(plan, path.as_ptr(), 1024, 0, 1);
            assert_eq!(rc, ESP_OK);

            // Verify it was added
            assert_eq!((*plan).total_count(), 1);

            // Add defaults
            let rc = rs_wipe_plan_add_defaults(plan);
            assert_eq!(rc, ESP_OK);
            assert_eq!((*plan).total_count(), 10); // 1 + 9 defaults

            rs_wipe_plan_destroy(plan);
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_add_target_invalid_priority
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_add_target_invalid_priority() {
        unsafe {
            let plan = rs_wipe_plan_create(0);
            assert!(!plan.is_null());

            let path = CString::new("/sdcard/test.txt").unwrap();
            let rc = rs_wipe_plan_add_target(plan, path.as_ptr(), 1024, 0, 99);
            assert_eq!(rc, ESP_ERR_INVALID_ARG);

            rs_wipe_plan_destroy(plan);
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_progress_and_completion
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_progress_and_completion() {
        unsafe {
            let plan = rs_wipe_plan_create(0); // Zeros
            assert!(!plan.is_null());

            let path_a = CString::new("/sdcard/a.txt").unwrap();
            let path_b = CString::new("/sdcard/b.txt").unwrap();
            rs_wipe_plan_add_target(plan, path_a.as_ptr(), 500, 0, 2);
            rs_wipe_plan_add_target(plan, path_b.as_ptr(), 500, 0, 2);

            assert_eq!(rs_wipe_plan_is_complete(plan), 0);
            assert_eq!(rs_wipe_plan_progress(plan), 0);

            // Complete first target
            let next = rs_wipe_plan_next_pending(plan);
            assert!(next >= 0);
            rs_wipe_plan_mark_completed(plan, next);
            assert_eq!(rs_wipe_plan_progress(plan), 50);

            // Complete second target
            let next = rs_wipe_plan_next_pending(plan);
            assert!(next >= 0);
            rs_wipe_plan_mark_completed(plan, next);
            assert_eq!(rs_wipe_plan_progress(plan), 100);
            assert_eq!(rs_wipe_plan_is_complete(plan), 1);

            // No more pending
            assert_eq!(rs_wipe_plan_next_pending(plan), -1);

            rs_wipe_plan_destroy(plan);
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_generate_block
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_generate_block() {
        unsafe {
            // Zeros pattern
            let plan = rs_wipe_plan_create(0);
            assert!(!plan.is_null());

            let mut buf = [0xAA_u8; 128];
            let rc = rs_wipe_plan_generate_block(plan, 0, buf.as_mut_ptr(), buf.len());
            assert_eq!(rc, ESP_OK);
            assert!(buf.iter().all(|&b| b == 0x00));

            rs_wipe_plan_destroy(plan);

            // Random pattern
            let plan = rs_wipe_plan_create(2);
            assert!(!plan.is_null());

            let mut buf = [0u8; 128];
            let rc = rs_wipe_plan_generate_block(plan, 0, buf.as_mut_ptr(), buf.len());
            assert_eq!(rc, ESP_OK);
            // Should not be all zeros
            assert!(!buf.iter().all(|&b| b == 0x00));

            rs_wipe_plan_destroy(plan);
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_passes_required
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_passes_required() {
        unsafe {
            let plan_zeros = rs_wipe_plan_create(0);
            assert_eq!(rs_wipe_plan_passes_required(plan_zeros), 1);
            rs_wipe_plan_destroy(plan_zeros);

            let plan_dod = rs_wipe_plan_create(3);
            assert_eq!(rs_wipe_plan_passes_required(plan_dod), 3);
            rs_wipe_plan_destroy(plan_dod);

            let plan_gutmann = rs_wipe_plan_create(4);
            assert_eq!(rs_wipe_plan_passes_required(plan_gutmann), 35);
            rs_wipe_plan_destroy(plan_gutmann);
        }
    }

    // -----------------------------------------------------------------------
    // test_ffi_mark_failed
    // -----------------------------------------------------------------------

    #[test]
    fn test_ffi_mark_failed() {
        unsafe {
            let plan = rs_wipe_plan_create(0);
            assert!(!plan.is_null());

            let path = CString::new("/sdcard/test.txt").unwrap();
            rs_wipe_plan_add_target(plan, path.as_ptr(), 100, 0, 2);

            let reason = CString::new("I/O error").unwrap();
            let rc = rs_wipe_plan_mark_failed(plan, 0, reason.as_ptr());
            assert_eq!(rc, ESP_OK);

            // Plan should be "complete" (all targets resolved)
            assert_eq!(rs_wipe_plan_is_complete(plan), 1);

            rs_wipe_plan_destroy(plan);
        }
    }

    // -----------------------------------------------------------------------
    // test_empty_plan_is_not_complete
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_plan_is_not_complete() {
        let plan = WipePlan::new(WipePattern::Zeros);
        // Empty plan should not be considered complete
        assert!(!plan.is_complete());
    }

    // -----------------------------------------------------------------------
    // test_wipe_target_is_directory_flag
    // -----------------------------------------------------------------------

    #[test]
    fn test_wipe_target_is_directory_flag() {
        let mut plan = WipePlan::new(WipePattern::Zeros);
        plan.add_target("/sdcard/dir/", 0, true, WipePriority::Normal);
        plan.add_target("/sdcard/file.txt", 1024, false, WipePriority::Normal);

        assert!(plan.targets()[0].is_directory);
        assert!(!plan.targets()[1].is_directory);
    }

    // -----------------------------------------------------------------------
    // test_wipe_pattern_from_i32
    // -----------------------------------------------------------------------

    #[test]
    fn test_wipe_pattern_from_i32() {
        assert_eq!(WipePattern::from_i32(0), Some(WipePattern::Zeros));
        assert_eq!(WipePattern::from_i32(1), Some(WipePattern::Ones));
        assert_eq!(WipePattern::from_i32(2), Some(WipePattern::Random));
        assert_eq!(WipePattern::from_i32(3), Some(WipePattern::DoD3Pass));
        assert_eq!(WipePattern::from_i32(4), Some(WipePattern::Gutmann));
        assert_eq!(WipePattern::from_i32(5), None);
        assert_eq!(WipePattern::from_i32(-1), None);
    }

    // -----------------------------------------------------------------------
    // test_wipe_priority_from_u8
    // -----------------------------------------------------------------------

    #[test]
    fn test_wipe_priority_from_u8() {
        assert_eq!(WipePriority::from_u8(0), Some(WipePriority::Critical));
        assert_eq!(WipePriority::from_u8(1), Some(WipePriority::High));
        assert_eq!(WipePriority::from_u8(2), Some(WipePriority::Normal));
        assert_eq!(WipePriority::from_u8(3), Some(WipePriority::Low));
        assert_eq!(WipePriority::from_u8(4), None);
    }
}
