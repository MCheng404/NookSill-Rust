//! Relaunch when a graphics setting changes.
//!
//! `FrameSlot` is chosen and the adapter is bound once in `boot`, so neither
//! `GpuAccel` nor `Adapter` can take effect live. Leaving them as "applies on
//! next start" made the toggle look broken; the dock now quits and a waiter
//! relaunches it once the single-instance mutex is free.

use nooksill_core::config::Key;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;

impl super::App {
    /// Compare the graphics pair against the snapshot and relaunch on a change.
    ///
    /// Runs every round, so an ini edited outside the settings window is caught
    /// as well as a click in the UI.
    pub(super) fn step_gfx_restart(&mut self) {
        let accel = self.dock.cfg.get_i32(Key::GPU_ACCEL) != 0;
        // Compare before cloning: this runs on every round and the common case
        // is no change.
        if accel == self.gfx_key.0 && self.dock.cfg.adapter == self.gfx_key.1 {
            return;
        }
        self.gfx_key = (accel, self.dock.cfg.adapter.clone());
        // The new process reads the ini, so the change must reach disk. The
        // shutdown path flushes pending writes before the waiter launches it.
        self.touch_cfg();
        self.pending_restart = true;
        // SAFETY: `PostQuitMessage` only queues WM_QUIT on this thread.
        unsafe { PostQuitMessage(0) };
    }
}
