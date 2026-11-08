//! Calendar painting, clicks and message translation.

use super::paint::draw_panel;
use super::win::client_pos;
use super::*;

impl CalendarWindow {
    /// Paints one frame.
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let state = self.state.clone();
        let today = self.today;
        let hover = self.hover;
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        let mut hits = Hits::default();
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            hits = draw_panel(ui, dark, frosted, &state, today, hover);
        })?;
        self.hits = hits;
        // Push the alarm out after painting. The panel has no animation timer
        // and only wakes on events; without this `due()` stays true while
        // visible and the main loop would submit at 60 fps forever.
        self.next_repaint = Instant::now() + Duration::from_secs(3600);
        Ok(())
    }

    pub(super) fn click(&mut self, hit: Hit) {
        match hit {
            Hit::Nav(0) => self.state.step(-1, 0),
            Hit::Nav(1) => self.state.step(1, 0),
            Hit::Nav(2) => self.state.step(0, -1),
            Hit::Nav(3) => self.state.step(0, 1),
            Hit::Nav(_) => self.state.go_today(self.today),
            Hit::Cell(k) => {
                let c = &self.state.cells()[k];
                let (y, m, d) = (c.year, c.month, c.day);
                self.state.select(y, m, d);
            }
        }
        self.next_repaint = Instant::now();
    }

    pub(super) fn on_message(
        &mut self,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg {
            WM_MOUSEMOVE => {
                let (x, y) = client_pos(lparam, self.ppp);
                let p = egui::pos2(x, y);
                if self.hover != self.hits.at(p) {
                    self.hover = self.hits.at(p);
                    self.next_repaint = Instant::now();
                }
                crate::cursor::set_hand_if(self.hover.is_some());
                None
            }
            WM_LBUTTONDOWN => Some(LRESULT(0)),
            WM_LBUTTONUP => {
                let (x, y) = client_pos(lparam, self.ppp);
                if let Some(hit) = self.hits.at(egui::pos2(x, y)) {
                    self.click(hit);
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                crate::cursor::set_hand_if(self.hover.is_some());
                Some(LRESULT(1))
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let _ = BeginPaint(self.hwnd, &mut ps);
                    let _ = EndPaint(self.hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            _ => None,
        }
    }
}
