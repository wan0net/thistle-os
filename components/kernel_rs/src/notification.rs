// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Notification manager
//
// Centralized notification queue. Apps post notifications; the window manager
// reads them to display alerts, toasts, and badges. Manages lifecycle:
// creation, priority, expiry, dismissal, and history.

use std::ffi::CStr;
use std::os::raw::c_char;

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;
const ESP_FAIL: i32 = -1;
const ESP_ERR_NO_MEM: i32 = 0x101;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TITLE_MAX_LEN: usize = 64;
const BODY_MAX_LEN: usize = 256;

// ---------------------------------------------------------------------------
// NotificationPriority
// ---------------------------------------------------------------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Urgent = 3,
}

impl NotificationPriority {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Low),
            1 => Some(Self::Normal),
            2 => Some(Self::High),
            3 => Some(Self::Urgent),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// NotificationCategory
// ---------------------------------------------------------------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationCategory {
    Message = 0,
    System = 1,
    App = 2,
    Alert = 3,
    Progress = 4,
}

impl NotificationCategory {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Message),
            1 => Some(Self::System),
            2 => Some(Self::App),
            3 => Some(Self::Alert),
            4 => Some(Self::Progress),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_id: String,
    pub title: String,
    pub body: String,
    pub priority: NotificationPriority,
    pub category: NotificationCategory,
    pub timestamp: u32,
    pub expires_at: Option<u32>,
    pub read: bool,
    pub dismissed: bool,
    pub progress: Option<u8>,
    pub action_label: Option<String>,
}

// ---------------------------------------------------------------------------
// NotificationManager
// ---------------------------------------------------------------------------

pub struct NotificationManager {
    notifications: Vec<Notification>,
    max_capacity: usize,
    next_id: u32,
}

impl NotificationManager {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            notifications: Vec::with_capacity(max_capacity.min(256)),
            max_capacity,
            next_id: 1,
        }
    }

    /// Post a notification. Returns the assigned notification ID.
    pub fn post(
        &mut self,
        app_id: &str,
        title: &str,
        body: &str,
        priority: NotificationPriority,
        category: NotificationCategory,
    ) -> u32 {
        self.evict_if_needed();

        let id = self.next_id;
        self.next_id += 1;

        let notification = Notification {
            id,
            app_id: app_id.to_string(),
            title: truncate(title, TITLE_MAX_LEN),
            body: truncate(body, BODY_MAX_LEN),
            priority,
            category,
            timestamp: 0,
            expires_at: None,
            read: false,
            dismissed: false,
            progress: None,
            action_label: None,
        };
        self.notifications.push(notification);
        id
    }

    /// Post a notification with an expiry time.
    pub fn post_with_expiry(
        &mut self,
        app_id: &str,
        title: &str,
        body: &str,
        priority: NotificationPriority,
        category: NotificationCategory,
        expires_at: u32,
    ) -> u32 {
        let id = self.post(app_id, title, body, priority, category);
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.expires_at = Some(expires_at);
        }
        id
    }

    /// Post a progress notification (category = Progress, priority = Normal).
    pub fn post_progress(
        &mut self,
        app_id: &str,
        title: &str,
        body: &str,
        progress: u8,
    ) -> u32 {
        let id = self.post(
            app_id,
            title,
            body,
            NotificationPriority::Normal,
            NotificationCategory::Progress,
        );
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.progress = Some(progress.min(100));
        }
        id
    }

    /// Update progress on an existing progress notification.
    pub fn update_progress(&mut self, id: u32, progress: u8) -> Result<(), i32> {
        let n = self.notifications.iter_mut().find(|n| n.id == id)
            .ok_or(ESP_ERR_INVALID_ARG)?;
        if n.category != NotificationCategory::Progress {
            return Err(ESP_ERR_INVALID_STATE);
        }
        n.progress = Some(progress.min(100));
        Ok(())
    }

    /// Set an action label on a notification.
    pub fn set_action(&mut self, id: u32, label: &str) -> Result<(), i32> {
        let n = self.notifications.iter_mut().find(|n| n.id == id)
            .ok_or(ESP_ERR_INVALID_ARG)?;
        n.action_label = Some(label.to_string());
        Ok(())
    }

    /// Get a notification by ID.
    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.notifications.iter().find(|n| n.id == id)
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, id: u32) -> Result<(), i32> {
        let n = self.notifications.iter_mut().find(|n| n.id == id)
            .ok_or(ESP_ERR_INVALID_ARG)?;
        n.read = true;
        Ok(())
    }

    /// Dismiss a notification.
    pub fn dismiss(&mut self, id: u32) -> Result<(), i32> {
        let n = self.notifications.iter_mut().find(|n| n.id == id)
            .ok_or(ESP_ERR_INVALID_ARG)?;
        n.dismissed = true;
        Ok(())
    }

    /// Dismiss all notifications from a specific app. Returns the count dismissed.
    pub fn dismiss_all_from_app(&mut self, app_id: &str) -> usize {
        let mut count = 0;
        for n in self.notifications.iter_mut() {
            if n.app_id == app_id && !n.dismissed {
                n.dismissed = true;
                count += 1;
            }
        }
        count
    }

    /// Count of unread, non-dismissed notifications.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read && !n.dismissed).count()
    }

    /// Count of unread, non-dismissed notifications from a specific app.
    pub fn unread_count_by_app(&self, app_id: &str) -> usize {
        self.notifications.iter()
            .filter(|n| n.app_id == app_id && !n.read && !n.dismissed)
            .count()
    }

    /// Active notifications: not dismissed, not expired. Sorted by priority
    /// descending, then timestamp descending.
    pub fn active_notifications(&self) -> Vec<&Notification> {
        let mut result: Vec<&Notification> = self.notifications.iter()
            .filter(|n| !n.dismissed)
            .collect();
        result.sort_by(|a, b| {
            b.priority.cmp(&a.priority)
                .then(b.timestamp.cmp(&a.timestamp))
        });
        result
    }

    /// All notifications from a specific app.
    pub fn notifications_by_app(&self, app_id: &str) -> Vec<&Notification> {
        self.notifications.iter()
            .filter(|n| n.app_id == app_id)
            .collect()
    }

    /// All notifications of a specific category.
    pub fn notifications_by_category(&self, category: NotificationCategory) -> Vec<&Notification> {
        self.notifications.iter()
            .filter(|n| n.category == category)
            .collect()
    }

    /// Whether there are any active urgent notifications.
    pub fn has_urgent(&self) -> bool {
        self.notifications.iter()
            .any(|n| n.priority == NotificationPriority::Urgent && !n.dismissed && !n.read)
    }

    /// The oldest unread urgent notification.
    pub fn next_urgent(&self) -> Option<&Notification> {
        self.notifications.iter()
            .filter(|n| n.priority == NotificationPriority::Urgent && !n.dismissed && !n.read)
            .min_by_key(|n| n.timestamp)
    }

    /// Dismiss notifications that have expired. Returns count expired.
    pub fn expire_old(&mut self, current_time: u32) -> usize {
        let mut count = 0;
        for n in self.notifications.iter_mut() {
            if !n.dismissed {
                if let Some(exp) = n.expires_at {
                    if current_time >= exp {
                        n.dismissed = true;
                        count += 1;
                    }
                }
            }
        }
        count
    }

    /// Dismiss everything.
    pub fn clear_all(&mut self) {
        for n in self.notifications.iter_mut() {
            n.dismissed = true;
        }
    }

    /// Total notification count (including dismissed).
    pub fn total_count(&self) -> usize {
        self.notifications.len()
    }

    /// Count of active (not dismissed) notifications.
    pub fn active_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.dismissed).count()
    }

    /// Most recent N notifications (including dismissed), ordered newest first.
    pub fn history(&self, limit: usize) -> Vec<&Notification> {
        let mut result: Vec<&Notification> = self.notifications.iter().collect();
        result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(b.id.cmp(&a.id)));
        result.truncate(limit);
        result
    }

    /// Evict one notification if at capacity.
    /// Priority: oldest dismissed Low -> oldest dismissed Normal ->
    /// oldest dismissed High -> oldest dismissed Urgent ->
    /// oldest active Low.
    fn evict_if_needed(&mut self) {
        if self.notifications.len() < self.max_capacity {
            return;
        }

        // Try dismissed Low first
        if let Some(idx) = self.find_eviction_candidate(true, NotificationPriority::Low) {
            self.notifications.remove(idx);
            return;
        }
        // Then dismissed Normal
        if let Some(idx) = self.find_eviction_candidate(true, NotificationPriority::Normal) {
            self.notifications.remove(idx);
            return;
        }
        // Then dismissed High
        if let Some(idx) = self.find_eviction_candidate(true, NotificationPriority::High) {
            self.notifications.remove(idx);
            return;
        }
        // Then dismissed Urgent
        if let Some(idx) = self.find_eviction_candidate(true, NotificationPriority::Urgent) {
            self.notifications.remove(idx);
            return;
        }
        // Last resort: oldest active Low
        if let Some(idx) = self.find_eviction_candidate(false, NotificationPriority::Low) {
            self.notifications.remove(idx);
            return;
        }
        // If nothing else, evict the oldest notification regardless
        if !self.notifications.is_empty() {
            self.notifications.remove(0);
        }
    }

    fn find_eviction_candidate(&self, dismissed: bool, priority: NotificationPriority) -> Option<usize> {
        self.notifications.iter()
            .enumerate()
            .filter(|(_, n)| n.dismissed == dismissed && n.priority == priority)
            .min_by_key(|(_, n)| n.id)
            .map(|(idx, _)| idx)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}

// ---------------------------------------------------------------------------
// C FFI exports
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_manager_create(max_capacity: u32) -> *mut NotificationManager {
    let mgr = Box::new(NotificationManager::new(max_capacity as usize));
    Box::into_raw(mgr)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_manager_destroy(mgr: *mut NotificationManager) {
    if !mgr.is_null() {
        let _ = unsafe { Box::from_raw(mgr) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_post(
    mgr: *mut NotificationManager,
    app_id: *const c_char,
    title: *const c_char,
    body: *const c_char,
    priority: u8,
    category: u8,
) -> i32 {
    if mgr.is_null() || app_id.is_null() || title.is_null() || body.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let prio = match NotificationPriority::from_u8(priority) {
        Some(p) => p,
        None => return ESP_ERR_INVALID_ARG,
    };
    let cat = match NotificationCategory::from_u8(category) {
        Some(c) => c,
        None => return ESP_ERR_INVALID_ARG,
    };
    let mgr = unsafe { &mut *mgr };
    let app_id = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    let title = unsafe { CStr::from_ptr(title) }.to_str().unwrap_or("");
    let body = unsafe { CStr::from_ptr(body) }.to_str().unwrap_or("");

    mgr.post(app_id, title, body, prio, cat) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_post_progress(
    mgr: *mut NotificationManager,
    app_id: *const c_char,
    title: *const c_char,
    body: *const c_char,
    progress: u8,
) -> i32 {
    if mgr.is_null() || app_id.is_null() || title.is_null() || body.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let mgr = unsafe { &mut *mgr };
    let app_id = unsafe { CStr::from_ptr(app_id) }.to_str().unwrap_or("");
    let title = unsafe { CStr::from_ptr(title) }.to_str().unwrap_or("");
    let body = unsafe { CStr::from_ptr(body) }.to_str().unwrap_or("");

    mgr.post_progress(app_id, title, body, progress) as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_update_progress(
    mgr: *mut NotificationManager,
    id: u32,
    progress: u8,
) -> i32 {
    if mgr.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let mgr = unsafe { &mut *mgr };
    match mgr.update_progress(id, progress) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_mark_read(mgr: *mut NotificationManager, id: u32) -> i32 {
    if mgr.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let mgr = unsafe { &mut *mgr };
    match mgr.mark_read(id) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_dismiss(mgr: *mut NotificationManager, id: u32) -> i32 {
    if mgr.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let mgr = unsafe { &mut *mgr };
    match mgr.dismiss(id) {
        Ok(()) => ESP_OK,
        Err(e) => e,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_unread_count(mgr: *const NotificationManager) -> i32 {
    if mgr.is_null() {
        return 0;
    }
    let mgr = unsafe { &*mgr };
    mgr.unread_count() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_has_urgent(mgr: *const NotificationManager) -> i32 {
    if mgr.is_null() {
        return 0;
    }
    let mgr = unsafe { &*mgr };
    if mgr.has_urgent() { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_active_count(mgr: *const NotificationManager) -> i32 {
    if mgr.is_null() {
        return 0;
    }
    let mgr = unsafe { &*mgr };
    mgr.active_count() as i32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rs_notification_expire(
    mgr: *mut NotificationManager,
    current_time: u32,
) -> i32 {
    if mgr.is_null() {
        return ESP_ERR_INVALID_ARG;
    }
    let mgr = unsafe { &mut *mgr };
    mgr.expire_old(current_time) as i32
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn make_mgr() -> NotificationManager {
        NotificationManager::new(100)
    }

    // ── test_create_manager ──────────────────────────────────────────────

    #[test]
    fn test_create_manager() {
        let mgr = make_mgr();
        assert_eq!(mgr.total_count(), 0);
        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.unread_count(), 0);
    }

    // ── test_post_and_id_increments ──────────────────────────────────────

    #[test]
    fn test_post_and_id_increments() {
        let mut mgr = make_mgr();
        let id1 = mgr.post("app.test", "Hello", "World", NotificationPriority::Normal, NotificationCategory::App);
        let id2 = mgr.post("app.test", "Hello2", "World2", NotificationPriority::Normal, NotificationCategory::App);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert!(id2 > id1);
    }

    // ── test_get_by_id ──────────────────────────────────────────────────

    #[test]
    fn test_get_by_id() {
        let mut mgr = make_mgr();
        let id = mgr.post("app.test", "Title", "Body", NotificationPriority::High, NotificationCategory::Message);
        let n = mgr.get(id).unwrap();
        assert_eq!(n.id, id);
        assert_eq!(n.app_id, "app.test");
        assert_eq!(n.title, "Title");
        assert_eq!(n.body, "Body");
        assert_eq!(n.priority, NotificationPriority::High);
        assert_eq!(n.category, NotificationCategory::Message);
        assert!(!n.read);
        assert!(!n.dismissed);
    }

    // ── test_get_nonexistent ────────────────────────────────────────────

    #[test]
    fn test_get_nonexistent() {
        let mgr = make_mgr();
        assert!(mgr.get(999).is_none());
    }

    // ── test_mark_read ──────────────────────────────────────────────────

    #[test]
    fn test_mark_read() {
        let mut mgr = make_mgr();
        let id = mgr.post("app.test", "T", "B", NotificationPriority::Normal, NotificationCategory::App);
        assert!(!mgr.get(id).unwrap().read);
        assert_eq!(mgr.mark_read(id), Ok(()));
        assert!(mgr.get(id).unwrap().read);
    }

    // ── test_dismiss ────────────────────────────────────────────────────

    #[test]
    fn test_dismiss() {
        let mut mgr = make_mgr();
        let id = mgr.post("app.test", "T", "B", NotificationPriority::Normal, NotificationCategory::App);
        assert!(!mgr.get(id).unwrap().dismissed);
        assert_eq!(mgr.dismiss(id), Ok(()));
        assert!(mgr.get(id).unwrap().dismissed);
    }

    // ── test_dismiss_all_from_app ────────────────────────────────────────

    #[test]
    fn test_dismiss_all_from_app() {
        let mut mgr = make_mgr();
        mgr.post("app.a", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.a", "T2", "B2", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.b", "T3", "B3", NotificationPriority::Normal, NotificationCategory::App);

        let count = mgr.dismiss_all_from_app("app.a");
        assert_eq!(count, 2);
        assert_eq!(mgr.active_count(), 1); // only app.b remains
    }

    // ── test_unread_count ───────────────────────────────────────────────

    #[test]
    fn test_unread_count() {
        let mut mgr = make_mgr();
        mgr.post("app.test", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.test", "T2", "B2", NotificationPriority::Normal, NotificationCategory::App);
        assert_eq!(mgr.unread_count(), 2);

        let id = 1;
        mgr.mark_read(id).unwrap();
        assert_eq!(mgr.unread_count(), 1);
    }

    // ── test_unread_count_by_app ────────────────────────────────────────

    #[test]
    fn test_unread_count_by_app() {
        let mut mgr = make_mgr();
        mgr.post("app.a", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.a", "T2", "B2", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.b", "T3", "B3", NotificationPriority::Normal, NotificationCategory::App);

        assert_eq!(mgr.unread_count_by_app("app.a"), 2);
        assert_eq!(mgr.unread_count_by_app("app.b"), 1);
        assert_eq!(mgr.unread_count_by_app("app.c"), 0);
    }

    // ── test_active_notifications_sorted ────────────────────────────────

    #[test]
    fn test_active_notifications_sorted() {
        let mut mgr = make_mgr();

        // Post with different priorities and timestamps
        let id_low = mgr.post("app", "Low", "B", NotificationPriority::Low, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_low) {
            n.timestamp = 100;
        }

        let id_high = mgr.post("app", "High", "B", NotificationPriority::High, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_high) {
            n.timestamp = 200;
        }

        let id_normal = mgr.post("app", "Normal", "B", NotificationPriority::Normal, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_normal) {
            n.timestamp = 300;
        }

        let active = mgr.active_notifications();
        assert_eq!(active.len(), 3);
        // Should be sorted: High first, then Normal, then Low
        assert_eq!(active[0].priority, NotificationPriority::High);
        assert_eq!(active[1].priority, NotificationPriority::Normal);
        assert_eq!(active[2].priority, NotificationPriority::Low);
    }

    // ── test_filter_by_app ──────────────────────────────────────────────

    #[test]
    fn test_filter_by_app() {
        let mut mgr = make_mgr();
        mgr.post("app.a", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.b", "T2", "B2", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app.a", "T3", "B3", NotificationPriority::Normal, NotificationCategory::App);

        let from_a = mgr.notifications_by_app("app.a");
        assert_eq!(from_a.len(), 2);
        for n in &from_a {
            assert_eq!(n.app_id, "app.a");
        }
    }

    // ── test_filter_by_category ──────────────────────────────────────────

    #[test]
    fn test_filter_by_category() {
        let mut mgr = make_mgr();
        mgr.post("app", "T1", "B1", NotificationPriority::Normal, NotificationCategory::Message);
        mgr.post("app", "T2", "B2", NotificationPriority::Normal, NotificationCategory::System);
        mgr.post("app", "T3", "B3", NotificationPriority::Normal, NotificationCategory::Message);

        let messages = mgr.notifications_by_category(NotificationCategory::Message);
        assert_eq!(messages.len(), 2);
        for n in &messages {
            assert_eq!(n.category, NotificationCategory::Message);
        }

        let system = mgr.notifications_by_category(NotificationCategory::System);
        assert_eq!(system.len(), 1);
    }

    // ── test_has_urgent_and_next_urgent ──────────────────────────────────

    #[test]
    fn test_has_urgent_and_next_urgent() {
        let mut mgr = make_mgr();
        assert!(!mgr.has_urgent());
        assert!(mgr.next_urgent().is_none());

        let id1 = mgr.post("app", "Urgent1", "B", NotificationPriority::Urgent, NotificationCategory::Alert);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id1) {
            n.timestamp = 200;
        }

        let id2 = mgr.post("app", "Urgent2", "B", NotificationPriority::Urgent, NotificationCategory::Alert);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id2) {
            n.timestamp = 100; // older
        }

        assert!(mgr.has_urgent());
        let next = mgr.next_urgent().unwrap();
        assert_eq!(next.timestamp, 100, "next_urgent should return oldest");
    }

    // ── test_post_with_expiry_and_expire_old ─────────────────────────────

    #[test]
    fn test_post_with_expiry_and_expire_old() {
        let mut mgr = make_mgr();
        let id = mgr.post_with_expiry(
            "app", "Expiring", "B",
            NotificationPriority::Normal,
            NotificationCategory::App,
            1000,
        );
        assert!(mgr.get(id).unwrap().expires_at == Some(1000));

        // Before expiry
        let count = mgr.expire_old(999);
        assert_eq!(count, 0);
        assert!(!mgr.get(id).unwrap().dismissed);

        // At expiry
        let count = mgr.expire_old(1000);
        assert_eq!(count, 1);
        assert!(mgr.get(id).unwrap().dismissed);
    }

    // ── test_expire_doesnt_touch_non_expired ────────────────────────────

    #[test]
    fn test_expire_doesnt_touch_non_expired() {
        let mut mgr = make_mgr();
        mgr.post_with_expiry("app", "Far future", "B", NotificationPriority::Normal, NotificationCategory::App, 9999);
        mgr.post("app", "No expiry", "B", NotificationPriority::Normal, NotificationCategory::App);

        let count = mgr.expire_old(5000);
        // At time 5000: expires_at=9999 has NOT expired. No-expiry also fine.
        assert_eq!(count, 0);
        assert_eq!(mgr.active_count(), 2);
    }

    // ── test_post_progress_and_update ────────────────────────────────────

    #[test]
    fn test_post_progress_and_update() {
        let mut mgr = make_mgr();
        let id = mgr.post_progress("app", "Downloading", "50%", 50);
        let n = mgr.get(id).unwrap();
        assert_eq!(n.category, NotificationCategory::Progress);
        assert_eq!(n.progress, Some(50));

        mgr.update_progress(id, 75).unwrap();
        assert_eq!(mgr.get(id).unwrap().progress, Some(75));

        mgr.update_progress(id, 100).unwrap();
        assert_eq!(mgr.get(id).unwrap().progress, Some(100));
    }

    // ── test_update_progress_on_non_progress_fails ──────────────────────

    #[test]
    fn test_update_progress_on_non_progress_fails() {
        let mut mgr = make_mgr();
        let id = mgr.post("app", "Normal", "B", NotificationPriority::Normal, NotificationCategory::App);
        let result = mgr.update_progress(id, 50);
        assert_eq!(result, Err(ESP_ERR_INVALID_STATE));
    }

    // ── test_progress_clamped_to_100 ────────────────────────────────────

    #[test]
    fn test_progress_clamped_to_100() {
        let mut mgr = make_mgr();
        let id = mgr.post_progress("app", "Download", "B", 150);
        assert_eq!(mgr.get(id).unwrap().progress, Some(100));

        mgr.update_progress(id, 200).unwrap();
        assert_eq!(mgr.get(id).unwrap().progress, Some(100));
    }

    // ── test_set_action_label ────────────────────────────────────────────

    #[test]
    fn test_set_action_label() {
        let mut mgr = make_mgr();
        let id = mgr.post("app", "Msg", "B", NotificationPriority::Normal, NotificationCategory::Message);
        assert!(mgr.get(id).unwrap().action_label.is_none());

        mgr.set_action(id, "Reply").unwrap();
        assert_eq!(mgr.get(id).unwrap().action_label.as_deref(), Some("Reply"));
    }

    // ── test_title_truncation ────────────────────────────────────────────

    #[test]
    fn test_title_truncation() {
        let mut mgr = make_mgr();
        let long_title = "A".repeat(100);
        let id = mgr.post("app", &long_title, "B", NotificationPriority::Normal, NotificationCategory::App);
        let n = mgr.get(id).unwrap();
        assert_eq!(n.title.len(), TITLE_MAX_LEN);
    }

    // ── test_body_truncation ────────────────────────────────────────────

    #[test]
    fn test_body_truncation() {
        let mut mgr = make_mgr();
        let long_body = "B".repeat(500);
        let id = mgr.post("app", "T", &long_body, NotificationPriority::Normal, NotificationCategory::App);
        let n = mgr.get(id).unwrap();
        assert_eq!(n.body.len(), BODY_MAX_LEN);
    }

    // ── test_capacity_eviction_dismissed_low_first ──────────────────────

    #[test]
    fn test_capacity_eviction_dismissed_low_first() {
        let mut mgr = NotificationManager::new(3);

        let id1 = mgr.post("app", "Low1", "B", NotificationPriority::Low, NotificationCategory::App);
        let id2 = mgr.post("app", "Normal1", "B", NotificationPriority::Normal, NotificationCategory::App);
        mgr.dismiss(id1).unwrap(); // dismiss the Low one

        // Capacity is 3, we have 2. Add one more to fill.
        let _id3 = mgr.post("app", "High1", "B", NotificationPriority::High, NotificationCategory::App);
        assert_eq!(mgr.total_count(), 3);

        // Now adding a 4th should evict the dismissed Low
        let _id4 = mgr.post("app", "New", "B", NotificationPriority::Normal, NotificationCategory::App);
        assert_eq!(mgr.total_count(), 3);
        assert!(mgr.get(id1).is_none(), "dismissed Low should have been evicted");
        assert!(mgr.get(id2).is_some());
    }

    // ── test_capacity_eviction_dismissed_normal_next ────────────────────

    #[test]
    fn test_capacity_eviction_dismissed_normal_next() {
        let mut mgr = NotificationManager::new(3);

        let id1 = mgr.post("app", "Normal1", "B", NotificationPriority::Normal, NotificationCategory::App);
        let id2 = mgr.post("app", "High1", "B", NotificationPriority::High, NotificationCategory::App);
        mgr.dismiss(id1).unwrap(); // dismiss the Normal one

        let _id3 = mgr.post("app", "High2", "B", NotificationPriority::High, NotificationCategory::App);
        assert_eq!(mgr.total_count(), 3);

        // Adding 4th should evict the dismissed Normal (no dismissed Low exists)
        let _id4 = mgr.post("app", "New", "B", NotificationPriority::Normal, NotificationCategory::App);
        assert_eq!(mgr.total_count(), 3);
        assert!(mgr.get(id1).is_none(), "dismissed Normal should have been evicted");
        assert!(mgr.get(id2).is_some());
    }

    // ── test_capacity_eviction_active_low_last_resort ───────────────────

    #[test]
    fn test_capacity_eviction_active_low_last_resort() {
        let mut mgr = NotificationManager::new(3);

        let id1 = mgr.post("app", "Low1", "B", NotificationPriority::Low, NotificationCategory::App);
        let _id2 = mgr.post("app", "High1", "B", NotificationPriority::High, NotificationCategory::App);
        let _id3 = mgr.post("app", "High2", "B", NotificationPriority::High, NotificationCategory::App);
        assert_eq!(mgr.total_count(), 3);
        // No dismissed notifications, so active Low should be evicted

        let _id4 = mgr.post("app", "New", "B", NotificationPriority::Normal, NotificationCategory::App);
        assert_eq!(mgr.total_count(), 3);
        assert!(mgr.get(id1).is_none(), "active Low should have been evicted as last resort");
    }

    // ── test_clear_all ──────────────────────────────────────────────────

    #[test]
    fn test_clear_all() {
        let mut mgr = make_mgr();
        mgr.post("app", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app", "T2", "B2", NotificationPriority::High, NotificationCategory::App);
        mgr.post("app", "T3", "B3", NotificationPriority::Urgent, NotificationCategory::Alert);

        mgr.clear_all();
        assert_eq!(mgr.active_count(), 0);
        assert_eq!(mgr.total_count(), 3); // still in history
    }

    // ── test_history ────────────────────────────────────────────────────

    #[test]
    fn test_history() {
        let mut mgr = make_mgr();

        let id1 = mgr.post("app", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id1) {
            n.timestamp = 100;
        }

        let id2 = mgr.post("app", "T2", "B2", NotificationPriority::Normal, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id2) {
            n.timestamp = 300;
        }

        let id3 = mgr.post("app", "T3", "B3", NotificationPriority::Normal, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id3) {
            n.timestamp = 200;
        }

        mgr.dismiss(id1).unwrap();

        let hist = mgr.history(2);
        assert_eq!(hist.len(), 2);
        // Most recent by timestamp first (300, then 200)
        assert_eq!(hist[0].timestamp, 300);
        assert_eq!(hist[1].timestamp, 200);
    }

    // ── test_total_count_vs_active_count ────────────────────────────────

    #[test]
    fn test_total_count_vs_active_count() {
        let mut mgr = make_mgr();
        let id1 = mgr.post("app", "T1", "B1", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app", "T2", "B2", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("app", "T3", "B3", NotificationPriority::Normal, NotificationCategory::App);

        assert_eq!(mgr.total_count(), 3);
        assert_eq!(mgr.active_count(), 3);

        mgr.dismiss(id1).unwrap();
        assert_eq!(mgr.total_count(), 3);
        assert_eq!(mgr.active_count(), 2);
    }

    // ── test_priority_ordering ──────────────────────────────────────────

    #[test]
    fn test_priority_ordering() {
        assert!(NotificationPriority::Urgent > NotificationPriority::High);
        assert!(NotificationPriority::High > NotificationPriority::Normal);
        assert!(NotificationPriority::Normal > NotificationPriority::Low);

        let mut mgr = make_mgr();
        // Post in random order
        let id_n = mgr.post("app", "Normal", "B", NotificationPriority::Normal, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_n) { n.timestamp = 1; }

        let id_u = mgr.post("app", "Urgent", "B", NotificationPriority::Urgent, NotificationCategory::Alert);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_u) { n.timestamp = 2; }

        let id_l = mgr.post("app", "Low", "B", NotificationPriority::Low, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_l) { n.timestamp = 3; }

        let id_h = mgr.post("app", "High", "B", NotificationPriority::High, NotificationCategory::App);
        if let Some(n) = mgr.notifications.iter_mut().find(|n| n.id == id_h) { n.timestamp = 4; }

        let active = mgr.active_notifications();
        assert_eq!(active[0].priority, NotificationPriority::Urgent);
        assert_eq!(active[1].priority, NotificationPriority::High);
        assert_eq!(active[2].priority, NotificationPriority::Normal);
        assert_eq!(active[3].priority, NotificationPriority::Low);
    }

    // ── test_multiple_apps_posting ──────────────────────────────────────

    #[test]
    fn test_multiple_apps_posting() {
        let mut mgr = make_mgr();
        mgr.post("messenger", "New msg", "Hi", NotificationPriority::Normal, NotificationCategory::Message);
        mgr.post("system", "Battery low", "10%", NotificationPriority::High, NotificationCategory::System);
        mgr.post("assistant", "Response ready", "...", NotificationPriority::Normal, NotificationCategory::App);
        mgr.post("messenger", "New msg2", "Hey", NotificationPriority::Normal, NotificationCategory::Message);

        assert_eq!(mgr.total_count(), 4);
        assert_eq!(mgr.unread_count_by_app("messenger"), 2);
        assert_eq!(mgr.unread_count_by_app("system"), 1);
        assert_eq!(mgr.unread_count_by_app("assistant"), 1);
        assert_eq!(mgr.notifications_by_app("messenger").len(), 2);
    }

    // ── test_ffi_create_destroy ─────────────────────────────────────────

    #[test]
    fn test_ffi_create_destroy() {
        unsafe {
            let mgr = rs_notification_manager_create(50);
            assert!(!mgr.is_null());
            assert_eq!(rs_notification_active_count(mgr), 0);
            rs_notification_manager_destroy(mgr);
        }
    }

    // ── test_ffi_null_pointer_safety ────────────────────────────────────

    #[test]
    fn test_ffi_null_pointer_safety() {
        unsafe {
            // All FFI functions should handle null gracefully
            rs_notification_manager_destroy(std::ptr::null_mut());

            assert_eq!(rs_notification_post(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0, 0,
            ), ESP_ERR_INVALID_ARG);

            assert_eq!(rs_notification_post_progress(
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            ), ESP_ERR_INVALID_ARG);

            assert_eq!(rs_notification_update_progress(std::ptr::null_mut(), 1, 50), ESP_ERR_INVALID_ARG);
            assert_eq!(rs_notification_mark_read(std::ptr::null_mut(), 1), ESP_ERR_INVALID_ARG);
            assert_eq!(rs_notification_dismiss(std::ptr::null_mut(), 1), ESP_ERR_INVALID_ARG);
            assert_eq!(rs_notification_unread_count(std::ptr::null()), 0);
            assert_eq!(rs_notification_has_urgent(std::ptr::null()), 0);
            assert_eq!(rs_notification_active_count(std::ptr::null()), 0);
            assert_eq!(rs_notification_expire(std::ptr::null_mut(), 0), ESP_ERR_INVALID_ARG);
        }
    }

    // ── test_ffi_post_and_read_back ─────────────────────────────────────

    #[test]
    fn test_ffi_post_and_read_back() {
        unsafe {
            let mgr = rs_notification_manager_create(50);

            let app_id = CString::new("com.test.app").unwrap();
            let title = CString::new("Test Title").unwrap();
            let body = CString::new("Test Body").unwrap();

            let id = rs_notification_post(
                mgr,
                app_id.as_ptr(),
                title.as_ptr(),
                body.as_ptr(),
                1, // Normal
                2, // App
            );
            assert!(id > 0, "post should return positive ID");

            assert_eq!(rs_notification_unread_count(mgr), 1);
            assert_eq!(rs_notification_active_count(mgr), 1);

            assert_eq!(rs_notification_mark_read(mgr, id as u32), ESP_OK);
            assert_eq!(rs_notification_unread_count(mgr), 0);
            assert_eq!(rs_notification_active_count(mgr), 1); // still active, just read

            rs_notification_manager_destroy(mgr);
        }
    }

    // ── test_ffi_progress ───────────────────────────────────────────────

    #[test]
    fn test_ffi_progress() {
        unsafe {
            let mgr = rs_notification_manager_create(50);

            let app_id = CString::new("ota").unwrap();
            let title = CString::new("Updating").unwrap();
            let body = CString::new("Downloading...").unwrap();

            let id = rs_notification_post_progress(
                mgr,
                app_id.as_ptr(),
                title.as_ptr(),
                body.as_ptr(),
                25,
            );
            assert!(id > 0);

            assert_eq!(rs_notification_update_progress(mgr, id as u32, 75), ESP_OK);

            // Verify via Rust API
            let mgr_ref = &*mgr;
            let n = mgr_ref.get(id as u32).unwrap();
            assert_eq!(n.progress, Some(75));
            assert_eq!(n.category, NotificationCategory::Progress);

            rs_notification_manager_destroy(mgr);
        }
    }

    // ── test_ffi_dismiss_and_counts ─────────────────────────────────────

    #[test]
    fn test_ffi_dismiss_and_counts() {
        unsafe {
            let mgr = rs_notification_manager_create(50);

            let app = CString::new("app").unwrap();
            let t = CString::new("T").unwrap();
            let b = CString::new("B").unwrap();

            let id1 = rs_notification_post(mgr, app.as_ptr(), t.as_ptr(), b.as_ptr(), 1, 0);
            let id2 = rs_notification_post(mgr, app.as_ptr(), t.as_ptr(), b.as_ptr(), 3, 3);

            assert_eq!(rs_notification_active_count(mgr), 2);
            assert_eq!(rs_notification_has_urgent(mgr), 1);

            assert_eq!(rs_notification_dismiss(mgr, id1 as u32), ESP_OK);
            assert_eq!(rs_notification_active_count(mgr), 1);

            assert_eq!(rs_notification_dismiss(mgr, id2 as u32), ESP_OK);
            assert_eq!(rs_notification_active_count(mgr), 0);
            assert_eq!(rs_notification_has_urgent(mgr), 0);

            rs_notification_manager_destroy(mgr);
        }
    }
}
