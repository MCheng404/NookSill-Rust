//! Settings window message translation.

use super::win::{client_pos, current_modifiers, trace};
use super::*;

impl SettingsWindow {
    pub(super) fn on_message(
        &mut self,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        if trace() {
            eprintln!("[settings-proc] msg=0x{msg:04X} wparam=0x{:X}", wparam.0);
        }
        match msg {
            // The live background thread has a new frame: ask for a repaint.
            // Without it the window sleeps while nobody interacts and the
            // blur layer stays on the image from the last interaction.
            WM_APP_WAKE => {
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_MOUSEMOVE => {
                let (x, y) = client_pos(lparam, self.ppp);
                let first = !self.input.hovering();
                self.input.pointer_moved(x, y);
                if first {
                    // Without TME_LEAVE the window never receives
                    // WM_MOUSELEAVE and the hover state sticks.
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    };
                }
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_MOUSELEAVE => {
                self.input.pointer_gone();
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                self.input.pointer_button(
                    true,
                    if msg == WM_LBUTTONDOWN {
                        egui::PointerButton::Primary
                    } else {
                        egui::PointerButton::Secondary
                    },
                );
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_LBUTTONUP | WM_RBUTTONUP => {
                self.input.pointer_button(
                    false,
                    if msg == WM_LBUTTONUP {
                        egui::PointerButton::Primary
                    } else {
                        egui::PointerButton::Secondary
                    },
                );
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) as u16 as i16) as f32 / 120.0;
                self.input.wheel(delta);
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_KEYDOWN | WM_KEYUP => {
                self.input.set_modifiers(current_modifiers());
                let vk = wparam.0 as u32;
                if let Some(key) = map_vk(vk) {
                    self.input.key(key, msg == WM_KEYDOWN);
                    if key == egui::Key::Escape && msg == WM_KEYDOWN {
                        self.close_requested = true;
                    }
                }
                self.next_repaint = Some(Instant::now());
                None
            }
            // Text input. egui fields only accept `Event::Text`, so this
            // message is the only source of typed characters.
            WM_CHAR => {
                let ch = wparam.0 as u32;
                if let Some(c) = char::from_u32(ch) {
                    if !c.is_control() {
                        self.input.text(&c.to_string());
                        self.next_repaint = Some(Instant::now());
                    }
                }
                Some(LRESULT(0))
            }
            // Chinese IME: committed text arrives as `ImeEvent::Commit`, the
            // in-progress pinyin as `ImeEvent::Preedit`, which egui draws
            // underlined inside the field.
            WM_IME_COMPOSITION
            | WM_IME_STARTCOMPOSITION
            | WM_IME_ENDCOMPOSITION
            | WM_IME_SETCONTEXT => {
                // Anchor the candidate list at the focused field as the
                // composition starts; the value is published while painting.
                if msg == windows::Win32::UI::WindowsAndMessaging::WM_IME_STARTCOMPOSITION {
                    if let Some(caret) =
                        nooksill_settings::text_field::ime_caret(self.renderer.ctx())
                    {
                        self.ime.set_caret(self.hwnd, caret.x, caret.y, self.ppp);
                    }
                }
                let mut committed = String::new();
                let before = self.ime.composing().to_owned();
                let handled = self
                    .ime
                    .handle(self.hwnd, msg, wparam, lparam, &mut committed);
                // Commit before Preedit: one message can both commit and
                // start the next composition, and the reverse order would
                // delete the new composition as a stale preedit.
                if !committed.is_empty() {
                    self.input.ime_commit(&committed);
                }
                let after = self.ime.composing().to_owned();
                if after != before {
                    self.input.ime_preedit(&after);
                }
                if !committed.is_empty() || after != before {
                    // No "skip the next WM_CHAR" flag: `ime.handle` returns
                    // true for `WM_IME_COMPOSITION`, so the message is eaten
                    // here (`Some(LRESULT(0))`) and DefWindowProc will not
                    // send a WM_CHAR. A cross-message flag would instead
                    // swallow the first character of the next normal key.
                    self.next_repaint = Some(Instant::now());
                }
                if handled {
                    Some(LRESULT(0))
                } else {
                    None
                }
            }
            WM_SETCURSOR => {
                // TRUE means "the app manages the cursor", otherwise the class
                // cursor overrides it. Reuse egui's verdict from the last
                // frame instead of always using an arrow: a text field needs
                // an I-beam.
                self.cursor_to_system(self.cursor);
                Some(LRESULT(1))
            }
            WM_PAINT => {
                // The layered-window compositor draws all the content;
                // BeginPaint/EndPaint only tell the system the region has
                // been painted so it stops sending WM_PAINT.
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
