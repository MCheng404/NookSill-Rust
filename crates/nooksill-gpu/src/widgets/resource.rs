//! Resource monitor panel, opened from the "CPU xx%" card on the bar.
//!
//! Draws three progress bars: processor, memory and disk (C:).
//!
//! Like the settings window it is a second layered window inside the same
//! process, sharing the GPU device and owning its own `FrameRenderer`. This
//! panel refreshes on its own: the data changes every second, so it schedules
//! frames every second instead of relying on egui repaint hints.

use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::widgets::style::PanelStyle;
use egui::{Align2, FontId, Rect as EguiRect, Stroke};
use nooksill_platform::{CpuSampler, SysInfo};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    ShowWindow, GWLP_USERDATA, SW_HIDE, SW_SHOW, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};

/// Window class name.
pub const CLASS_NAME: &str = "NookSillResMon";

/// `VK_ESCAPE`. The `windows` crate has the constant, but one key is not
/// worth an extra import.
const VK_ESCAPE: usize = 0x1B;

/// Panel logical size (local coords). 340 wide, widened a little so the
/// progress bars have room.
const LOGICAL_SIZE: (f32, f32) = (340.0, 210.0);

/// Resource monitor panel.
pub struct ResMonWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    /// Needed by `Input::take` for `screen_rect`; without it egui clips the panel.
    input: crate::input::Input,
    /// Screen-space top-left, in physical pixels.
    pos: (i32, i32),
    /// Physical pixels per logical point.
    ppp: f32,
    cpu: CpuSampler,
    info: SysInfo,
    next_refresh: Instant,
    visible: bool,
    dark: bool,
}

impl std::fmt::Debug for ResMonWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResMonWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

impl ResMonWindow {
    /// Creates the window (hidden).
    pub fn new(gpu: &Gpu, ppp: f32, dark: bool) -> Result<Pin<Box<Self>>, GpuError> {
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
        let (x, y) = centered_origin(pw, ph);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_APPWINDOW | WS_EX_TOPMOST,
                class_pcwstr(),
                windows::core::w!("资源监视器"),
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
        .map_err(|_| GpuError::Win32("CreateWindowExW（资源监视器）"))?;

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (x, y),
            ppp,
            cpu: CpuSampler::new(),
            info: SysInfo::default(),
            next_refresh: Instant::now(),
            visible: false,
            dark,
        });
        win.attach();
        Ok(win)
    }

    /// Stores the self pointer on `GWLP_USERDATA`.
    ///
    /// Public so the main loop can call it separately; the settings window
    /// calls it inside `new`, here the caller owns the lazy-create-then-attach
    /// order.
    pub fn attach_public(&mut self) {
        self.attach();
    }

    fn attach(&mut self) {
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

    /// Shows the panel and samples the data once.
    pub fn show(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        self.visible = true;
        self.refresh();
        self.next_refresh = Instant::now() + Duration::from_secs(1);
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Whether the refresh deadline has passed.
    #[must_use]
    pub fn due(&self) -> bool {
        self.visible && self.next_refresh <= Instant::now()
    }

    /// Samples the data once more.
    pub fn refresh(&mut self) {
        self.info = nooksill_platform::sysinfo::sample(&mut self.cpu);
    }

    /// Paints one frame.
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        self.refresh();
        self.next_refresh = Instant::now() + Duration::from_secs(1);

        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let info = self.info;
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            draw_panel(ui, dark, frosted, info);
        })?;
        Ok(())
    }

    fn on_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            // The panel is read-only, so the cursor is always an arrow, but
            // it must be set here: an empty class cursor falls back to the
            // busy cursor.
            WM_SETCURSOR => {
                crate::cursor::set(crate::cursor::arrow());
                Some(LRESULT(1))
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP | WM_MOUSEMOVE => None,
            // Esc always hides the panel: it is the only way out once
            // click-outside dismissal is off.
            WM_KEYDOWN => {
                if wparam.0 == VK_ESCAPE {
                    self.hide();
                }
                Some(LRESULT(0))
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
}

/// Paints the panel: title plus three progress bars (processor, memory, disk).
///
/// Positions are explicit, like the original `ResDraw` (`DrawTextBox` takes
/// rects): egui's layout squeezes the right-aligned value out of existence,
/// since `with_layout(right_to_left)` in a `horizontal` gets no remaining
/// width.
fn draw_panel(ui: &mut egui::Ui, dark: bool, frosted: bool, info: SysInfo) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    // Shared style, which also avoids the `argbc` argument order bug the
    // timer panel used to have.
    let style = PanelStyle::current(dark, frosted);
    style.paint_frame(painter, rect);
    let (tc, sub) = (style.ink(), style.ink_sub());
    let accent = ui.visuals().selection.bg_fill;

    let pad = 16.0;
    let mut y = rect.top() + pad;

    // Title
    painter.text(
        egui::pos2(rect.left() + pad, y + 10.0),
        Align2::LEFT_TOP,
        "资源监视器",
        FontId::proportional(18.0),
        tc,
    );
    y += 30.0;
    painter.line_segment(
        [
            egui::pos2(rect.left() + pad, y),
            egui::pos2(rect.right() - pad, y),
        ],
        Stroke::new(1.0, sub),
    );
    y += 14.0;

    let rows = [
        ("处理器", info.cpu_percent, format!("{}%", info.cpu_percent)),
        (
            "内存",
            info.mem_percent,
            format!("{} / {} MB", info.mem_used_mb, info.mem_total_mb),
        ),
        (
            "磁盘（C:）",
            info.disk_percent,
            format!("可用 {} / {} GB", info.disk_free_gb, info.disk_total_gb),
        ),
    ];

    for (label, pct, detail) in rows {
        // Label on the left, value on the right (explicit positions, no
        // layout).
        painter.text(
            egui::pos2(rect.left() + pad, y + 8.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(13.0),
            tc,
        );
        painter.text(
            egui::pos2(rect.right() - pad, y + 8.0),
            Align2::RIGHT_TOP,
            detail,
            FontId::proportional(12.0),
            sub,
        );
        y += 22.0;
        // Progress bar
        let bar_h = 6.0;
        let track = EguiRect::from_min_size(
            egui::pos2(rect.left() + pad, y),
            egui::vec2(rect.width() - 2.0 * pad, bar_h),
        );
        painter.rect_filled(track, PanelStyle::pill(track.height()), sub);
        let pct = f32::from(pct.min(100));
        let filled =
            EguiRect::from_min_size(track.min, egui::vec2(track.width() * pct / 100.0, bar_h));
        if filled.width() > 1.0 {
            painter.rect_filled(filled, PanelStyle::pill(filled.height()), accent);
        }
        y += bar_h + 18.0;
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ResMonWindow;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer is stored by `attach` on the final address of the
    // pinned `Box<Self>`.
    let win = unsafe { &mut *ptr };
    if let Some(result) = win.on_message(msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl Drop for ResMonWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
        }
    }
}

/// The class name must be a UTF-16 wide string that outlives the process.
fn class_pcwstr() -> windows::core::PCWSTR {
    static BUF: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    let buf = BUF.get_or_init(|| {
        CLASS_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    });
    windows::core::PCWSTR(buf.as_ptr())
}

fn physical_size(ppp: f32) -> (i32, i32) {
    (
        (LOGICAL_SIZE.0 * ppp).round() as i32,
        (LOGICAL_SIZE.1 * ppp).round() as i32,
    )
}

/// Centers the panel in the work area.
fn centered_origin(pw: i32, ph: i32) -> (i32, i32) {
    let work = nooksill_platform::display::work_area(0);
    let cx = (work.left + work.right) / 2;
    let cy = (work.top + work.bottom) / 2;
    (cx - pw / 2, cy - ph / 2)
}

/// Tells whether this window is the resource monitor panel.
#[must_use]
pub fn is_resmon_window(hwnd: HWND) -> bool {
    let mut buf = [0_u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf) };
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == CLASS_NAME
}

/// Text shown on the panel (used by tests).
#[must_use]
pub fn summary(info: &SysInfo) -> String {
    format!(
        "CPU {}%  内存 {}%  磁盘 {}%",
        info.cpu_percent, info.mem_percent, info.disk_percent
    )
}
