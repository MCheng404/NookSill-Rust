//! Window messages: keys, pointer, IME and drag/drop.

use super::win::client_pos;
use super::*;

impl LaunchpadWindow {
    pub(super) fn on_message(
        &mut self,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        // The IME messages come first: they are a separate channel from the
        // keyboard messages and would fall into the `match` below.
        if matches!(
            msg,
            windows::Win32::UI::WindowsAndMessaging::WM_IME_STARTCOMPOSITION
                | windows::Win32::UI::WindowsAndMessaging::WM_IME_COMPOSITION
                | windows::Win32::UI::WindowsAndMessaging::WM_IME_ENDCOMPOSITION
                | windows::Win32::UI::WindowsAndMessaging::WM_IME_SETCONTEXT
        ) {
            let mut committed = String::new();
            let handled = self
                .ime
                .handle(self.hwnd, msg, wparam, lparam, &mut committed);
            if !committed.is_empty() {
                // The rename field takes Chinese text too.
                if self.renaming.is_some() {
                    for c in committed.chars() {
                        if self.rename_buf.chars().count() < 40 {
                            self.rename_buf.push(c);
                        }
                    }
                } else {
                    for c in committed.chars() {
                        if self.query.chars().count() < 32 {
                            self.query.push(c);
                        }
                    }
                    self.page = 0;
                    self.open_folder = None;
                }
            }
            if msg == windows::Win32::UI::WindowsAndMessaging::WM_IME_STARTCOMPOSITION {
                // Park the candidate window below the search box instead of
                // letting it float in the screen corner.
                self.ime
                    .set_caret(self.hwnd, self.size.0 * 0.5, 40.0 + SEARCH_H, self.ppp);
            }
            self.next_repaint = Instant::now();
            if handled {
                return Some(LRESULT(0));
            }
            return Some(LRESULT(Ime::default_proc(self.hwnd, msg, wparam, lparam)));
        }
        match msg {
            // Cursor: not answering this message falls back to the system
            // default. Hand over icons, buttons and menu items, arrow
            // elsewhere.
            WM_SETCURSOR => {
                let mut on_hit = false;
                if let Some((px, py)) = crate::cursor::client_pos(self.hwnd) {
                    on_hit = self.hit_at(px / self.ppp, py / self.ppp).is_some();
                }
                crate::cursor::set_hand_if(on_hit);
                Some(LRESULT(1))
            }
            WM_APP_WAKE => {
                // The icon thread has results: ask for a repaint.
                self.next_repaint = Instant::now();
                None
            }
            WM_MOUSEMOVE => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.cur = (x, y);
                if self.press.is_some() {
                    let dx = x - self.down.0;
                    let dy = y - self.down.1;
                    if !self.dragging && (dx.abs() > 8.0 || dy.abs() > 8.0) {
                        // Only apps can be dragged.
                        if let Some(Hit::Cell(k)) = self.press {
                            self.dragging = true;
                            self.drag_src = Some(k);
                            self.drag_page = self.page;
                        }
                    }
                }
                let h = self.hit_at(x, y);
                if self.hover != h || self.dragging {
                    self.hover = h;
                    self.next_repaint = Instant::now();
                }
                None
            }
            WM_LBUTTONDOWN => {
                let (x, y) = client_pos(lparam, self.ppp);
                self.down = (x, y);
                self.cur = (x, y);
                self.press = self.hit_at(x, y);
                self.dragging = false;
                self.drag_src = None;
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                let (x, y) = client_pos(lparam, self.ppp);
                let released = self.hit_at(x, y);
                if std::env::var("NOOKSILL_LP_TRACE").is_ok() {
                    eprintln!(
                        "[lp] LBUTTONUP ({x:.0},{y:.0}) press={:?} released={:?} menu={:?}",
                        self.press, released, self.menu
                    );
                }
                // With the menu open this click only picks a menu item or
                // closes the menu by clicking outside.
                if self.menu.is_some() {
                    // Only a click outside closes it; clicking an item is left
                    // to `run_menu_item`, which may open a submenu.
                    if let Some(Hit::MenuItem(i)) = released {
                        self.run_menu_item(i);
                    } else {
                        self.menu = None;
                    }
                    self.press = None;
                    self.next_repaint = Instant::now();
                    return Some(LRESULT(0));
                }
                // Clicking elsewhere while renaming commits it.
                if self.renaming.is_some() {
                    self.commit_rename();
                    self.press = None;
                    self.next_repaint = Instant::now();
                    return Some(LRESULT(0));
                }
                if self.dragging {
                    self.dragging = false;
                    self.drop(x, y);
                    self.drag_src = None;
                    self.next_repaint = Instant::now();
                    return Some(LRESULT(0));
                }
                match (self.press, released) {
                    // Clicked "move out" in the folder panel.
                    (Some(Hit::FolderOut(a)), Some(Hit::FolderOut(b))) if a == b => {
                        if let Some(fi) = self.open_folder {
                            if self.lp.take_from_folder(fi, a, TakeOut::ToTop) {
                                self.dirty_save = true;
                                if self.lp.folders()[fi].apps.len() <= 1 {
                                    self.open_folder = None;
                                }
                            }
                        }
                        self.next_repaint = Instant::now();
                    }
                    (Some(Hit::FolderCell(a)), Some(Hit::FolderCell(b))) if a == b => {
                        if let Some(fi) = self.open_folder {
                            if let Some(&ai) = self.lp.folders()[fi].apps.get(a) {
                                let path = self.lp.apps()[ai].path.clone();
                                nooksill_platform::sysact::open_uri(&path);
                                self.close();
                            }
                        }
                    }
                    (Some(Hit::Cell(a)), Some(Hit::Cell(b))) if a == b => {
                        self.activate(a);
                    }
                    (Some(Hit::Tab(a)), Some(Hit::Tab(b))) if a == b => {
                        let n = self.lp.tags().len();
                        if b <= n {
                            // Switching category resets the page and the
                            // selection, because the cell count changed.
                            self.cat = b;
                            self.page = 0;
                            self.sel = None;
                        } else if self.lp.add_tag("").is_some() {
                            // The plus adds a group with a placeholder name;
                            // right click renames it.
                            self.dirty_save = true;
                        }
                        self.next_repaint = Instant::now();
                    }
                    (Some(Hit::SortAz), Some(Hit::SortAz)) => {
                        self.lp.sort_az();
                        self.dirty_save = true;
                        self.next_repaint = Instant::now();
                    }
                    (Some(Hit::Plus), Some(Hit::Plus)) => {
                        // The plus used to open an "add app / folder / URL"
                        // menu; adding is done by dragging files in now, so
                        // the button only prints a hint. A button that does
                        // nothing is more confusing than no button.
                        eprintln!("[launchpad] 把文件、快捷方式或文件夹拖进窗口即可添加");
                    }
                    _ => {
                        // Clicking empty space closes the launchpad, the
                        // usual behaviour of an overlay.
                        if released.is_none() && self.open_folder.is_none() {
                            self.close();
                        } else if released.is_none() {
                            self.open_folder = None;
                            self.next_repaint = Instant::now();
                        }
                    }
                }
                self.press = None;
                Some(LRESULT(0))
            }
            WM_RBUTTONUP => {
                let (x, y) = client_pos(lparam, self.ppp);
                // Right click in the grid opens the menu of that cell.
                if std::env::var("NOOKSILL_LP_TRACE").is_ok() {
                    eprintln!(
                        "[lp] RBUTTONUP ({x},{y}) hit={:?} hits={}",
                        self.hit_at(x, y),
                        self.hits.len()
                    );
                }
                if self.open_folder.is_none() {
                    // Right click on a tab opens that group's menu (rename /
                    // delete). The first tab is "all" and the last one is the
                    // plus; neither has a menu.
                    if let Some(Hit::Tab(t)) = self.hit_at(x, y) {
                        if t >= 1 && t <= self.lp.tags().len() {
                            self.menu = Some(MenuState::Tag(t - 1));
                            self.menu_at = (x.min(self.size.0 - 200.0).max(8.0), y);
                            self.next_repaint = Instant::now();
                            return Some(LRESULT(0));
                        }
                    }
                    if let Some(Hit::Cell(k)) = self.hit_at(x, y) {
                        let idx = self.page * self.per() + k;
                        // In the filtered view a cell is an app; otherwise it
                        // is a top level entry.
                        if self.filtered() {
                            if let Some(&ai) = self.shown().get(idx) {
                                self.menu = Some(MenuState::App(ai));
                                self.menu_at = (x.min(self.size.0 - 170.0), y);
                                self.next_repaint = Instant::now();
                                return Some(LRESULT(0));
                            }
                        } else if idx < self.lp.len() {
                            self.menu = Some(MenuState::Cell(idx));
                            self.menu_at = (x.min(self.size.0 - 170.0), y);
                            self.next_repaint = Instant::now();
                            return Some(LRESULT(0));
                        }
                    }
                }
                // Right click in the folder panel takes that item out of the
                // folder.
                if let (Some(fi), Some(Hit::FolderCell(k))) = (self.open_folder, self.hit_at(x, y))
                {
                    if self.lp.take_from_folder(fi, k, TakeOut::ToTop) {
                        self.dirty_save = true;
                        if self.lp.folders()[fi].apps.len() <= 1 {
                            self.open_folder = None;
                        }
                        self.next_repaint = Instant::now();
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
                if delta > 0 {
                    self.turn_page(-1);
                } else if delta < 0 {
                    self.turn_page(1);
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
                // Arrow keys and Enter belong to the grid only when no
                // overlay is open; the menu, the rename field and the folder
                // panel keep their own key semantics.
                //
                // A chain of `if` instead of `match`: unmatched keys (Esc
                // among them) have to fall through to the branch below, and
                // `_ => return None` in a `match` would swallow Esc.
                if self.menu.is_none() && self.renaming.is_none() && self.open_folder.is_none() {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        VK_DOWN, VK_END, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
                        VK_UP,
                    };
                    let vk = wparam.0 as u16;
                    if vk == VK_LEFT.0 {
                        self.move_sel(-1, 0);
                        return Some(LRESULT(0));
                    } else if vk == VK_RIGHT.0 {
                        self.move_sel(1, 0);
                        return Some(LRESULT(0));
                    } else if vk == VK_UP.0 {
                        self.move_sel(0, -1);
                        return Some(LRESULT(0));
                    } else if vk == VK_DOWN.0 {
                        self.move_sel(0, 1);
                        return Some(LRESULT(0));
                    } else if vk == VK_HOME.0 {
                        self.sel = Some(0);
                        self.next_repaint = Instant::now();
                        return Some(LRESULT(0));
                    } else if vk == VK_END.0 {
                        self.sel = Some(self.page_len().saturating_sub(1));
                        self.next_repaint = Instant::now();
                        return Some(LRESULT(0));
                    } else if vk == VK_PRIOR.0 {
                        self.turn_page(-1);
                        return Some(LRESULT(0));
                    } else if vk == VK_NEXT.0 {
                        self.turn_page(1);
                        return Some(LRESULT(0));
                    } else if vk == VK_RETURN.0 {
                        self.enter_sel();
                        return Some(LRESULT(0));
                    }
                }
                if wparam.0 == VK_ESCAPE.0 as usize {
                    // Back out one layer at a time: menu, rename, folder,
                    // window.
                    if self.menu.is_some() {
                        self.menu = None;
                        self.next_repaint = Instant::now();
                        return Some(LRESULT(0));
                    }
                    if self.renaming.is_some() {
                        self.renaming = None;
                        self.rename_buf.clear();
                        self.next_repaint = Instant::now();
                        return Some(LRESULT(0));
                    }
                    if self.open_folder.is_some() {
                        self.open_folder = None;
                        self.next_repaint = Instant::now();
                    } else {
                        self.close();
                    }
                }
                None
            }
            WM_CHAR => {
                let ch = wparam.0 as u32;
                // While renaming the keyboard only edits the name, never the
                // search box.
                if self.renaming.is_some() {
                    match char::from_u32(ch) {
                        Some('\r') | Some('\n') => self.commit_rename(),
                        Some('\u{1b}') => {
                            self.renaming = None;
                            self.rename_buf.clear();
                        }
                        Some('\u{8}') => {
                            self.rename_buf.pop();
                        }
                        Some(c) if !c.is_control() && self.rename_buf.chars().count() < 40 => {
                            self.rename_buf.push(c);
                        }
                        _ => {}
                    }
                    self.next_repaint = Instant::now();
                    return Some(LRESULT(0));
                }
                match char::from_u32(ch) {
                    Some('\u{8}') => {
                        self.query.pop();
                    }
                    Some('\u{1b}') => {}
                    Some(c) if !c.is_control() && self.query.chars().count() < 32 => {
                        self.query.push(c);
                    }
                    _ => {}
                }
                self.page = 0;
                self.open_folder = None;
                // The filtered result changed, so the old selection no longer
                // points at the same app.
                self.sel = None;
                self.next_repaint = Instant::now();
                None
            }
            WM_DROPFILES => {
                let hdrop = HDROP(wparam.0 as *mut core::ffi::c_void);
                let n = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
                let mut files = Vec::new();
                for i in 0..n {
                    let len = unsafe { DragQueryFileW(hdrop, i, None) };
                    let mut buf = vec![0_u16; len as usize + 1];
                    let got = unsafe { DragQueryFileW(hdrop, i, Some(buf.as_mut_slice())) };
                    if got > 0 {
                        buf.truncate(got as usize);
                        files.push(String::from_utf16_lossy(&buf));
                    }
                }
                unsafe {
                    DragFinish(hdrop);
                }
                self.add_paths(files);
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
