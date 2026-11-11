//! Widget cards, clock, icons and bubbles.

use super::*;

impl Dock {
    /// Clock card: bar, time and date, sized by `clock_w`.
    ///
    /// Position comes from the widget strip, so `WSide` works.
    pub(super) fn paint_clock_card(
        &self,
        painter: &egui::Painter,
        p: Proj,
        dark: bool,
        lines: &[ClockLine],
    ) {
        let m = &self.metrics;
        // The clock is the only strip member today, so index 0 is the fallback.
        let idx = m
            .widgets
            .iter()
            .position(|(k, _)| k == layout::WidgetKind::Clock)
            .unwrap_or(0);
        let left = m
            .widgets
            .left_of(idx, self.geom.bar_l, self.geom.bar_w, m.gap)
            .unwrap_or(self.geom.bar_l + self.geom.bar_w + m.gap + 6.0);
        let c = self.map(left, self.geom.bar_top, m.clock_w as f32, m.bar_h, p);
        self.paint_card_bg(painter, c, dark, p);

        let (tc, dc) = (WidgetStyle::ink(dark), WidgetStyle::ink_sub(dark));
        // Rows split the card height; the card can show one or two lines.
        // Derive font size from logical row height so DPI is applied once.
        let n = lines.len().max(1);
        // The date line nearly fills the card, so horizontal padding stays wide.
        let (pad_x, pad_y) = (WidgetStyle::PAD_X, WidgetStyle::PAD_Y);
        let line_h = (c.height() - pad_y * 2.0) / n as f32;
        let avail = c.width() - pad_x * 2.0;
        // Skip degenerate cards; a zero width makes the font size negative and
        // epaint asserts on that.
        if !(line_h > 4.0 && avail > WidgetStyle::MIN_TEXT_WIDTH) {
            return;
        }
        let mut prev: Option<&ClockLine> = None;
        for (k, line) in lines.iter().enumerate() {
            // Time is one step larger; CJK glyphs already look heavier.
            let size0 = clock_line_size(line_h, line.is_date);
            let color = if line.is_date { dc } else { tc };
            // Measure once and reuse the galley; `painter.text` would shape twice.
            let mut galley =
                painter.layout_no_wrap(line.text.clone(), FontId::proportional(size0), color);
            if galley.size().x > avail && galley.size().x > 0.0 {
                let size = (size0 * avail / galley.size().x).max(1.0);
                galley =
                    painter.layout_no_wrap(line.text.clone(), FontId::proportional(size), color);
            }
            let row = EguiRect::from_min_max(
                Pos2::new(c.left(), c.top() + pad_y + line_h * k as f32),
                Pos2::new(c.right(), c.top() + pad_y + line_h * (k + 1) as f32),
            );
            let pos = Align2::CENTER_CENTER
                .align_size_within_rect(galley.size(), row)
                .min;
            painter.galley(pos, galley, color);
            // Hairline between the time and date groups.
            if let Some(pv) = prev {
                if pv.is_date != line.is_date {
                    let y = c.top() + pad_y + line_h * k as f32;
                    let ink = WidgetStyle::rule(dark);
                    painter.line_segment(
                        [
                            Pos2::new(c.left() + c.width() * 0.14, y),
                            Pos2::new(c.left() + c.width() * 0.86, y),
                        ],
                        Stroke::new(p.k, ink),
                    );
                }
            }
            prev = Some(line);
        }
    }

    /// Separator: diagonal line 0.84 S tall, rising from the icon bottom.
    pub(super) fn paint_separators(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let m = &self.metrics;
        let color = if dark {
            argbc(110, 255, 255, 255)
        } else {
            argbc(120, 60, 60, 70)
        };
        for (i, it) in self.items.iter().enumerate() {
            // Skip separators still growing or shrinking; a 1 px stub is noise.
            if !it.is_sep() || it.cur < 2.0 {
                continue;
            }
            let (lx, ly, lw, lh) = layout::map_rect(
                self.cfg.edge,
                self.geom.act_c[i] + it.slide,
                self.geom.icon_bottom - m.s * 0.88,
                0.0,
                m.s * 0.84,
                m.lh as f32,
            );
            let a = self.to_egui(lx, ly, p);
            let b = self.to_egui(lx + lw, ly + lh, p);
            // `SepWidth` is 0..16, default 2; 0.7 keeps the old 1.4 px look.
            let sw = self.pxf(self.cfg.get_i32(Key::SEP_WIDTH).clamp(0, 16));
            painter.line_segment([a, b], Stroke::new((sw * 0.7).max(0.5) * p.k, color));
        }
    }

    /// Icons, running dots and the merge highlight.
    pub(super) fn paint_icons(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        // The dragged item is not drawn in its slot; see `paint_drag_ghost`.
        let dragged = self.dragging().map(|d| d.index);
        for (i, it) in self.items.iter().enumerate() {
            if it.is_sep() || it.cur < 1.0 || dragged == Some(i) {
                continue;
            }
            let sz = it.cur;
            let lx = self.geom.slot_l[i] + it.slide;
            // Bounce moves outward along the bar thickness, so subtract from `v`.
            let bo = it.bounce_offset(self.metrics.s);
            self.paint_icon(painter, p, dark, i, it, lx, sz, bo);
            // Highlighted group: release here to merge.
            if self.merge_target() == Some(i) {
                self.paint_merge_hint(painter, p, i, sz);
            }
            // Draw the dot only for running apps.
            if self.cfg.dots && it.running {
                self.paint_running_dot(painter, p, dark, lx, sz);
            }
        }
    }

    /// One icon: optional plate plus glyph or texture.
    #[expect(clippy::too_many_arguments, reason = "all inputs for one icon")]
    pub(super) fn paint_icon(
        &self,
        painter: &egui::Painter,
        p: Proj,
        dark: bool,
        index: usize,
        it: &DockItem,
        lx: f32,
        sz: f32,
        bounce: f32,
    ) {
        let m = &self.metrics;
        let (ix, iy, iw, ih) = layout::map_rect(
            self.cfg.edge,
            lx,
            self.geom.icon_bottom - sz - bounce,
            sz,
            sz,
            m.lh as f32,
        );
        let rect = EguiRect::from_min_size(self.to_egui(ix, iy, p), egui::vec2(iw * p.k, ih * p.k));

        // Plate: 0 none, 1 solid, 2 translucent with a stroke.
        // `PlateTheme`: 0 force light, 1 force dark, 2 follow the dock.
        let plate = self.cfg.get_i32(Key::PLATE_MODE);
        let mut inner = rect;
        if plate > 0 {
            let pr = (sz * self.cfg.get_i32(Key::PlateRadius) as f32 / 100.0).min(sz / 2.0);
            let radius = CornerRadius::same(r8(pr * p.k));
            let plate_dark = match self.cfg.get_i32(Key::PLATE_THEME) {
                0 => false,
                1 => true,
                _ => dark,
            };
            let a = (self.cfg.get_i32(Key::PLATE_OPACITY).clamp(0, 100) * 255 / 100) as u8;
            let (r, g, b) = if plate == 1 {
                if plate_dark {
                    (48, 50, 58)
                } else {
                    (244, 245, 249)
                }
            } else if plate_dark {
                (18, 19, 24)
            } else {
                (255, 255, 255)
            };
            painter.rect_filled(rect, radius, argbc(a, r, g, b));
            // Only the translucent plate gets a stroke.
            if plate == 2 {
                painter.rect_stroke(
                    rect,
                    radius,
                    Stroke::new(p.k, argbc(60, 255, 255, 255)),
                    StrokeKind::Middle,
                );
            }
            // Inset the icon 13% so the plate shows around it.
            let inset = sz * 0.13;
            inner = EguiRect::from_min_size(
                rect.min + egui::vec2(inset * p.k, inset * p.k),
                egui::vec2((iw - 2.0 * inset) * p.k, (ih - 2.0 * inset) * p.k),
            );
        }

        // `UiAccentDockIcons` is independent of the bar accent; off means blue.
        let accent = if self.cfg.get_i32(Key::UI_ACCENT_DOCK_ICONS) != 0 {
            self.cfg.runtime_icon_accent
        } else {
            None
        }
        .map_or_else(
            || Color32::from_rgb(46, 123, 246),
            |a| argbc(255, a[0], a[1], a[2]),
        );
        let style = crate::glyph::IconStyle::new(
            Color32::WHITE,
            accent,
            crate::glyph::IconState::default(),
        );

        // Built-in entries (Start, Launchpad) have no shell icon; draw them.
        if let Some(kind) = crate::glyph::Builtin::from_path(&it.path) {
            crate::glyph::paint_tile(painter, inner, kind, style, dark);
            return;
        }

        // Texture: sample `bbox`, scale to `fill`, centred on the long side.
        // Scaling is left to the GPU; the size changes every frame.
        if let Some(icon) = self.icons.get(index).and_then(Option::as_ref) {
            let (aw, ah) = icon.aspect();
            let long = inner.width() * icon.fill;
            let dst = EguiRect::from_center_size(inner.center(), egui::vec2(aw * long, ah * long));
            painter.image(icon.tex.id(), dst, icon.uv(), Color32::WHITE);
            return;
        }

        // No shell icon: draw a glyph by item kind.
        crate::glyph::paint_kind(painter, inner, it.kind, style, dark);
    }

    /// Merge hint: accent stroke around the target group icon.
    pub(super) fn paint_merge_hint(&self, painter: &egui::Painter, p: Proj, index: usize, sz: f32) {
        let Some((ix, iy, iw, ih)) = self.icon_rect(index) else {
            return;
        };
        let rect = EguiRect::from_min_size(self.to_egui(ix, iy, p), egui::vec2(iw * p.k, ih * p.k));
        let accent = self.cfg.runtime_icon_accent.unwrap_or([46, 123, 246]);
        let color = Color32::from_rgb(accent[0], accent[1], accent[2]);
        painter.rect_stroke(
            rect.expand(3.0 * p.k),
            CornerRadius::same(r8(sz * 0.16 * p.k)),
            Stroke::new(2.0 * p.k, color),
            StrokeKind::Middle,
        );
    }

    /// Running indicator: a horizontal bar or a dot at the bar bottom.
    pub(super) fn paint_running_dot(
        &self,
        painter: &egui::Painter,
        p: Proj,
        dark: bool,
        lx: f32,
        sz: f32,
    ) {
        let m = &self.metrics;
        let round = self.cfg.get_i32(Key::DotStyle) != 0;
        // Bar 0.34 S wide and S/12 tall (min 3.6); dot diameter S/10 (min 7).
        let mut pw = if round { m.s / 10.0 } else { m.s * 0.34 };
        let mut ph = m.s / 12.0;
        if ph < 3.6 {
            ph = 3.6;
        }
        if round {
            ph = pw;
        }
        if pw < 7.0 {
            pw = 7.0;
        }

        let (dx, dy, dw, dh) = layout::map_rect(
            self.cfg.edge,
            lx + sz / 2.0 - pw / 2.0,
            self.geom.bar_bottom - 2.5 - ph,
            pw,
            ph,
            m.lh as f32,
        );
        let dot = EguiRect::from_min_size(self.to_egui(dx, dy, p), egui::vec2(dw * p.k, dh * p.k));
        let radius = CornerRadius::same(r8((dot.width().min(dot.height())) / 2.0));
        // Translucent black halo, not a stroke, so a white dot reads anywhere.
        painter.rect_filled(
            dot.expand(p.k),
            CornerRadius::same(r8(dot.width().min(dot.height()) / 2.0 + p.k)),
            argbc(120, 0, 0, 0),
        );
        let fill = if let Some(a) = self.cfg.runtime_accent {
            argbc(255, a[0], a[1], a[2])
        } else if dark {
            argbc(255, 255, 255, 255)
        } else {
            argbc(255, 24, 26, 34)
        };
        painter.rect_filled(dot, radius, fill);
    }

    /// Drag ghost: translucent, 15% larger than the slot.
    ///
    /// Uses client coordinates directly; it follows the cursor, not the dock axis.
    pub(super) fn paint_drag_ghost(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let Some(d) = self.dragging() else {
            return;
        };
        let sz = self.metrics.s * 1.15;
        // Offset upward so the cursor grips the lower part of the icon.
        let lx = d.pt.0 - sz / 2.0;
        let ly = d.pt.1 - sz * 0.6;
        let rect = EguiRect::from_min_size(self.to_egui(lx, ly, p), egui::vec2(sz * p.k, sz * p.k));

        // Alpha 0.78.
        let tint = Color32::from_white_alpha(200);
        if let Some(icon) = self.icons.get(d.index).and_then(Option::as_ref) {
            let (aw, ah) = icon.aspect();
            let long = rect.width() * icon.fill;
            let dst = EguiRect::from_center_size(rect.center(), egui::vec2(aw * long, ah * long));
            painter.image(icon.tex.id(), dst, icon.uv(), tint);
            return;
        }
        // No texture yet (favicon pending): draw a translucent initial tile.
        let Some(it) = self.items.get(d.index) else {
            return;
        };
        painter.rect_filled(
            rect,
            CornerRadius::same(r8(6.0 * p.k)),
            if dark {
                argbc(160, 90, 100, 130)
            } else {
                argbc(140, 160, 90, 100)
            },
        );
        if let Some(ch) = it.name.chars().next() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                ch.to_string(),
                FontId::proportional((sz * 0.4).max(10.0) * p.k),
                Color32::from_white_alpha(230),
            );
        }
    }

    /// Hover bubble; only when labels are on and the icon has grown.
    pub(super) fn paint_hover_bubble(&self, painter: &egui::Painter, p: Proj) {
        // No label while dragging; the ghost already sits under the cursor.
        if self.dragging().is_some() {
            return;
        }
        let Some(hot) = self.hot else { return };
        if !self.cfg.labels || !it_visible(self, hot) {
            return;
        }
        self.paint_bubble(painter, hot, p);
    }

    /// Bar body: solid or gradient, with an optional border.
    pub(super) fn paint_bar(
        &self,
        painter: &egui::Painter,
        rect: EguiRect,
        rad: f32,
        dark: bool,
        op_override: Option<f32>,
    ) {
        // Widget cards can override opacity; `None` follows the bar.
        let op = op_override.unwrap_or(self.cfg.opacity as f32 / 100.0);
        if op <= 0.0 {
            return;
        }
        let (c1, c2) = self.bar_colors(dark);
        // Each stop has its own alpha; `BgColor3 < 0` disables the mid stop.
        let alpha = |key: Key| -> u8 {
            let pct = self.cfg.get_i32(key).clamp(0, 100) as f32 / 100.0;
            (op * 255.0 * pct).round() as u8
        };
        let from = Color32::from_rgba_unmultiplied(
            c1.0 as u8,
            c1.1 as u8,
            c1.2 as u8,
            alpha(Key::GRAD_A1),
        );
        let to = Color32::from_rgba_unmultiplied(
            c2.0 as u8,
            c2.1 as u8,
            c2.2 as u8,
            alpha(Key::GRAD_A2),
        );
        let mid_rgb = self.cfg.get_i32(Key::BG_COLOR3);
        let mid = (mid_rgb >= 0).then(|| {
            let v = mid_rgb as u32;
            Color32::from_rgba_unmultiplied(
                (v >> 16) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
                alpha(Key::GRAD_A3),
            )
        });
        // Angle defaults to 90; vertical docks keep the old per-edge direction.
        let raw_angle = self.cfg.get_i32(Key::GRAD_ANGLE);
        let angle_deg = if raw_angle == 90 {
            if self.cfg.edge.is_horizontal() {
                0.0
            } else {
                90.0
            }
        } else {
            raw_angle.rem_euclid(360) as f32
        };
        let grad = BarGradient {
            from,
            mid,
            to,
            angle_deg,
        };
        // `BgMode::Solid` draws one colour; gradient and frost share this path.
        // Frost is not implemented yet, so the default visibly degrades.
        if self.cfg.get_i32(Key::BgMode) == 0 {
            painter.rect_filled(rect, CornerRadius::same(r8(rad)), from);
        } else {
            // `gradient_rect` has only 4 vertices, so the fill would be square.
            painter.add(rounded_gradient(rect, rad, grad));
        }
        if self.cfg.border {
            let base = if dark { 90.0 } else { 150.0 };
            let a = (base * if op < 0.35 { op / 0.35 } else { 1.0 }) as u8;
            // `BorderWidth` 1..6 maps to 1.2..3.7 px; 1 matches the old default.
            let step = self.cfg.get_i32(Key::BORDER_WIDTH).clamp(1, 6) - 1;
            let w = 1.2 + step as f32 * 0.5;
            painter.rect_stroke(
                rect,
                CornerRadius::same(r8(rad)),
                Stroke::new(w, Color32::from_rgba_unmultiplied(255, 255, 255, a)),
                StrokeKind::Middle,
            );
        }
    }

    /// Two bar colours: synced accent, then custom background, then default.
    ///
    /// The accent is mixed into the base rather than used directly.
    pub(super) fn bar_colors(&self, dark: bool) -> ((f32, f32, f32), (f32, f32, f32)) {
        if let Some(a) = self.cfg.runtime_accent {
            let t = [a[0] as f32, a[1] as f32, a[2] as f32];
            let mix = |base: [f32; 3], k: f32| {
                [
                    base[0] * (1.0 - k) + t[0] * k,
                    base[1] * (1.0 - k) + t[1] * k,
                    base[2] * (1.0 - k) + t[2] * k,
                ]
            };
            return if dark {
                let c1 = mix([58.0, 60.0, 68.0], 0.30);
                let c2 = mix([30.0, 31.0, 36.0], 0.22);
                ((c1[0], c1[1], c1[2]), (c2[0], c2[1], c2[2]))
            } else {
                let c1 = mix([255.0, 255.0, 255.0], 0.16);
                let c2 = mix([232.0, 235.0, 243.0], 0.28);
                ((c1[0], c1[1], c1[2]), (c2[0], c2[1], c2[2]))
            };
        }
        if !self.cfg.accent_tone && self.cfg.bg_color1 >= 0 {
            let c1 = color::from_colorref(self.cfg.bg_color1 as u32);
            let c2 = if self.cfg.bg_color2 >= 0 {
                color::from_colorref(self.cfg.bg_color2 as u32)
            } else {
                // No end colour: darken the start by 25% to keep a gradient.
                color::mix(c1, color::rgb(0, 0, 0), 0.25)
            };
            return (to_rgb(c1), to_rgb(c2));
        }
        if dark {
            ((58.0, 60.0, 68.0), (30.0, 31.0, 36.0))
        } else {
            ((255.0, 255.0, 255.0), (232.0, 235.0, 243.0))
        }
    }

    /// Hover bubble: width is text + 24, height 28, radius 9.
    ///
    /// Placement depends on the dock edge and is clamped inside the window.
    pub(super) fn paint_bubble(&self, painter: &egui::Painter, index: usize, p: Proj) {
        let k = p.k;
        let m = &self.metrics;
        let it = &self.items[index];
        let sz = it.cur;
        let text = it.name.as_str();
        let (ix, iy, iw, ih) = layout::map_rect(
            self.cfg.edge,
            self.geom.slot_l[index],
            self.geom.icon_bottom - sz,
            sz,
            sz,
            m.lh as f32,
        );
        let font = FontId::proportional(12.0);
        let color = argbc(255, 245, 245, 247);
        // Reuse the galley for measuring and drawing; `painter.text` shapes twice.
        let galley = painter.layout_no_wrap(text.to_owned(), font, color);
        let text_w = galley.size().x;
        let w = text_w + 24.0 * k;
        let h = 28.0 * k;
        let (mut x, mut y) = match self.cfg.edge {
            Edge::Top => (ix + iw / 2.0 - w / 2.0, iy + ih + 8.0 * k),
            Edge::Left => (ix + iw + 10.0 * k, iy + ih / 2.0 - h / 2.0),
            Edge::Right => (ix - 10.0 * k - w, iy + ih / 2.0 - h / 2.0),
            Edge::Bottom => (ix + iw / 2.0 - w / 2.0, iy - 8.0 * k - h),
        };
        // Clamp 4 px inside the window; vertical docks swap width and height.
        let (win_w, win_h) = layout::physical_size(self.cfg.edge, m.lw, m.lh);
        let (win_w, win_h) = (win_w as f32, win_h as f32);
        if x < 4.0 * k {
            x = 4.0 * k;
        }
        if x + w > win_w - 4.0 * k {
            x = win_w - 4.0 * k - w;
        }
        if y < 4.0 * k {
            y = 4.0 * k;
        }
        if y + h > win_h - 4.0 * k {
            y = win_h - 4.0 * k - h;
        }
        let p0 = self.to_egui(x, y, p);
        let rect = EguiRect::from_min_size(p0, egui::vec2(w, h));
        painter.rect_filled(
            rect,
            CornerRadius::same(r8(9.0 * k)),
            argbc(228, 38, 38, 42),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(r8(9.0 * k)),
            Stroke::new(1.0 * k, argbc(50, 255, 255, 255)),
            StrokeKind::Middle,
        );
        let pos = Align2::CENTER_CENTER
            .align_size_within_rect(galley.size(), rect)
            .min;
        painter.galley(pos, galley, color);
    }

    /// Dock-local logical to canvas coordinates, including the hide offset.
    pub(super) fn map(&self, u: f32, v: f32, w: f32, h: f32, p: Proj) -> EguiRect {
        let (x, y, ww, hh) = layout::map_rect(self.cfg.edge, u, v, w, h, self.metrics.lh as f32);
        EguiRect::from_min_size(self.to_egui(x, y, p), egui::vec2(ww * p.k, hh * p.k))
    }

    /// Physical to canvas coordinates, including the hide offset.
    pub(super) fn to_egui(&self, x: f32, y: f32, p: Proj) -> Pos2 {
        Pos2::new((x + p.tx) * p.k, (y + p.ty) * p.k)
    }
}
