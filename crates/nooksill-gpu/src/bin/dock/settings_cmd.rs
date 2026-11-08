//! Applies commands coming from the settings window.

use super::misc::{apply_low_power, apply_recommended, url_host};
use super::*;

impl App {
    pub(super) fn run_settings_cmd(&mut self, cmd: Command) -> bool {
        match cmd {
            // Closing is handled in `tick_settings`; the window must be hidden.
            Command::Close => false,
            Command::OpenConfigDir => {
                let dir = self.paths.dir.display().to_string();
                if let Err(e) = launch::open_folder(&dir) {
                    self.status(format!("打开配置目录失败：{e}"));
                }
                false
            }
            Command::OpenUrl(url) => {
                sysact::open_uri(url);
                false
            }
            // Commands from the Items page.
            Command::EditItem(index, edit) => {
                self.apply_item_edit(index, edit);
                false
            }
            Command::RemoveItem(index) => {
                if index < self.raw.len() {
                    self.raw.remove(index);
                    self.apply_item_change();
                }
                false
            }
            Command::MoveItem(index, delta) => {
                self.move_item(index, delta);
                false
            }
            Command::ExtractKid(group, kid) => {
                match group::extract_kid(&mut self.raw, group, kid) {
                    Ok(_) => {
                        self.apply_item_change();
                        self.status("已移出分组");
                    }
                    Err(e) => self.status(format!("移出失败：{e}")),
                }
                false
            }
            Command::DissolveGroup(index) => {
                match group::dissolve_item(&mut self.raw, index) {
                    Ok(_) => {
                        self.apply_item_change();
                        self.status("已解散分组，子项放回栏体");
                    }
                    Err(e) => self.status(format!("解散失败：{e}")),
                }
                false
            }
            Command::PickIcon(index) => {
                self.change_icon(index);
                false
            }
            Command::ResetIcon(index) => {
                self.reset_icon(index);
                false
            }
            Command::MoveKid(gi, ki, delta) => {
                self.move_kid(gi, ki, delta);
                false
            }
            Command::AddNote => {
                self.raw
                    .push(Item::new(ItemKind::Note, "速记", "nooksill:notes"));
                self.apply_item_change();
                self.status("已添加速记条目");
                false
            }
            Command::OpenNotes => {
                self.open_notes(None);
                false
            }
            Command::CheckUpdate => {
                self.start_update_check();
                false
            }
            Command::DownloadUpdate => {
                self.start_update_download();
                false
            }
            Command::ApplyUpdate => {
                self.apply_update();
                false
            }
            Command::ToggleAutostart(on) => {
                if theme::set_autostart(on) {
                    // Read the value back: a policy-blocked write can still
                    // report success.
                    let real = theme::get_autostart();
                    let text = if real {
                        "已设置开机自启动"
                    } else {
                        "已取消开机自启动"
                    };
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.auto_start = real;
                        sw.state.status = text.to_owned();
                    }
                } else {
                    let err = theme::last_registry_error();
                    let real = theme::get_autostart();
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.status = format!("写入注册表失败（错误码 {err}）");
                        sw.state.auto_start = real;
                    }
                }
                false
            }
            Command::ResetDefaults => {
                self.dock.cfg = Config::defaults();
                self.status("已恢复默认设置");
                true
            }
            Command::Recommended => {
                apply_recommended(&mut self.dock.cfg);
                self.status("已套用推荐设置（尺寸与停靠边没动）");
                true
            }
            Command::LowPower => {
                apply_low_power(&mut self.dock.cfg);
                self.status("已关闭高开销视觉效果");
                true
            }
            // Shares the right-click menu path: edit `raw`, swap the list,
            // schedule the write.
            Command::AddSeparator => {
                self.raw.push(Item::new(ItemKind::Separator, "", ""));
                self.apply_item_change();
                false
            }
            Command::AddTrash => {
                self.raw.push(Item::new(ItemKind::Trash, "回收站", ""));
                self.apply_item_change();
                false
            }
            Command::AddUrl => {
                let url = self
                    .settings_slot
                    .as_ref()
                    .map(|sw| sw.state.url_input.trim().to_owned())
                    .unwrap_or_default();
                if url.is_empty() {
                    return false;
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    self.status("网址要以 http:// 或 https:// 开头");
                    return false;
                }
                let name = url_host(&url);
                self.raw.push(Item::new(ItemKind::Url, &name, url.as_str()));
                if let Some(sw) = self.settings_slot.as_mut() {
                    sw.state.url_input.clear();
                }
                self.apply_item_change();
                self.status(format!("已添加 {name}"));
                false
            }
            Command::AddFile => {
                let owner = self.settings_hwnd();
                let filters = [("应用与快捷方式", "*.exe;*.lnk"), ("所有文件", "*.*")];
                if let Some(path) =
                    nooksill_platform::filedialog::open_file("添加应用或文件", owner, &filters)
                {
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "新条目".to_owned());
                    self.raw
                        .push(Item::new(ItemKind::File, &name, path.to_string_lossy()));
                    self.apply_item_change();
                    self.status(format!("已添加 {name}"));
                }
                false
            }
            Command::AddFolder => {
                let owner = self.settings_hwnd();
                if let Some(path) = nooksill_platform::filedialog::open_folder("添加文件夹", owner)
                {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "新文件夹".to_owned());
                    self.raw
                        .push(Item::new(ItemKind::Folder, &name, path.to_string_lossy()));
                    self.apply_item_change();
                    self.status(format!("已添加 {name}"));
                }
                false
            }
            Command::NewGroup => {
                self.raw.push(Item::new(ItemKind::Group, "新建分组", ""));
                self.apply_item_change();
                self.status("已新建分组：把图标拖进去即可归组");
                false
            }
            Command::PickIconDir => {
                let owner = self.settings_hwnd();
                if let Some(dir) =
                    nooksill_platform::filedialog::open_folder("选择图标文件夹", owner)
                {
                    let text = dir.display().to_string();
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.icon_dir = text.clone();
                    }
                    self.status(format!("图标文件夹：{text}"));
                }
                false
            }
            Command::AutoMatch => {
                let dir = self
                    .settings_slot
                    .as_ref()
                    .map(|sw| sw.state.icon_dir.clone())
                    .unwrap_or_default();
                if dir.is_empty() {
                    self.status("先选一个图标文件夹");
                    return false;
                }
                let mut matched = 0usize;
                for item in &mut self.raw {
                    if !item.icon_src.is_empty() || item.kind == ItemKind::Separator {
                        continue;
                    }
                    let candidate = std::path::Path::new(&dir).join(format!("{}.ico", item.name));
                    if candidate.is_file() {
                        item.icon_src = candidate.display().to_string();
                        matched += 1;
                    }
                }
                if matched == 0 {
                    self.status("没有找到同名 .ico");
                    return false;
                }
                // The icon source changed; without clearing the cache
                // `replace_items` keeps the old icons by path.
                self.dock.clear_icons();
                self.apply_item_change();
                self.status(format!("匹配到 {matched} 个图标"));
                false
            }
            Command::ExportAll => {
                let owner = self.settings_hwnd();
                if let Some(path) =
                    nooksill_platform::filedialog::save_file("导出完整配置", owner, "NookSill.ini")
                {
                    // Start from the on-disk ini to keep unknown keys; items and
                    // settings overwrite on top.
                    let mut ini =
                        nooksill_core::ini::Ini::load(&self.paths.ini).unwrap_or_default();
                    store::save_items(&self.raw, &mut ini);
                    self.dock.cfg.write_ini(&mut ini);
                    match ini.save(&path) {
                        Ok(()) => self.status(format!("已导出到 {}", path.display())),
                        Err(e) => self.status(format!("导出失败：{e}")),
                    }
                }
                false
            }
            Command::Import => {
                let owner = self.settings_hwnd();
                let filters = [("NookSill 配置", "*.ini"), ("所有文件", "*.*")];
                if let Some(path) =
                    nooksill_platform::filedialog::open_file("导入配置", owner, &filters)
                {
                    match nooksill_core::ini::Ini::load(&path) {
                        Ok(ini) => {
                            self.dock.cfg = Config::from_ini(&ini);
                            self.raw = store::load_items(&ini);
                            self.dock.clear_icons();
                            self.apply_item_change();
                            self.touch_cfg();
                            self.status(format!("已导入 {}", path.display()));
                        }
                        Err(e) => self.status(format!("导入失败：{e}")),
                    }
                }
                false
            }
        }
    }

    /// HWND of the settings window, used as the file dialog owner.
    ///
    /// `None` before the window exists: the dialog then parents to the desktop
    /// and still works, it just does not stay above the settings window.
    pub(super) fn settings_hwnd(&self) -> Option<isize> {
        self.settings_slot.as_ref().map(|sw| sw.hwnd().0 as isize)
    }

    /// Write a line to the settings status bar and show a notice on the dock.
    ///
    /// Before the settings window exists (most of the time) the status bar path
    /// is a silent no-op, and the dock notice is the feedback the user sees.
    pub(super) fn status(&mut self, text: impl Into<String>) {
        let text = text.into();
        if let Some(sw) = self.settings_slot.as_mut() {
            sw.state.status = text.clone();
        }
        self.dock.show_notice(text);
    }
}
