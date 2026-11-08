//! IME support for a window.
//!
//! `WM_CHAR` only delivers text on commit, so composition is read from
//! `GCS_COMPSTR` / `GCS_RESULTSTR` and the candidate window is anchored to the
//! caret; the system still draws the candidate list itself.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::Ime::{
    ImmAssociateContextEx, ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext,
    ImmSetCompositionWindow, CFS_POINT, COMPOSITIONFORM, GCS_COMPSTR, GCS_RESULTSTR, HIMC,
    IACE_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT,
    WM_IME_STARTCOMPOSITION,
};

/// IME state for one window.
///
/// Only the in-progress composition is cached; everything else is queried
/// on demand.
#[derive(Debug, Default)]
pub struct Ime {
    /// Text being composed but not yet committed (pinyin or candidates).
    composing: String,
}

impl Ime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Call once after window creation: make sure it has an IME context.
    ///
    /// `IACE_DEFAULT` restores the default context; a no-op for most windows, but
    /// the only way to recover one a host process replaced.
    pub fn enable(hwnd: HWND) {
        unsafe {
            // Null HIMC plus `IACE_DEFAULT` restores the system default context.
            let _ = ImmAssociateContextEx(hwnd, HIMC(std::ptr::null_mut()), IACE_DEFAULT);
        }
    }

    /// Text still being composed; the UI draws it inline in the field.
    #[must_use]
    pub fn composing(&self) -> &str {
        &self.composing
    }

    #[must_use]
    pub fn is_composing(&self) -> bool {
        !self.composing.is_empty()
    }

    /// Clear the composition, e.g. on focus loss or Esc.
    pub fn cancel(&mut self) {
        self.composing.clear();
    }

    /// Handle a `WM_IME_*` message.
    ///
    /// Returns `true` when the message is fully handled and the caller must not
    /// call `DefWindowProc`. Committed text is appended to `committed`.
    pub fn handle(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        committed: &mut String,
    ) -> bool {
        match msg {
            WM_IME_STARTCOMPOSITION => {
                self.composing.clear();
                true
            }
            WM_IME_COMPOSITION => {
                // lparam is a bitmask: one frame can carry both RESULTSTR and
                // COMPSTR.
                let flags = lparam.0 as u32;
                if flags & GCS_RESULTSTR.0 != 0 {
                    if let Some(s) = composition_string(hwnd, GCS_RESULTSTR) {
                        committed.push_str(&s);
                    }
                    self.composing.clear();
                }
                if flags & GCS_COMPSTR.0 != 0 {
                    self.composing = composition_string(hwnd, GCS_COMPSTR).unwrap_or_default();
                }
                true
            }
            WM_IME_ENDCOMPOSITION => {
                self.composing.clear();
                true
            }
            WM_IME_SETCONTEXT => {
                // Let the system draw the candidate window. `wparam == 0` means
                // focus lost; clear the composition or a half-typed string
                // stays visible forever.
                if wparam.0 == 0 {
                    self.composing.clear();
                }
                false
            }
            _ => false,
        }
    }

    /// Anchor the candidate window at the caret.
    ///
    /// `x` / `y` are client coordinates in logical points; `ppp` converts them
    /// because `ImmSetCompositionWindow` expects physical pixels.
    pub fn set_caret(&self, hwnd: HWND, x: f32, y: f32, ppp: f32) {
        let himc = unsafe { ImmGetContext(hwnd) };
        if himc.0.is_null() {
            return;
        }
        let form = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT {
                x: (x * ppp).round() as i32,
                y: (y * ppp).round() as i32,
            },
            rcArea: RECT::default(),
        };
        unsafe {
            let _ = ImmSetCompositionWindow(himc, &raw const form);
            let _ = ImmReleaseContext(hwnd, himc);
        }
    }

    /// Hand an unhandled message back to the system.
    ///
    /// Extracted because `WM_IME_SETCONTEXT` must reach `DefWindowProc`, otherwise
    /// no candidate window appears.
    pub fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }.0
    }
}

/// Read one composition string.
///
/// `ImmGetCompositionStringW` uses a two-call protocol (ask length, then content)
/// and returns a byte count, so a UTF-16 length is half of it.
fn composition_string(
    hwnd: HWND,
    kind: windows::Win32::UI::Input::Ime::IME_COMPOSITION_STRING,
) -> Option<String> {
    let himc = unsafe { ImmGetContext(hwnd) };
    if himc.0.is_null() {
        return None;
    }
    let result = (|| {
        let bytes = unsafe { ImmGetCompositionStringW(himc, kind, None, 0) };
        if bytes <= 0 {
            return None;
        }
        let mut buf = vec![0_u16; bytes as usize / 2 + 1];
        let got = unsafe {
            ImmGetCompositionStringW(
                himc,
                kind,
                Some(buf.as_mut_ptr().cast()),
                (buf.len() * 2) as u32,
            )
        };
        if got <= 0 {
            return None;
        }
        buf.truncate(got as usize / 2);
        Some(String::from_utf16_lossy(&buf))
    })();
    unsafe {
        let _ = ImmReleaseContext(hwnd, himc);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let ime = Ime::new();
        assert!(!ime.is_composing());
        assert_eq!(ime.composing(), "");
    }

    /// Start and end drive the state machine; neither touches HIMC, so a null
    /// window is enough to test them.
    #[test]
    fn start_and_end_composition_are_handled() {
        let mut ime = Ime::new();
        let mut committed = String::new();
        let null = HWND(std::ptr::null_mut());
        assert!(ime.handle(
            null,
            WM_IME_STARTCOMPOSITION,
            WPARAM(0),
            LPARAM(0),
            &mut committed
        ));
        assert!(!ime.is_composing());
        assert!(ime.handle(
            null,
            WM_IME_ENDCOMPOSITION,
            WPARAM(0),
            LPARAM(0),
            &mut committed
        ));
        assert!(!ime.is_composing());
        assert!(committed.is_empty(), "组合消息不该产生上屏文字");
    }

    /// `WM_IME_SETCONTEXT` must pass through to `DefWindowProc`, but blur still
    /// has to clear the composition.
    #[test]
    fn setcontext_is_passed_through_and_clears_on_blur() {
        let mut ime = Ime::new();
        ime.composing = "nihao".to_owned();
        let mut committed = String::new();
        let null = HWND(std::ptr::null_mut());
        let handled = ime.handle(
            null,
            WM_IME_SETCONTEXT,
            WPARAM(0),
            LPARAM(0),
            &mut committed,
        );
        assert!(!handled, "要交回 DefWindowProc");
        assert!(!ime.is_composing(), "失焦应当清掉组合串");
    }

    #[test]
    fn cancel_clears_composition() {
        let mut ime = Ime::new();
        ime.composing = "nihao".to_owned();
        assert!(ime.is_composing());
        ime.cancel();
        assert!(!ime.is_composing());
    }
}
