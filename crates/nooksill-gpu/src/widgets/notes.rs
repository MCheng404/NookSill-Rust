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

use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, CornerRadius, FontId, Rect as EguiRect, Stroke};
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

const CLASS_NAME: &str = "NookSillNotePanel";

/// Notes shown per page.
pub const PAGE_SIZE: usize = 6;
/// Panel logical size.
const W: f32 = 420.0;
const H: f32 = 456.0;
const PAD: f32 = 16.0;
const HEADER_H: f32 = 40.0;
const ROW_H: f32 = 56.0;
const FOOTER_H: f32 = 34.0;
const BTN_W: f32 = 56.0;
const BTN_H: f32 = 24.0;

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
        let (x, y) = centered_origin(pw, ph);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_APPWINDOW | WS_EX_TOPMOST,
                class_pcwstr(),
                windows::core::w!("NookSill 速记"),
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
        let work = nooksill_platform::display::work_area(0);
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
        page_count(self.notes.len())
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
            action = paint(ui, dark, frosted, &notes, page, pages, hover);
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
                let h = row_at(x, y, self.notes.len(), self.page);
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

/// Rect of row `k` in the window (logical coords).
fn row_rect(k: usize) -> EguiRect {
    EguiRect::from_min_size(
        egui::pos2(PAD, PAD + HEADER_H + k as f32 * ROW_H),
        egui::vec2(W - PAD * 2.0, ROW_H - 4.0),
    )
}

/// Which row is hit (footer buttons excluded).
fn row_at(x: f32, y: f32, total: usize, page: usize) -> Option<usize> {
    let start = page * PAGE_SIZE;
    let shown = total.saturating_sub(start).min(PAGE_SIZE);
    (0..shown).find(|&k| row_rect(k).contains(egui::pos2(x, y)))
}

/// Paints the panel. Returns the action of this frame.
fn paint(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    notes: &[Note],
    page: usize,
    pages: usize,
    hover: Option<usize>,
) -> Option<NoteAction> {
    let rect = ui.max_rect();
    // Same base fill, overlay and text tiers as the other four panels.
    let style = PanelStyle::current(dark, frosted);
    let (bg, ink, faint, border, field) = palette(&style);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(PanelStyle::R_PANEL), bg);
    painter.rect_filled(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        style.overlay(),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.left() + PAD, rect.top() + PAD + 2.0),
        Align2::LEFT_TOP,
        "速记",
        FontId::proportional(15.0),
        ink,
    );

    let mut action = None;
    // "New" button, top right of the header.
    let new_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - BTN_W, rect.top() + PAD),
        egui::vec2(BTN_W, BTN_H),
    );
    let new_resp = ui.interact(new_rect, egui::Id::unique("note-new"), egui::Sense::click());
    painter.rect_filled(new_rect, CornerRadius::same(PanelStyle::R_SMALL), field);
    painter.text(
        new_rect.center(),
        Align2::CENTER_CENTER,
        "＋ 新建",
        FontId::proportional(12.0),
        ink,
    );
    if new_resp.clicked() {
        action = Some(NoteAction::New);
    }

    // List.
    let start = page * PAGE_SIZE;
    let shown = notes.len().saturating_sub(start).min(PAGE_SIZE);
    if shown == 0 {
        painter.text(
            egui::pos2(rect.center().x, rect.top() + HEADER_H + 80.0),
            Align2::CENTER_CENTER,
            "还没有速记，点右上角「＋ 新建」",
            FontId::proportional(13.0),
            faint,
        );
    }
    for k in 0..shown {
        let note = &notes[start + k];
        let r = row_rect(k);
        if hover == Some(k) {
            painter.rect_filled(r, CornerRadius::same(PanelStyle::R_CARD), field);
        }
        let row_resp = ui.interact(r, egui::Id::unique(("note-row", k)), egui::Sense::click());
        painter.text(
            egui::pos2(r.left() + 10.0, r.top() + 8.0),
            Align2::LEFT_TOP,
            fit_text(painter, &note.display_title(), r.width() - 140.0, 13.0),
            FontId::proportional(13.0),
            ink,
        );
        painter.text(
            egui::pos2(r.left() + 10.0, r.top() + 30.0),
            Align2::LEFT_TOP,
            fit_text(painter, &note.preview(), r.width() - 140.0, 11.0),
            FontId::proportional(11.0),
            faint,
        );

        // In-row buttons. Registered after the whole row, so egui's hit
        // order lets the buttons win.
        let del_rect = EguiRect::from_min_size(
            egui::pos2(r.right() - BTN_W - 6.0, r.center().y - BTN_H / 2.0),
            egui::vec2(BTN_W, BTN_H),
        );
        let edit_rect = EguiRect::from_min_size(
            egui::pos2(del_rect.left() - BTN_W - 6.0, del_rect.top()),
            egui::vec2(BTN_W, BTN_H),
        );
        for (br, label, act) in [
            (edit_rect, "编辑", NoteAction::Edit(start + k)),
            (del_rect, "删除", NoteAction::Remove(start + k)),
        ] {
            let resp = ui.interact(
                br,
                egui::Id::unique(("note-btn", k, label)),
                egui::Sense::click(),
            );
            painter.rect_filled(br, PanelStyle::pill(br.height()), field);
            painter.text(
                br.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(11.5),
                if label == "删除" {
                    Color32::from_rgb(230, 110, 110)
                } else {
                    ink
                },
            );
            if resp.clicked() {
                action = Some(act);
            }
        }
        // When no button was hit, clicking the row means edit.
        if row_resp.clicked() && action.is_none() {
            action = Some(NoteAction::Edit(start + k));
        }
    }

    // Footer: paging and the item count.
    let fy = rect.bottom() - PAD - FOOTER_H;
    painter.text(
        egui::pos2(rect.left() + PAD, fy + FOOTER_H / 2.0),
        Align2::LEFT_CENTER,
        format!("{} 条 · 第 {} / {} 页", notes.len(), page + 1, pages),
        FontId::proportional(11.0),
        faint,
    );
    let prev_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - BTN_W * 2.0 - 8.0, fy + 5.0),
        egui::vec2(BTN_W, BTN_H),
    );
    let next_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - BTN_W, fy + 5.0),
        egui::vec2(BTN_W, BTN_H),
    );
    let prev = ui.interact(
        prev_rect,
        egui::Id::unique("note-prev"),
        egui::Sense::click(),
    );
    let next = ui.interact(
        next_rect,
        egui::Id::unique("note-next"),
        egui::Sense::click(),
    );
    for (r, label, enabled) in [
        (prev_rect, "上一页", page > 0),
        (next_rect, "下一页", page + 1 < pages),
    ] {
        painter.rect_filled(r, CornerRadius::same(PanelStyle::R_SMALL), field);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            if enabled { ink } else { faint },
        );
    }
    if prev.clicked() && page > 0 {
        return Some(NoteAction::Page(-1));
    }
    if next.clicked() && page + 1 < pages {
        return Some(NoteAction::Page(1));
    }
    action
}

/// Total number of pages. Pure function so it can be tested without a window.
fn page_count(total: usize) -> usize {
    total.div_ceil(PAGE_SIZE).max(1)
}

/// Offscreen entry point (`examples/panels_shot.rs`).
///
/// Screenshots must use the real window's painting code; a hand-built
/// approximation would not catch anything. This builds no window and touches
/// no Win32 API, it only paints the panel into the caller's `Ui`.
pub fn paint_preview(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    book: &NoteBook,
    page: usize,
    hover: Option<usize>,
) {
    let _ = paint(
        ui,
        dark,
        frosted,
        &book.notes,
        page,
        page_count(book.notes.len()),
        hover,
    );
}

fn palette(style: &PanelStyle) -> (Color32, Color32, Color32, Color32, Color32) {
    // Keeps five semantic names so the drawing code below stays unchanged;
    // all values come from the shared style.
    (
        style.bg(),
        style.ink(),
        style.ink_sub(),
        style.border(),
        style.card(),
    )
}

/// Truncates overlong text and appends an ellipsis.
fn fit_text(painter: &egui::Painter, text: &str, max_w: f32, size: f32) -> String {
    let font = FontId::proportional(size);
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
        <= max_w
    {
        return text.to_owned();
    }
    let mut out: Vec<char> = text.chars().collect();
    while !out.is_empty() {
        out.pop();
        let s: String = out.iter().collect();
        if painter
            .layout_no_wrap(format!("{s}…"), font.clone(), Color32::WHITE)
            .size()
            .x
            <= max_w
        {
            return format!("{s}…");
        }
    }
    String::new()
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

fn centered_origin(pw: i32, ph: i32) -> (i32, i32) {
    let work = nooksill_platform::display::work_area(0);
    (
        (work.left + work.right) / 2 - pw / 2,
        (work.top + work.bottom) / 2 - ph / 2,
    )
}

fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x / ppp, y / ppp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_laid_out_sequentially() {
        let a = row_rect(0);
        let b = row_rect(1);
        assert!(b.top() > a.top());
        assert_eq!(a.left(), b.left());
        assert_eq!(a.width(), b.width());
    }

    #[test]
    fn row_at_matches_the_visible_page() {
        let a = row_rect(0).center();
        assert_eq!(row_at(a.x, a.y, 3, 0), Some(0));
        let b = row_rect(1).center();
        assert_eq!(row_at(b.x, b.y, 3, 0), Some(1));
        // Row 0 of page 2 is also index 0 within the page; the global index
        // is `page * PAGE_SIZE` plus it.
        assert_eq!(row_at(a.x, a.y, 10, 1), Some(0));
        assert_eq!(row_at(5.0, 5.0, 3, 0), None, "标题栏不算行");
    }

    #[test]
    fn page_count_has_a_floor_of_one() {
        assert_eq!(page_count(0), 1);
        assert_eq!(page_count(1), 1);
        assert_eq!(page_count(PAGE_SIZE), 1);
        assert_eq!(page_count(PAGE_SIZE + 1), 2);
    }

    #[test]
    fn max_notes_divides_into_pages() {
        assert_eq!(page_count(nooksill_core::note::MAX_NOTES), 6);
    }
}
