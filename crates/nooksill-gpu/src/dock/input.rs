//! Pointer, drag, reorder and click handling.

use super::*;

impl Dock {
    /// Create a dock. `work` is the work area in physical pixels.
    #[must_use]
    pub fn new(cfg: Config, items: Vec<DockItem>, work: LRect) -> Self {
        Self {
            // Only pinned items exist at creation; running apps arrive after
            // the first scan. `set_pinned` fixes the split later.
            pinned: items.len(),
            widget_click: None,
            cpu_sampler: nooksill_platform::CpuSampler::new(),
            sysinfo: nooksill_platform::SysInfo::default(),
            cfg,
            items,
            work,
            mouse: (-1.0, -1.0),
            hover: false,
            hot: None,
            hide_cur: 0.0,
            anim: true,
            metrics: Metrics::default(),
            geom: DockGeom::default(),
            prev_act_c: Vec::new(),
            icons: Vec::new(),
            dirty: true,
            mode: DockMode::Always,
            hide_target: false,
            force_hide: false,
            last_near_ms: 0,
            hide_delay_ms: Self::HIDE_GRACE_MS,
            show_delay_ms: 0,
            show_pending_ms: None,
            dpi: 96,
            drag: None,
            sep: None,
            sep_dirty: false,
            moved: false,
            order_changed: false,
            pending_moves: Vec::new(),
            press: None,
            press_path: None,
            click: None,
            press_force: false,
            look_epoch: 0,
            menu_at: None,
            dropped_paths: Vec::new(),
            dropped_at: None,
            dropped_group: None,
            tray_toggle: false,
            tray_menu: false,
            merge_target: None,
            pending_merge: None,
            drop_outside: false,
            remove_preview: None,
            external_drop: None,
            slot_shift: false,
            prev_slot_l: Vec::new(),
            pending_remove: None,
            notice: None,
        }
    }

    /// Record a dropped batch and the drop point (called from `WndProc`).
    ///
    /// `x`/`y` are client physical pixels. The first message of a batch sets
    /// the anchor, since one drop can arrive as several `WM_DROPFILES`.
    pub fn push_dropped_paths(&mut self, paths: Vec<String>, x: f32, y: f32) {
        if self.dropped_paths.is_empty() {
            self.dropped_at = Some(self.drop_index_at(x, y));
            self.dropped_group = self.drop_group_at(x, y);
        }
        self.dropped_paths.extend(paths);
    }

    /// Take one batch. Paths, index and group are taken together to keep them
    /// from mixing with the next batch.
    pub fn take_dropped_paths(&mut self) -> DroppedBatch {
        DroppedBatch {
            paths: std::mem::take(&mut self.dropped_paths),
            index: self.dropped_at.take(),
            group: self.dropped_group.take(),
        }
    }

    /// Update the live insert index while an external drag passes over.
    ///
    /// Marks dirty only when the index changes; `DragOver` would otherwise
    /// redraw every frame.
    pub fn set_external_drop(&mut self, x: f32, y: f32) {
        let idx = self.drop_index_at(x, y).min(self.items.len());
        if self.external_drop != Some(idx) {
            self.external_drop = Some(idx);
            self.slot_shift = true;
            self.mark_dirty();
        }
    }

    /// External drag left or dropped: clear the insert slot.
    pub fn clear_external_drop(&mut self) {
        if self.external_drop.take().is_some() {
            self.slot_shift = true;
            self.mark_dirty();
        }
    }

    /// Current insert index; used by offscreen checks and tests.
    #[must_use]
    pub const fn external_drop(&self) -> Option<usize> {
        self.external_drop
    }

    /// Group under the drop point, if any.
    ///
    /// Returns a `dock.items` index; the caller checks it is within `raw`.
    #[must_use]
    pub fn drop_group_at(&self, x: f32, y: f32) -> Option<usize> {
        let i = self.hit_at(x, y)?;
        self.items
            .get(i)
            .filter(|it| it.kind == ItemKind::Group)
            .map(|_| i)
    }

    /// Record a tray click. `toggle` is the left button; otherwise right.
    pub fn push_tray_click(&mut self, toggle: bool) {
        if toggle {
            self.tray_toggle = true;
        } else {
            self.tray_menu = true;
        }
        self.mark_dirty();
    }

    /// Take the pending tray right-click.
    pub fn take_tray_menu(&mut self) -> bool {
        std::mem::take(&mut self.tray_menu)
    }

    /// Take the pending tray left-click.
    pub fn take_tray_toggle(&mut self) -> bool {
        std::mem::take(&mut self.tray_toggle)
    }

    /// How long a notice stays visible.
    pub(super) const NOTICE_MS: u128 = 2200;
    /// Fade-out duration at the end.
    pub(super) const NOTICE_FADE_MS: u128 = 300;

    /// Show a short notice on the dock.
    pub fn show_notice(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
        self.mark_dirty();
        self.anim = true;
    }

    /// Whether the notice is still alive; keeps frames coming.
    #[must_use]
    pub fn notice_active(&self) -> bool {
        self.notice
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed().as_millis() < Self::NOTICE_MS)
    }

    /// Drop the notice once it has expired.
    pub fn expire_notice(&mut self) {
        if !self.notice_active() && self.notice.is_some() {
            self.notice = None;
            self.mark_dirty();
        }
    }

    /// Notice opacity at `elapsed_ms` (0..1).
    ///
    /// Saturating: drawing can happen a few ms after the deadline, and a plain
    /// subtraction would underflow.
    #[must_use]
    pub(super) fn notice_alpha(elapsed_ms: u128) -> f32 {
        let remain = Self::NOTICE_MS.saturating_sub(elapsed_ms);
        (remain as f32 / Self::NOTICE_FADE_MS as f32).clamp(0.0, 1.0)
    }

    /// Screen rectangle of one slot, physical pixels.
    ///
    /// Slot geometry is window-relative, so popups need the window offset.
    #[must_use]
    pub fn slot_screen_rect(
        &self,
        index: usize,
        win_x: i32,
        win_y: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let l = self.geom.slot_l.get(index)?;
        let r = self.geom.slot_r.get(index)?;
        let top = self.geom.bar_top;
        let bottom = self.geom.bar_bottom;
        Some((
            win_x + l.round() as i32,
            win_y + top.round() as i32,
            win_x + r.round() as i32,
            win_y + bottom.round() as i32,
        ))
    }

    /// Mark for relayout; the next loop recomputes `hot`.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Take the dirty flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Hit test at a physical-pixel point using last frame's geometry.
    ///
    /// `hot` can be stale when press and move arrive in the same message.
    #[must_use]
    pub fn hit_at(&self, u: f32, v: f32) -> Option<usize> {
        let visible: Vec<bool> = self.items.iter().map(|i| i.cur > 1.0).collect();
        layout::hit_index(
            u,
            v,
            &self.geom.slot_l,
            &self.geom.slot_r,
            &visible,
            self.metrics.gap,
            self.geom.bar_top,
            self.metrics.z,
            self.metrics.lh as f32,
        )
    }

    /// Screen rectangle of a widget card, physical pixels.
    ///
    /// `win_x`/`win_y` are the window origin on screen.
    #[must_use]
    pub fn widget_screen_rect(
        &self,
        kind: layout::WidgetKind,
        win_x: i32,
        win_y: i32,
    ) -> Option<(f32, f32, f32, f32)> {
        let m = &self.metrics;
        for (i, (k, _)) in m.widgets.iter().enumerate() {
            if k != kind {
                continue;
            }
            let left = m
                .widgets
                .left_of(i, self.geom.bar_l, self.geom.bar_w, m.gap)?;
            let w = m.widgets.width_of(k)? as f32;
            return Some((
                left + win_x as f32,
                self.geom.bar_top + win_y as f32,
                left + w + win_x as f32,
                self.geom.bar_top + m.bar_h + win_y as f32,
            ));
        }
        None
    }

    /// Which widget card is under the cursor.
    ///
    /// The vertical range is the bar, widened by 4 px on each side.
    #[must_use]
    pub fn widget_at(&self, u: f32, v: f32) -> Option<layout::WidgetKind> {
        let m = &self.metrics;
        for (i, (kind, w)) in m.widgets.iter().enumerate() {
            let Some(left) = m
                .widgets
                .left_of(i, self.geom.bar_l, self.geom.bar_w, m.gap)
            else {
                continue;
            };
            let right = left + w as f32;
            let top = self.geom.bar_top - 4.0;
            let bottom = self.geom.bar_top + m.bar_h + 4.0;
            if u >= left && u <= right && v >= top && v <= bottom {
                return Some(kind);
            }
        }
        None
    }

    /// Record a widget-card click.
    pub fn click_widget(&mut self, kind: layout::WidgetKind) {
        self.widget_click = Some(kind);
    }

    /// Take the pending widget-card click.
    pub fn take_widget_click(&mut self) -> Option<layout::WidgetKind> {
        self.widget_click.take()
    }

    /// Left press: record the hit and arm a drag. Returns whether to
    /// `SetCapture`.
    pub fn press_at(&mut self, u: f32, v: f32, force: bool) -> bool {
        let hit = self.hit_at(u, v);
        // Separator scaling takes priority over drag; a separator is never
        // reordered.
        let on_sep = hit.is_some_and(|i| self.items[i].kind == ItemKind::Separator);
        if on_sep && self.cfg.get_i32(Key::SEP_SCALE) != 0 {
            self.press = None;
            self.drag = None;
            self.sep = Some(SepDrag {
                y0: v,
                icon0: self.pxi(self.cfg.icon_size),
                zoom0: self.pxi(self.cfg.zoom_size),
                // Backdate one throttle window so the first move takes effect.
                last: Instant::now() - SEP_THROTTLE,
            });
            return true; // SetCapture, or a release outside the window is lost.
        }
        self.press = hit;
        self.press_path = hit.map(|i| self.items[i].path.clone());
        self.press_force = force;
        self.drag = self.press.map(|index| DragState {
            index,
            pt: (u, v),
            // Not a drag yet; the threshold must be crossed first.
            active: false,
            press_pt: (u, v),
        });
        self.press.is_some()
    }

    /// Whether the cursor should show the vertical resize cursor.
    #[must_use]
    pub fn sep_cursor(&self, u: f32, v: f32) -> bool {
        if self.sep.is_some() {
            return true;
        }
        if self.cfg.get_i32(Key::SEP_SCALE) == 0 {
            return false;
        }
        self.hit_at(u, v)
            .is_some_and(|i| self.items[i].kind == ItemKind::Separator)
    }

    /// Take the separator-changed flag; the main loop persists it.
    pub fn take_sep_dirty(&mut self) -> bool {
        std::mem::take(&mut self.sep_dirty)
    }

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
    pub(super) fn group_under(&self, u: f32, v: f32, exclude: usize) -> Option<usize> {
        for (i, it) in self.items.iter().enumerate() {
            if i == exclude || it.kind != ItemKind::Group || it.cur <= 1.0 {
                continue;
            }
            let Some((ix, iy, iw, ih)) = self.icon_rect(i) else {
                continue;
            };
            // 4 px slack; the gaps between icons are too narrow otherwise.
            let pad = 4.0;
            if u >= ix - pad && u <= ix + iw + pad && v >= iy - pad && v <= iy + ih + pad {
                return Some(i);
            }
        }
        None
    }

    /// Icon rectangle of item `i`, window physical pixels.
    pub(super) fn icon_rect(&self, i: usize) -> Option<(f32, f32, f32, f32)> {
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
    pub(super) fn place_for(&self, u: f32, dragged: usize) -> usize {
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
    pub(super) fn move_item(&mut self, from: usize, to: usize) {
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
