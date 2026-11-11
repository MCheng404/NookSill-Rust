//! Global hotkey registration with runtime rebinding.
//!
//! One hotkey per window. Registration can fail when another process owns the
//! combination, so a failed rebind rolls back to the previous one.

use nooksill_core::hotkey::Hotkey;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};

/// Stable id for the only global hotkey; `WM_HOTKEY` ignores wparam today.
const HOTKEY_ID: i32 = 1;

/// Owns the registered combination; `Drop` unregisters it.
#[derive(Debug)]
pub struct GlobalHotkey {
    hwnd: HWND,
    current: Option<Hotkey>,
}

impl GlobalHotkey {
    /// Register `combo` when `enabled`; a failure is reported but not fatal.
    #[must_use]
    pub fn register(hwnd: HWND, combo: Hotkey, enabled: bool) -> Self {
        let mut this = Self {
            hwnd,
            current: None,
        };
        if enabled && !this.try_register(combo) {
            eprintln!("global hotkey {} could not be registered", combo.display());
        }
        this
    }

    /// Apply a new combination, or disable the hotkey. Returns false when the
    /// new combination was taken and the old one was restored.
    pub fn rebind(&mut self, combo: Hotkey, enabled: bool) -> bool {
        let want = enabled.then_some(combo);
        if want == self.current {
            return true;
        }
        let previous = self.current;
        self.unregister();
        if let Some(hk) = want {
            if self.try_register(hk) {
                return true;
            }
            if let Some(prev) = previous {
                if self.try_register(prev) {
                    eprintln!(
                        "global hotkey {} is taken; kept {}",
                        hk.display(),
                        prev.display()
                    );
                    return false;
                }
            }
            return false;
        }
        true
    }

    fn try_register(&mut self, hk: Hotkey) -> bool {
        let ok = unsafe {
            RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_ID,
                HOT_KEY_MODIFIERS(hk.modifiers),
                hk.vk,
            )
        }
        .is_ok();
        if ok {
            self.current = Some(hk);
        }
        ok
    }

    fn unregister(&mut self) {
        if self.current.take().is_some() {
            let _ = unsafe { UnregisterHotKey(Some(self.hwnd), HOTKEY_ID) };
        }
    }
}

impl Drop for GlobalHotkey {
    fn drop(&mut self) {
        self.unregister();
    }
}
