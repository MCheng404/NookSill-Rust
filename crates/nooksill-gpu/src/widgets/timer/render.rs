//! Timer window painting and height fitting.

use super::paint::{draw_panel, Input};
use super::win::physical_size;
use super::*;
use crate::widgets::framework::{self, PanelKind};

impl ToolWindow {
    /// Paints one frame. `now_ms` is the caller's time base.
    pub fn render(&mut self, gpu: &Gpu, now_ms: i64) -> Result<(), GpuError> {
        // A countdown that reached zero stops the timer. Persist that: the
        // ini would otherwise keep the value from before the last run and the
        // next start would resume a finished timer.
        if self.state.tick(now_ms) {
            self.dirty = true;
            self.alarm = Some(Instant::now());
            nooksill_platform::audio::chime();
        }
        let alarm_on = self.alarm.is_some_and(|at| at.elapsed() < ALARM_HOLD);
        if !alarm_on {
            self.alarm = None;
        }
        // The panel height follows the todo count (too much blank space
        // looks bad).
        self.fit_height(gpu)?;

        let raw = self.input.take(self.renderer.size(), self.ppp);
        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let state = &self.state;
        let hover = self.hover;
        let typing = self.typing;
        let typed = self.typed.clone();
        let composing = self.ime.composing().to_owned();
        let frosted = self.renderer.frost_on();
        let mut hits = Hits::default();

        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            hits = draw_panel(
                ui,
                dark,
                frosted,
                state,
                now_ms,
                hover,
                Input {
                    typing,
                    typed: &typed,
                    composing: &composing,
                    alarm: alarm_on,
                },
            );
        })?;
        self.hits = hits;

        // Repaint every 50 ms while running (the stopwatch shows 0.01 s);
        // when stopped nothing schedules a frame.
        self.next_repaint = if self.state.running {
            Instant::now() + TICK
        } else if alarm_on {
            // One repaint to clear the badge; there is no timer after that.
            Instant::now() + Duration::from_millis(200)
        } else {
            Instant::now() + Duration::from_secs(3600)
        };
        Ok(())
    }

    /// Adjusts the window height to the current todo count (and re-centers).
    ///
    /// Only the offscreen target and DIB are rebuilt; the egui context and
    /// the wgpu pipeline survive, which is what `FrameRenderer::resize`
    /// does.
    pub(super) fn fit_height(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        let (pw, ph) = physical_size(self.ppp, panel_h(self.state.todos.len(), self.state.mode));
        if (pw, ph) == (self.size.0, self.size.1) {
            return Ok(());
        }
        self.renderer.resize(gpu, pw as u32, ph as u32)?;
        self.size = (pw, ph);
        let (x, y) = match self.placement {
            Some((anchor, edge)) => {
                framework::place_panel(PanelKind::Tool, (pw, ph), Some(anchor), edge, self.ppp)
            }
            None => nooksill_platform::display::centered_on_target(pw, ph),
        };
        self.pos = (x, y);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                pw,
                ph,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
        }
        // The blur texture is sized to the old window; a resize must recapture.
        framework::refresh_backdrop(&mut self.renderer, self.hwnd, self.ppp);
        Ok(())
    }
}
