//! Drag state, reordering and release handling.

use super::*;

impl Dock {
    /// Dragging: update the position and reorder when needed.
    ///
    /// The 6 px threshold keeps a click from starting as a drag.
    pub fn drag_to(&mut self, u: f32, v: f32) {
        if let Some(sep) = self.sep.as_mut() {
            if sep.last.elapsed() < SEP_THROTTLE {
                return;
            }
            sep.last = Instant::now();
            let (icon, zoom) = layout::sep_scale(v - sep.y0, sep.icon0, sep.zoom0);
            // Change config only when the size really changed; otherwise a
            // still cursor would trigger window rebuilds.
            if icon != self.pxi(self.cfg.icon_size) {
                // `sep_scale` returns physical pixels; config stores logical.
                self.cfg
                    .apply_i32(Key::IconSize, nooksill_core::units::unscale(icon, self.dpi));
                self.cfg
                    .apply_i32(Key::ZoomSize, nooksill_core::units::unscale(zoom, self.dpi));
                self.sep_dirty = true;
                self.mark_dirty();
            }
            return;
        }
        let Some(mut d) = self.drag.take() else {
            return;
        };
        d.pt = (u, v);
        if !d.active {
            let (dx, dy) = (u - d.press_pt.0, v - d.press_pt.1);
            // Locked docks never start a drag.
            if !self.cfg.lock && dx.abs().max(dy.abs()) > DRAG_THRESHOLD {
                d.active = true;
            }
        }
        if d.active {
            // Leaving the window is the removal gesture; 24 px tolerance.
            let (ww, wh) = self.physical_size();
            let outside = u < -24.0 || v < -24.0 || u > ww + 24.0 || v > wh + 24.0;
            self.drop_outside = outside;
            // Collapse the dragged item so neighbours close in.
            let preview = outside.then_some(d.index);
            if self.remove_preview != preview {
                self.remove_preview = preview;
                self.slot_shift = true;
                self.mark_dirty();
            }
            if outside {
                self.merge_target = None;
            } else {
                // A group under the cursor suppresses reorder; otherwise the
                // group moves away and cannot be aimed at.
                let over_group = self.group_under(u, v, d.index);
                self.merge_target = over_group;
                if over_group.is_none() {
                    // `drag` was taken above, so `place_for` must ignore this
                    // index explicitly or every move overshoots by one.
                    let target = self.clamp_segment(self.place_for(u, d.index), d.index);
                    if target != d.index {
                        self.move_item(d.index, target);
                        d.index = target;
                        // Keep `press` in sync, or the click pair check fails.
                        self.press = Some(target);
                    }
                }
            }
        }
        self.drag = Some(d);
    }

    /// Whether the cursor is over a group icon, ignoring `exclude`.
    ///
    /// Tests the icon square, not the wider slot, so the slot edge does not
    /// swallow a drop next to the group.
    pub(in crate::dock) fn group_under(&self, u: f32, v: f32, exclude: usize) -> Option<usize> {
        // icon_rect is in window pixels; input arrives in logical dock axes.
        let (px, py) = self.window_point(u, v);
        for (i, it) in self.items.iter().enumerate() {
            if i == exclude || it.kind != ItemKind::Group || it.cur <= 1.0 {
                continue;
            }
            let Some((ix, iy, iw, ih)) = self.icon_rect(i) else {
                continue;
            };
            // 4 px slack; the gaps between icons are too narrow otherwise.
            let pad = 4.0;
            if px >= ix - pad && px <= ix + iw + pad && py >= iy - pad && py <= iy + ih + pad {
                return Some(i);
            }
        }
        None
    }

    /// Icon rectangle of item `i`, window physical pixels.
    pub(in crate::dock) fn icon_rect(&self, i: usize) -> Option<(f32, f32, f32, f32)> {
        let it = self.items.get(i)?;
        let sz = it.cur;
        if sz <= 0.0 {
            return None;
        }
        let lx = *self.geom.slot_l.get(i)? + it.slide;
        Some(layout::map_rect(
            self.cfg.edge,
            lx,
            self.geom.icon_bottom - sz,
            sz,
            sz,
            self.metrics.lh as f32,
        ))
    }

    /// Insert index for a drag at `u`.
    ///
    /// Counts item centres left of the cursor, excluding the dragged one, which
    /// matches `move_item`'s remove-then-insert semantics.
    pub(in crate::dock) fn place_for(&self, u: f32, dragged: usize) -> usize {
        self.geom
            .act_c
            .iter()
            .enumerate()
            .filter(|(i, c)| *i != dragged && **c < u)
            .count()
    }

    /// The first `n` items are pinned (from config); the rest are running apps.
    pub fn set_pinned(&mut self, n: usize) {
        self.pinned = n.min(self.items.len());
    }

    /// Whether an item is held, including presses below the drag threshold.
    ///
    /// The main loop uses it to avoid rebuilding `items` and clearing the
    /// press. `dragging()` alone misses the frames before the threshold.
    #[must_use]
    pub fn press_active(&self) -> bool {
        self.drag.is_some() || self.press.is_some()
    }

    /// Move item `from` to `to`. Icon textures move with it.
    pub(in crate::dock) fn move_item(&mut self, from: usize, to: usize) {
        if from == to || from >= self.items.len() || to >= self.items.len() {
            return;
        }
        // Record positions first; `update` uses them for the slide.
        for (it, &l) in self.items.iter_mut().zip(&self.geom.slot_l) {
            it.prev_l = l;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        if from < self.icons.len() && to < self.icons.len() {
            let icon = self.icons.remove(from);
            self.icons.insert(to, icon);
        }
        self.moved = true;
        self.order_changed = true;
        self.pending_moves.push((from, to));
    }

    /// Take the order-changed flag.
    pub fn take_order_changed(&mut self) -> bool {
        std::mem::take(&mut self.order_changed)
    }

    /// Take the reorder queue, in order.
    pub fn take_pending_moves(&mut self) -> Vec<(usize, usize)> {
        std::mem::take(&mut self.pending_moves)
    }

    /// Take the pending merge (dragged -> group).
    pub fn take_pending_merge(&mut self) -> Option<(usize, usize)> {
        self.pending_merge.take()
    }

    /// Take the pending removal index.
    pub fn take_pending_remove(&mut self) -> Option<usize> {
        self.pending_remove.take()
    }

    /// Whether the current drag left the bar; drawing shows the remove hint.
    #[must_use]
    pub fn drag_outside(&self) -> bool {
        self.drop_outside
    }

    /// Group currently under the drag; drawing highlights it.
    #[must_use]
    pub fn merge_target(&self) -> Option<usize> {
        self.merge_target
    }

    /// Active drag, if any; drawing uses it for the ghost icon.
    #[must_use]
    pub fn dragging(&self) -> Option<&DragState> {
        self.drag.as_ref().filter(|d| d.active)
    }

    /// Left release: a click only when press and release hit the same item.
    ///
    /// Dragging never produces a click.
    pub fn release_at(&mut self, u: f32, v: f32) -> Option<usize> {
        if self.sep.take().is_some() {
            // Scaling is not a click; the size already landed in `cfg`.
            return None;
        }
        // Check this first, or every reorder would also launch an app.
        let drag = self.drag.take();
        let was_dragging = drag.as_ref().is_some_and(|d| d.active);
        let pressed = self.press.take();
        if was_dragging {
            // Released outside the bar means remove.
            if self.drop_outside {
                self.drop_outside = false;
                self.merge_target = None;
                if let Some(src) = drag.map(|d| d.index) {
                    self.pending_remove = Some(src);
                    self.order_changed = false;
                }
                return None;
            }
            // Released over a group means merge.
            if let (Some(src), Some(dst)) = (drag.map(|d| d.index), self.merge_target.take()) {
                if src != dst {
                    self.pending_merge = Some((src, dst));
                    self.order_changed = false;
                }
            }
            return None;
        }
        self.merge_target = None;
        self.drop_outside = false;
        // Launch the pressed item, not a fresh hit: magnification moves icons
        // between press and release. Identity is by path, since scanning shifts.
        let key = self.press_path.take();
        let hit = match (pressed, key) {
            (Some(a), Some(path)) if self.items.get(a).is_some_and(|it| it.path == path) => {
                Some((a, self.press_force))
            }
            _ => None,
        };
        let _ = u;
        let _ = v;
        self.click = hit;
        hit.map(|(i, _)| i)
    }

    /// Cancel a press or drag without producing a click.
    ///
    /// Called on `WM_CAPTURECHANGED`: the release will never arrive. Clearing
    /// `press` avoids pairing the next click with this stale press.
    pub fn cancel_drag(&mut self) {
        // Keep `sep_dirty`: the size change still needs persisting.
        self.sep = None;
        let was = self.drag.take().is_some_and(|d| d.active);
        self.press = None;
        self.press_path = None;
        // Clear the merge target too; the group must not stay highlighted.
        self.merge_target = None;
        self.drop_outside = false;
        // Do not clear `click`: `ReleaseCapture` also sends `WM_CAPTURECHANGED`
        // after `release_at` has already settled the click.
        if was {
            // Aborted drag: snap back instead of staying midway.
            self.mark_dirty();
            self.anim = true;
        }
    }

    /// Middle button: emit a forced-new-instance click.
    ///
    /// Reuses the `click` channel and clears the press state so no drag
    /// candidate is left behind.
    pub fn force_click(&mut self, u: f32, v: f32) -> Option<usize> {
        let hit = self.hit_at(u, v);
        if hit.is_some() {
            self.press = None;
            self.drag = None;
            self.merge_target = None;
            self.click = hit.map(|i| (i, true));
        }
        hit
    }

    /// Record a right-click for the main loop.
    ///
    /// Also cancels a press or drag in progress.
    pub fn request_menu(&mut self, u: f32, v: f32) {
        self.cancel_drag();
        self.menu_at = Some((u, v));
        self.mark_dirty();
    }
}
