//! Timer window messages, clicks and text entry.

use super::win::{client_pos, now_ms};
use super::*;

impl ToolWindow {
    pub(super) fn on_message(
        &mut self,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        // The IME comes first: composing pinyin produces no `WM_CHAR`, it
        // only goes through `WM_IME_*`.
        if matches!(
            msg,
            windows::Win32::UI::WindowsAndMessaging::WM_IME_STARTCOMPOSITION
                | windows::Win32::UI::WindowsAndMessaging::WM_IME_COMPOSITION
                | windows::Win32::UI::WindowsAndMessaging::WM_IME_ENDCOMPOSITION
                | windows::Win32::UI::WindowsAndMessaging::WM_IME_SETCONTEXT
        ) {
            let mut committed = String::new();
            // Only collect characters while typing, otherwise committed text
            // has nowhere to go.
            let handled = self
                .ime
                .handle(self.hwnd, msg, wparam, lparam, &mut committed);
            if self.typing && !committed.is_empty() {
                for c in committed.chars() {
                    if self.typed.chars().count() < 90 {
                        self.typed.push(c);
                    }
                }
            }
            if msg == windows::Win32::UI::WindowsAndMessaging::WM_IME_STARTCOMPOSITION
                && self.typing
            {
                // Place the candidate window near the input row (panel
                // bottom).
                let y = self.size.1 as f32 / self.ppp - 34.0;
                self.ime.set_caret(self.hwnd, 16.0, y, self.ppp);
            }
            self.next_repaint = Instant::now();
            if handled {
                return Some(LRESULT(0));
            }
            return Some(LRESULT(Ime::default_proc(self.hwnd, msg, wparam, lparam)));
        }
        match msg {
            WM_MOUSEMOVE => {
                // `WM_MOUSEMOVE` carries client coords in physical pixels;
                // dividing by `ppp` gives the logical point.
                let (x, y) = client_pos(lparam, self.ppp);
                let p = egui::pos2(x, y);
                if self.hover != Some(p) {
                    self.hover = Some(p);
                    self.next_repaint = Instant::now();
                }
                None
            }
            WM_LBUTTONUP => {
                let (x, y) = client_pos(lparam, self.ppp);
                let p = egui::pos2(x, y);
                if let Some(hit) = self.hits.at(p) {
                    self.click(hit);
                    self.next_repaint = Instant::now();
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => Some(LRESULT(0)),
            WM_SETCURSOR => {
                // Clickable regions use the hand cursor. Layered windows do
                // not get `WM_NCHITTEST`, so the cursor must be set here;
                // the mapping lives in `crate::cursor`.
                crate::cursor::set_hand_if(self.hover.and_then(|p| self.hits.at(p)).is_some());
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

    /// Executes one click.
    pub(super) fn click(&mut self, hit: Hit) {
        let now = now_ms();
        match hit {
            Hit::Mode(mode) => {
                self.state.set_mode(mode);
                self.dirty = true;
            }
            Hit::Action(Action::ToggleRun) => {
                self.state.toggle_run(now);
                self.dirty = true;
            }
            Hit::Action(Action::Reset) => {
                self.state.reset();
                self.dirty = true;
            }
            // Minus and plus one minute: the countdown total was hardcoded to
            // 5 minutes with no way to change it, so the widget only had one
            // usable duration.
            Hit::Action(Action::MinusMinute) => {
                self.state.total_ms = (self.state.total_ms - 60_000).max(60_000);
                self.state.reset();
                self.dirty = true;
            }
            Hit::Action(Action::PlusMinute) => {
                self.state.total_ms = (self.state.total_ms + 60_000).min(99 * 60_000);
                self.state.reset();
                self.dirty = true;
            }
            Hit::Todo(i) => {
                if self.state.toggle_todo(i) {
                    self.dirty = true;
                }
            }
            Hit::Delete(i) => {
                if self.state.remove_todo(i) {
                    self.dirty = true;
                }
            }
            Hit::Add => {
                // Todo text entry: enter the typing state, accumulate
                // characters from the keyboard messages and submit on Enter.
                // `WM_CHAR` delivers committed characters, but candidates
                // still being composed are not handled.
                if self.state.todos.len() >= MAX_TODOS {
                    return;
                }
                self.typing = true;
                self.typed.clear();
            }
        }
    }

    /// Keyboard input, used while typing a todo.
    pub(super) fn on_char(&mut self, ch: u32) -> bool {
        if !self.typing {
            return false;
        }
        match char::from_u32(ch) {
            Some('\r') | Some('\n') => {
                if !self.typed.trim().is_empty() {
                    self.state.add_todo(&self.typed.clone());
                    self.dirty = true;
                }
                self.typed.clear();
                self.typing = false;
            }
            Some('\u{1b}') => {
                self.typed.clear();
                self.typing = false;
            }
            Some('\u{8}') => {
                self.typed.pop();
            }
            // Hard limit of 90 characters: `Todo::new` truncates at 96, but
            // blocking here avoids a silent cut after the user typed a lot.
            Some(c) if !c.is_control() && self.typed.chars().count() < 90 => {
                self.typed.push(c);
            }
            _ => {}
        }
        true
    }
}
