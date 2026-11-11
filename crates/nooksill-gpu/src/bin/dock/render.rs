//! Frame presentation, positioning and idle waiting.

use super::misc::{raw_input, FS_CHECK_MS};
use super::sys::now_parts;
use super::*;

impl App {
    /// Print the screen rect of every widget card in the strip.
    ///
    /// Automation needs screen coordinates; letting the app report them beats
    /// recomputing the window offset in a script.
    pub(super) fn dump_widgets(&mut self) {
        if !trace::geom() {
            return;
        }
        let dock = &*self.dock;
        // Print only when the geometry changed: a per-frame dump floods the
        // log on 50 ms frames.
        let mut line = String::new();
        for (i, (kind, w)) in dock.metrics.widgets.iter().enumerate() {
            if let Some(left) =
                dock.metrics
                    .widgets
                    .left_of(i, dock.geom.bar_l, dock.geom.bar_w, dock.metrics.gap)
            {
                line.push_str(&format!(
                    "GEOM widget={kind:?} screen=({x},{y}) size=({w},{h})\n",
                    x = left as i32 + self.pos.0,
                    y = dock.geom.bar_top as i32 + self.pos.1,
                    w = w,
                    h = dock.metrics.bar_h as i32
                ));
            }
        }
        if line != self.widgets_dump {
            self.widgets_dump = line.clone();
            eprint!("{line}");
        }
    }

    /// Draw one frame: egui paints the dock, offscreen, read back, layered
    /// window.
    ///
    /// The `NOOKSILL_FPS` line reports both the rate and the cost per frame:
    /// the two windows of this process share one thread, so "how expensive is
    /// a bar frame" decides how much the settings window gets.
    pub(super) fn render(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let started = Instant::now();
        let drawn = self.render_frame();
        if trace::fps() {
            self.fps_ms += started.elapsed().as_secs_f64() * 1000.0;
            self.fps_frames += 1;
            let dt = self.fps_at.elapsed();
            if dt >= Duration::from_secs(1) {
                eprintln!(
                    "[fps] {:.1} 帧/秒 · {:.2} ms/帧（hidden={} anim={}）",
                    f32::from(u16::try_from(self.fps_frames).unwrap_or(u16::MAX))
                        / dt.as_secs_f32(),
                    self.fps_ms / f64::from(self.fps_frames.max(1)),
                    self.dock.fully_hidden(),
                    self.dock.anim
                );
                self.fps_frames = 0;
                self.fps_ms = 0.0;
                self.fps_at = Instant::now();
            }
        }
        drawn
    }

    fn render_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Panel styling is a process-wide preference (the seven popup
        // constructors take no `Config`), synced per frame so a settings change
        // applies next frame. Costs two table lookups and two atomic writes.
        nooksill_gpu::panel_style::sync_prefs(&self.dock.cfg);
        // The dock theme, not the settings theme: `UiTheme` and `DockTheme`
        // invert 0/1, so mixing them repaints the dock in the wrong palette.
        // Clock text comes from `core::clock`, which owns the date format,
        // weekday, line count and ordering.
        let clock: Option<Vec<ClockLine>> = if self.dock.cfg.clock {
            Some(clock::clock_lines(&self.dock.cfg, &now_parts()))
        } else {
            None
        };
        // Re-read the accent (the smoke color ramp needs it) and dark mode when
        // the system appearance changed; the theme bump is `look_epoch`.
        if self.dock.look_epoch != self.look_epoch {
            self.look_epoch = self.dock.look_epoch;
            self.accent = theme::accent().unwrap_or(PALETTE[0]);
            self.system_dark = theme::is_dark();
        }
        let dark = self.dock.cfg.dock_is_dark(self.system_dark);

        let ppp = self.ppp;
        // Resolve the accent once for both switches: `accent::resolve` queries
        // DWM across processes when following the system, so resolving per field
        // doubles the round-trip.
        let dock_accent_on = self.dock.cfg.get_i32(Key::UI_ACCENT_DOCK) != 0;
        let icon_on = self.dock.cfg.get_i32(Key::UI_ACCENT_DOCK_ICONS) != 0
            || self.dock.cfg.get_i32(Key::UI_ACCENT_CC_ICONS) != 0;
        let resolved = if dock_accent_on || icon_on {
            nooksill_gpu::accent::resolve(&self.dock.cfg)
        } else {
            None
        };
        self.dock.cfg.runtime_accent = if dock_accent_on { resolved } else { None };
        self.dock.cfg.runtime_icon_accent = if icon_on { resolved } else { None };
        let cc_accent = if self.dock.cfg.get_i32(Key::UI_ACCENT_CC_ICONS) != 0 {
            self.dock.cfg.runtime_icon_accent
        } else {
            None
        };
        if let Some(win) = self.cc_slot.as_mut() {
            win.set_icon_accent(cc_accent);
        }
        // A reflection is this frame's mirrored pixels, so it belongs in the
        // presentation layer; hand it the geometry first.
        self.frame.set_reflection(self.dock.reflection_spec());
        // Resolve the backdrop once: `render` borrows the renderer mutably, so
        // the texture handle cannot be looked up inside the closure.
        let frost = self.frame.frost_backdrop();
        let dock = &*self.dock;
        // Hoist all smoke inputs out of the closure: it holds `&Dock` and
        // `&mut SmokeFx` at once, so reading other `self` fields would conflict.
        let tune = Tuning::from_config(&dock.cfg, self.accent);
        let view = SmokeView {
            // The glow covers the whole docking strip (icons + widget band),
            // the same union used for cursor and occlusion tests.
            bar: dock.hot_phys(),
            offset: dock.hide_offset(),
            // Smoke works in window pixels; the dock stores logical axes.
            cursor: dock
                .hover
                .then(|| dock.window_point(dock.mouse.0, dock.mouse.1)),
            now_s: self.start.elapsed().as_secs_f64(),
        };
        let smoke = &mut self.smoke;
        let raw = raw_input(self.frame.size(), ppp, self.start);
        self.frame
            .render(&self.gpu, raw, self.hwnd, self.pos, |ui| {
                // Smoke is a background glow and must come first; painting it
                // on top would tint the icons.
                smoke.paint(ui, ppp, &tune, view);
                dock.paint(ui, ppp, dark, clock.as_deref(), frost);
            })?;
        self.dump_widgets();
        Ok(())
    }

    /// Smoke needs a frame whenever visible. Pacing comes from the global
    /// frame cap / VSync setting, not a per-effect timer.
    pub(super) fn step_smoke(&mut self) -> bool {
        let tune = Tuning::from_config(&self.dock.cfg, self.accent);
        if !tune.enabled || self.fs_hidden || self.dock.fully_hidden() {
            return false;
        }
        true
    }

    /// Where the current edge, reservation and size put the window.
    pub(super) fn desired_origin(&self) -> (i32, i32) {
        layout::window_origin(
            self.dock.cfg.edge,
            self.work,
            self.reserved,
            self.size.0,
            self.size.1,
        )
    }

    /// Recompute the window origin for the current edge, reservation and size.
    pub(super) fn reposition(&mut self) {
        self.pos = self.desired_origin();
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                self.pos.0,
                self.pos.1,
                self.size.0,
                self.size.1,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Wait for the next event: wake on messages, every frame interval while
    /// animating, poll the cursor at 100 ms while auto-hide is active, and wake
    /// for smoke.
    ///
    /// Worker threads never need polling; each posts `WM_APP_WAKE` to break the
    /// wait, so this can sleep up to the full `FS_CHECK_MS`.
    ///
    /// `yielded` means the bar is skipping its frames because the settings
    /// window is animating: the smoke then has nothing to pace, and asking for
    /// a frame interval would spin the loop at 1 ms for no work.
    pub(super) fn wait(&mut self, yielded: bool) {
        let smoke_on = !yielded
            && Tuning::from_config(&self.dock.cfg, self.accent).enabled
            && !self.fs_hidden
            && !self.dock.fully_hidden();
        // 1 ms timer precision only while something animates. The settings
        // window repaint counts too: it is not in `dock.anim` and would sit at
        // ~64 Hz under the default 15.6 ms granularity.
        let settings_active = self
            .settings_slot
            .as_ref()
            .is_some_and(|sw| sw.has_pending_repaint());
        // A hidden dock renders no frame, so its stale deadline must not wake us.
        let frame_repaint = if self.dock.fully_hidden() || self.fs_hidden {
            None
        } else {
            self.frame.repaint_in()
        };
        self.set_timer_resolution(
            self.dock.anim || smoke_on || settings_active || frame_repaint.is_some(),
        );
        // Active = continuous repaint. Idle keeps the "zero frames" promise.
        let mut timeout = if self.dock.anim || smoke_on {
            self.frame_interval_ms()
        } else if self.dock.needs_hide_tick() {
            100
        } else {
            FS_CHECK_MS
        };
        // Panel dismissal polls global key state.
        if self.panel_open() && self.dock.cfg.get_i32(Key::PANEL_DISMISS) != 0 {
            timeout = timeout.min(nooksill_gpu::popup::POLL_MS);
        }
        // The settings window owns an egui repaint deadline. Without this the
        // main loop could sleep a full second and stall its UI.
        if let Some(sw) = &self.settings_slot {
            if let Some(left) = sw.repaint_in() {
                if left != Duration::MAX {
                    timeout = timeout.min(left.as_millis().clamp(1, u128::from(u32::MAX)) as u32);
                }
            }
        }
        // egui can ask for a frame while none of the dock's own flags are set.
        if let Some(left) = frame_repaint {
            timeout = timeout.min(left.as_millis().clamp(1, u128::from(u32::MAX)) as u32);
        }
        unsafe {
            MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }

    /// Frame pacing. 0 = unlimited (1 ms pump); VSync overrides the cap.
    pub(super) fn frame_interval_ms(&self) -> u32 {
        let cfg = &self.dock.cfg;
        let vsync = cfg.get_i32(Key::VSYNC) != 0;
        let vsync_ms = if vsync {
            nooksill_gpu::perf::vsync_interval_ms(self.hwnd)
        } else {
            16
        };
        nooksill_gpu::perf::pacing_ms(cfg.get_i32(Key::FRAME_CAP), vsync, vsync_ms)
    }
}
