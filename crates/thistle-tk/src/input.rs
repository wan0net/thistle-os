// SPDX-License-Identifier: BSD-3-Clause
//! Input event handling and dispatch.
//!
//! The module defines [`InputEvent`] (touch + keyboard) and a
//! [`dispatch_input`] function that performs hit-testing against the widget
//! tree and invokes the appropriate widget callbacks.

use crate::tree::UiTree;
use crate::widget::{Widget, WidgetId};

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
        InputEvent::TouchMove { .. } => {
            // Move events could be used for scrolling in the future.
            false
        }
        InputEvent::KeyDown { code } => dispatch_key(tree, code),
        InputEvent::KeyUp { .. } => false, // key-up not handled yet
        InputEvent::CharInput { ch } => dispatch_char(tree, ch),
    }
}

// ---------------------------------------------------------------------------
// Touch handling
// ---------------------------------------------------------------------------

fn dispatch_touch_down(tree: &mut UiTree, x: i32, y: i32) -> bool {
    let Some(hit) = tree.find_at_point(x, y) else {
        return false;
    };

    // If a focusable widget was tapped, give it focus.
    let is_focusable = matches!(tree.get(hit), Some(Widget::TextInput(_)));
    if is_focusable {
        tree.set_focus(Some(hit));
    }

    true
}

fn dispatch_touch_up(tree: &mut UiTree, x: i32, y: i32) -> bool {
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

    false
}

// ---------------------------------------------------------------------------
// Keyboard handling — routed to focused widget
// ---------------------------------------------------------------------------

fn dispatch_key(tree: &mut UiTree, code: u32) -> bool {
    let Some(focused) = tree.focus() else {
        return false;
    };

    match code {
        KEY_BACKSPACE => handle_backspace(tree, focused),
        KEY_LEFT => handle_cursor_move(tree, focused, -1),
        KEY_RIGHT => handle_cursor_move(tree, focused, 1),
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
