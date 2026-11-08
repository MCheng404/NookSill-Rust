//! Notes and group panels: open, tick and edit.

use super::*;

impl App {
    /// Open the notes panel. `index` is the dock item; `None` (tray or context
    /// menu) anchors at the cursor instead.
    pub(super) fn open_notes(&mut self, index: Option<usize>) {
        let anchor = index
            .and_then(|i| self.dock.slot_screen_rect(i, self.pos.0, self.pos.1))
            .unwrap_or_else(|| {
                let mut pt = POINT::default();
                let _ = unsafe { GetCursorPos(&raw mut pt) };
                (pt.x, pt.y, pt.x, pt.y)
            });
        let dark = self
            .dock
            .cfg
            .dock_is_dark(nooksill_platform::theme::is_dark());
        if self.note_slot.is_none() {
            match NoteWindow::new(&self.gpu, self.ppp, dark) {
                Ok(w) => self.note_slot = Some(w),
                Err(e) => {
                    eprintln!("速记面板创建失败：{e}");
                    return;
                }
            }
        }
        if let Some(w) = self.note_slot.as_mut() {
            w.show(&self.notes, anchor);
        }
    }

    /// Notes panel: render a frame and handle its action.
    pub(super) fn step_notes(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(w) = self.note_slot.as_mut() else {
            return Ok(false);
        };
        let mut need_frame = false;
        if w.due() {
            w.render(&self.gpu)?;
            need_frame = true;
        }
        match w.take_action() {
            None | Some(NoteAction::Page(_)) => {}
            Some(NoteAction::New) => self.edit_note(None),
            Some(NoteAction::Edit(k)) => self.edit_note(Some(k)),
            Some(NoteAction::Remove(k)) => self.remove_note(k),
        }
        Ok(need_frame)
    }

    /// Create or edit a note. Text entry uses `edit_win` for multiline + IME.
    pub(super) fn edit_note(&mut self, index: Option<usize>) {
        let (title, body) = match index {
            Some(k) => match self.notes.notes.get(k) {
                Some(n) => (n.title.clone(), n.body.clone()),
                None => return,
            },
            None => (String::new(), String::new()),
        };
        let fields = vec![
            EditField {
                label: "标题".into(),
                value: title,
                hint: "留空则取正文第一行".into(),
                multiline: false,
            },
            EditField {
                label: "正文".into(),
                value: body,
                hint: "想写什么就写什么，回车换行".into(),
                multiline: true,
            },
        ];
        let Some(out) = self.edit_dialog("速记", fields, "保存") else {
            return;
        };
        if !out.accepted {
            return;
        }
        let title = out.value(0);
        let body = out.value(1);
        if title.trim().is_empty() && body.trim().is_empty() {
            self.status("空白速记没有保存");
            return;
        }
        let ok = match index {
            Some(k) => self.notes.update(k, title, body),
            None => self.notes.add(title, body).is_some(),
        };
        if !ok {
            self.status("速记已达上限或下标失效");
            return;
        }
        self.touch_notes();
        if let Some(w) = self.note_slot.as_mut() {
            w.set_notes(&self.notes);
        }
        self.status("已保存速记");
    }

    /// Remove a note, confirming through the edit dialog (0 fields = plain
    /// confirmation) to prevent accidental loss.
    pub(super) fn remove_note(&mut self, index: usize) {
        let Some(n) = self.notes.notes.get(index) else {
            return;
        };
        let label = n.display_title();
        let out = self.edit_dialog(&format!("删除「{label}」？"), Vec::new(), "删除");
        if !out.is_some_and(|o| o.accepted) {
            return;
        }
        if !self.notes.remove(index) {
            return;
        }
        self.touch_notes();
        if let Some(w) = self.note_slot.as_mut() {
            w.set_notes(&self.notes);
        }
        self.status(format!("已删除「{label}」"));
    }

    /// Mark `[Notes]` dirty; shares the debounce window with config and items.
    pub(super) fn touch_notes(&mut self) {
        self.dirty_notes = Some(Box::new(self.notes.clone()));
        self.dirty_at.get_or_insert_with(Instant::now);
    }

    /// Open a group: pop up its contents.
    pub(super) fn open_group(&mut self, index: usize) {
        let Some(item) = self.raw.get(index) else {
            return;
        };
        if item.kind != ItemKind::Group {
            return;
        }
        let kids: Vec<GroupKid> = item
            .kids
            .iter()
            .map(|k| GroupKid {
                name: if k.name.is_empty() {
                    k.path.clone()
                } else {
                    k.name.clone()
                },
                path: k.path.clone(),
                kind: k.kind,
                running: k.running,
            })
            .collect();
        let name = if item.name.is_empty() {
            "分组".to_owned()
        } else {
            item.name.clone()
        };
        // Anchor: the group icon on screen; the panel pops up above it.
        let anchor = self
            .dock
            .slot_screen_rect(index, self.pos.0, self.pos.1)
            .unwrap_or((self.pos.0, self.pos.1, self.pos.0, self.pos.1));
        let dark = self
            .dock
            .cfg
            .dock_is_dark(nooksill_platform::theme::is_dark());
        if self.group_slot.is_none() {
            match GroupWindow::new(&self.gpu, self.ppp, dark) {
                Ok(w) => self.group_slot = Some(w),
                Err(e) => {
                    eprintln!("分组面板创建失败：{e}");
                    return;
                }
            }
        }
        if let Some(w) = self.group_slot.as_mut() {
            if let Err(e) = w.open(&self.gpu, &name, kids, anchor) {
                eprintln!("分组面板打开失败：{e}");
                return;
            }
        }
        self.group_index = Some(index);
    }

    /// Group panel: render a frame and handle the action it produced.
    pub(super) fn step_group(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(w) = self.group_slot.as_mut() else {
            return Ok(false);
        };
        let mut need_frame = false;
        if w.due() {
            w.render(&self.gpu)?;
            need_frame = true;
        }
        let action = w.take_action();
        match action {
            GroupAction::None => {}
            GroupAction::Launch(k) => {
                // The panel shows child `k` of the group at `group_index`.
                let kid = self
                    .group_index
                    .and_then(|gi| self.raw.get(gi))
                    .and_then(|g| g.kids.get(k))
                    .cloned();
                if let Some(kid) = kid {
                    let toggle = self.dock.cfg.get_i32(Key::CLICK_TOGGLE) != 0;
                    self.opener.submit(OpenJob {
                        item: Box::new(kid),
                        force: false,
                        toggle,
                        browser: self.dock.cfg.get_i32(Key::BROWSER),
                    });
                    self.last_launch_ms = Some(self.now_ms());
                }
            }
            GroupAction::Remove(k) => {
                if let Some(gi) = self.group_index {
                    match group::extract_kid(&mut self.raw, gi, k) {
                        Ok(_) => {
                            self.apply_item_change();
                            self.status("已移出分组");
                            // Contents changed; reopen to stay in sync.
                            self.open_group(gi);
                        }
                        Err(e) => self.status(format!("移出失败：{e}")),
                    }
                }
            }
            GroupAction::Dissolve => {
                if let Some(gi) = self.group_index {
                    match group::dissolve_item(&mut self.raw, gi) {
                        Ok(_) => {
                            self.apply_item_change();
                            self.status("已解散分组");
                        }
                        Err(e) => self.status(format!("解散失败：{e}")),
                    }
                }
                if let Some(w) = self.group_slot.as_mut() {
                    w.hide();
                }
                self.group_index = None;
            }
            GroupAction::Move(k, delta) => {
                if let Some(gi) = self.group_index {
                    // Order changed; reopen to show the new arrangement.
                    if self.move_kid(gi, k, delta) {
                        self.open_group(gi);
                    }
                }
            }
        }
        Ok(need_frame)
    }
}
