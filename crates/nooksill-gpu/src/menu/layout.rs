//! Menu geometry: size, hit testing, hover and placement.

use super::paint::estimate_width;
use super::*;

impl Menu {
    /// Returns the width reserved for the check mark, kept for every item of
    /// a menu.
    pub(super) fn gutter(&self) -> f32 {
        if self.entries.iter().any(|e| e.checkable) {
            18.0
        } else {
            0.0
        }
    }

    /// Returns the logical size `(width, height)` of the menu.
    ///
    /// The width is driven by the longest label; CJK and Latin characters are
    /// estimated separately because a uniform per-character width would be
    /// far too narrow for CJK labels.
    #[must_use]
    pub fn logical_size(&self) -> (f32, f32) {
        let mut w: f32 = MIN_W;
        let mut h = PAD_Y * 2.0;
        let gutter = self.gutter();
        for e in &self.entries {
            w = w.max(estimate_width(&e.label) + PAD_X * 2.0 + gutter);
            h += ITEM_H;
            if e.sep_after {
                h += SEP_H;
            }
        }
        (w, h)
    }

    /// Returns the entry index hit by a logical point inside the menu.
    ///
    /// Separators are skipped, they are not clickable.
    #[must_use]
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let (w, _) = self.logical_size();
        if x < 0.0 || x > w {
            return None;
        }
        let mut top = PAD_Y;
        for (i, e) in self.entries.iter().enumerate() {
            if y >= top && y < top + ITEM_H {
                return Some(i);
            }
            top += ITEM_H;
            if e.sep_after {
                top += SEP_H;
            }
        }
        None
    }

    /// Updates the hover index, returns whether it changed.
    pub fn set_hover(&mut self, at: Option<usize>) -> bool {
        if self.hover == at {
            return false;
        }
        self.hover = at;
        true
    }

    /// Returns the command of the entry at `index`.
    #[must_use]
    pub fn cmd_at(&self, index: usize) -> Option<MenuCmd> {
        self.entries.get(index).map(|e| e.cmd)
    }

    /// Places the menu near the cursor, clamped inside `bounds`.
    ///
    /// Prefers below the cursor and flips above when there is no room, which
    /// is the normal case for a bottom-docked Dock.
    #[must_use]
    pub fn place(cursor: (f32, f32), size: (f32, f32), bounds: (f32, f32, f32, f32)) -> (f32, f32) {
        let (cx, cy) = cursor;
        let (w, h) = size;
        let (left, top, right, bottom) = bounds;

        let mut x = cx;
        let mut y = cy;
        if x + w > right {
            x = right - w;
        }
        if x < left {
            x = left;
        }
        if y + h > bottom {
            y = cy - h;
        }
        if y < top {
            y = top;
        }
        (x, y)
    }
}
