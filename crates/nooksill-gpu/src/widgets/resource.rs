//! Resource monitor panel, opened from the "CPU xx%" card on the bar.
//!
//! Rows are chosen and ordered by `[widgets] ResOrder`; the panel resizes to
//! fit them. CPU / memory / disk draw a 0..100% bar, the rest show a value and
//! a caption. No separator line under the title: spacing carries the rhythm.
//!
//! Like the settings window it is a second layered window inside the same
//! process, sharing the GPU device and owning its own `FrameRenderer`. This
//! panel refreshes on its own: the data changes every second, so it schedules
//! frames every second instead of relying on egui repaint hints.

use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, FontId, Rect as EguiRect};
use nooksill_core::resource::{self, ResKind};
use nooksill_platform::SysInfo;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    GWLP_USERDATA, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};

/// Window class name.
pub const CLASS_NAME: &str = "NookSillResMon";

/// `VK_ESCAPE`. The `windows` crate has the constant, but one key is not
/// worth an extra import.
const VK_ESCAPE: usize = 0x1B;

/// Panel logical width; height follows the selected rows.
pub const PANEL_W: f32 = 360.0;

/// Layout constants; `calendar/paint.rs` uses the same "numbers live here" rule.
const SIDE_PAD: f32 = 18.0;
const TOP_PAD: f32 = 16.0;
const TITLE_H: f32 = 28.0;
const HEAD_GAP: f32 = 10.0;
const BAR_ROW_H: f32 = 44.0;
const TEXT_ROW_H: f32 = 32.0;
const ROW_GAP: f32 = 8.0;
const BAR_TOP: f32 = 26.0;
const BAR_H: f32 = 7.0;
const BOTTOM_PAD: f32 = 12.0;
const FONT_TITLE: f32 = 18.0;
const FONT_LABEL: f32 = 13.0;
const FONT_VALUE: f32 = 15.0;
const FONT_EXTRA: f32 = 12.0;

/// Row height: bar rows need room for the track, text rows do not.
fn row_height(kind: ResKind) -> f32 {
    if kind.has_bar() {
        BAR_ROW_H
    } else {
        TEXT_ROW_H
    }
}

/// Logical panel height for `order`.
#[must_use]
pub fn logical_height(order: &[ResKind]) -> f32 {
    let rows: f32 = order.iter().map(|k| row_height(*k)).sum();
    let gaps = order.len().saturating_sub(1) as f32 * ROW_GAP;
    TOP_PAD + TITLE_H + HEAD_GAP + rows + gaps + BOTTOM_PAD
}

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
    info: SysInfo,
    /// Selected rows, in display order.
    order: Vec<ResKind>,
    /// Current physical size; a row change needs a renderer resize.
    size: (i32, i32),
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

        let order = resource::parse_order("");
        let (pw, ph) = physical_size(ppp, &order);
        let (x, y) = nooksill_platform::display::centered_on_target(pw, ph);
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("Resource Monitor"));
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
        .map_err(|_| GpuError::Win32("CreateWindowExW (resource monitor)"))?;

        let lease = crate::widgets::framework::WindowLease::new(hwnd);
        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let hwnd = lease.defuse();
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (x, y),
            ppp,
            info: SysInfo::default(),
            order,
            size: (pw, ph),
            next_refresh: Instant::now(),
            visible: false,
            dark,
        });
        win.attach();
        Ok(win)
    }

    /// Stores the self pointer on `GWLP_USERDATA`; `new` is the only caller.
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

    /// Shows the panel under the published placement policy.
    ///
    /// The renderer is resized to fit `order` before the window is placed: a
    /// stale offscreen target would clip the last rows.
    pub fn show_at(
        &mut self,
        gpu: &Gpu,
        anchor: (f32, f32, f32, f32),
        edge: nooksill_core::config::Edge,
        order: &[ResKind],
    ) -> Result<(), GpuError> {
        self.order = order.to_vec();
        let (pw, ph) = physical_size(self.ppp, &self.order);
        if (pw, ph) != self.size {
            self.renderer.resize(gpu, pw as u32, ph as u32)?;
            self.size = (pw, ph);
        }
        let (x, y) = crate::widgets::framework::show_panel(
            &mut self.renderer,
            self.hwnd,
            crate::widgets::framework::PanelKind::Resource,
            (pw, ph),
            Some(anchor),
            edge,
            self.ppp,
        );
        self.pos = (x, y);
        self.visible = true;
        self.next_refresh = Instant::now() + Duration::from_secs(1);
        Ok(())
    }

    /// Replace the snapshot; the main loop pushes it every frame from the
    /// sampler thread, so the panel never calls the system itself.
    pub fn set_info(&mut self, info: SysInfo) {
        self.info = info;
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        crate::widgets::framework::hide_panel(&mut self.renderer, self.hwnd);
    }

    /// Whether the refresh deadline has passed or a backdrop frame arrived.
    #[must_use]
    pub fn due(&mut self) -> bool {
        self.visible && (self.next_refresh <= Instant::now() || self.renderer.frost_pending())
    }

    /// Paints one frame.
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        self.next_refresh = Instant::now() + Duration::from_secs(1);

        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let info = self.info;
        let order = self.order.clone();
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            draw_panel(ui, dark, frosted, info, &order);
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

/// Text anchors and optional bar of one row, in panel-local logical points.
#[derive(Copy, Clone, Debug, PartialEq)]
struct RowLayout {
    label: egui::Pos2,
    value: egui::Pos2,
    bar: Option<EguiRect>,
}

/// Explicit row geometry; egui's horizontal layout drops the right-aligned
/// value when the remaining width runs out.
fn row_layouts(rect: EguiRect, order: &[ResKind]) -> Vec<RowLayout> {
    let left = rect.left() + SIDE_PAD;
    let right = rect.right() - SIDE_PAD;
    let mut y = rect.top() + TOP_PAD + TITLE_H + HEAD_GAP;
    order
        .iter()
        .map(|kind| {
            let row = RowLayout {
                label: egui::pos2(left, y),
                value: egui::pos2(right, y),
                bar: kind.has_bar().then(|| {
                    EguiRect::from_min_max(
                        egui::pos2(left, y + BAR_TOP),
                        egui::pos2(right, y + BAR_TOP + BAR_H),
                    )
                }),
            };
            y += row_height(*kind) + ROW_GAP;
            row
        })
        .collect()
}

/// `(label, value, secondary value, percent)` for one resource.
///
/// There is no caption line by design: the panel is a dashboard, not a
/// description. Temperature and upload rate ride beside the main value.
fn row_text(kind: ResKind, info: &SysInfo) -> (&'static str, String, Option<String>, u8) {
    let t = nooksill_core::i18n::t;
    let f = nooksill_core::i18n::fmt;
    match kind {
        ResKind::Cpu => (
            t("Processor"),
            format!("{}%", info.cpu_percent),
            info.cpu_temp_c.map(|c| format!("{c}°C")),
            info.cpu_percent,
        ),
        ResKind::Gpu => (
            t("GPU"),
            info.gpu_percent
                .map_or_else(|| "—".to_owned(), |v| format!("{v}%")),
            None,
            info.gpu_percent.unwrap_or(0),
        ),
        ResKind::Memory => (
            t("Memory"),
            format!("{}%", info.mem_percent),
            None,
            info.mem_percent,
        ),
        ResKind::Disk => (
            t("Disk (C:)"),
            format!("{}%", info.disk_percent),
            None,
            info.disk_percent,
        ),
        ResKind::Network => (
            t("Network"),
            f("↓ {0} KB/s", &[info.net_rx_kbps.to_string().as_str()]),
            Some(f("↑ {0} KB/s", &[info.net_tx_kbps.to_string().as_str()])),
            0,
        ),
        ResKind::Processes => (t("Processes"), info.process_count.to_string(), None, 0),
        ResKind::Uptime => (
            t("Uptime"),
            f(
                "{0}d {1}h",
                &[
                    (info.uptime_secs / 86_400).to_string().as_str(),
                    ((info.uptime_secs % 86_400) / 3600).to_string().as_str(),
                ],
            ),
            None,
            0,
        ),
    }
}

/// Paints the panel from the same function the offscreen shots use.
pub fn paint_preview(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    info: SysInfo,
    order: &[ResKind],
) {
    draw_panel(ui, dark, frosted, info, order);
}

fn draw_panel(ui: &mut egui::Ui, dark: bool, frosted: bool, info: SysInfo, order: &[ResKind]) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let style = PanelStyle::current(dark, frosted);
    style.paint_frame(painter, rect);
    let (tc, sub) = (style.ink(), style.ink_sub());
    let accent = ui.visuals().selection.bg_fill;

    painter.text(
        egui::pos2(rect.left() + SIDE_PAD, rect.top() + TOP_PAD),
        Align2::LEFT_TOP,
        nooksill_core::i18n::t("Resource Monitor"),
        FontId::proportional(FONT_TITLE),
        tc,
    );

    let track_fill = if dark {
        Color32::from_white_alpha(30)
    } else {
        Color32::from_black_alpha(26)
    };
    for (row, kind) in row_layouts(rect, order).into_iter().zip(order) {
        let (label, value, extra, pct) = row_text(*kind, &info);
        painter.text(
            row.label,
            Align2::LEFT_TOP,
            label,
            FontId::proportional(FONT_LABEL),
            tc,
        );
        // Right-aligned value, measured so the secondary value can sit beside it.
        let galley = painter.layout_no_wrap(value, FontId::proportional(FONT_VALUE), tc);
        let value_left = row.value.x - galley.size().x;
        painter.galley(egui::pos2(value_left, row.value.y), galley, tc);
        if let Some(extra) = extra {
            painter.text(
                egui::pos2(value_left - 10.0, row.value.y + 3.0),
                Align2::RIGHT_TOP,
                extra,
                FontId::proportional(FONT_EXTRA),
                sub,
            );
        }
        if let Some(bar) = row.bar {
            painter.rect_filled(bar, PanelStyle::pill(bar.height()), track_fill);
            let filled = bar.width() * f32::from(pct.min(100)) / 100.0;
            if filled > 1.0 {
                let rect = EguiRect::from_min_size(bar.min, egui::vec2(filled, bar.height()));
                painter.rect_filled(rect, PanelStyle::pill(rect.height()), accent);
            }
        }
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
        crate::widgets::framework::destroy_panel(&mut self.renderer, self.hwnd);
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

fn physical_size(ppp: f32, order: &[ResKind]) -> (i32, i32) {
    (
        (PANEL_W * ppp).round() as i32,
        (logical_height(order) * ppp).round() as i32,
    )
}

/// Tells whether this window is the resource monitor panel.
#[must_use]
pub fn is_resmon_window(hwnd: HWND) -> bool {
    let mut buf = [0_u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf) };
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == CLASS_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> SysInfo {
        SysInfo {
            cpu_percent: 42,
            mem_percent: 73,
            disk_percent: 18,
            mem_used_mb: 6144,
            mem_total_mb: 8192,
            disk_free_gb: 120,
            disk_total_gb: 512,
            net_rx_kbps: 520,
            net_tx_kbps: 88,
            process_count: 214,
            uptime_secs: 3 * 86_400 + 5 * 3600,
            gpu_percent: Some(35),
            cpu_temp_c: Some(58),
        }
    }

    fn rect(order: &[ResKind]) -> EguiRect {
        EguiRect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(PANEL_W, logical_height(order)),
        )
    }

    #[test]
    fn rows_follow_the_order_and_never_overlap() {
        let order = resource::parse_order("uptime,net,cpu,proc,mem,disk,gpu");
        let panel = rect(&order);
        let rows = row_layouts(panel, &order);
        assert_eq!(rows.len(), order.len());
        for (i, (row, kind)) in rows.iter().zip(&order).enumerate() {
            assert_eq!(row.bar.is_some(), kind.has_bar());
            assert!(row.label.x >= panel.left() + SIDE_PAD - 0.01);
            assert!(row.value.x <= panel.right() - SIDE_PAD + 0.01);
            if i > 0 {
                let prev_end = rows[i - 1].label.y + row_height(order[i - 1]);
                assert!(
                    prev_end <= row.label.y,
                    "第 {} 行与第 {i} 行贴在一起",
                    i - 1
                );
            }
        }
    }

    #[test]
    fn height_matches_the_last_row() {
        for order in [
            resource::parse_order("cpu"),
            resource::parse_order("cpu,mem,disk"),
            resource::parse_order(""),
        ] {
            let panel = rect(&order);
            let rows = row_layouts(panel, &order);
            let last = rows.last().expect("至少一行");
            let bottom = last.label.y + row_height(*order.last().expect("至少一项"));
            assert!((panel.bottom() - bottom - BOTTOM_PAD).abs() < 0.01);
        }
    }

    #[test]
    fn row_text_covers_every_kind_without_captions() {
        let data = info();
        for kind in ResKind::ALL {
            let (label, value, _extra, pct) = row_text(kind, &data);
            assert!(!label.is_empty());
            assert!(!value.is_empty(), "{kind:?} 缺主数值");
            assert!(pct <= 100);
        }
        assert_eq!(row_text(ResKind::Cpu, &data).2.as_deref(), Some("58°C"));
        assert_eq!(row_text(ResKind::Processes, &data).2, None, "不再显示说明");
        assert_eq!(row_text(ResKind::Processes, &data).1, "214");
        assert_eq!(row_text(ResKind::Network, &data).3, 0);
    }

    #[test]
    fn missing_gpu_reading_shows_a_dash() {
        let mut data = info();
        data.gpu_percent = None;
        let (_, value, extra, pct) = row_text(ResKind::Gpu, &data);
        assert_eq!(value, "—");
        assert_eq!(extra, None);
        assert_eq!(pct, 0);
        assert!(ResKind::Gpu.has_bar());
    }
}
