// SPDX-License-Identifier: BSD-3-Clause
//! Arena-based widget tree.
//!
//! Widgets are stored in a flat [`Vec`](alloc::vec::Vec) arena indexed by
//! [`WidgetId`].  Each node tracks its parent and up to 32 children (using
//! [`heapless::Vec`]).
//!
//! The tree owns all widgets. Apps manipulate widgets through their
//! [`WidgetId`] handles via methods on [`UiTree`].

use alloc::vec::Vec;
use heapless::Vec as HVec;

use crate::widget::{Widget, WidgetId};

/// Maximum number of children per node.
pub const MAX_CHILDREN: usize = 32;

// ---------------------------------------------------------------------------
// Internal node
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Node {
    widget: Widget,
    parent: Option<WidgetId>,
    children: HVec<WidgetId, MAX_CHILDREN>,
    /// Generation counter — incremented on removal to invalidate stale ids.
    generation: u16,
    /// `true` when this slot is occupied.
    alive: bool,
}

// ---------------------------------------------------------------------------
// UiTree
// ---------------------------------------------------------------------------

/// An arena-backed tree of widgets.
///
/// The root node is created at construction time and always lives at index 0.
pub struct UiTree {
    nodes: Vec<Node>,
    root: WidgetId,
    /// Free-list of removed node indices for reuse.
    free: Vec<WidgetId>,
    /// The widget that currently holds keyboard / input focus.
    focus: Option<WidgetId>,
}

impl UiTree {
    /// Create a new tree with the given widget as root.
    pub fn new(root_widget: Widget) -> Self {
        let root_node = Node {
            widget: root_widget,
            parent: None,
            children: HVec::new(),
            generation: 0,
            alive: true,
        };
        Self {
            nodes: alloc::vec![root_node],
            root: 0,
            free: Vec::new(),
            focus: None,
        }
    }

    /// The id of the root widget (always `0`).
    pub fn root(&self) -> WidgetId {
        self.root
    }

    // -- Focus management --------------------------------------------------

    /// Get the currently focused widget id, if any.
    pub fn focus(&self) -> Option<WidgetId> {
        self.focus
    }

    /// Set input focus to the given widget (or `None` to clear).
    pub fn set_focus(&mut self, id: Option<WidgetId>) {
        self.focus = id;
    }

    // -- Accessors ---------------------------------------------------------

    /// Get a shared reference to a widget by id.
    ///
    /// Returns `None` if the id is invalid or the node has been removed.
    pub fn get(&self, id: WidgetId) -> Option<&Widget> {
        self.nodes
            .get(id as usize)
            .filter(|n| n.alive)
            .map(|n| &n.widget)
    }

    /// Get a mutable reference to a widget by id.
    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut Widget> {
        self.nodes
            .get_mut(id as usize)
            .filter(|n| n.alive)
            .map(|n| &mut n.widget)
    }

    /// Return the children ids of a node.  Returns an empty slice for leaf
    /// nodes or invalid ids.
    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.nodes
            .get(id as usize)
            .filter(|n| n.alive)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    /// Return the parent id of a node.
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes
            .get(id as usize)
            .filter(|n| n.alive)
            .and_then(|n| n.parent)
    }

    // -- Mutation -----------------------------------------------------------

    /// Add a child widget under `parent`.
    ///
    /// Returns the new widget's id, or `None` if the parent is invalid or its
    /// children list is full.
    pub fn add_child(&mut self, parent: WidgetId, mut widget: Widget) -> Option<WidgetId> {
        // Validate parent.
        if !self
            .nodes
            .get(parent as usize)
            .map_or(false, |n| n.alive)
        {
            return None;
        }

        // Allocate a node (reuse free slot or push new).
        let id = if let Some(free_id) = self.free.pop() {
            let slot = &mut self.nodes[free_id as usize];
            slot.widget = {
                widget.common_mut().id = free_id;
                widget
            };
            slot.parent = Some(parent);
            slot.children.clear();
            slot.generation = slot.generation.wrapping_add(1);
            slot.alive = true;
            free_id
        } else {
            let id = self.nodes.len() as WidgetId;
            widget.common_mut().id = id;
            self.nodes.push(Node {
                widget,
                parent: Some(parent),
                children: HVec::new(),
                generation: 0,
                alive: true,
            });
            id
        };

        // Add to parent's children list.
        let parent_node = &mut self.nodes[parent as usize];
        if parent_node.children.push(id).is_err() {
            // Children list full — rollback.
            self.nodes[id as usize].alive = false;
            self.free.push(id);
            return None;
        }

        Some(id)
    }

    /// Remove a widget and all its descendants from the tree.
    ///
    /// Returns `true` if the widget was found and removed.
    pub fn remove(&mut self, id: WidgetId) -> bool {
        if id == self.root {
            return false; // cannot remove root
        }
        if !self
            .nodes
            .get(id as usize)
            .map_or(false, |n| n.alive)
        {
            return false;
        }

        // Collect descendants depth-first.
        let mut to_remove = alloc::vec::Vec::new();
        self.collect_subtree(id, &mut to_remove);

        // Unlink from parent.
        if let Some(parent_id) = self.nodes[id as usize].parent {
            let parent = &mut self.nodes[parent_id as usize];
            if let Some(pos) = parent.children.iter().position(|&c| c == id) {
                parent.children.swap_remove(pos);
            }
        }

        // Mark all as dead.
        for &rid in &to_remove {
            self.nodes[rid as usize].alive = false;
            self.nodes[rid as usize].children.clear();
            self.free.push(rid);

            // Clear focus if removed.
            if self.focus == Some(rid) {
                self.focus = None;
            }
        }

        true
    }

    /// Mark a widget (and its ancestors) as dirty so the renderer knows to
    /// repaint.
    pub fn mark_dirty(&mut self, id: WidgetId) {
        let mut current = Some(id);
        while let Some(cid) = current {
            if let Some(node) = self.nodes.get_mut(cid as usize).filter(|n| n.alive) {
                node.widget.common_mut().dirty = true;
                current = node.parent;
            } else {
                break;
            }
        }
    }

    /// Clear the dirty flag on every widget in the tree.
    pub fn clear_dirty(&mut self) {
        for node in self.nodes.iter_mut().filter(|n| n.alive) {
            node.widget.common_mut().dirty = false;
        }
    }

    // -- Traversal ----------------------------------------------------------

    /// Walk the tree depth-first (pre-order) starting from `start`, calling
    /// `visitor` for each alive node.  If `visitor` returns `false`, the
    /// subtree rooted at that node is skipped.
    pub fn walk<F>(&self, start: WidgetId, visitor: &mut F)
    where
        F: FnMut(WidgetId, &Widget) -> bool,
    {
        if let Some(node) = self.nodes.get(start as usize).filter(|n| n.alive) {
            if !node.widget.common().visible {
                return;
            }
            if visitor(start, &node.widget) {
                // Visit children.  We need to clone the children list because
                // the borrow checker won't let us hold &self while iterating.
                let children: HVec<WidgetId, MAX_CHILDREN> = node.children.clone();
                for &child in children.iter() {
                    self.walk(child, visitor);
                }
            }
        }
    }

    /// Find the deepest widget whose bounding box contains `(x, y)`.
    ///
    /// Returns `None` if no visible widget contains the point.
    pub fn find_at_point(&self, x: i32, y: i32) -> Option<WidgetId> {
        self.find_at_point_rec(self.root, x, y)
    }

    // -- Internal helpers ---------------------------------------------------

    fn find_at_point_rec(&self, id: WidgetId, x: i32, y: i32) -> Option<WidgetId> {
        let node = self.nodes.get(id as usize).filter(|n| n.alive)?;
        let c = node.widget.common();
        if !c.visible {
            return None;
        }

        let inside = x >= c.pos.x
            && y >= c.pos.y
            && x < c.pos.x + c.size.w as i32
            && y < c.pos.y + c.size.h as i32;

        if !inside {
            return None;
        }

        // Check children in reverse order (last child is drawn on top).
        for &child in node.children.iter().rev() {
            if let Some(hit) = self.find_at_point_rec(child, x, y) {
                return Some(hit);
            }
        }

        Some(id)
    }

    fn collect_subtree(&self, id: WidgetId, out: &mut alloc::vec::Vec<WidgetId>) {
        out.push(id);
        if let Some(node) = self.nodes.get(id as usize).filter(|n| n.alive) {
            for &child in node.children.iter() {
                self.collect_subtree(child, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::*;

    fn label(text: &str) -> Widget {
        let mut l = LabelWidget::default();
        let _ = l.text.push_str(text);
        Widget::Label(l)
    }

    #[test]
    fn add_and_get() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        let child = tree.add_child(tree.root(), label("hello")).unwrap();

        assert!(tree.get(child).is_some());
        assert_eq!(tree.children(tree.root()).len(), 1);
    }

    #[test]
    fn remove_subtree() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        let a = tree.add_child(tree.root(), Widget::Container(ContainerWidget::default())).unwrap();
        let _b = tree.add_child(a, label("nested")).unwrap();

        assert!(tree.remove(a));
        assert!(tree.get(a).is_none());
        assert!(tree.children(tree.root()).is_empty());
    }

    #[test]
    fn cannot_remove_root() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        assert!(!tree.remove(tree.root()));
    }

    #[test]
    fn walk_visits_all() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        tree.add_child(tree.root(), label("a")).unwrap();
        tree.add_child(tree.root(), label("b")).unwrap();

        let mut visited = alloc::vec::Vec::new();
        tree.walk(tree.root(), &mut |id, _| {
            visited.push(id);
            true
        });
        assert_eq!(visited.len(), 3); // root + 2 children
    }

    #[test]
    fn find_at_point_basic() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        {
            let root = tree.get_mut(tree.root()).unwrap();
            let c = root.common_mut();
            c.pos = Pos { x: 0, y: 0 };
            c.size = Size { w: 100, h: 100 };
        }

        let child = tree.add_child(tree.root(), label("btn")).unwrap();
        {
            let w = tree.get_mut(child).unwrap();
            let c = w.common_mut();
            c.pos = Pos { x: 10, y: 10 };
            c.size = Size { w: 50, h: 20 };
        }

        assert_eq!(tree.find_at_point(15, 15), Some(child));
        assert_eq!(tree.find_at_point(80, 80), Some(tree.root()));
        assert_eq!(tree.find_at_point(200, 200), None);
    }

    #[test]
    fn dirty_propagates_to_ancestors() {
        let mut tree = UiTree::new(Widget::Container(ContainerWidget::default()));
        let child = tree.add_child(tree.root(), label("x")).unwrap();
        tree.clear_dirty();

        tree.mark_dirty(child);
        assert!(tree.get(child).unwrap().common().dirty);
        assert!(tree.get(tree.root()).unwrap().common().dirty);
    }
}
