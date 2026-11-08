//! Context menu commands, rename commit and top-level activation.

use super::*;

impl LaunchpadWindow {
    /// Release: computes the drop target and merges or reorders.
    pub(super) fn drop(&mut self, x: f32, y: f32) {
        let Some(src) = self.drag_src else { return };
        // The drop target is only computed on the same page: after paging the
        // cells mean something else and the old index no longer matches.
        if let Some(Hit::Cell(k)) = self.hit_at(x, y) {
            if self.page == self.drag_page {
                if self.filtered() {
                    // In the filtered view a cell is an **app**, not a top
                    // level entry. The drop means "insert before this app"
                    // and never merges into a folder: two adjacent results
                    // need not be adjacent in the full list, so the folder
                    // would be impossible to find again.
                    let (si, ti) = (self.page * self.per() + src, self.page * self.per() + k);
                    let shown = self.shown();
                    if let (Some(&sa), Some(&ta)) = (shown.get(si), shown.get(ti)) {
                        if self.lp.move_app_before(sa, ta) {
                            self.dirty_save = true;
                        }
                    }
                } else {
                    let on_icon = self
                        .icon_rects
                        .iter()
                        .any(|(h, r)| *h == Hit::Cell(k) && r.contains(Pos2::new(x, y)));
                    if self.lp.drop_at(src, k, on_icon) {
                        self.dirty_save = true;
                    }
                }
            }
        }
        self.press = None;
    }

    /// The items the current menu should show.
    ///
    /// Three levels: a top level entry (app or folder), an app of the
    /// filtered view, and the tag group picker for an app.
    pub(super) fn menu_items(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        match self.menu {
            Some(MenuState::Cell(idx)) => match self.lp.top().get(idx) {
                Some(Top::Folder(_)) => {
                    out = vec!["打开".into(), "重命名…".into(), "解散文件夹".into()];
                }
                Some(Top::App(_)) => {
                    out = vec![
                        "打开".into(),
                        "重命名…".into(),
                        "移动到标签组…".into(),
                        "添加到 Dock 栏".into(),
                        "从起动台移除".into(),
                    ];
                }
                None => {}
            },
            Some(MenuState::App(_)) => {
                out = vec![
                    "打开".into(),
                    "移动到标签组…".into(),
                    "添加到 Dock 栏".into(),
                ];
            }
            Some(MenuState::Category(ai)) => {
                let cur = self.lp.category_of(ai);
                for (i, t) in self.lp.tags().iter().enumerate() {
                    // The current group is marked with a Chinese suffix rather
                    // than a checkmark: the font has no glyph for it and
                    // would draw a tofu box.
                    out.push(if i + 1 == cur {
                        format!("{t}（当前）")
                    } else {
                        t.to_string()
                    });
                }
                out.push("自动分类（清除）".into());
                out.push("（返回）".into());
            }
            Some(MenuState::Tag(_)) => {
                out.push("重命名…".into());
                // The last group is not offered for deletion: without it the
                // launchpad would have no category bar left.
                if self.lp.tags().len() > 1 {
                    out.push("删除这一组".into());
                }
            }
            None => {}
        }
        out
    }

    /// Runs the menu item at index `i`.
    pub(super) fn run_menu_item(&mut self, i: usize) {
        if std::env::var("NOOKSILL_LP_TRACE").is_ok() {
            eprintln!("[lp] 菜单项 {i}，作用于 {:?}", self.menu);
        }
        let Some(state) = self.menu else { return };
        let labels = self.menu_items();
        let Some(label) = labels.get(i).map(String::as_str) else {
            return;
        };
        // The menu of a tag group.
        if let MenuState::Tag(ti) = state {
            match label {
                "重命名…" => {
                    self.rename_buf = self.lp.tags().get(ti).cloned().unwrap_or_default();
                    self.renaming = Some(RenameTarget::Tag(ti));
                }
                // The guard is equivalent to the if body (a false guard falls
                // through to `_` as well), and `remove_tag` only runs once.
                "删除这一组" if self.lp.remove_tag(ti) => {
                    self.dirty_save = true;
                    // Category numbers start at 1, so removing group `ti+1`
                    // shifts the later ones down; if the removed group was
                    // selected the category falls back to "all", otherwise it
                    // would point at a neighbouring group.
                    if self.cat == ti + 1 {
                        self.cat = 0;
                    } else if self.cat > ti + 1 {
                        self.cat -= 1;
                    }
                    self.page = 0;
                    self.sel = None;
                }
                _ => {}
            }
            self.menu = None;
            self.next_repaint = Instant::now();
            return;
        }
        // Submenu: pick a tag group for the app.
        if let MenuState::Category(ai) = state {
            match label {
                "自动分类（清除）" => self.lp.set_category(ai, None),
                "（返回）" => {
                    self.menu = Some(MenuState::App(ai));
                    self.next_repaint = Instant::now();
                    return;
                }
                _ => {
                    // The current group may carry a CJK suffix, so strip it
                    // before comparing.
                    let bare = label.trim_end_matches("（当前）");
                    if let Some(pos) = self.lp.tags().iter().position(|t| t == bare) {
                        self.lp.set_category(ai, Some(pos + 1));
                    }
                }
            }
            self.dirty_save = true;
            self.menu = None;
            self.next_repaint = Instant::now();
            return;
        }
        // The menu of an app in the filtered view.
        if let MenuState::App(ai) = state {
            match label {
                "打开" => {
                    let path = self.lp.apps()[ai].path.clone();
                    nooksill_platform::sysact::open_uri(&path);
                    self.close();
                }
                "移动到标签组…" => self.menu = Some(MenuState::Category(ai)),
                "添加到 Dock 栏" => {
                    self.menu = None;
                    let app = &self.lp.apps()[ai];
                    self.cmds.push(LaunchpadCmd::AddToDock {
                        name: app.name.clone(),
                        path: app.path.clone(),
                    });
                }
                _ => {}
            }
            self.next_repaint = Instant::now();
            return;
        }
        let MenuState::Cell(idx) = state else {
            return;
        };
        match label {
            "打开" => self.activate_top(idx),
            "重命名…" => {
                self.rename_buf = self.lp.name_of(self.lp.top()[idx]).to_owned();
                self.renaming = Some(RenameTarget::Cell(idx));
            }
            "添加到 Dock 栏" => {
                if let Some(Top::App(ai)) = self.lp.top().get(idx).copied() {
                    let app = &self.lp.apps()[ai];
                    self.cmds.push(LaunchpadCmd::AddToDock {
                        name: app.name.clone(),
                        path: app.path.clone(),
                    });
                }
            }
            "移动到标签组…" => {
                if let Some(Top::App(ai)) = self.lp.top().get(idx).copied() {
                    self.menu = Some(MenuState::Category(ai));
                }
            }
            "从起动台移除" => {
                if self.lp.remove_top(idx) {
                    self.dirty_save = true;
                    self.clamp_page();
                }
            }
            "解散文件夹" => {
                let changed = self.lp.dissolve(idx);
                if changed {
                    self.dirty_save = true;
                    self.clamp_page();
                }
            }
            _ => {}
        }
        self.menu = None;
        self.next_repaint = Instant::now();
    }

    /// Commits a rename of a cell or a tag group.
    pub(super) fn commit_rename(&mut self) {
        if let Some(target) = self.renaming.take() {
            let name = self.rename_buf.trim().to_owned();
            // An empty name always cancels: `rename` and `set_tag` reject
            // it anyway, and keeping the buffer would make it look like the
            // rename succeeded.
            if !name.is_empty() {
                let ok = match target {
                    RenameTarget::Cell(idx) => idx < self.lp.len() && self.lp.rename(idx, &name),
                    RenameTarget::Tag(i) => self.lp.set_tag(i, &name),
                };
                if ok {
                    self.dirty_save = true;
                }
            }
        }
        self.rename_buf.clear();
        self.next_repaint = Instant::now();
    }

    /// Opens the top level entry at `idx` (not affected by paging).
    pub(super) fn activate_top(&mut self, idx: usize) {
        match self.lp.top().get(idx).copied() {
            Some(Top::App(i)) => {
                let path = self.lp.apps()[i].path.clone();
                nooksill_platform::sysact::open_uri(&path);
                self.close();
            }
            Some(Top::Folder(f)) => {
                self.open_folder = Some(f);
                self.next_repaint = Instant::now();
            }
            None => {}
        }
    }
}
