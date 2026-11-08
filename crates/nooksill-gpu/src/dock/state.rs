//! Size, per-frame update, auto-hide and system sampling.

use super::*;

impl Dock {
    /// Config length (96 DPI logical px) to physical pixels.
    ///
    /// Same conversion as `calc_metrics`; everything after that is physical.
    pub(super) fn pxf(&self, v: i32) -> f32 {
        nooksill_core::units::scale(v, self.dpi) as f32
    }

    /// Same as `pxf`, integer version.
    pub(super) fn pxi(&self, v: i32) -> i32 {
        nooksill_core::units::scale(v, self.dpi)
    }

    /// Window size in physical pixels; the egui canvas is this divided by ppp.
    ///
    /// `lw`/`lh` are physical despite the name; do not use them as egui sizes.
    #[must_use]
    pub fn physical_size(&self) -> (f32, f32) {
        // Vertical docks transpose: window width is `lh`, height is `lw`.
        let (w, h) = layout::physical_size(self.cfg.edge, self.metrics.lw, self.metrics.lh);
        (w as f32, h as f32)
    }

    /// Same, rounded to `i32` for window coordinates.
    #[must_use]
    pub fn physical_size_i32(&self) -> (i32, i32) {
        let (w, h) = self.physical_size();
        (w.round() as i32, h.round() as i32)
    }

    /// AppBar thickness to reserve, physical pixels.
    #[must_use]
    pub fn reserve_thickness(&self) -> i32 {
        layout::reserve_thickness(self.metrics.bar_h, self.metrics.margin)
    }

    /// Advance one frame: metrics, magnify animation, slots and hit test.
    ///
    /// Geometry comes from `nooksill_core::layout`; this only fills `geom`.
    pub fn update(&mut self, dpi: i32, dt_ms: f32) {
        self.dpi = dpi;
        let kinds: Vec<SlotKind> = self.items.iter().map(DockItem::slot_kind).collect();
        let n_sep = kinds.iter().filter(|k| k.is_separator()).count();
        let m = layout::calc_metrics(&self.cfg, dpi, kinds.len(), n_sep, self.work);
        self.metrics = m;

        // Base centres feed the magnification weights; never use zoomed ones.
        let base_c = layout::base_centers(&m, &kinds, m.widget_extra);

        // Magnify animation: exponential approach to the target size.
        let view = layout::View {
            mouse: self.mouse,
            hover: self.hover,
            hide_cur: self.hide_cur,
        };
        let targets =
            layout::magnify_targets(&m, &kinds, &base_c, &self.prev_act_c, view, self.cfg.zoom);
        let mut anim = false;
        for (it, target) in self.items.iter_mut().zip(&targets) {
            anim |= layout::lerp_to(&mut it.cur, *target, dt_ms);
        }

        // A reorder this frame decides whether to add slide offsets.
        let moved = std::mem::take(&mut self.moved);

        // Hit test against this frame's slots, otherwise bubbles lag one frame.
        let mut cur: Vec<f32> = self.items.iter().map(|i| i.cur).collect();
        // Dragging outside collapses the item to 0 width so neighbours close in.
        // The preview lapses once `drag` is taken; no other cleanup needed.
        if let Some(i) = self.remove_preview.filter(|_| self.drag.is_some()) {
            if let Some(c) = cur.get_mut(i) {
                *c = 0.0;
            }
        }
        // External drop reserves one icon-wide slot at the landing index.
        let insert = self.external_drop.map(|at| (at.min(cur.len()), m.s));
        let slots = layout::place_slots_insert(&m, &cur, m.widget_extra, insert);
        let bars = layout::bars(&m);
        self.hot = layout::hit_test(&m, &bars, &slots, &cur, view);

        self.geom = DockGeom {
            insert_l: slots.insert_l,
            bar_l: slots.bar_l,
            bar_w: slots.bar_w,
            bar_top: bars.top,
            bar_bottom: bars.bottom,
            icon_bottom: bars.icon_bottom,
            slot_l: slots.slot_l.clone(),
            slot_r: slots.slot_r,
            act_c: slots.act_c.clone(),
            base_c,
        };
        // Reorder slide: move from last frame's slot to this frame's.
        if moved {
            for (it, &l) in self.items.iter_mut().zip(&slots.slot_l) {
                it.slide += it.prev_l - l;
            }
        }
        // Insert and remove-preview also move icons; slide from the old slot.
        // Never fires together with the reorder slide above.
        if self.slot_shift {
            self.slot_shift = false;
            for (it, (&old, &new)) in self
                .items
                .iter_mut()
                .zip(self.prev_slot_l.iter().zip(&slots.slot_l))
            {
                it.slide += old - new;
            }
        }
        self.prev_slot_l = slots.slot_l.clone();
        for it in &mut self.items {
            if it.slide.abs() > 0.5 {
                it.slide *= 0.62;
                anim = true;
            } else {
                it.slide = 0.0;
            }
        }

        // Bouncing needs frames, otherwise the icon freezes mid-air.
        if self.tick_bounce() {
            anim = true;
        }

        // Next frame's `map_actual_to_base` uses this frame's centres.
        self.prev_act_c = slots.act_c;
        self.anim = anim;
    }

    /// Grace period after the cursor leaves before hiding, ms.
    ///
    /// Prevents the dock popping out when crossing the screen edge.
    pub const HIDE_GRACE_MS: u64 = 450;

    /// Whether the main loop must keep calling `tick_auto_hide`.
    ///
    /// Must stay true while `hide_cur` is mid-animation, or the dock stays
    /// hidden after a forced hide is released.
    #[must_use]
    pub fn needs_hide_tick(&self) -> bool {
        self.auto_hide() || self.force_hide || self.hide_cur > 0.0
    }

    /// Fully hidden once the animation finished; nothing remains on screen.
    ///
    /// The main loop skips frames then; drawing while hidden only burns CPU.
    #[must_use]
    pub fn fully_hidden(&self) -> bool {
        self.hide_cur > 0.999
    }

    /// Set the hide and show delays in ms; applied on the next tick.
    pub fn set_hide_delays(&mut self, hide_ms: u64, show_ms: u64) {
        self.hide_delay_ms = hide_ms;
        self.show_delay_ms = show_ms;
    }

    /// Advance the hide animation. Returns whether it is still running.
    pub fn tick_hide(&mut self, target_hidden: bool) -> bool {
        let target = if target_hidden { 1.0 } else { 0.0 };
        let d = target - self.hide_cur;
        if d.abs() > 0.01 {
            self.hide_cur += d * 0.25;
            self.anim = true;
            true
        } else {
            self.hide_cur = target;
            false
        }
    }

    /// Refresh the system sample.
    ///
    /// Not per frame: the OS calls are expensive and CPU load needs a 1 s window.
    pub fn refresh_sysinfo(&mut self) {
        self.sysinfo = nooksill_platform::sysinfo::sample(&mut self.cpu_sampler);
    }

    /// Last sample.
    #[must_use]
    pub const fn sysinfo(&self) -> nooksill_platform::SysInfo {
        self.sysinfo
    }
}
