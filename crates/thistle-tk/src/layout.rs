// SPDX-License-Identifier: BSD-3-Clause
//! Flexbox-like layout engine.
//!
//! The engine recursively walks the widget tree and positions children inside
//! their parent container according to [`Direction`], [`Align`], gap, and
//! padding settings.  Size hints ([`SizeHint`](crate::widget::SizeHint))
//! support fixed pixels, percentage of parent, and flex-grow.

use crate::tree::UiTree;
use crate::widget::{Pos, Size, SizeHint, Widget, WidgetId};

// ---------------------------------------------------------------------------
// Layout enums
// ---------------------------------------------------------------------------

/// Primary axis direction for a container's children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    Row,
    #[default]
    Column,
}

/// Alignment along an axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

/// Rectangle in absolute (screen) coordinates used during layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run layout on the entire tree starting from its root.
///
/// `viewport` is the available screen area.  After this call every widget's
/// `common().pos` and `common().size` reflect its absolute screen position.
pub fn layout(tree: &mut UiTree, viewport: Rect) {
    let root = tree.root();
    layout_node(tree, root, viewport);
}

// ---------------------------------------------------------------------------
// Recursive layout
// ---------------------------------------------------------------------------

fn layout_node(tree: &mut UiTree, id: WidgetId, available: Rect) {
    // 1. Resolve this node's own size.
    let (width_hint, height_hint) = {
        let w = &tree.get(id).unwrap().common();
        (w.width_hint, w.height_hint)
    };
    let resolved_w = resolve_hint(width_hint, available.w);
    let resolved_h = resolve_hint(height_hint, available.h);

    // Write position and size.
    {
        let common = tree.get_mut(id).unwrap().common_mut();
        common.pos = Pos {
            x: available.x,
            y: available.y,
        };
        common.size = Size {
            w: resolved_w,
            h: resolved_h,
        };
    }

    // 2. If this is a container, lay out its children.
    let children: alloc::vec::Vec<WidgetId> = tree.children(id).to_vec();
    if children.is_empty() {
        return;
    }

    // Read container-specific layout props.
    let (direction, gap, align, cross_align, padding) = match tree.get(id).unwrap() {
        Widget::Container(c) => (
            c.direction,
            c.gap,
            c.align,
            c.cross_align,
            c.common.padding,
        ),
        _ => {
            // Non-containers don't layout children — but if someone added
            // children anyway, just stack them at (0,0).
            return;
        }
    };

    let content = padded_rect(available.x, available.y, resolved_w, resolved_h, padding);

    layout_children(tree, &children, content, direction, gap, align, cross_align);
}

fn layout_children(
    tree: &mut UiTree,
    children: &[WidgetId],
    content: Rect,
    direction: Direction,
    gap: u16,
    align: Align,
    cross_align: Align,
) {
    let main_total = main_size(direction, content.w, content.h);
    let cross_total = cross_size(direction, content.w, content.h);

    // --- First pass: measure fixed / percent children, collect flex totals ---
    let child_count = children.len();
    let total_gap = if child_count > 1 {
        gap as u32 * (child_count as u32 - 1)
    } else {
        0
    };

    let mut infos: alloc::vec::Vec<ChildInfo> = alloc::vec::Vec::with_capacity(child_count);
    let mut fixed_main: u32 = 0;
    let mut flex_total: f32 = 0.0;

    for &cid in children {
        let widget = tree.get(cid).unwrap();
        let c = widget.common();
        let (main_hint, cross_hint) = match direction {
            Direction::Row => (c.width_hint, c.height_hint),
            Direction::Column => (c.height_hint, c.width_hint),
        };

        let resolved_cross = resolve_hint(cross_hint, cross_total);

        match main_hint {
            SizeHint::Fixed(px) => {
                fixed_main += px;
                infos.push(ChildInfo {
                    id: cid,
                    main: px,
                    cross: resolved_cross,
                    flex: 0.0,
                });
            }
            SizeHint::Percent(pct) => {
                let px = ((main_total as f32) * pct.clamp(0.0, 1.0)) as u32;
                fixed_main += px;
                infos.push(ChildInfo {
                    id: cid,
                    main: px,
                    cross: resolved_cross,
                    flex: 0.0,
                });
            }
            SizeHint::Flex(weight) => {
                flex_total += weight;
                infos.push(ChildInfo {
                    id: cid,
                    main: 0,
                    cross: resolved_cross,
                    flex: weight,
                });
            }
            SizeHint::Auto => {
                // For Auto, give a default size of 0 (content-sized widgets
                // will be sized by their own rendering; for containers, the
                // recursive call will resolve it).
                infos.push(ChildInfo {
                    id: cid,
                    main: 0,
                    cross: resolved_cross,
                    flex: 0.0,
                });
            }
        }
    }

    // --- Distribute remaining space to flex children ---
    let remaining = main_total.saturating_sub(fixed_main + total_gap);
    if flex_total > 0.0 {
        for info in infos.iter_mut() {
            if info.flex > 0.0 {
                info.main = ((remaining as f32) * (info.flex / flex_total)) as u32;
            }
        }
    }

    // --- Compute starting offset along the main axis (for alignment) ---
    let used_main: u32 = infos.iter().map(|i| i.main).sum::<u32>() + total_gap;
    let extra = main_total.saturating_sub(used_main);

    let (mut main_cursor, space_between) = match align {
        Align::Start => (0i32, 0u32),
        Align::Center => (extra as i32 / 2, 0),
        Align::End => (extra as i32, 0),
        Align::SpaceBetween => {
            if child_count > 1 {
                (0, extra / (child_count as u32 - 1))
            } else {
                (0, 0)
            }
        }
    };

    // --- Position each child ---
    for info in &infos {
        let cross_offset = match cross_align {
            Align::Start => 0i32,
            Align::Center => (cross_total.saturating_sub(info.cross) / 2) as i32,
            Align::End => (cross_total.saturating_sub(info.cross)) as i32,
            Align::SpaceBetween => 0, // not meaningful on cross axis
        };

        let (child_x, child_y, child_w, child_h) = match direction {
            Direction::Row => (
                content.x + main_cursor,
                content.y + cross_offset,
                info.main,
                info.cross,
            ),
            Direction::Column => (
                content.x + cross_offset,
                content.y + main_cursor,
                info.cross,
                info.main,
            ),
        };

        let child_rect = Rect {
            x: child_x,
            y: child_y,
            w: child_w,
            h: child_h,
        };

        layout_node(tree, info.id, child_rect);

        main_cursor += info.main as i32 + gap as i32 + space_between as i32;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct ChildInfo {
    id: WidgetId,
    main: u32,
    cross: u32,
    flex: f32,
}

fn resolve_hint(hint: SizeHint, parent: u32) -> u32 {
    match hint {
        SizeHint::Fixed(px) => px,
        SizeHint::Percent(pct) => ((parent as f32) * pct.clamp(0.0, 1.0)) as u32,
        SizeHint::Flex(_) => parent, // flex nodes get sized later; use parent as upper bound
        SizeHint::Auto => parent,    // auto fills parent by default
    }
}

fn main_size(dir: Direction, w: u32, h: u32) -> u32 {
    match dir {
        Direction::Row => w,
        Direction::Column => h,
    }
}

fn cross_size(dir: Direction, w: u32, h: u32) -> u32 {
    match dir {
        Direction::Row => h,
        Direction::Column => w,
    }
}

fn padded_rect(x: i32, y: i32, w: u32, h: u32, padding: (u16, u16, u16, u16)) -> Rect {
    let (pl, pt, pr, pb) = padding;
    Rect {
        x: x + pl as i32,
        y: y + pt as i32,
        w: w.saturating_sub(pl as u32 + pr as u32),
        h: h.saturating_sub(pt as u32 + pb as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::UiTree;
    use crate::widget::*;

    #[test]
    fn simple_column_layout() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget {
            direction: Direction::Column,
            gap: 4,
            ..Default::default()
        }));

        let child_a = tree
            .add_child(tree.root(), Widget::Label(LabelWidget::default()))
            .unwrap();
        let child_b = tree
            .add_child(tree.root(), Widget::Label(LabelWidget::default()))
            .unwrap();

        // Give children fixed heights.
        tree.get_mut(child_a).unwrap().common_mut().height_hint = SizeHint::Fixed(20);
        tree.get_mut(child_b).unwrap().common_mut().height_hint = SizeHint::Fixed(30);

        let vp = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 200,
        };
        layout(&mut tree, vp);

        let a = tree.get(child_a).unwrap().common();
        let b = tree.get(child_b).unwrap().common();
        assert_eq!(a.pos.y, 0);
        assert_eq!(a.size.h, 20);
        assert_eq!(b.pos.y, 24); // 20 + 4 gap
        assert_eq!(b.size.h, 30);
    }
}
