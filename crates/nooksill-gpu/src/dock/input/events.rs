//! Construction, event queues, notices and drop anchors.

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
            geom_epoch: 0,
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
    /// `x`/`y` are logical dock axes. The first message of a batch sets the
    /// anchor, since one drop can arrive as several `WM_DROPFILES`.
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
    /// Takes logical axes, like `hit_at`.
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
    pub(in crate::dock) const NOTICE_MS: u128 = 2200;
    /// Fade-out duration at the end.
    pub(in crate::dock) const NOTICE_FADE_MS: u128 = 300;

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
    pub(in crate::dock) fn notice_alpha(elapsed_ms: u128) -> f32 {
        let remain = Self::NOTICE_MS.saturating_sub(elapsed_ms);
        (remain as f32 / Self::NOTICE_FADE_MS as f32).clamp(0.0, 1.0)
    }
}
