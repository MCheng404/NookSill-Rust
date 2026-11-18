//! Global hotkey registration with runtime rebinding.
//!
//! One hotkey per window. Registration can fail when another process owns the
//! combination, so a failed rebind rolls back to the previous one.

use nooksill_core::hotkey::Hotkey;
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};

/// Stable ids for the two global hotkeys; `WM_HOTKEY` carries this id in wparam.
pub const HOTKEY_TOGGLE_ID: i32 = 1;
pub const HOTKEY_LAUNCHPAD_ID: i32 = 2;

/// Combination actually registered for Launchpad, for the settings page.
static LAUNCHPAD_ACTIVE: OnceLock<Mutex<Option<Hotkey>>> = OnceLock::new();

/// Publish the combination that really registered (after any fallback).
pub fn set_launchpad_active(hk: Hotkey) {
    let cell = LAUNCHPAD_ACTIVE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cell.lock() {
        *guard = Some(hk);
    }
}

/// The combination in effect for the Launchpad hotkey, if registered.
#[must_use]
pub fn launchpad_active() -> Option<Hotkey> {
    LAUNCHPAD_ACTIVE
        .get()
        .and_then(|c| c.lock().ok())
        .and_then(|g| *g)
}

/// Owns the registered combination; `Drop` unregisters it.
#[derive(Debug)]
pub struct GlobalHotkey {
    hwnd: HWND,
    id: i32,
    current: Option<Hotkey>,
}

impl GlobalHotkey {
    /// Register `combo` when `enabled`; a failure is reported but not fatal.
    #[must_use]
    pub fn register(hwnd: HWND, combo: Hotkey, enabled: bool) -> Self {
        Self::register_id(hwnd, HOTKEY_TOGGLE_ID, combo, enabled)
    }

    /// Register with an explicit id, used for the second (Launchpad) hotkey.
    #[must_use]
    pub fn register_id(hwnd: HWND, id: i32, combo: Hotkey, enabled: bool) -> Self {
        let mut this = Self {
            hwnd,
            id,
            current: None,
        };
        if enabled && !this.try_register(combo) {
            eprintln!("global hotkey {} could not be registered", combo.display());
        }
        this
    }

    /// The combination currently registered, if any.
    #[must_use]
    pub const fn active(&self) -> Option<Hotkey> {
        self.current
    }

    /// Whether no combination is currently registered.
    #[must_use]
    pub const fn is_unregistered(&self) -> bool {
        self.current.is_none()
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
        let id = self.id;
        let hwnd = self.hwnd;
        let ok = unsafe { RegisterHotKey(Some(hwnd), id, HOT_KEY_MODIFIERS(hk.modifiers), hk.vk) }
            .is_ok();
        if ok {
            self.current = Some(hk);
        }
        ok
    }

    fn unregister(&mut self) {
        if self.current.take().is_some() {
            let _ = unsafe { UnregisterHotKey(Some(self.hwnd), self.id) };
        }
    }
}

impl Drop for GlobalHotkey {
    fn drop(&mut self) {
        self.unregister();
    }
}
