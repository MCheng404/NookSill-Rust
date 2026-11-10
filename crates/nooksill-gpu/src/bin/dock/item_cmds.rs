//! Item commands that open dialogs: rename, new group, new folder.

use super::menu_win::{frost_spec, show_menu};
use super::satellite::ensure_settings;
use super::*;

impl App {
    /// Rename an item and edit its target and arguments. Modal.
    pub(super) fn rename_item(&mut self, index: usize, item: &Item) {
        // Groups have only a name; two extra empty fields would look like
        // something was left out.
        let is_group = item.kind == ItemKind::Group;
        let mut fields = vec![EditField {
            label: nooksill_core::i18n::t("Name").into(),
            value: item.name.clone(),
            hint: nooksill_core::i18n::t("Shown in the hover bubble and group panel").into(),
            multiline: false,
        }];
        if !is_group {
            fields.push(EditField {
                label: nooksill_core::i18n::t("Target").into(),
                value: item.path.clone(),
                hint: match item.kind {
                    ItemKind::Url => "https://example.com".into(),
                    ItemKind::Trash => {
                        nooksill_core::i18n::t("The Recycle Bin needs no target").into()
                    }
                    _ => nooksill_core::i18n::t("Program path or folder path").into(),
                },
                multiline: false,
            });
            fields.push(EditField {
                label: nooksill_core::i18n::t("Arguments").into(),
                value: item.args.clone(),
                hint: nooksill_core::i18n::t("Extra command-line arguments (optional)").into(),
                multiline: false,
            });
        }
        let Some(out) = self.edit_dialog(
            nooksill_core::i18n::t("Edit item"),
            fields,
            nooksill_core::i18n::t("Save"),
        ) else {
            return;
        };
        if !out.accepted {
            return;
        }
        let name = out.value(0).trim().to_owned();
        let path = if is_group {
            item.path.clone()
        } else {
            out.value(1).trim().to_owned()
        };
        let args = if is_group {
            item.args.clone()
        } else {
            out.value(2).trim().to_owned()
        };
        let Some(target) = self.raw.get_mut(index) else {
            return;
        };
        // Touch only these three fields; `kids` must survive so renaming a
        // group does not empty it.
        target.name = name;
        target.path = path;
        target.args = args;
        self.apply_item_change();
        self.status(nooksill_core::i18n::t("Saved"));
    }

    /// Show the edit dialog; `None` means the user cancelled.
    pub(super) fn edit_dialog(
        &mut self,
        title: &str,
        fields: Vec<EditField>,
        ok_label: &str,
    ) -> Option<nooksill_gpu::edit_win::EditOutcome> {
        let mut pt = POINT::default();
        let _ = unsafe { GetCursorPos(&raw mut pt) };
        let cursor = (pt.x as f32 / self.ppp, pt.y as f32 / self.ppp);
        let w = self.work;
        let bounds = (
            w.left as f32 / self.ppp,
            w.top as f32 / self.ppp,
            w.right as f32 / self.ppp,
            w.bottom as f32 / self.ppp,
        );
        let dark = self
            .dock
            .cfg
            .dock_is_dark(nooksill_platform::theme::is_dark());
        match nooksill_gpu::edit_win::show_edit(
            &self.gpu,
            &mut self.edit_slot,
            EditRequest {
                title: title.to_owned(),
                fields,
                ok_label: ok_label.to_owned(),
            },
            cursor,
            bounds,
            dark,
            self.ppp,
        ) {
            Ok(out) => Some(out),
            Err(e) => {
                eprintln!("编辑弹窗失败：{e}");
                None
            }
        }
    }

    /// Create a new group (folder).
    pub(super) fn new_group(&mut self) {
        let fields = vec![EditField {
            label: nooksill_core::i18n::t("Group name").into(),
            value: nooksill_core::i18n::t("New group").into(),
            hint: nooksill_core::i18n::t("Name of this folder").into(),
            multiline: false,
        }];
        let Some(out) = self.edit_dialog(
            nooksill_core::i18n::t("New group"),
            fields,
            nooksill_core::i18n::t("Create"),
        ) else {
            return;
        };
        if !out.accepted {
            return;
        }
        let name = out.value(0).trim().to_owned();
        if name.is_empty() {
            self.status(nooksill_core::i18n::t("Group name cannot be empty"));
            return;
        }
        self.raw.push(group::new_group(&name));
        self.apply_item_change();
        self.status(nooksill_core::i18n::fmt(
            "Group {0} created",
            &[name.to_string().as_str()],
        ));
    }

    /// Create a folder on disk and add it to the dock.
    ///
    /// Two steps: pick a parent directory, then a name. A name alone cannot
    /// decide the location, and guessing one would hide the new folder.
    pub(super) fn new_folder(&mut self) {
        let owner = self.settings_hwnd();
        let Some(parent) = nooksill_platform::filedialog::open_folder(
            nooksill_core::i18n::t("Choose a location for the new folder"),
            owner,
        ) else {
            return;
        };
        let fields = vec![EditField {
            label: nooksill_core::i18n::t("Folder name").into(),
            value: nooksill_core::i18n::t("New folder").into(),
            hint: nooksill_core::i18n::t("Cannot contain path separators").into(),
            multiline: false,
        }];
        let Some(out) = self.edit_dialog(
            nooksill_core::i18n::t("New folder"),
            fields,
            nooksill_core::i18n::t("Create"),
        ) else {
            return;
        };
        if !out.accepted {
            return;
        }
        let name = out.value(0).trim().to_owned();
        match nooksill_platform::filedialog::create_dir(&parent, &name) {
            Ok(path) => {
                let label = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                self.raw
                    .push(Item::new(ItemKind::Folder, &label, path.to_string_lossy()));
                self.apply_item_change();
                self.status(nooksill_core::i18n::fmt(
                    "Folder {0} created",
                    &[label.to_string().as_str()],
                ));
            }
            Err(e) => self.status(e),
        }
    }

    /// Tray clicks: left toggles visibility, right opens the menu.
    ///
    /// The menu reuses the dock's self-drawn modal window instead of
    /// `TrackPopupMenu`, which requires a foreground window.
    pub(super) fn step_tray(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.dock.take_tray_toggle() {
            let hidden = self.dock.toggle_force_hide();
            self.status(if hidden {
                nooksill_core::i18n::t("Dock hidden (click the tray icon to restore)")
            } else {
                nooksill_core::i18n::t("Dock shown")
            });
        }
        if !self.dock.take_tray_menu() {
            return Ok(false);
        }
        let mut pt = POINT::default();
        let _ = unsafe { GetCursorPos(&raw mut pt) };
        let cursor = (pt.x as f32 / self.ppp, pt.y as f32 / self.ppp);
        let w = self.work;
        let bounds = (
            w.left as f32 / self.ppp,
            w.top as f32 / self.ppp,
            w.right as f32 / self.ppp,
            w.bottom as f32 / self.ppp,
        );
        let dark = match self.dock.cfg.get_i32(Key::MENU_THEME) {
            0 => false,
            1 => true,
            _ => self.dock.cfg.dock_is_dark(theme::is_dark()),
        };
        let menu = Menu::tray(self.dock.force_hide);
        let cmd = show_menu(
            &self.gpu,
            &mut self.menu_slot,
            menu,
            cursor,
            bounds,
            dark,
            self.ppp,
            self.start,
            frost_spec(&self.dock.cfg, Key::MENU_FROST, nooksill_gpu::menu::RADIUS),
        )
        .unwrap_or_else(|e| {
            eprintln!("托盘菜单显示失败：{e}");
            None
        });
        match cmd {
            Some(MenuCmd::ToggleDock) => {
                let hidden = self.dock.toggle_force_hide();
                self.status(if hidden {
                    nooksill_core::i18n::t("Dock hidden (click the tray icon to restore)")
                } else {
                    nooksill_core::i18n::t("Dock shown")
                });
            }
            Some(MenuCmd::Settings) => {
                let sw = ensure_settings(
                    &self.gpu,
                    &mut self.settings_slot,
                    &self.paths,
                    settings_win::effective_ppp(&self.dock.cfg),
                )?;
                sw.state.auto_start = theme::get_autostart();
                sw.show(frost_spec(&self.dock.cfg, Key::UI_FROST, 20.0));
            }
            Some(MenuCmd::Launchpad) => self.open_launchpad()?,
            Some(MenuCmd::ExitDock) => {
                // Normal exit path: `Drop` removes the tray icon and AppBar and
                // flushes config.
                unsafe { PostQuitMessage(0) };
            }
            _ => {}
        }
        self.dock.mark_dirty();
        Ok(true)
    }
}
