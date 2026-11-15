//! Timer window lifecycle: creation, visibility and repaint timing.

use super::win::{class_pcwstr, physical_size, wndproc};
use super::*;
use crate::widgets::framework::{self, PanelKind};

impl ToolWindow {
    /// Creates the window (hidden).
    pub fn new(
        gpu: &Gpu,
        ppp: f32,
        dark: bool,
        state: ToolState,
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

        let (pw, ph) = physical_size(ppp, panel_h(state.todos.len()));
        let (x, y) = nooksill_platform::display::centered_on_target(pw, ph);
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("Timer · To-do"));
        let hwnd = unsafe {
            CreateWindowExW(
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
        .map_err(|_| GpuError::Win32("CreateWindowExW (timer panel)"))?;

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (x, y),
            size: (pw, ph),
            placement: None,
            ppp,
            state,
            hits: Hits::default(),
            hover: None,
            next_repaint: Instant::now(),
            visible: false,
            dark,
            dirty: false,
            close_requested: false,
            typed: String::new(),
            typing: false,
            ime: Ime::new(),
        });
        win.attach();
        // The todo field accepts Chinese, so the window needs an IME
        // context.
        Ime::enable(hwnd);
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

    /// Opens beside `anchor` (or centred, per the published policy).
    pub fn show_at(&mut self, anchor: (f32, f32, f32, f32), edge: Edge) {
        self.placement = Some((anchor, edge));
        let (pw, ph) = physical_size(self.ppp, panel_h(self.state.todos.len()));
        let (x, y) =
            framework::place_panel(PanelKind::Tool, (pw, ph), Some(anchor), edge, self.ppp);
        self.pos = (x, y);
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                pw,
                ph,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
            );
        }
        framework::apply_backdrop(&mut self.renderer, self.hwnd, self.ppp);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        self.visible = true;
        self.next_repaint = Instant::now();
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        framework::release_backdrop(&mut self.renderer, self.hwnd);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.typing = false;
        self.typed.clear();
    }

    /// Whether the next frame is due, including a new backdrop capture.
    #[must_use]
    pub fn due(&mut self) -> bool {
        self.visible && (self.next_repaint <= Instant::now() || self.renderer.frost_pending())
    }
}
