//! Quick notes panel: lists every note and hands create, edit and delete to
//! the main loop.
//!
//! No input fields
//!
//! The panel is a `WS_EX_NOACTIVATE` layered window and taking keyboard focus
//! is awkward. Text entry and the IME go through the modal dialog in
//! [`crate::widgets::editor`], which already handles `WM_CHAR`, `WM_IME_*` and
//! a multiline `TextEdit`. The panel only shows the list and reports which row
//! was picked; the action is executed by `App`.
//!
//! Paging instead of scrolling
//!
//! A `ScrollArea` shifts its child Ui, so `ui.interact` rects and the
//! `RawInput` window coords need extra conversion and it becomes easy to see
//! one row and click another. There are at most [`MAX_NOTES`] notes, so a
//! fixed 6 per page plus paging buttons is enough and never misaligns.

use std::pin::Pin;
use std::time::Instant;

use nooksill_core::note::{Note, NoteBook};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_USERDATA, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE,
    SW_SHOW, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};

mod paint;
pub use paint::paint_preview;

const CLASS_NAME: &str = "NookSillNotePanel";

/// Notes shown per page.
pub const PAGE_SIZE: usize = 6;
/// Panel logical size.
pub(super) const W: f32 = 420.0;
pub(super) const H: f32 = 456.0;
pub(super) const PAD: f32 = 16.0;
pub(super) const HEADER_H: f32 = 40.0;
pub(super) const ROW_H: f32 = 56.0;
pub(super) const FOOTER_H: f32 = 34.0;
pub(super) const BTN_W: f32 = 56.0;
pub(super) const BTN_H: f32 = 24.0;

/// Panel logical size (offscreen previews).
pub const SIZE: (f32, f32) = (W, H);

/// An action produced by the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteAction {
    /// Create a note (the dialog asks for title and body).
    New,
    /// Edit note `k`.
    Edit(usize),
    /// Delete note `k`.
    Remove(usize),
    /// Turn the page (handled inside the panel, not sent to the main loop).
    Page(i32),
}

/// Quick notes panel.
pub struct NoteWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    input: crate::input::Input,
    pos: (i32, i32),
    ppp: f32,
    dark: bool,
    visible: bool,
    notes: Vec<Note>,
    page: usize,
    hover: Option<usize>,
    /// Action waiting for the main loop.
    action: Option<NoteAction>,
    next_repaint: Option<Instant>,
}

impl std::fmt::Debug for NoteWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .field("notes", &self.notes.len())
            .finish_non_exhaustive()
    }
}

impl NoteWindow {
    /// Creates the window (hidden).
    pub fn new(gpu: &Gpu, ppp: f32, dark: bool) -> Result<Pin<Box<Self>>, GpuError> {
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
        let pw = (W * ppp).round() as i32;
        let ph = (H * ppp).round() as i32;
        let (x, y) = nooksill_platform::display::centered_on_target(pw, ph);
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("NookSill Notes"));
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
        .map_err(|_| GpuError::Win32("CreateWindowExW（速记面板）"))?;

        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (x, y),
            ppp,
            dark,
            visible: false,
            notes: Vec::new(),
            page: 0,
            hover: None,
            action: None,
            next_repaint: None,
        });
        win.attach();
        Ok(win)
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

    #[must_use]
    pub const fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Refreshes the content and shows the panel. `anchor` is the screen rect
    /// (physical px) of the notes icon.
    pub fn show(&mut self, book: &NoteBook, anchor: (i32, i32, i32, i32)) {
        self.notes = book.notes.clone();
        self.clamp_page();
        self.hover = None;
        self.action = None;
        let pw = (W * self.ppp).round() as i32;
        let ph = (H * self.ppp).round() as i32;
        let (al, at, ar, _) = anchor;
        let cx = (al + ar) / 2;
        let work = nooksill_platform::display::work_area_at(cx, at);
        let mut x = cx - pw / 2;
        if x < work.left {
            x = work.left;
        }
        if x + pw > work.right {
            x = work.right - pw;
        }
        let mut y = at - ph - 8;
        if y < work.top {
            y = work.top;
        }
        self.pos = (x, y);
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                pw,
                ph,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
        self.visible = true;
        self.next_repaint = Some(Instant::now());
    }

    /// Feeds new data in and keeps the current page.
    pub fn set_notes(&mut self, book: &NoteBook) {
        self.notes = book.notes.clone();
        self.clamp_page();
        self.next_repaint = Some(Instant::now());
    }

    fn clamp_page(&mut self) {
        let pages = self.pages();
        if self.page >= pages {
            self.page = pages.saturating_sub(1);
        }
    }

    /// Number of pages (at least 1, so an empty panel can still show "new").
    #[must_use]
    pub fn pages(&self) -> usize {
        paint::page_count(self.notes.len())
    }

    fn turn_page(&mut self, delta: i32) {
        let pages = self.pages() as i32;
        let next = (self.page as i32 + delta).clamp(0, pages - 1);
        self.page = next.max(0) as usize;
        self.hover = None;
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.hover = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Takes the pending action.
    pub fn take_action(&mut self) -> Option<NoteAction> {
        self.action.take()
    }

    #[must_use]
    pub fn due(&self) -> bool {
        self.visible && self.next_repaint.is_some_and(|t| t <= Instant::now())
    }

    /// Paints one frame.
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        self.next_repaint = None;
        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let notes = self.notes.clone();
        let page = self.page;
        let hover = self.hover;
        let pages = self.pages();
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        let mut action = None;
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            action = paint::paint(ui, dark, frosted, &notes, page, pages, hover);
        })?;
        match action {
            // Paging only changes the panel's own state and does not use the
            // main loop's action channel.
            Some(NoteAction::Page(delta)) => {
                self.turn_page(delta);
                self.next_repaint = Some(Instant::now());
            }
            Some(a) => {
                self.action = Some(a);
                self.next_repaint = Some(Instant::now());
            }
            None => {}
        }
        Ok(())
    }

    fn on_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_SETCURSOR => {
                crate::cursor::set(crate::cursor::arrow());
                Some(LRESULT(1))
            }
            WM_MOUSEMOVE => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.input.pointer_moved(x, y);
                let h = paint::row_at(x, y, self.notes.len(), self.page);
                if h != self.hover {
                    self.hover = h;
                    self.next_repaint = Some(Instant::now());
                }
                None
            }
            WM_LBUTTONDOWN => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.input.pointer_moved(x, y);
                self.input
                    .pointer_button(true, egui::PointerButton::Primary);
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_LBUTTONUP => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.input.pointer_moved(x, y);
                self.input
                    .pointer_button(false, egui::PointerButton::Primary);
                self.next_repaint = Some(Instant::now());
                None
            }
            WM_RBUTTONDOWN | WM_RBUTTONUP => None,
            WM_KEYDOWN => {
                if wparam.0 == 0x1B {
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
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut NoteWindow;
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

impl Drop for NoteWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
        }
    }
}

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

fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x / ppp, y / ppp)
}
