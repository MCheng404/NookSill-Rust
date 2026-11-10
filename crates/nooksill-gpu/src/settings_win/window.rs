//! Settings window lifecycle: creation, scaling, show/hide and frame timing.

use super::win::{centered_origin, class_pcwstr, physical_size, wndproc};
use super::*;

impl SettingsWindow {
    /// Creates the window (hidden by default) and its renderer.
    ///
    /// Returns `Pin<Box<Self>>` instead of a bare value: the window procedure
    /// looks the state up through `GWLP_USERDATA`, so the address must stay
    /// stable. Moving the value afterwards leaves the procedure with a stale
    /// pointer.
    ///
    /// The first construction is slow: installing the CJK fonts plus compiling
    /// the wgpu pipelines.
    pub fn new(gpu: &Gpu, state: SettingsState, ppp: f32) -> Result<Pin<Box<Self>>, GpuError> {
        let hinst =
            unsafe { GetModuleHandleW(None) }.map_err(|_| GpuError::Win32("GetModuleHandleW"))?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            lpszClassName: class_pcwstr(),
            hCursor: crate::cursor::class_cursor(),
            hIcon: crate::app_icon().unwrap_or_default(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&raw const wc);
        }

        let (pw, ph) = physical_size(ppp);
        let (x, y) = centered_origin(pw, ph);
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("NookSill Settings"));
        let hwnd = unsafe {
            CreateWindowExW(
                // `WS_EX_LAYERED` is required by `UpdateLayeredWindow`,
                // `WS_EX_APPWINDOW` puts the window in the taskbar, and
                // `WS_EX_TOPMOST` keeps it above the Dock.
                WS_EX_LAYERED | WS_EX_APPWINDOW | WS_EX_TOPMOST,
                class_pcwstr(),
                windows::core::PCWSTR(title.as_ptr()),
                WS_POPUP,
                x,
                y,
                pw,
                ph,
                None,
                None,
                Some(windows::Win32::Foundation::HINSTANCE(hinst.0)),
                None,
            )
        }
        .map_err(|_| GpuError::Win32("CreateWindowExW（设置窗口）"))?;

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: Input::default(),
            pos: (x, y),
            ppp,
            state,
            outcome: Outcome::default(),
            cursor: egui::CursorIcon::Default,
            next_repaint: None,
            close_requested: false,
            visible: false,
            ui_theme: None,
            frame_ms: 0.0,
            samples: 0,
            last_stats_push: Instant::now(),
            ime: crate::ime::Ime::new(),
        });
        // Attach at the final address; `Pin<Box<..>>` guarantees it stays put.
        win.attach();
        // Without an IME context the candidate window never appears next to
        // the input fields.
        crate::ime::Ime::enable(hwnd);
        Ok(win)
    }

    #[must_use]
    pub const fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Stores the state pointer in `GWLP_USERDATA` so the window procedure
    /// can find it.
    ///
    /// Only `new` may call this, on the final address of the `Pin<Box<Self>>`;
    /// moving the value afterwards invalidates the stored pointer.
    pub(super) fn attach(&mut self) {
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                std::ptr::from_mut::<Self>(self) as isize,
            );
        }
    }

    /// The scale currently in effect.
    ///
    /// The main loop compares it with the target derived from the config and
    /// rescales when they differ.
    #[must_use]
    pub fn ppp(&self) -> f32 {
        self.ppp
    }

    /// Rebuilds the window in place at a new scale (custom DPI change).
    ///
    /// The renderer is replaced entirely: offscreen buffer, DIB and egui
    /// context are rebuilt for the new ppp (the font atlas is uploaded again,
    /// tens of milliseconds, acceptable for a settings change). The window
    /// rect is resized and re-centered; the logical size stays the same.
    pub fn rescale(&mut self, gpu: &Gpu, ppp: f32) -> Result<(), GpuError> {
        if (self.ppp - ppp).abs() < f32::EPSILON {
            return Ok(());
        }
        let (pw, ph) = physical_size(ppp);
        // The new renderer has no background layer and no capture thread yet,
        // so read the spec out first.
        let frost = self.renderer.frost_spec();
        self.renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        self.ppp = ppp;
        // A new renderer means a new egui context (dark visuals by default).
        // `ui_theme` still holds the old theme marker, so `apply_theme` would
        // think it already ran and skip: light users would jump back to dark
        // on any scale change. Reset it to force the next frame to reapply.
        self.ui_theme = None;
        let (x, y) = centered_origin(pw, ph);
        self.pos = (x, y);
        unsafe { SetWindowPos(self.hwnd, None, x, y, pw, ph, SWP_NOACTIVATE | SWP_NOZORDER) }
            .map_err(|_| GpuError::Win32("SetWindowPos（设置窗口缩放）"))?;
        // Capture after the move, so the backdrop is the area behind the new
        // rect. The window is already excluded from capture (see `show`), so
        // this cannot pick up itself.
        if let Some(spec) = frost {
            spec.apply(&mut self.renderer, self.hwnd, self.ppp);
            self.renderer.start_live_frost(self.hwnd, spec, self.ppp);
        }
        Ok(())
    }

    /// Re-apply the localized title after the UI language changes.
    pub fn refresh_title(&self) {
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("NookSill Settings"));
        // SAFETY: the HSTRING outlives the call, and Win32 copies the text.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                self.hwnd,
                windows::core::PCWSTR(title.as_ptr()),
            );
        }
    }

    /// Shows and activates the window.
    ///
    /// When already visible it is only brought to the front: repeated
    /// `SetForegroundWindow` calls are allowed within one process, so there is
    /// no foreground lock problem.
    pub fn show(&mut self, frost: Option<crate::frost::Spec>) {
        match frost {
            Some(spec) => {
                // Capture the backdrop before `ShowWindow`: the window is not
                // on screen yet, so it cannot recurse into itself.
                spec.apply(&mut self.renderer, self.hwnd, self.ppp);
                // The capture thread then keeps updating the backdrop with
                // whatever is behind the window, which requires the window to
                // stay excluded from screen capture.
                crate::frost::exclude_from_capture(self.hwnd);
                self.renderer.start_live_frost(self.hwnd, spec, self.ppp);
            }
            None => {
                self.renderer.clear_frost();
                self.renderer.stop_live_frost();
            }
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            if GetForegroundWindow() != self.hwnd {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
        self.visible = true;
        self.next_repaint = Some(Instant::now());
    }

    pub fn hide(&mut self) {
        // The hidden entry point only lives for this session: closing the
        // window takes back the advanced card, so the next open needs the
        // five version clicks again.
        self.state.advanced = false;
        self.state.version_clicks = 0;
        self.renderer.stop_live_frost();
        crate::frost::allow_capture(self.hwnd);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.visible = false;
        self.close_requested = false;
        // A hidden window needs no repaints; asking every 16 ms would be
        // pure waste.
        self.next_repaint = None;
    }

    /// Wakes the window to draw one frame after an external thread changed
    /// `state`.
    pub fn wake(&mut self) {
        if self.visible {
            self.next_repaint = Some(Instant::now());
        }
    }

    /// Whether the next frame is due.
    #[must_use]
    pub fn due(&self) -> bool {
        self.visible && self.next_repaint.is_some_and(|t| t <= Instant::now())
    }

    /// Whether egui has a pending repaint deadline (animation, slider drag,
    /// hover transition, ...).
    ///
    /// A pending deadline means the window is producing frames continuously:
    /// the 1 ms timeout of `MsgWaitForMultipleObjectsEx` is swallowed by the
    /// ~15.6 ms system timer granularity, which would pin the settings window
    /// at ~64 Hz. The main loop raises the precision to 1 ms while this holds
    /// and restores it once idle.
    ///
    /// Visibility alone is not a usable signal: a visible but still window
    /// must keep sleeping for a full second, because a permanent 1 ms
    /// precision raises the whole machine's standby power draw.
    #[must_use]
    pub fn has_pending_repaint(&self) -> bool {
        self.visible && self.next_repaint.is_some()
    }

    /// Time until the next repaint. `None` = hidden, `MAX` = idle.
    #[must_use]
    pub fn repaint_in(&self) -> Option<Duration> {
        if !self.visible {
            return None;
        }
        Some(self.next_repaint.map_or(Duration::MAX, |t| {
            t.saturating_duration_since(Instant::now())
        }))
    }
}
