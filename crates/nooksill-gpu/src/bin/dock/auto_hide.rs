//! Fullscreen and auto-hide ticks, one step per main-loop pass.

use super::misc::{cursor_near, FS_CHECK_MS};
use super::*;

impl App {
    /// Dock strip rect in screen coordinates (icon bar + widget band + shadow
    /// padding).
    ///
    /// Occlusion must cover the whole strip: a window covering only the clock
    /// would otherwise not count, and smart hide would leave the two overlapped.
    pub(super) fn bar_screen_rect(&self) -> LRect {
        let (x, y, w, h) = self.dock.hot_phys();
        LRect::new(
            self.pos.0 + x.round() as i32,
            self.pos.1 + y.round() as i32,
            self.pos.0 + (x + w).round() as i32,
            self.pos.1 + (y + h).round() as i32,
        )
    }

    /// Millisecond timestamp since process start; the shared time base.
    pub(super) fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(0)
    }

    /// Hide the dock entirely while an immersive fullscreen app has focus.
    ///
    /// Once per second is enough: the check is two read-only queries and
    /// triggers no rendering.
    pub(super) fn step_fullscreen(&mut self) -> bool {
        // Skip right after a launch; see `occlusion::LAUNCH_GRACE_MS`.
        if occlusion::in_launch_grace(self.last_launch_ms, self.now_ms()) {
            return false;
        }
        if self.last_fs_check.elapsed() < Duration::from_millis(u64::from(FS_CHECK_MS)) {
            return false;
        }
        self.last_fs_check = Instant::now();
        // Foreground state shares this throttle window, so no extra system
        // enumerations. Maximized windows hug the screen edge where the cursor
        // sweeps the hot zone most, hence the longer delay tier.
        self.fg_maximized = occlusion::maximized_foreground();
        let fs = occlusion::is_fullscreen_foreground();
        if trace::fs() {
            eprintln!("[fs] {} -> {}", occlusion::fullscreen_trace(), fs);
        }
        if fs == self.fs_hidden {
            return false;
        }
        self.fs_hidden = fs;
        unsafe {
            let _ = ShowWindow(self.hwnd, if fs { SW_HIDE } else { SW_SHOWNA });
        }
        if trace::geom() {
            eprintln!(
                "[fullscreen] {}",
                if fs {
                    nooksill_core::i18n::t("Hide Dock")
                } else {
                    nooksill_core::i18n::t("Restore Dock")
                }
            );
        }
        // Paint at once when showing again, otherwise the window stays blank
        // until the next event.
        !fs
    }

    /// Auto-hide and smart hide.
    ///
    /// The cursor must be polled: once collapsed, the content sits outside the
    /// visible area with alpha 0, so mouse messages pass through and no
    /// `WM_MOUSEMOVE` ever arrives.
    pub(super) fn step_auto_hide(&mut self) -> bool {
        if !self.dock.needs_hide_tick() {
            return false;
        }
        let now_ms = self.now_ms();
        let near = cursor_near(&self.dock, self.hwnd);
        let busy = self.dock_busy();
        // Only smart hide needs enumeration, and `EnumWindows` over every
        // top-level window is too costly to run on the UI thread each 100 ms.
        // The verdict comes from the scanner thread; an injected value wins so
        // the uncover branch can be tested without minimizing real windows.
        let covered =
            self.dock.mode == DockMode::SmartHide && self.fake_covered.unwrap_or(self.covered);
        // Two delay tiers by scenario; config changes apply on the next tick.
        // `get_i32` already clamps to the NUMERIC_KEYS range; the extra max(0)
        // keeps a bad value from becoming a huge `u64`.
        let (hide_key, show_key) = if self.fg_maximized {
            (Key::HIDE_DELAY_MAX, Key::SHOW_DELAY_MAX)
        } else {
            (Key::HIDE_DELAY, Key::SHOW_DELAY)
        };
        self.dock.set_hide_delays(
            self.dock.cfg.get_i32(hide_key).max(0) as u64,
            self.dock.cfg.get_i32(show_key).max(0) as u64,
        );
        let before = self.dock.hide_target;
        self.dock.tick_auto_hide(near.hit, busy, covered, now_ms);
        // Log the near verdict only when it flips: that is what explains a
        // missing expand or collapse.
        if trace::geom() && self.last_near != Some(near.hit) {
            self.last_near = Some(near.hit);
            eprintln!(
                "[near] {} cursor=({},{}) hot=({:.0},{:.0},{:.0}x{:.0}) busy={busy} covered={covered}",
                near.hit, near.cursor.0, near.cursor.1, near.hot.0, near.hot.1, near.hot.2, near.hot.3
            );
        }
        if before == self.dock.hide_target {
            return false;
        }
        if trace::geom() {
            // Name the covering window in smart-hide mode; a bare bool would
            // leave the cause unknown.
            let by = if self.dock.mode == DockMode::SmartHide {
                occlusion::covering_window(self.bar_screen_rect(), &[self.hwnd])
                    .1
                    .map_or_else(String::new, |t| format!(" by {t:?}"))
            } else {
                String::new()
            };
            eprintln!(
                "[hide] target={} near={} busy={busy} covered={covered}{by} cur={:.3} \
                 cursor=({},{}) hot=({:.0},{:.0},{:.0}x{:.0})",
                self.dock.hide_target,
                near.hit,
                self.dock.hide_cur,
                near.cursor.0,
                near.cursor.1,
                near.hot.0,
                near.hot.1,
                near.hot.2,
                near.hot.3
            );
        }
        true
    }
}
