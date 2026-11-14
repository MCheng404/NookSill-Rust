//! Applies commands coming from the settings window.

use super::misc::{apply_low_power, apply_recommended, url_host};
use super::*;

impl App {
    /// Scan the icon folder for every item and build the preview rows.
    ///
    /// Read-only: nothing is written to the items, so the preview can be
    /// refreshed as often as the user likes.
    fn load_icon_library(&mut self) {
        let Some(sw) = self.settings_slot.as_ref() else {
            return;
        };
        let dir = sw.state.icon_dir.clone();
        if dir.is_empty() {
            return;
        }
        let ctx = sw.ctx().clone();
        let names: Vec<String> = self.raw.iter().map(|i| i.name.to_string()).collect();
        const EXTS: &[&str] = &["ico", "png", "jpg", "jpeg", "bmp", "gif", "webp"];
        // Decoding is synchronous on the UI thread; cap the batch so a huge
        // bar cannot freeze Settings for seconds.
        const MAX_PREVIEW: usize = 80;
        let mut rows = Vec::with_capacity(names.len().min(MAX_PREVIEW));
        for name in names.into_iter().take(MAX_PREVIEW) {
            let found = EXTS.iter().find_map(|ext| {
                let candidate = std::path::Path::new(&dir).join(format!("{name}.{ext}"));
                candidate.is_file().then_some(candidate)
            });
            let Some(path) = found else {
                rows.push(nooksill_settings::LibIcon {
                    name,
                    path: String::new(),
                    tex: None,
                });
                continue;
            };
            let shown = path.display().to_string();
            let is_ico = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ico"));
            let bitmap = if is_ico {
                nooksill_platform::icon::resource_icon(&shown, 0)
            } else {
                nooksill_platform::image_icon::decode_image_icon(&shown)
            };
            let tex = bitmap.map(|b| {
                // 64 px covers the 36 px tile; a 256 px .ico frame or a large
                // JPEG would otherwise cost a full texture each.
                let b = nooksill_platform::icon::scale_to(b, 64);
                ctx.load_texture(
                    format!("library:{shown}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [b.width as usize, b.height as usize],
                        &b.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
            rows.push(nooksill_settings::LibIcon {
                name,
                path: shown,
                tex,
            });
        }
        if let Some(sw) = self.settings_slot.as_mut() {
            sw.state.icon_library = rows;
            sw.wake();
        }
    }

    pub(super) fn run_settings_cmd(&mut self, cmd: Command) -> bool {
        match cmd {
            // Closing is handled in `tick_settings`; the window must be hidden.
            Command::Close => false,
            Command::OpenConfigDir => {
                let dir = self.paths.dir.display().to_string();
                if let Err(e) = launch::open_folder(&dir) {
                    self.status(nooksill_core::i18n::fmt(
                        "Could not open the config folder: {0}",
                        &[e.to_string().as_str()],
                    ));
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
                        self.status(nooksill_core::i18n::t("Moved out of group"));
                    }
                    Err(e) => self.status(nooksill_core::i18n::fmt(
                        "Could not move out: {0}",
                        &[e.to_string().as_str()],
                    )),
                }
                false
            }
            Command::DissolveGroup(index) => {
                match group::dissolve_item(&mut self.raw, index) {
                    Ok(_) => {
                        self.apply_item_change();
                        self.status(nooksill_core::i18n::t(
                            "Group dissolved; children returned to the bar",
                        ));
                    }
                    Err(e) => self.status(nooksill_core::i18n::fmt(
                        "Could not dissolve: {0}",
                        &[e.to_string().as_str()],
                    )),
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
                self.raw.push(Item::new(
                    ItemKind::Note,
                    nooksill_core::i18n::t("Notes"),
                    "nooksill:notes",
                ));
                self.apply_item_change();
                self.status(nooksill_core::i18n::t("Notes item added"));
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
                        nooksill_core::i18n::t("Autostart enabled")
                    } else {
                        nooksill_core::i18n::t("Autostart disabled")
                    };
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.auto_start = real;
                        sw.state.status = text.to_owned();
                    }
                } else {
                    let err = theme::last_registry_error();
                    let real = theme::get_autostart();
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.status = nooksill_core::i18n::fmt(
                            "Registry write failed (code {0})",
                            &[err.to_string().as_str()],
                        );
                        sw.state.auto_start = real;
                    }
                }
                false
            }
            Command::ResetDefaults => {
                self.dock.cfg = Config::defaults();
                self.status(nooksill_core::i18n::t("Defaults restored"));
                true
            }
            Command::Recommended => {
                apply_recommended(&mut self.dock.cfg);
                self.status(nooksill_core::i18n::t(
                    "Recommended settings applied (size and edge unchanged)",
                ));
                true
            }
            Command::LowPower => {
                apply_low_power(&mut self.dock.cfg);
                self.status(nooksill_core::i18n::t("Costly visual effects disabled"));
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
                self.raw.push(Item::new(
                    ItemKind::Trash,
                    nooksill_core::i18n::t("Recycle Bin"),
                    "",
                ));
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
                    self.status(nooksill_core::i18n::t("URL must start with http:"));
                    return false;
                }
                let name = url_host(&url);
                self.raw.push(Item::new(ItemKind::Url, &name, url.as_str()));
                if let Some(sw) = self.settings_slot.as_mut() {
                    sw.state.url_input.clear();
                }
                self.apply_item_change();
                self.status(nooksill_core::i18n::fmt(
                    "Added {0}",
                    &[name.to_string().as_str()],
                ));
                false
            }
            Command::AddFile => {
                let owner = self.settings_hwnd();
                let filters = [
                    (nooksill_core::i18n::t("Apps and shortcuts"), "*.exe;*.lnk"),
                    (nooksill_core::i18n::t("All files"), "*.*"),
                ];
                if let Some(path) = nooksill_platform::filedialog::open_file(
                    nooksill_core::i18n::t("Add app or file"),
                    owner,
                    &filters,
                ) {
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| nooksill_core::i18n::t("New item").to_owned());
                    self.raw
                        .push(Item::new(ItemKind::File, &name, path.to_string_lossy()));
                    self.apply_item_change();
                    self.status(nooksill_core::i18n::fmt(
                        "Added {0}",
                        &[name.to_string().as_str()],
                    ));
                }
                false
            }
            Command::AddFolder => {
                let owner = self.settings_hwnd();
                if let Some(path) = nooksill_platform::filedialog::open_folder(
                    nooksill_core::i18n::t("Add folder…"),
                    owner,
                ) {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| nooksill_core::i18n::t("Untitled folder").to_owned());
                    self.raw
                        .push(Item::new(ItemKind::Folder, &name, path.to_string_lossy()));
                    self.apply_item_change();
                    self.status(nooksill_core::i18n::fmt(
                        "Added {0}",
                        &[name.to_string().as_str()],
                    ));
                }
                false
            }
            Command::NewGroup => {
                self.raw.push(Item::new(
                    ItemKind::Group,
                    nooksill_core::i18n::t("New group"),
                    "",
                ));
                self.apply_item_change();
                self.status(nooksill_core::i18n::t(
                    "Group created; drop icons onto it to add them",
                ));
                false
            }
            Command::PickIconDir => {
                let owner = self.settings_hwnd();
                if let Some(dir) = nooksill_platform::filedialog::open_folder(
                    nooksill_core::i18n::t("Choose icon folder"),
                    owner,
                ) {
                    let text = dir.display().to_string();
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.icon_dir = text.clone();
                    }
                    self.status(nooksill_core::i18n::fmt(
                        "Icon folder: {0}",
                        &[text.to_string().as_str()],
                    ));
                }
                false
            }
            Command::LoadIconLibrary => {
                self.load_icon_library();
                false
            }
            Command::UseIcon { index, path } => {
                if index < self.raw.len() {
                    self.raw[index].icon_src = path;
                    // Same invalidation as the picker: the texture cache is
                    // keyed by path, so the old icon would stick otherwise.
                    self.dock.clear_icon(index);
                    self.icons.forget(index, &self.raw[index].path);
                    self.apply_item_change();
                    let name = self.raw[index].name.to_string();
                    self.status(nooksill_core::i18n::fmt(
                        "Icon changed for {0}",
                        &[name.as_str()],
                    ));
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
                    self.status(nooksill_core::i18n::t("Choose an icon folder first"));
                    return false;
                }
                // `.ico` first for backwards compatibility, then the still
                // image formats WIC decodes.
                const ICON_EXTS: &[&str] = &["ico", "png", "jpg", "jpeg", "bmp", "gif", "webp"];
                let mut matched = 0usize;
                for item in &mut self.raw {
                    if !item.icon_src.is_empty() || item.kind == ItemKind::Separator {
                        continue;
                    }
                    let found = ICON_EXTS.iter().find_map(|ext| {
                        let candidate =
                            std::path::Path::new(&dir).join(format!("{}.{ext}", item.name));
                        candidate.is_file().then_some(candidate)
                    });
                    if let Some(path) = found {
                        item.icon_src = path.display().to_string();
                        matched += 1;
                    }
                }
                if matched == 0 {
                    self.status(nooksill_core::i18n::t("No matching icon found"));
                    return false;
                }
                // The icon source changed; without clearing the cache
                // `replace_items` keeps the old icons by path.
                self.dock.clear_icons();
                self.apply_item_change();
                self.status(nooksill_core::i18n::fmt(
                    "Matched {0} icons",
                    &[matched.to_string().as_str()],
                ));
                false
            }
            Command::ExportAll => {
                let owner = self.settings_hwnd();
                if let Some(path) = nooksill_platform::filedialog::save_file(
                    nooksill_core::i18n::t("Export full config"),
                    owner,
                    "NookSill.ini",
                ) {
                    // Start from the on-disk ini to keep unknown keys; items and
                    // settings overwrite on top.
                    let mut ini =
                        nooksill_core::ini::Ini::load(&self.paths.ini).unwrap_or_default();
                    store::save_items(&self.raw, &mut ini);
                    self.dock.cfg.write_ini(&mut ini);
                    match ini.save(&path) {
                        Ok(()) => self.status(nooksill_core::i18n::fmt(
                            "Exported to {0}",
                            &[path.display().to_string().as_str()],
                        )),
                        Err(e) => self.status(nooksill_core::i18n::fmt(
                            "Export failed: {0}",
                            &[e.to_string().as_str()],
                        )),
                    }
                }
                false
            }
            Command::Import => {
                let owner = self.settings_hwnd();
                let filters = [
                    (nooksill_core::i18n::t("NookSill config"), "*.ini"),
                    (nooksill_core::i18n::t("All files"), "*.*"),
                ];
                if let Some(path) = nooksill_platform::filedialog::open_file(
                    nooksill_core::i18n::t("Import config"),
                    owner,
                    &filters,
                ) {
                    match nooksill_core::ini::Ini::load(&path) {
                        Ok(ini) => {
                            self.dock.cfg = Config::from_ini(&ini);
                            self.raw = store::load_items(&ini);
                            self.dock.clear_icons();
                            self.apply_item_change();
                            self.touch_cfg();
                            self.status(nooksill_core::i18n::fmt(
                                "Imported {0}",
                                &[path.display().to_string().as_str()],
                            ));
                        }
                        Err(e) => self.status(nooksill_core::i18n::fmt(
                            "Import failed: {0}",
                            &[e.to_string().as_str()],
                        )),
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
