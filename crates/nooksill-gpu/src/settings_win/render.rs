//! Settings window painting and theme/stat bookkeeping.

use super::shell::shell_with_frost;
use super::win::trace;
use super::*;

impl SettingsWindow {
    /// Draws one frame and presents it. Returns the cursor egui wants and the
    /// delay until the next repaint.
    ///
    /// `cfg` is the Dock's own config: whatever `settings_ui` changes in it
    /// takes effect on the next frame, with no copy and no notification.
    pub fn render(&mut self, gpu: &Gpu, cfg: &mut Config) -> Result<(), GpuError> {
        let started = Instant::now();
        self.apply_theme(cfg.get_i32(Key::UiTheme));

        let raw = self.input.take(self.renderer.size(), self.ppp);
        if trace() && !raw.events.is_empty() {
            eprintln!("[settings] 交给 egui 的输入事件：{:?}", raw.events);
        }
        let pos = self.pos;
        let hwnd = self.hwnd;
        let frost = self.renderer.frost_on();
        let state = &mut self.state;
        let mut outcome = Outcome::default();
        let out = self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            shell_with_frost(ui, state, cfg, &mut outcome, frost);
        })?;

        self.outcome = outcome;
        if trace() && (self.outcome.changed || self.outcome.command.is_some()) {
            eprintln!(
                "[settings] UI 输出 changed={} command={:?}",
                self.outcome.changed, self.outcome.command
            );
        }
        // Only "close" makes the window disappear. Every other command (add
        // or remove an item, open a folder, ...) keeps the window in place:
        // after adding an item the user wants to see the bar change.
        if self.outcome.command == Some(Command::Close) {
            self.close_requested = true;
        }
        self.cursor = out.cursor;
        self.cursor_to_system(out.cursor);
        let frame_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.update_stats(frame_ms);
        if super::win::perf() && self.last_stats_push.elapsed() < Duration::from_millis(1) {
            eprintln!(
                "[settings] {:.0} 帧/秒 · {:.2} ms/帧（连续={}）",
                (self.samples.saturating_sub(self.probe_samples) as f64) / 0.4,
                frame_ms,
                out.repaint_delay.is_zero()
            );
            self.probe_samples = self.samples;
        }

        // egui asks for "as soon as possible" while it animates; free-running
        // on that costs a full core and starves the bar, which shares this
        // thread. Pace continuous repaints to the display instead: input still
        // renders immediately, because a message sets the deadline itself.
        self.continuous = out.repaint_delay.is_zero();
        self.next_repaint = if out.repaint_delay == Duration::MAX {
            None
        } else if self.continuous {
            Some(Instant::now() + self.repaint_cap)
        } else {
            Some(Instant::now() + out.repaint_delay)
        };
        Ok(())
    }

    /// Applies the settings window theme (`UiTheme`), which is independent of
    /// the bar theme (`DockTheme`).
    ///
    /// 0 = light, 1 = dark, 2 = follow the system. Called on the first frame
    /// and whenever the value changes.
    ///
    /// Uses the official `set_theme` to switch `ThemePreference` instead of
    /// overwriting the current theme's styles with `set_visuals`: the latter
    /// changes the style but not the preference, so any later `set_theme`
    /// (the screenshot tool uses it too) would load a different default
    /// style and the theme and the visuals would disagree. `set_theme` only
    /// touches one preference field and is cheaper than rebuilding `Visuals`.
    pub(super) fn apply_theme(&mut self, mode: i32) {
        if self.ui_theme == Some(mode) {
            return;
        }
        self.ui_theme = Some(mode);
        let dark = match mode {
            1 => true,
            2 => nooksill_platform::theme::is_dark(),
            _ => false,
        };
        self.renderer.ctx().set_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
    }

    pub(super) fn update_stats(&mut self, dt: f32) {
        self.frame_ms = if self.frame_ms <= 0.0 {
            dt
        } else {
            // Exponential smoothing: a single slow frame should not push the
            // reading to an extreme value.
            self.frame_ms * 0.9 + dt * 0.1
        };
        self.samples += 1;
        if self.last_stats_push.elapsed() >= Duration::from_millis(400) {
            self.last_stats_push = Instant::now();
            self.state.stats = format!(
                "设置窗口 {:.2} ms/帧 · {:.0} fps · {:.1} 万次 Present",
                self.frame_ms,
                1000.0 / self.frame_ms.max(0.01),
                self.samples as f32 / 10000.0
            );
        }
    }

    /// Sets the egui cursor shape on the system cursor.
    ///
    /// The mapping lives in [`crate::cursor`]; keep a single copy of it, a
    /// missing branch is exactly how the cursor bug happened.
    pub(super) fn cursor_to_system(&self, icon: egui::CursorIcon) {
        crate::cursor::apply(icon);
    }
}
