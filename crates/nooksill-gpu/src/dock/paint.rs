//! Bar body, hint marks and geometry projection.

use super::*;

impl Dock {
    /// Insert index for an external drop in logical dock axes.
    ///
    /// `u` is the along-bar coordinate; callers already map window pixels.
    /// `usize::MAX` excludes no icon from the centre count.
    #[must_use]
    pub fn drop_index_at(&self, u: f32, _v: f32) -> usize {
        self.place_for(u, usize::MAX)
    }

    /// Allowed insert index during a drag.
    ///
    /// Running apps may enter the pinned zone; the main loop pins them there.
    /// Pinned items may not cross into the temporary segment (no config form).
    pub(super) fn clamp_segment(&self, target: usize, dragged: usize) -> usize {
        let n = self.pinned.min(self.items.len());
        if dragged < n {
            // After removal the pinned segment has n-1 items: [0, n-1].
            target.min(n.saturating_sub(1))
        } else {
            target.min(self.items.len().saturating_sub(1))
        }
    }

    /// Upload an already processed icon.
    ///
    /// Pixel work runs on the loader threads; texture creation must stay on the
    /// UI thread. Out-of-range indices are ignored.
    pub fn install_icon(&mut self, index: usize, ctx: &egui::Context, ready: &ReadyIcon) {
        if index >= self.items.len() {
            return;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [ready.width as usize, ready.height as usize],
            &ready.rgba,
        );
        let tex = ctx.load_texture(
            format!("dock-icon-{index}"),
            image,
            // Linear filtering: the icon is scaled, nearest would alias.
            egui::TextureOptions::LINEAR,
        );

        if self.icons.len() < self.items.len() {
            self.icons.resize(self.items.len(), None);
        }
        self.icons[index] = Some(IconTex {
            tex,
            src_w: ready.width,
            src_h: ready.height,
            bbox: ready.fit.bbox,
            fill: ready.fit.fill,
        });
    }

    /// Whether auto-hide is enabled; both non-`Always` modes count.
    #[must_use]
    pub fn auto_hide(&self) -> bool {
        self.mode != DockMode::Always
    }

    /// Advance auto-hide. Returns whether this frame needs a redraw.
    ///
    /// `covered` only applies to `SmartHide`. Showing needs continuous hover,
    /// hiding needs continuous absence; defaults 450/0 keep the old timing.
    pub fn tick_auto_hide(&mut self, near: bool, busy: bool, covered: bool, now_ms: u64) -> bool {
        // Forced hide wins over mode, cursor and coverage.
        if self.force_hide {
            self.show_pending_ms = None;
            return self.tick_hide(true);
        }
        if self.mode == DockMode::Always {
            // Still animate: releasing forced hide must slide `hide_cur` to 0.
            self.show_pending_ms = None;
            return self.tick_hide(false);
        }
        // Smart hide: uncovered counts as near.
        let forced = self.mode == DockMode::SmartHide && !covered;
        if near || busy || forced {
            self.last_near_ms = now_ms;
            if self.hide_target {
                // Hidden: require continuous hover for `show_delay_ms`; 0
                // shows instantly.
                if self.show_delay_ms == 0 {
                    self.show_pending_ms = None;
                    self.hide_target = false;
                    self.mark_dirty();
                } else {
                    match self.show_pending_ms {
                        None => self.show_pending_ms = Some(now_ms),
                        Some(since) if now_ms.saturating_sub(since) >= self.show_delay_ms => {
                            self.show_pending_ms = None;
                            self.hide_target = false;
                            self.mark_dirty();
                        }
                        _ => {}
                    }
                }
            } else {
                self.show_pending_ms = None;
            }
        } else {
            // Cancel a pending show; the hide delay still starts at the last
            // near time.
            self.show_pending_ms = None;
            if !self.hide_target && now_ms.saturating_sub(self.last_near_ms) > self.hide_delay_ms {
                // Clear hover, or icons slide down still magnified.
                self.hide_target = true;
                self.hover = false;
                self.mouse = (-1.0, -1.0);
                self.mark_dirty();
            }
        }
        self.tick_hide(self.hide_target)
    }

    /// Paint the whole dock.
    ///
    /// Call order is z-order. `ppp` is physical pixels per logical point;
    /// `clock` is `None` when the clock card is not shown. `frost` is the
    /// backdrop layer for this frame: `Some` makes every bar-shaped piece draw
    /// the blur behind itself under its translucent fill.
    pub fn paint(
        &self,
        ui: &egui::Ui,
        ppp: f32,
        dark: bool,
        clock: Option<&[ClockLine]>,
        frost: Option<crate::frost::Backdrop>,
    ) {
        let p = self.projection(ppp, frost);
        let painter = ui.painter();

        let bar = self.paint_bar_area(painter, p, dark);
        if self.items.is_empty() {
            painter.text(
                bar.center(),
                Align2::CENTER_CENTER,
                nooksill_core::i18n::t("Drop apps, folders or URLs here"),
                FontId::proportional(13.0),
                Color32::from_rgba_unmultiplied(255, 255, 255, 190),
            );
        }
        // Strip position comes from `WidgetStrip::left_of`.
        // With `WMerge` the strip shares one capsule, so draw it first.
        self.paint_widget_capsule(painter, p, dark);
        if let Some(lines) = clock {
            if !lines.is_empty() {
                self.paint_clock_card(painter, p, dark, lines);
            }
        }
        self.paint_ctrl_center_card(painter, p, dark);
        self.paint_resource_card(painter, p, dark);
        self.paint_tool_card(painter, p, dark);
        self.paint_tray_card(painter, p, dark);
        self.paint_separators(painter, p, dark);
        self.paint_icons(painter, p, dark);
        self.paint_insert_mark(painter, p, dark);
        // The drag ghost must come after all slot content, or icons cover it.
        self.paint_drag_ghost(painter, p, dark);
        self.paint_hover_bubble(painter, p);
        self.paint_drag_hint(painter, p, bar);
        // Notices go last, above icons and the hover bubble.
        self.paint_notice(painter, p, bar, dark);
    }

    /// Per-frame coordinate transform.
    ///
    /// `k` converts physical to logical points. Apply `(tx, ty)` before
    /// multiplying by `k`; the wrong order is off by half a screen at 150%.
    pub(super) fn projection(&self, ppp: f32, frost: Option<crate::frost::Backdrop>) -> Proj {
        let (tx, ty) = self.hide_offset();
        Proj {
            k: 1.0 / ppp,
            tx,
            ty,
            frost,
        }
    }

    /// How far the hide animation pushed the dock, physical pixels.
    ///
    /// Smoke and cloud effects also draw outside the bar and must move with it.
    #[must_use]
    pub fn hide_offset(&self) -> (f32, f32) {
        let m = &self.metrics;
        let off = self.hide_cur * (m.bar_h + m.margin + 14.0);
        match self.cfg.edge {
            Edge::Top => (0.0, -off),
            Edge::Left => (-off, 0.0),
            Edge::Right => (off, 0.0),
            Edge::Bottom => (0.0, off),
        }
    }

    /// Bar rectangle in window physical coordinates, without the hide offset.
    ///
    /// `geom` is in dock-local logical coordinates; effects need screen space.
    #[must_use]
    pub fn bar_phys(&self) -> (f32, f32, f32, f32) {
        layout::map_rect(
            self.cfg.edge,
            self.geom.bar_l,
            self.geom.bar_top,
            self.geom.bar_w,
            self.metrics.bar_h,
            self.metrics.lh as f32,
        )
    }

    /// Bounding rect of the whole dock band, window physical coordinates.
    ///
    /// Shared by near detection, occlusion and hit fallback. The widget order
    /// can be reversed, so take the union over every member.
    #[must_use]
    pub fn hot_phys(&self) -> (f32, f32, f32, f32) {
        let m = &self.metrics;
        let mut lo = self.geom.bar_l;
        let mut hi = self.geom.bar_l + self.geom.bar_w;
        for (i, (_, w)) in m.widgets.iter().enumerate() {
            if let Some(left) = m
                .widgets
                .left_of(i, self.geom.bar_l, self.geom.bar_w, m.gap)
            {
                lo = lo.min(left);
                hi = hi.max(left + w as f32);
            }
        }
        // Include the 6 px seam between strip and bar; it reads as the dock.
        let (x, y, w, h) = layout::map_rect(
            self.cfg.edge,
            lo - 6.0,
            self.geom.bar_top,
            (hi - lo + 12.0).max(1.0),
            m.bar_h,
            m.lh as f32,
        );
        // Extend toward the docked edge to cover the bar shadow.
        let margin = m.margin;
        match self.cfg.edge {
            Edge::Bottom => (x, y, w, h + margin),
            Edge::Top => (x, y - margin, w, h + margin),
            Edge::Left => (x - margin, y, w + margin, h),
            Edge::Right => (x, y, w + margin, h),
        }
    }

    /// How far the dock reaches into the screen, physical pixels.
    ///
    /// A magnified icon still counts as inside the dock.
    #[must_use]
    pub fn inner_reach(&self) -> f32 {
        if self.cfg.zoom {
            self.metrics.z
        } else {
            self.metrics.s
        }
    }

    /// Bar body. Returns its canvas rect, used for the empty-state text.
    pub(super) fn paint_bar_area(&self, painter: &egui::Painter, p: Proj, dark: bool) -> EguiRect {
        let m = &self.metrics;
        let bar = self.map(
            self.geom.bar_l,
            self.geom.bar_top,
            self.geom.bar_w,
            m.bar_h,
            p,
        );
        self.paint_bar(painter, bar, self.bar_radius(p), dark, None, p);
        bar
    }

    /// Bar radius, capped at half the bar height; more draws an ellipse.
    pub(super) fn bar_radius(&self, p: Proj) -> f32 {
        let r = self.pxf(self.cfg.radius).min(self.metrics.bar_h / 2.0);
        r * p.k
    }

    /// Whether the strip shares one capsule (`WMerge`, on by default).
    pub(super) fn widgets_merged(&self) -> bool {
        self.cfg.get_i32(Key::W_MERGE) != 0 && self.metrics.widgets.len > 0
    }

    /// Shared capsule for the widget strip.
    ///
    /// Spans the first to the last member; the gaps stay inside the capsule.
    pub(super) fn paint_widget_capsule(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        if !self.widgets_merged() {
            return;
        }
        let m = &self.metrics;
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for i in 0..m.widgets.len {
            let Some(left) = m
                .widgets
                .left_of(i, self.geom.bar_l, self.geom.bar_w, m.gap)
            else {
                continue;
            };
            let Some((_, w)) = m.widgets.items.get(i).copied().flatten() else {
                continue;
            };
            lo = lo.min(left);
            hi = hi.max(left + w as f32);
        }
        if hi <= lo {
            return;
        }
        let c = self.map(lo, self.geom.bar_top, hi - lo, m.bar_h, p);
        self.paint_bar(
            painter,
            c,
            self.card_radius(p),
            dark,
            self.widget_opacity(),
            p,
        );
    }

    /// Card background. Does nothing in merged mode; the capsule covers it.
    pub(super) fn paint_card_bg(&self, painter: &egui::Painter, c: EguiRect, dark: bool, p: Proj) {
        if self.widgets_merged() {
            return;
        }
        self.paint_bar(
            painter,
            c,
            self.card_radius(p),
            dark,
            self.widget_opacity(),
            p,
        );
    }

    /// Card background opacity override; `None` follows the bar.
    pub(super) fn widget_opacity(&self) -> Option<f32> {
        let v = self.cfg.get_i32(Key::WIDGET_OPACITY);
        (v > 0).then(|| v as f32 / 100.0)
    }

    /// Card radius; `0` follows the bar. Capped at half the bar height.
    pub(super) fn card_radius(&self, p: Proj) -> f32 {
        let v = self.cfg.get_i32(Key::WIDGET_RADIUS);
        if v <= 0 {
            return self.bar_radius(p);
        }
        self.pxf(v).min(self.metrics.bar_h / 2.0) * p.k
    }

    /// System tray card: a chevron that opens the Windows notification area.
    pub(super) fn paint_tray_card(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let m = &self.metrics;
        let Some(idx) = m
            .widgets
            .iter()
            .position(|(k, _)| k == layout::WidgetKind::Tray)
        else {
            return;
        };
        let Some(left) = m
            .widgets
            .left_of(idx, self.geom.bar_l, self.geom.bar_w, m.gap)
        else {
            return;
        };
        let width = m.widgets.width_of(layout::WidgetKind::Tray).unwrap_or(0);
        if width <= 0 {
            return;
        }
        let c = self.map(left, self.geom.bar_top, width as f32, m.bar_h, p);
        self.paint_card_bg(painter, c, dark, p);

        let s = (c.width().min(c.height()) * WidgetStyle::ICON_TRAY).max(3.0);
        let center = c.center();
        let color = WidgetStyle::ink(dark);
        let stroke = Stroke::new((s * 0.15).max(1.2), color);
        painter.line_segment(
            [
                Pos2::new(center.x - s * 0.62, center.y - s * 0.30),
                Pos2::new(center.x, center.y + s * 0.34),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(center.x, center.y + s * 0.34),
                Pos2::new(center.x + s * 0.62, center.y - s * 0.30),
            ],
            stroke,
        );
    }

    /// Control center card: a sliders glyph; clicking opens the system panel.
    pub(super) fn paint_ctrl_center_card(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let m = &self.metrics;
        let Some(idx) = m
            .widgets
            .iter()
            .position(|(k, _)| k == layout::WidgetKind::CtrlCenter)
        else {
            return;
        };
        let Some(left) = m
            .widgets
            .left_of(idx, self.geom.bar_l, self.geom.bar_w, m.gap)
        else {
            return;
        };
        let width = m
            .widgets
            .width_of(layout::WidgetKind::CtrlCenter)
            .unwrap_or(0);
        if width <= 0 {
            return;
        }
        let c = self.map(left, self.geom.bar_top, width as f32, m.bar_h, p);
        self.paint_card_bg(painter, c, dark, p);
        let color = WidgetStyle::ink(dark);
        // Smaller than the clock dial; the sliders already have padding.
        let s = c.width().min(c.height()) * WidgetStyle::ICON_CTRL;
        let icon = EguiRect::from_center_size(c.center(), egui::vec2(s, s));
        crate::glyph::sliders(painter, icon, crate::glyph::IconStyle::plain(color));
    }

    pub(super) fn paint_tool_card(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let m = &self.metrics;
        let Some(idx) = m
            .widgets
            .iter()
            .position(|(k, _)| k == layout::WidgetKind::Tool)
        else {
            return;
        };
        let Some(left) = m
            .widgets
            .left_of(idx, self.geom.bar_l, self.geom.bar_w, m.gap)
        else {
            return;
        };
        let width = m.widgets.width_of(layout::WidgetKind::Tool).unwrap_or(0);
        if width <= 0 {
            return;
        }
        let c = self.map(left, self.geom.bar_top, width as f32, m.bar_h, p);
        self.paint_card_bg(painter, c, dark, p);

        // Clock face plus hour and minute hands, sized from the short side.
        let size = (c.width().min(c.height()) * WidgetStyle::ICON_TOOL).max(4.0);
        let center = c.center();
        let color = WidgetStyle::ink(dark);
        let stroke = Stroke::new((size * 0.09).max(1.0), color);
        painter.circle_stroke(center, size / 2.0, stroke);
        painter.line_segment(
            [center, Pos2::new(center.x, center.y - size * 0.32)],
            stroke,
        );
        painter.line_segment(
            [
                center,
                Pos2::new(center.x + size * 0.28, center.y + size * 0.18),
            ],
            stroke,
        );
    }

    pub(super) fn paint_resource_card(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let m = &self.metrics;
        let Some(idx) = m
            .widgets
            .iter()
            .position(|(k, _)| k == layout::WidgetKind::Resource)
        else {
            return; // No resource card in the strip.
        };
        let Some(left) = m
            .widgets
            .left_of(idx, self.geom.bar_l, self.geom.bar_w, m.gap)
        else {
            return;
        };
        let width = m
            .widgets
            .width_of(layout::WidgetKind::Resource)
            .unwrap_or(0);
        if width <= 0 {
            return;
        }
        let c = self.map(left, self.geom.bar_top, width as f32, m.bar_h, p);
        self.paint_card_bg(painter, c, dark, p);

        // Wide cards also show memory.
        let text = if m.widgets.on_left || layout::WidgetKind::Resource.is_wide(&self.cfg) {
            format!(
                "CPU {}%   RAM {}%",
                self.sysinfo.cpu_percent, self.sysinfo.mem_percent
            )
        } else {
            format!("CPU {}%", self.sysinfo.cpu_percent)
        };
        let color = WidgetStyle::ink(dark);
        // Shrink to fit; a negative width would panic epaint's glyph atlas.
        let avail = c.width() - WidgetStyle::PAD_X * 2.0;
        if avail <= WidgetStyle::MIN_TEXT_WIDTH {
            return;
        }
        let size0 = self.card_font_size(c.height());
        let mut galley = painter.layout_no_wrap(text.clone(), FontId::proportional(size0), color);
        if galley.size().x > avail && galley.size().x > 0.0 {
            let size = (size0 * avail / galley.size().x).max(1.0);
            galley = painter.layout_no_wrap(text, FontId::proportional(size), color);
        }
        let pos = Align2::CENTER_CENTER
            .align_size_within_rect(galley.size(), c)
            .min;
        painter.galley(pos, galley, color);
    }

    /// Font size for widget-card text, in logical points.
    ///
    /// Uses the card's logical height so DPI is applied once; the caller
    /// shrinks further when the width does not fit.
    pub(super) fn card_font_size(&self, card_h: f32) -> f32 {
        (card_h * WidgetStyle::FONT_RATIO).max(WidgetStyle::FONT_MIN)
    }
}
