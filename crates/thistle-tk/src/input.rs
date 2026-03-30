// SPDX-License-Identifier: BSD-3-Clause
//! Input event handling and dispatch.
//!
//! The module defines [`InputEvent`] (touch + keyboard) and a
//! [`dispatch_input`] function that performs hit-testing against the widget
//! tree and invokes the appropriate widget callbacks.

use core::sync::atomic::{AtomicI32, Ordering};

use crate::tree::UiTree;
use crate::widget::{Widget, WidgetId};

/// Last Y coordinate of an active touch, used to compute scroll deltas.
/// A value of -1 means no active touch.
static LAST_TOUCH_Y: AtomicI32 = AtomicI32::new(-1);

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// An input event from the hardware layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    TouchDown { x: i32, y: i32 },
    TouchUp { x: i32, y: i32 },
    TouchMove { x: i32, y: i32 },
    KeyDown { code: u32 },
    KeyUp { code: u32 },
    /// Synthetic character input (from a keyboard driver or IME).
    CharInput { ch: char },
}

// ---------------------------------------------------------------------------
// Well-known key codes
// ---------------------------------------------------------------------------

/// Backspace / delete-left.
pub const KEY_BACKSPACE: u32 = 0x08;
/// Enter / return.
pub const KEY_ENTER: u32 = 0x0D;
/// Tab.
pub const KEY_TAB: u32 = 0x09;
/// Left arrow.
pub const KEY_LEFT: u32 = 0x25;
/// Right arrow.
pub const KEY_RIGHT: u32 = 0x27;

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch an input event to the widget tree.
///
/// For touch events the function performs hit-testing to find the target
/// widget.  For key events the currently focused widget receives the event.
///
/// Returns `true` if any widget handled the event.
pub fn dispatch_input(tree: &mut UiTree, event: &InputEvent) -> bool {
    match *event {
        InputEvent::TouchDown { x, y } => dispatch_touch_down(tree, x, y),
        InputEvent::TouchUp { x, y } => dispatch_touch_up(tree, x, y),
        InputEvent::TouchMove { x, y } => dispatch_touch_move(tree, x, y),
        InputEvent::KeyDown { code } => dispatch_key(tree, code),
        InputEvent::KeyUp { .. } => false, // key-up not handled yet
        InputEvent::CharInput { ch } => dispatch_char(tree, ch),
    }
}

// ---------------------------------------------------------------------------
// Touch handling
// ---------------------------------------------------------------------------

fn dispatch_touch_down(tree: &mut UiTree, x: i32, y: i32) -> bool {
    // If any dropdown is open and the tap is outside it, close it.
    close_open_dropdowns_if_outside(tree, x, y);

    let Some(hit) = tree.find_at_point(x, y) else {
        return false;
    };

    // If a focusable widget was tapped, give it focus.
    let is_focusable = matches!(
        tree.get(hit),
        Some(Widget::TextInput(_))
            | Some(Widget::Button(_))
            | Some(Widget::Switch(_))
            | Some(Widget::Checkbox(_))
            | Some(Widget::Slider(_))
            | Some(Widget::Dropdown(_))
    );
    if is_focusable {
        tree.set_focus(Some(hit));
    }

    // Handle slider drag on touch-down.
    if let Some(Widget::Slider(_)) = tree.get(hit) {
        update_slider_value(tree, hit, x);
        LAST_TOUCH_Y.store(y, Ordering::Relaxed);
        return true;
    }

    // Set pressed state on the hit widget.
    if let Some(w) = tree.get_mut(hit) {
        w.common_mut().pressed = true;
    }
    tree.mark_dirty(hit);

    // Record initial touch position for scroll tracking.
    LAST_TOUCH_Y.store(y, Ordering::Relaxed);

    true
}

fn dispatch_touch_up(tree: &mut UiTree, x: i32, y: i32) -> bool {
    // Clear scroll tracking state.
    LAST_TOUCH_Y.store(-1, Ordering::Relaxed);

    // Clear pressed state on all widgets via a full tree walk.
    clear_all_pressed(tree);

    let Some(hit) = tree.find_at_point(x, y) else {
        return false;
    };

    // Fire button on_press callback on touch-up (standard mobile UX).
    let callback = match tree.get(hit) {
        Some(Widget::Button(btn)) => btn.on_press,
        _ => None,
    };

    if let Some(cb) = callback {
        let id = tree.get(hit).unwrap().common().id;
        cb(id);
        tree.mark_dirty(hit);
        return true;
    }

    // Handle Switch toggle on tap.
    if let Some(Widget::Switch(_)) = tree.get(hit) {
        let (on_change, id, new_state) = {
            let Widget::Switch(sw) = tree.get_mut(hit).unwrap() else {
                unreachable!()
            };
            sw.on = !sw.on;
            (sw.on_change, sw.common.id, sw.on)
        };
        tree.mark_dirty(hit);
        if let Some(cb) = on_change {
            cb(id, if new_state { "on" } else { "off" });
        }
        return true;
    }

    // Handle Checkbox toggle on tap.
    if let Some(Widget::Checkbox(_)) = tree.get(hit) {
        let (on_change, id, new_state) = {
            let Widget::Checkbox(cb_w) = tree.get_mut(hit).unwrap() else {
                unreachable!()
            };
            cb_w.checked = !cb_w.checked;
            (cb_w.on_change, cb_w.common.id, cb_w.checked)
        };
        tree.mark_dirty(hit);
        if let Some(cb) = on_change {
            cb(id, if new_state { "checked" } else { "unchecked" });
        }
        return true;
    }

    // Handle Dropdown: tap toggles open state, or selects an option.
    if let Some(Widget::Dropdown(_)) = tree.get(hit) {
        handle_dropdown_tap(tree, hit, x, y);
        return true;
    }

    false
}

/// Clear the `pressed` flag on every widget in the tree.
fn clear_all_pressed(tree: &mut UiTree) {
    let root = tree.root();
    // Collect ids first to avoid borrow issues.
    let mut ids = alloc::vec::Vec::new();
    tree.walk(root, &mut |id, w| {
        if w.common().pressed {
            ids.push(id);
        }
        true
    });
    for id in ids {
        if let Some(w) = tree.get_mut(id) {
            w.common_mut().pressed = false;
        }
        tree.mark_dirty(id);
    }
}

// ---------------------------------------------------------------------------
// Scroll handling — TouchMove drives scrollable containers
// ---------------------------------------------------------------------------

fn dispatch_touch_move(tree: &mut UiTree, x: i32, y: i32) -> bool {
    let last_y = LAST_TOUCH_Y.swap(y, Ordering::Relaxed);
    if last_y < 0 {
        return false;
    }

    // Handle slider drag on touch-move.
    if let Some(hit) = tree.find_at_point(x, y) {
        if let Some(Widget::Slider(_)) = tree.get(hit) {
            update_slider_value(tree, hit, x);
            return true;
        }
    }

    let delta_y = last_y - y; // positive = scroll down (content moves up)
    if delta_y == 0 {
        return false;
    }

    // Find the widget at the touch point and scroll the nearest scrollable ancestor.
    if let Some(hit) = tree.find_at_point(x, y) {
        return apply_scroll(tree, hit, delta_y);
    }
    false
}

fn apply_scroll(tree: &mut UiTree, target: WidgetId, delta: i32) -> bool {
    // Walk up the tree from target to find the first scrollable container.
    let mut current = Some(target);
    while let Some(id) = current {
        if let Some(Widget::Container(c)) = tree.get(id) {
            let container_h = c.common.size.h as i32;
            let container_y = c.common.pos.y;

            // Calculate content height from children.
            let content_h = tree
                .children(id)
                .iter()
                .filter_map(|&child_id| {
                    tree.get(child_id).map(|w| {
                        let c = w.common();
                        (c.pos.y - container_y) + c.size.h as i32
                    })
                })
                .max()
                .unwrap_or(0);

            if content_h > container_h {
                // This container has overflowing content — scroll it.
                let max_scroll = (content_h - container_h).max(0);
                if let Some(Widget::Container(c)) = tree.get_mut(id) {
                    let new_offset = c.scroll_offset + delta;
                    c.scroll_offset = new_offset.clamp(0, max_scroll);
                    tree.mark_dirty(id);
                    return true;
                }
            }
        }
        current = tree.parent(id);
    }
    false
}

// ---------------------------------------------------------------------------
// Keyboard handling — routed to focused widget
// ---------------------------------------------------------------------------

fn dispatch_key(tree: &mut UiTree, code: u32) -> bool {
    // Tab cycles focus regardless of whether something is focused.
    if code == KEY_TAB {
        return focus_next(tree);
    }

    let Some(focused) = tree.focus() else {
        return false;
    };

    match code {
        KEY_BACKSPACE => handle_backspace(tree, focused),
        KEY_LEFT => handle_cursor_move(tree, focused, -1),
        KEY_RIGHT => handle_cursor_move(tree, focused, 1),
        KEY_ENTER => handle_enter(tree, focused),
        _ => false,
    }
}

fn dispatch_char(tree: &mut UiTree, ch: char) -> bool {
    let Some(focused) = tree.focus() else {
        return false;
    };

    let (changed, id) = {
        let Some(Widget::TextInput(input)) = tree.get_mut(focused) else {
            return false;
        };
        let pos = input.cursor_pos as usize;
        // Insert character at cursor position.
        if pos <= input.text.len() {
            // heapless::String doesn't have insert, so we rebuild.
            let mut new_text = heapless::String::<256>::new();
            for (i, c) in input.text.chars().enumerate() {
                if i == pos {
                    let _ = new_text.push(ch);
                }
                let _ = new_text.push(c);
            }
            if pos >= input.text.len() {
                let _ = new_text.push(ch);
            }
            input.text = new_text;
            input.cursor_pos += 1;
        }
        (input.on_change, input.common.id)
    };

    tree.mark_dirty(focused);

    // Fire the on_change callback outside the mutable borrow.
    if let Some(cb) = changed {
        if let Some(Widget::TextInput(input)) = tree.get(focused) {
            cb(id, input.text.as_str());
        }
    }

    true
}

fn handle_backspace(tree: &mut UiTree, focused: WidgetId) -> bool {
    let (changed, id) = {
        let Some(Widget::TextInput(input)) = tree.get_mut(focused) else {
            return false;
        };
        if input.cursor_pos == 0 {
            return false;
        }
        let pos = (input.cursor_pos - 1) as usize;
        // Remove character at pos.
        let mut new_text = heapless::String::<256>::new();
        for (i, c) in input.text.chars().enumerate() {
            if i != pos {
                let _ = new_text.push(c);
            }
        }
        input.text = new_text;
        input.cursor_pos -= 1;
        (input.on_change, input.common.id)
    };

    tree.mark_dirty(focused);

    if let Some(cb) = changed {
        if let Some(Widget::TextInput(input)) = tree.get(focused) {
            cb(id, input.text.as_str());
        }
    }

    true
}

fn handle_cursor_move(tree: &mut UiTree, focused: WidgetId, delta: i32) -> bool {
    let Some(Widget::TextInput(input)) = tree.get_mut(focused) else {
        return false;
    };
    let new_pos = input.cursor_pos as i32 + delta;
    if new_pos >= 0 && new_pos <= input.text.len() as i32 {
        input.cursor_pos = new_pos as u16;
        tree.mark_dirty(focused);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Enter key handling — Button, Switch, Checkbox, Dropdown
// ---------------------------------------------------------------------------

fn handle_enter(tree: &mut UiTree, focused: WidgetId) -> bool {
    // Button: fire on_press.
    if let Some(Widget::Button(_)) = tree.get(focused) {
        let callback = {
            let Widget::Button(btn) = tree.get(focused).unwrap() else {
                unreachable!()
            };
            btn.on_press
        };
        if let Some(cb) = callback {
            let id = tree.get(focused).unwrap().common().id;
            cb(id);
            tree.mark_dirty(focused);
            return true;
        }
        return false;
    }

    // Switch: toggle.
    if let Some(Widget::Switch(_)) = tree.get(focused) {
        let (on_change, id, new_state) = {
            let Widget::Switch(sw) = tree.get_mut(focused).unwrap() else {
                unreachable!()
            };
            sw.on = !sw.on;
            (sw.on_change, sw.common.id, sw.on)
        };
        tree.mark_dirty(focused);
        if let Some(cb) = on_change {
            cb(id, if new_state { "on" } else { "off" });
        }
        return true;
    }

    // Checkbox: toggle.
    if let Some(Widget::Checkbox(_)) = tree.get(focused) {
        let (on_change, id, new_state) = {
            let Widget::Checkbox(cb_w) = tree.get_mut(focused).unwrap() else {
                unreachable!()
            };
            cb_w.checked = !cb_w.checked;
            (cb_w.on_change, cb_w.common.id, cb_w.checked)
        };
        tree.mark_dirty(focused);
        if let Some(cb) = on_change {
            cb(id, if new_state { "checked" } else { "unchecked" });
        }
        return true;
    }

    // Dropdown: toggle open.
    if let Some(Widget::Dropdown(_)) = tree.get(focused) {
        let Widget::Dropdown(dd) = tree.get_mut(focused).unwrap() else {
            unreachable!()
        };
        dd.open = !dd.open;
        tree.mark_dirty(focused);
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Slider touch helpers
// ---------------------------------------------------------------------------

fn update_slider_value(tree: &mut UiTree, id: WidgetId, x: i32) {
    let (on_change, wid, new_val) = {
        let Some(Widget::Slider(sl)) = tree.get_mut(id) else {
            return;
        };
        let c = &sl.common;
        let left = c.pos.x;
        let width = c.size.w as i32;
        if width <= 0 {
            return;
        }
        let rel_x = (x - left).clamp(0, width);
        let range = (sl.max - sl.min) as i32;
        let val = sl.min as i32 + (rel_x * range) / width;
        let val = val.clamp(sl.min as i32, sl.max as i32) as u8;
        sl.value = val;
        (sl.on_change, sl.common.id, val)
    };
    tree.mark_dirty(id);
    if let Some(cb) = on_change {
        let mut buf = heapless::String::<8>::new();
        let _ = core::fmt::Write::write_fmt(&mut buf, format_args!("{}", new_val));
        cb(wid, buf.as_str());
    }
}

// ---------------------------------------------------------------------------
// Dropdown helpers
// ---------------------------------------------------------------------------

fn close_open_dropdowns_if_outside(tree: &mut UiTree, x: i32, y: i32) {
    // Collect IDs of open dropdowns.
    let root = tree.root();
    let mut open_ids = alloc::vec::Vec::new();
    tree.walk(root, &mut |id, w| {
        if let Widget::Dropdown(dd) = w {
            if dd.open {
                open_ids.push(id);
            }
        }
        true
    });

    for id in open_ids {
        let inside = {
            let Some(Widget::Dropdown(dd)) = tree.get(id) else {
                continue;
            };
            let c = &dd.common;
            let total_h = c.size.h as i32 + (c.size.h as i32 * dd.options.len() as i32);
            x >= c.pos.x
                && x < c.pos.x + c.size.w as i32
                && y >= c.pos.y
                && y < c.pos.y + total_h
        };
        if !inside {
            if let Some(Widget::Dropdown(dd)) = tree.get_mut(id) {
                dd.open = false;
            }
            tree.mark_dirty(id);
        }
    }
}

fn handle_dropdown_tap(tree: &mut UiTree, id: WidgetId, _x: i32, y: i32) {
    let (was_open, item_h, pos_y, option_count, on_change, wid) = {
        let Some(Widget::Dropdown(dd)) = tree.get(id) else {
            return;
        };
        (
            dd.open,
            dd.common.size.h as i32,
            dd.common.pos.y,
            dd.options.len(),
            dd.on_change,
            dd.common.id,
        )
    };

    if !was_open {
        // Open the dropdown.
        if let Some(Widget::Dropdown(dd)) = tree.get_mut(id) {
            dd.open = true;
        }
        tree.mark_dirty(id);
        return;
    }

    // Dropdown is open — check if tap is on an option.
    let list_top = pos_y + item_h;
    if y >= list_top && option_count > 0 {
        let option_idx = ((y - list_top) / item_h) as usize;
        if option_idx < option_count {
            let selected_text = {
                let Widget::Dropdown(dd) = tree.get_mut(id).unwrap() else {
                    unreachable!()
                };
                dd.selected = option_idx as u8;
                dd.open = false;
                dd.options
                    .get(option_idx)
                    .map(|s| {
                        let mut buf = heapless::String::<32>::new();
                        let _ = buf.push_str(s.as_str());
                        buf
                    })
            };
            tree.mark_dirty(id);
            if let Some(cb) = on_change {
                if let Some(text) = &selected_text {
                    cb(wid, text.as_str());
                }
            }
            return;
        }
    }

    // Tap on the closed portion — just close it.
    if let Some(Widget::Dropdown(dd)) = tree.get_mut(id) {
        dd.open = false;
    }
    tree.mark_dirty(id);
}

// ---------------------------------------------------------------------------
// Tab focus navigation
// ---------------------------------------------------------------------------

fn focus_next(tree: &mut UiTree) -> bool {
    let focusable_ids = collect_focusable(tree);
    if focusable_ids.is_empty() {
        return false;
    }

    let current = tree.focus();
    let next_idx = match current {
        Some(id) => {
            let pos = focusable_ids.iter().position(|&fid| fid == id).unwrap_or(0);
            (pos + 1) % focusable_ids.len()
        }
        None => 0,
    };
    tree.set_focus(Some(focusable_ids[next_idx]));
    true
}

fn collect_focusable(tree: &UiTree) -> alloc::vec::Vec<WidgetId> {
    let mut result = alloc::vec::Vec::new();
    collect_focusable_recursive(tree, tree.root(), &mut result);
    result
}

fn collect_focusable_recursive(
    tree: &UiTree,
    id: WidgetId,
    result: &mut alloc::vec::Vec<WidgetId>,
) {
    if let Some(widget) = tree.get(id) {
        if !widget.common().visible {
            return;
        }
        match widget {
            Widget::TextInput(_)
            | Widget::Button(_)
            | Widget::Switch(_)
            | Widget::Checkbox(_)
            | Widget::Slider(_)
            | Widget::Dropdown(_) => {
                result.push(id);
            }
            _ => {}
        }
    }
    for &child in tree.children(id) {
        collect_focusable_recursive(tree, child, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::*;

    #[test]
    fn touch_up_fires_button() {
        use core::sync::atomic::{AtomicBool, Ordering};
        static PRESSED: AtomicBool = AtomicBool::new(false);

        fn on_press(_id: WidgetId) {
            PRESSED.store(true, Ordering::SeqCst);
        }

        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        {
            let root = tree.get_mut(tree.root()).unwrap();
            let c = root.common_mut();
            c.size = Size { w: 200, h: 200 };
        }

        let btn = tree
            .add_child(
                tree.root(),
                Widget::Button(ButtonWidget {
                    on_press: Some(on_press),
                    ..Default::default()
                }),
            )
            .unwrap();
        {
            let w = tree.get_mut(btn).unwrap();
            let c = w.common_mut();
            c.pos = Pos { x: 10, y: 10 };
            c.size = Size { w: 80, h: 30 };
        }

        PRESSED.store(false, Ordering::SeqCst);
        let handled = dispatch_input(&mut tree, &InputEvent::TouchUp { x: 20, y: 20 });
        assert!(handled);
        assert!(PRESSED.load(Ordering::SeqCst));
    }

    #[test]
    fn char_input_to_text_field() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        {
            let root = tree.get_mut(tree.root()).unwrap();
            let c = root.common_mut();
            c.size = Size { w: 200, h: 200 };
        }

        let input_id = tree
            .add_child(
                tree.root(),
                Widget::TextInput(TextInputWidget::default()),
            )
            .unwrap();
        {
            let w = tree.get_mut(input_id).unwrap();
            let c = w.common_mut();
            c.pos = Pos { x: 0, y: 0 };
            c.size = Size { w: 100, h: 20 };
        }

        // Focus the text input.
        tree.set_focus(Some(input_id));

        // Type "Hi".
        dispatch_input(&mut tree, &InputEvent::CharInput { ch: 'H' });
        dispatch_input(&mut tree, &InputEvent::CharInput { ch: 'i' });

        if let Some(Widget::TextInput(input)) = tree.get(input_id) {
            assert_eq!(input.text.as_str(), "Hi");
            assert_eq!(input.cursor_pos, 2);
        } else {
            panic!("expected TextInput");
        }

        // Backspace.
        dispatch_input(&mut tree, &InputEvent::KeyDown { code: KEY_BACKSPACE });

        if let Some(Widget::TextInput(input)) = tree.get(input_id) {
            assert_eq!(input.text.as_str(), "H");
            assert_eq!(input.cursor_pos, 1);
        }
    }
}
