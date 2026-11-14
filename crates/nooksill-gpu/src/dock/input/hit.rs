//! Hit testing, slot rectangles and widget clicks.

use super::*;

impl Dock {
    /// Window physical pixels to logical (along-bar, from-edge) coordinates.
    ///
    /// Win32 reports client pixels; `layout` works in dock axes, which swap
    /// and flip on the left/right/top edges.
    #[must_use]
    pub fn logical_point(&self, px: f32, py: f32) -> (f32, f32) {
        let lh = self.metrics.lh as f32;
        if lh <= 0.0 {
            return (px, py);
        }
        layout::to_logical(self.cfg.edge, px, py, lh)
    }

    /// Logical coordinates back to window physical pixels.
    #[must_use]
    pub fn window_point(&self, u: f32, v: f32) -> (f32, f32) {
        let (x, y, _, _) = layout::map_rect(self.cfg.edge, u, v, 0.0, 0.0, self.metrics.lh as f32);
        (x, y)
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
        let (x, y, w, h) = layout::map_rect(
            self.cfg.edge,
            *l,
            top,
            *r - *l,
            bottom - top,
            self.metrics.lh as f32,
        );
        Some((
            win_x + x.round() as i32,
            win_y + y.round() as i32,
            win_x + (x + w).round() as i32,
            win_y + (y + h).round() as i32,
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

    /// Whether a relayout is pending, without consuming it.
    ///
    /// The main loop runs a catch-up layout before painting when a step that
    /// changes the item list ran after the frame's own layout pass.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
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
            let width = m.widgets.width_of(k)? as f32;
            let (x, y, w, h) = layout::map_rect(
                self.cfg.edge,
                left,
                self.geom.bar_top,
                width,
                m.bar_h,
                m.lh as f32,
            );
            return Some((
                x + win_x as f32,
                y + win_y as f32,
                x + w + win_x as f32,
                y + h + win_y as f32,
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
}
