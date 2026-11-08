//! Group panel message translation.

use super::win::client_pos;
use super::*;

impl GroupWindow {
    pub(super) fn on_message(
        &mut self,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg {
            // The icon thread handed back a result.
            //
            // Icons arrive asynchronously; without this the panel would stay
            // on the placeholder until the user moves the mouse.
            WM_APP_WAKE => {
                self.next_repaint = Some(Instant::now());
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                crate::cursor::set(crate::cursor::arrow());
                Some(LRESULT(1))
            }
            WM_MOUSEMOVE => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.input.pointer_moved(x, y);
                let h = self.hit_at(x, y);
                if h != self.hover {
                    self.hover = h;
                    self.next_repaint = Some(Instant::now());
                }
                None
            }
            WM_LBUTTONDOWN => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.input.pointer_moved(x, y);
                self.press = self.hit_at(x, y);
                self.input
                    .pointer_button(true, egui::PointerButton::Primary);
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_LBUTTONUP => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.input.pointer_moved(x, y);
                self.input
                    .pointer_button(false, egui::PointerButton::Primary);
                let released = self.hit_at(x, y);
                // Whether the press landed on the x or on the icon is
                // decided by `paint`; here only the pressed child is
                // recorded, and the release must hit the same cell.
                if self.press == released {
                    self.released = released;
                }
                self.press = None;
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_RBUTTONDOWN | WM_RBUTTONUP => None,
            WM_KEYDOWN => {
                // Esc hides the panel; it closes even without the mouse
                // capture, which is the only way out when `PanelDismiss` is
                // off.
                if wparam.0 == 0x1B {
                    self.hide();
                }
                Some(LRESULT(0))
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
