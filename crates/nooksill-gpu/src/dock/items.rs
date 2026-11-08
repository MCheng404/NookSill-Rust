//! Item list replacement, icons, bounce and tray flags.

use super::*;

impl Dock {
    /// Replace the whole item list (config hot reload).
    ///
    /// Geometry is left to the next `update()`: the window rect depends on the
    /// new metrics. All interaction state is cleared, stale indices would hit
    /// the wrong item.
    pub fn replace_items(&mut self, items: Vec<DockItem>) {
        // Reuse icons by path; a full reload flashes the fallback glyphs.
        let mut old: std::collections::HashMap<String, Option<IconTex>> =
            std::collections::HashMap::new();
        if self.icons.len() == self.items.len() {
            for (it, icon) in self.items.iter().zip(self.icons.drain(..)) {
                if !it.path.is_empty() {
                    // Duplicate paths keep the first icon only.
                    old.entry(it.path.clone()).or_insert(icon);
                }
            }
        }
        self.icons = items
            .iter()
            .map(|it| {
                if it.path.is_empty() {
                    None
                } else {
                    old.remove(&it.path).flatten()
                }
            })
            .collect();

        // Carry the entrance size over. `from_item` starts at 0, and the scanner
        // rebuilds this list every ~1.2 s, so resetting would blink the whole bar.
        let mut cur: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for it in &self.items {
            if !it.path.is_empty() {
                cur.entry(it.path.clone()).or_insert(it.cur);
            }
        }
        let prev = std::mem::take(&mut self.items);
        self.items = items;
        for (i, it) in self.items.iter_mut().enumerate() {
            it.cur = if it.path.is_empty() {
                // Groups and separators have no path; they never reorder.
                prev.get(i)
                    .filter(|p| p.path.is_empty())
                    .map_or(0.0, |p| p.cur)
            } else {
                cur.remove(&it.path).unwrap_or(0.0)
            };
        }
        self.hover = false;
        self.mouse = (-1.0, -1.0);
        self.press = None;
        self.drag = None;
        self.click = None;
        self.menu_at = None;
        // Slot centres belong to the old item count.
        self.prev_act_c.clear();
        self.mark_dirty();
        self.anim = true;
    }

    /// Take the pending context-menu request.
    pub fn take_menu_request(&mut self) -> Option<(f32, f32)> {
        self.menu_at.take()
    }

    /// Toggle the forced-hidden state. Returns the new state.
    pub fn toggle_force_hide(&mut self) -> bool {
        self.force_hide = !self.force_hide;
        if self.force_hide {
            // Clear hover, otherwise icons slide away while magnified.
            self.hover = false;
            self.mouse = (-1.0, -1.0);
        }
        self.mark_dirty();
        self.anim = true;
        self.force_hide
    }

    /// Bounce one item, used as launch feedback.
    ///
    /// A configured `Bounce=1` that never fires would be worse than no option.
    pub fn start_bounce(&mut self, index: usize) {
        if !self.cfg.bounce {
            return;
        }
        if let Some(it) = self.items.get_mut(index) {
            it.bounce_at = Some(Instant::now());
            self.anim = true;
            self.mark_dirty();
        }
    }

    /// Advance the bounce animation. Returns whether any item is still bouncing.
    pub(super) fn tick_bounce(&mut self) -> bool {
        let mut active = false;
        for it in &mut self.items {
            if let Some(t) = it.bounce_at {
                let ms = u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX);
                if layout::bounce_active(ms) {
                    active = true;
                } else {
                    it.bounce_at = None;
                }
            }
        }
        active
    }

    /// Take the pending click as `(index, force_new_instance)`.
    pub fn take_click(&mut self) -> Option<(usize, bool)> {
        self.click.take()
    }

    /// Whether the item already has a texture (diagnostics).
    #[must_use]
    pub fn has_icon(&self, index: usize) -> bool {
        self.icons.get(index).is_some_and(Option::is_some)
    }

    /// Reflection band below the bar. `None` disables it.
    ///
    /// Bottom edge only: left/right docks would need a horizontal mirror, which
    /// the original never implemented. Height is `ReflSize% x icon + gap + 2`.
    #[must_use]
    pub fn reflection_spec(&self) -> Option<crate::frame::Reflection> {
        if !self.cfg.reflect || self.cfg.edge != Edge::Bottom {
            return None;
        }
        // The bar is offset during the hide animation; mirroring it then would
        // sample empty pixels.
        if self.hide_offset().1.abs() > 0.5 && self.cfg.edge == Edge::Bottom {
            return None;
        }
        let m = &self.metrics;
        let bars = layout::bars(m);
        let gap = self.pxi(self.cfg.get_i32(Key::REFL_GAP).clamp(0, 30));
        let height = (m.refl_h - gap as f32 - 2.0).round() as i32;
        if height < 2 || bars.top < 0.0 {
            return None;
        }
        Some(crate::frame::Reflection {
            src_top: bars.top.round() as i32,
            src_h: m.bar_h.round() as i32,
            gap,
            height,
            opacity: self.cfg.get_i32(Key::REFL_OPACITY).clamp(0, 100) as f32 / 100.0,
        })
    }

    /// Drop every loaded icon.
    ///
    /// `replace_items` keeps icons by path, so a changed icon source would never
    /// be read without this.
    pub fn clear_icons(&mut self) {
        self.icons.clear();
    }

    /// Drop one icon.
    ///
    /// `clear_icon` targets the case where `path` is unchanged but `icon_src`
    /// changed; `clear_icons` would blink the whole bar.
    pub fn clear_icon(&mut self, index: usize) {
        if index < self.icons.len() {
            self.icons[index] = None;
        }
    }
}
