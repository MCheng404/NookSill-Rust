//! Pointer, tray and menu command handling.

use super::menu_win::{frost_spec, show_menu};
use super::satellite::{ensure_launchpad, ensure_settings, tick_settings};
use super::*;

impl App {
    /// Open the launchpad; the menu entry and the `nooksill:launchpad` item
    /// share this path.
    pub(super) fn open_launchpad(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Follow the config theme (`UiTheme` may force light), not the system.
        let dark = self
            .dock
            .cfg
            .dock_is_dark(nooksill_platform::theme::is_dark());
        let lw = ensure_launchpad(
            &self.gpu,
            &mut self.launchpad_slot,
            &self.paths,
            self.ppp,
            dark,
        )?;
        // Grid size follows the config on every open; settings may have changed.
        lw.set_grid(
            self.dock.cfg.get_i32(Key::LP_COLS).max(3) as usize,
            self.dock.cfg.get_i32(Key::LP_ROWS).max(2) as usize,
        );
        lw.open(frost_spec(&self.dock.cfg, Key::LP_FROST, 0.0));
        Ok(())
    }

    /// Handle a click. Runs in the main loop, not in the window proc:
    /// `ShellExecuteW` can block for tens of milliseconds.
    pub(super) fn step_click(&mut self) {
        let Some((i, force)) = self.dock.take_click() else {
            return;
        };
        if trace::geom() {
            eprintln!(
                "[click] 第 {i} 项 force={force} mouse={:?} slot_l={:?}",
                self.dock.mouse, self.dock.geom.slot_l
            );
        }
        // An item click switches apps, so panels step aside (the card path does
        // not come through here).
        self.hide_panels();
        // A mismatch between icon count and config must stay visible rather
        // than being swallowed.
        let Some(item) = self.item_at(i) else {
            eprintln!("点击了第 {i} 项，但配置里没有对应条目");
            return;
        };

        // Groups and notes are folders/panels, not programs to launch; without
        // these branches they fall into launch's `Unsupported` case.
        if item.kind == ItemKind::Group {
            self.open_group(i);
            self.dock.start_bounce(i);
            return;
        }
        if item.kind == ItemKind::Note {
            self.open_notes(Some(i));
            self.dock.start_bounce(i);
            return;
        }

        // Built-in entries are not files: `ShellExecute` fails on `nooksill:`
        // paths and the failure is invisible to the user.
        if let Some(kind) = nooksill_gpu::glyph::Builtin::from_path(&item.path) {
            match kind {
                nooksill_gpu::glyph::Builtin::Launchpad => {
                    if let Err(e) = self.open_launchpad() {
                        eprintln!("打开起动台失败：{e}");
                    }
                }
                nooksill_gpu::glyph::Builtin::Start => nooksill_platform::sysact::open_start(),
                // Unreachable: `ItemKind::Note` returned above. Kept as a
                // guard against future built-in path changes.
                nooksill_gpu::glyph::Builtin::Notes => self.open_notes(Some(i)),
                // `from_path` only recognizes the three built-in paths above;
                // the other `Builtin` variants only pick fallback glyphs.
                _ => {}
            }
            self.dock.start_bounce(i);
            return;
        }

        // `force` (Shift+click or middle click) starts a new instance and also
        // skips activation, otherwise a second instance never opens.
        let toggle = self.dock.cfg.get_i32(Key::CLICK_TOGGLE) != 0;
        // Hand off to a worker: cold `ShellExecuteW` takes tens of milliseconds
        // and activation enumerates top-level windows.
        self.opener.submit(OpenJob {
            item: Box::new(item.clone()),
            force,
            toggle,
            browser: self.dock.cfg.get_i32(Key::BROWSER),
        });
        self.last_launch_ms = Some(self.now_ms());
        // Bounce only when an action really happened; failures stay still.
        self.dock.start_bounce(i);
    }

    /// Context menu. Modal: blocks until the user picks an entry or cancels.
    pub(super) fn step_menu(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let Some((u, v)) = self.dock.take_menu_request() else {
            return Ok(false);
        };
        let hit = self.dock.hit_at(u, v);
        let target = hit.and_then(|i| self.item_at(i));
        let running = target.is_some_and(|it| it.running);
        // Transient items (running but unpinned) get no structural commands:
        // they are not config, and those commands index into `raw`, so a
        // transient index would edit the wrong entry.
        let temp = hit.is_some_and(|i| i >= self.raw.len());
        // `MnAuto` / `MnAdd` / `MnNotes` decide which entries appear. Autostart
        // is read live from the registry because the user may have changed it
        // in Task Manager.
        let opts = nooksill_gpu::menu::MenuOpts {
            autostart: (self.dock.cfg.get_i32(Key::MN_AUTO) != 0).then(theme::get_autostart),
            add_items: self.dock.cfg.get_i32(Key::MN_ADD) != 0,
            notes: self.dock.cfg.get_i32(Key::MN_NOTES) != 0,
        };
        let Some(menu) = Menu::build(target, running, temp, opts) else {
            return Ok(false);
        };

        // The cursor picks the popup point; the work area decides which way to
        // flip. Use the work area, not the full screen, so the menu never
        // covers the taskbar.
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
        // `MenuTheme`: 0 light, 1 dark, 2 follow the dock theme.
        let dark = match self.dock.cfg.get_i32(Key::MENU_THEME) {
            0 => false,
            1 => true,
            _ => self.dock.cfg.dock_is_dark(theme::is_dark()),
        };
        let cmd = match show_menu(
            &self.gpu,
            &mut self.menu_slot,
            menu,
            cursor,
            bounds,
            dark,
            self.ppp,
            self.start,
            frost_spec(&self.dock.cfg, Key::MENU_FROST, nooksill_gpu::menu::RADIUS),
        ) {
            Ok(cmd) => cmd,
            Err(e) => {
                eprintln!("菜单显示失败：{e}");
                None
            }
        };

        if let Some(cmd) = cmd {
            if trace::geom() {
                eprintln!(
                    "[menu] 命中 hit={hit:?} name={:?} cmd={cmd:?}",
                    hit.and_then(|i| self.item_at(i)).map(|it| it.name.clone())
                );
            }
            if cmd == MenuCmd::Launchpad {
                self.open_launchpad()?;
            } else if cmd == MenuCmd::ToggleAutostart {
                // Same entry point as the settings page: read the value back,
                // since a policy-blocked write can still report success.
                let want = !theme::get_autostart();
                if theme::set_autostart(want) {
                    let real = theme::get_autostart();
                    if let Some(sw) = self.settings_slot.as_mut() {
                        sw.state.auto_start = real;
                    }
                    self.status(if real {
                        nooksill_core::i18n::t("Autostart enabled")
                    } else {
                        nooksill_core::i18n::t("Autostart disabled")
                    });
                }
            } else if matches!(cmd, MenuCmd::AddFile | MenuCmd::AddFolder) {
                // Reuse the settings path, which owns the file dialogs and
                // `apply_item_change`.
                let add = if cmd == MenuCmd::AddFile {
                    Command::AddFile
                } else {
                    Command::AddFolder
                };
                self.run_settings_cmd(add);
            } else if cmd == MenuCmd::NewGroup {
                self.new_group();
            } else if cmd == MenuCmd::NewFolder {
                self.new_folder();
            } else if cmd == MenuCmd::Notes {
                self.open_notes(None);
            } else if cmd == MenuCmd::Settings {
                // The settings window activates another HWND and needs `gpu`,
                // so it cannot live in `run_menu_cmd`. Effective scale comes
                // from the config so a scale change applies on the next open.
                let sw = ensure_settings(
                    &self.gpu,
                    &mut self.settings_slot,
                    &self.paths,
                    settings_win::effective_ppp(&self.dock.cfg),
                )?;
                // Re-read every time; the user may have changed it in Task Manager.
                sw.state.auto_start = theme::get_autostart();
                sw.show(frost_spec(&self.dock.cfg, Key::UI_FROST, 20.0));
            } else {
                self.run_menu_cmd(cmd, hit);
            }
        }

        // The dock did not repaint while the menu was up; force a frame and
        // drop the stale hover (the cursor is likely off the icon by now).
        self.dock.mark_dirty();
        self.dock.hover = false;
        self.dock.mouse = (-1.0, -1.0);
        Ok(true)
    }

    /// Settings window. Non-modal: sliders must show live dock changes, so the
    /// message loop keeps running.
    pub(super) fn step_settings(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        // Snapshot before running the command: `ResetDefaults` also resets the
        // dock edge.
        let before = self.mount_key();

        let (changed, cmd) = tick_settings(
            &self.gpu,
            &mut self.settings_slot,
            &mut self.dock,
            &self.raw,
        )?;
        // Drain background update results; the worker also wakes the settings
        // window itself.
        self.pump_update();
        let mut need_frame = false;

        if changed {
            // Language is the only config value read by the text lookup itself.
            let before = nooksill_core::i18n::language();
            nooksill_core::i18n::configure(
                &self.dock.cfg.language,
                nooksill_platform::locale::ui_language_id(),
            );
            if nooksill_core::i18n::language() != before {
                if let Some(sw) = &self.settings_slot {
                    sw.refresh_title();
                }
            }
            // Config is shared by the dock and the settings window; `update()`
            // recomputes geometry next frame, so only mark dirty and schedule
            // a save here.
            self.touch_cfg();
            need_frame = true;
            self.dock.mark_dirty();
            // `Notes` switch: add the note item immediately. Turning it off
            // never deletes, since the user may have added their own.
            if self.dock.cfg.notes && !self.raw.iter().any(|i| i.kind == ItemKind::Note) {
                self.raw.push(Item::new(
                    ItemKind::Note,
                    nooksill_core::i18n::t("Notes"),
                    "nooksill:notes",
                ));
                self.apply_item_change();
                self.status(nooksill_core::i18n::t("Notes icon added"));
            }
            if trace::geom() {
                eprintln!("[settings] 配置有改动，排定写盘");
            }
        }
        if let Some(cmd) = cmd {
            need_frame |= self.run_settings_cmd(cmd);
        }

        if self.mount_key() != before {
            // Dock edge, reservation or hide mode changed: AppBar must be
            // re-registered and the window re-placed. A size change (side
            // docks transpose lw/lh) is handled by the next `step_relayout`;
            // this frame is placed with the old size.
            self.dock.mode = self.dock.cfg.mode;
            self.reserved = if appbar::should_reserve(
                self.dock.cfg.reserve,
                self.dock.mode == DockMode::Always,
                self.dock.cfg.edge,
                appbar::taskbar(),
            ) {
                self.appbar.apply(
                    self.dock.cfg.edge,
                    self.dock.reserve_thickness(),
                    display::monitor_rect(0),
                )
            } else {
                self.appbar.remove();
                None
            };
            self.reposition();
            if trace::geom() {
                eprintln!(
                    "[settings] 重挂 edge={:?} reserve={} mode={:?} -> pos=({},{}) reserved={:?}",
                    self.dock.cfg.edge,
                    self.dock.cfg.reserve,
                    self.dock.mode,
                    self.pos.0,
                    self.pos.1,
                    self.reserved
                );
            }
            need_frame = true;
        }

        Ok(need_frame)
    }

    /// The tuple that decides whether the window must be re-mounted.
    pub(super) fn mount_key(&self) -> (Edge, bool, DockMode) {
        (
            self.dock.cfg.edge,
            self.dock.cfg.reserve,
            self.dock.cfg.mode,
        )
    }

    /// Execute a command picked from the context menu.
    pub(super) fn run_menu_cmd(&mut self, cmd: MenuCmd, hit: Option<usize>) {
        match cmd {
            // Same path as a left click: activate if running, else launch.
            MenuCmd::Open => {
                let Some(item) = hit.and_then(|i| self.item_at(i)) else {
                    return;
                };
                let toggle = self.dock.cfg.get_i32(Key::CLICK_TOGGLE) != 0;
                self.opener.submit(OpenJob {
                    item: Box::new(item.clone()),
                    force: false,
                    toggle,
                    browser: self.dock.cfg.get_i32(Key::BROWSER),
                });
                self.last_launch_ms = Some(self.now_ms());
            }
            MenuCmd::OpenInExplorer => {
                if let Some(item) = hit.and_then(|i| self.item_at(i)) {
                    if let Err(e) = launch::open_folder(&item.path) {
                        eprintln!("打开文件夹「{}」失败：{e}", item.name);
                    }
                }
            }
            MenuCmd::QuitApp => {
                if let Some(exe) = hit
                    .and_then(|i| self.item_at(i))
                    .and_then(running::running_key)
                {
                    if !app_window::close_app(&exe) {
                        eprintln!("退出「{exe}」失败");
                    }
                }
            }
            MenuCmd::ExitDock => {
                // Normal exit path: `Drop` unregisters AppBar and releases the
                // single-instance lock.
                unsafe { PostQuitMessage(0) };
            }
            // Intercepted in `step_menu` before this point because they need
            // `gpu` or the registry; reaching here means the interception is
            // broken.
            MenuCmd::ToggleAutostart | MenuCmd::AddFile | MenuCmd::AddFolder => {
                eprintln!("`{cmd:?}` 走错了路径：它应当在主循环里被处理");
            }
            MenuCmd::Settings => {
                eprintln!("`Settings` 命令走错了路径：它应当在主循环里被处理");
            }
            MenuCmd::Launchpad => {
                eprintln!("`Launchpad` 命令走错了路径：它应当在主循环里被处理");
            }
            MenuCmd::AddSepAfter => {
                let Some(i) = hit else { return };
                if i >= self.raw.len() {
                    return;
                }
                self.raw
                    .insert(i + 1, Item::new(ItemKind::Separator, "", ""));
                self.apply_item_change();
            }
            MenuCmd::PinItem => {
                if let Some(i) = hit {
                    self.pin_item(i);
                }
            }
            MenuCmd::RemoveItem | MenuCmd::RemoveSep => {
                let Some(i) = hit else { return };
                if i >= self.raw.len() {
                    return;
                }
                // The last entry may not be removed: the dock would be empty
                // with nothing left to drag onto.
                if self.raw.len() <= 1 {
                    self.status(nooksill_core::i18n::t(
                        "This is the last item and cannot be removed.",
                    ));
                    return;
                }
                self.raw.remove(i);
                self.apply_item_change();
            }
            MenuCmd::Rename => {
                let Some(i) = hit else { return };
                let Some(item) = self.raw.get(i).cloned() else {
                    return;
                };
                self.rename_item(i, &item);
            }
            MenuCmd::Duplicate => {
                let Some(i) = hit else { return };
                let Some(item) = self.raw.get(i).cloned() else {
                    return;
                };
                let mut copy = item.clone();
                // The copy has no children, so a duplicated group stays empty
                // instead of becoming a group of groups.
                copy.kids.clear();
                self.raw.insert(i + 1, copy);
                self.apply_item_change();
                self.status(nooksill_core::i18n::fmt(
                    "Copied {0}",
                    &[item.name.to_string().as_str()],
                ));
            }
            MenuCmd::DissolveGroup => {
                let Some(i) = hit else { return };
                if i >= self.raw.len() {
                    return;
                }
                match group::dissolve_item(&mut self.raw, i) {
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
            }
            MenuCmd::OpenGroup => {
                let Some(i) = hit else { return };
                self.open_group(i);
            }
            MenuCmd::ChangeIcon => {
                if let Some(i) = hit {
                    self.change_icon(i);
                }
            }
            MenuCmd::ResetIcon => {
                if let Some(i) = hit {
                    self.reset_icon(i);
                }
            }
            MenuCmd::ToggleDock => {
                eprintln!("`ToggleDock` 只该由托盘菜单走 `step_tray` 处理");
            }
            MenuCmd::Notes => {
                eprintln!("`Notes` 走错了路径：它应当在主循环里被处理");
            }
            MenuCmd::NewGroup | MenuCmd::NewFolder => {
                eprintln!("`{cmd:?}` 走错了路径：它应当在主循环里被处理");
            }
        }
    }
}
