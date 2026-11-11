//! Timer window painting and height fitting.

use super::paint::{draw_panel, Input};
use super::win::physical_size;
use super::*;

impl ToolWindow {
    /// Paints one frame. `now_ms` is the caller's time base.
    pub fn render(&mut self, gpu: &Gpu, now_ms: i64) -> Result<(), GpuError> {
        // A countdown that reached zero stops the timer.
        self.state.tick(now_ms);
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
                },
            );
        })?;
        self.hits = hits;

        // Repaint every 50 ms while running (the stopwatch shows 0.01 s);
        // when stopped nothing schedules a frame.
        self.next_repaint = if self.state.running {
            Instant::now() + TICK
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
        let (pw, ph) = physical_size(self.ppp, panel_h(self.state.todos.len()));
        if (pw, ph) == (self.size.0, self.size.1) {
            return Ok(());
        }
        self.renderer.resize(gpu, pw as u32, ph as u32)?;
        self.size = (pw, ph);
        let (x, y) = nooksill_platform::display::centered_on_target(pw, ph);
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
        Ok(())
    }
}
