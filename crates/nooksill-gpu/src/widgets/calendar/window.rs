//! Calendar window lifecycle: creation, placement and dates.

use super::win::{class_pcwstr, physical_size, place, wndproc};
use super::*;

impl CalendarWindow {
    /// Creates the window (hidden). Use [`Self::show_at`] to place it by the
    /// clock card afterwards.
    pub fn new(
        gpu: &Gpu,
        ppp: f32,
        dark: bool,
        today: (i32, u32, u32),
    ) -> Result<Pin<Box<Self>>, GpuError> {
        let hinst =
            unsafe { GetModuleHandleW(None) }.map_err(|_| GpuError::Win32("GetModuleHandleW"))?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            lpszClassName: class_pcwstr(),
            // An empty class cursor shows the busy cursor on enter.
            hCursor: crate::cursor::class_cursor(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&raw const wc);
        }

        let (pw, ph) = physical_size(ppp);
        let (x, y) = (0, 0);
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("Calendar"));
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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
        .map_err(|_| GpuError::Win32("CreateWindowExW (calendar)"))?;

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (x, y),
            ppp,
            state: CalendarState::new(today),
            today,
            hover: None,
            hits: Hits::default(),
            visible: false,
            dark,
            next_repaint: Instant::now(),
        });
        win.attach();
        Ok(win)
    }

    pub(super) fn attach(&mut self) {
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                std::ptr::from_mut::<Self>(self) as isize,
            );
        }
    }

    /// Window handle. The main loop uses it to test whether a click landed
    /// inside the panel.
    #[must_use]
    pub const fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Screen rect of the clock card (physical px, `(left, top, right, bottom)`).
    ///
    /// Re-placed on every open: the bar may have changed edge, size or monitor.
    pub fn show_at(&mut self, anchor: (f32, f32, f32, f32), edge: Edge) {
        let (pw, ph) = physical_size(self.ppp);
        let (x, y) = place(pw, ph, anchor, edge, self.ppp);
        self.pos = (x, y);
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, x, y, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
        }
        self.visible = true;
        self.next_repaint = Instant::now();
    }

    /// Hides the panel. Click-outside dismissal is polled by the main loop
    /// (see `crate::popup`).
    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Whether the repaint deadline has passed.
    #[must_use]
    pub fn due(&self) -> bool {
        self.visible && self.next_repaint <= Instant::now()
    }

    /// Follows a "today" that changed across midnight.
    pub fn set_today(&mut self, today: (i32, u32, u32)) {
        if self.today != today {
            self.today = today;
            self.next_repaint = Instant::now();
        }
    }
}
