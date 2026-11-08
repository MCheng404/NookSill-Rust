//! Resource monitor window lifecycle and the occlusion cover target.

use super::*;

impl App {
    pub(super) fn step_resmon(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.debug_open_once()? {
            // Build the panel this frame; paint it next frame.
            return Ok(true);
        }
        // Read the click exactly once: `take_widget_click` clears the flag, so
        // two panels taking it means the first one eats the click.
        if let Some(kind) = self.dock.take_widget_click() {
            let dark = self
                .dock
                .cfg
                .dock_is_dark(nooksill_platform::theme::is_dark());
            if trace::geom() {
                eprintln!("[widget] 卡片被点：{kind:?}");
            }
            // Panels are mutually exclusive, so nothing ends up stacked.
            if kind != nooksill_core::layout::WidgetKind::Clock {
                if let Some(w) = self.calendar_slot.as_mut() {
                    w.hide();
                }
            }
            if kind != nooksill_core::layout::WidgetKind::CtrlCenter {
                if let Some(w) = self.cc_slot.as_mut() {
                    w.hide();
                }
            }
            match kind {
                nooksill_core::layout::WidgetKind::Clock => {
                    // Calendar popup: a second click on the same card closes it.
                    let open = self.calendar_slot.as_ref().is_some_and(|w| w.visible());
                    if open {
                        if let Some(w) = self.calendar_slot.as_mut() {
                            w.hide();
                        }
                    } else {
                        let today = nooksill_gpu::calendar_win::today();
                        if self.calendar_slot.is_none() {
                            let win = CalendarWindow::new(&self.gpu, self.ppp, dark, today)?;
                            self.calendar_slot = Some(win);
                        }
                        let anchor = self.dock.widget_screen_rect(
                            nooksill_core::layout::WidgetKind::Clock,
                            self.pos.0,
                            self.pos.1,
                        );
                        if let (Some(win), Some(anchor)) = (self.calendar_slot.as_mut(), anchor) {
                            win.set_today(today);
                            win.show_at(anchor, self.dock.cfg.edge);
                        }
                    }
                }
                nooksill_core::layout::WidgetKind::Resource => {
                    // Same as the calendar: a second click closes it.
                    if self.resmon_slot.as_ref().is_some_and(|w| w.visible()) {
                        if let Some(w) = self.resmon_slot.as_mut() {
                            w.hide();
                        }
                    } else {
                        if self.resmon_slot.is_none() {
                            let mut win = ResMonWindow::new(&self.gpu, self.ppp, dark)?;
                            win.attach_public();
                            self.resmon_slot = Some(win);
                        }
                        if let Some(win) = self.resmon_slot.as_mut() {
                            win.show();
                        }
                    }
                }
                nooksill_core::layout::WidgetKind::CtrlCenter => {
                    let open = self.cc_slot.as_ref().is_some_and(|w| w.visible());
                    if open {
                        if let Some(w) = self.cc_slot.as_mut() {
                            w.hide();
                        }
                    } else {
                        let energy =
                            self.dock.cfg.get_i32(nooksill_core::config::Key::CC_ENERGY) != 0;
                        let hotspot = self
                            .dock
                            .cfg
                            .get_i32(nooksill_core::config::Key::CC_HOTSPOT)
                            != 0;
                        if self.cc_slot.is_none() {
                            // Tile order is dragged by the user and stored in `[widgets] Order`.
                            let order =
                                nooksill_core::control::parse_order(&self.dock.cfg.cc_order);
                            let win =
                                CcWindow::new(&self.gpu, self.ppp, dark, energy, hotspot, &order)?;
                            self.cc_slot = Some(win);
                        }
                        let anchor = self.dock.widget_screen_rect(
                            nooksill_core::layout::WidgetKind::CtrlCenter,
                            self.pos.0,
                            self.pos.1,
                        );
                        if let (Some(win), Some(anchor)) = (self.cc_slot.as_mut(), anchor) {
                            win.show_at(anchor, self.dock.cfg.edge);
                        }
                    }
                }
                nooksill_core::layout::WidgetKind::Tray => {
                    // `Win+B` then Enter: pure keystroke simulation, no panel
                    // and no config change.
                    if trace::geom() {
                        eprintln!("[widget] 打开系统托盘");
                    }
                    nooksill_platform::sysact::open_tray();
                }
                nooksill_core::layout::WidgetKind::Tool => {
                    // A second click on the card closes it.
                    if self.tool_slot.as_ref().is_some_and(|w| w.visible()) {
                        if let Some(w) = self.tool_slot.as_mut() {
                            w.hide();
                        }
                    } else {
                        if self.tool_slot.is_none() {
                            // Read `[Tool]` only on the first open: while
                            // closed the panel is not in memory, and should not be.
                            let ini = Ini::load(&self.paths.ini)?;
                            let state = nooksill_core::tool::ToolState::load(&ini);
                            let win = ToolWindow::new(&self.gpu, self.ppp, dark, state)?;
                            self.tool_slot = Some(win);
                        }
                        if let Some(win) = self.tool_slot.as_mut() {
                            win.show();
                            if trace::geom() {
                                eprintln!("[widget] 计时器面板已显示");
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // Repaint when due; the data changes once a second.
        let mut need = false;
        if let Some(win) = self.resmon_slot.as_mut() {
            if win.visible() && win.due() {
                win.render(&self.gpu)?;
                need = true;
            }
        }
        // Launchpad: a fullscreen window, repainted only after interaction (the
        // icon thread nudges it via `PostMessage`). Commands are collected first
        // because applying them needs `&mut self`.
        let mut lp_cmds: Vec<nooksill_gpu::launchpad_win::LaunchpadCmd> = Vec::new();
        if let Some(win) = self.launchpad_slot.as_mut() {
            if win.due() {
                win.render(&self.gpu)?;
                need = true;
            }
            while let Some(cmd) = win.take_cmd() {
                lp_cmds.push(cmd);
            }
        }
        for cmd in lp_cmds {
            match cmd {
                nooksill_gpu::launchpad_win::LaunchpadCmd::AddToDock { name, path } => {
                    self.raw.push(Item::new(ItemKind::File, name, path));
                    self.apply_item_change();
                    need = true;
                }
            }
        }
        // Control center repaints only after interaction; a theme change asks
        // the main loop for one more frame.
        let mut cc_order: Option<Vec<nooksill_core::control::Tile>> = None;
        if let Some(win) = self.cc_slot.as_mut() {
            if win.due() {
                win.render(&self.gpu)?;
                need = true;
            }
            if win.dirty_theme {
                // The theme is not cached: `theme::is_dark()` reads the registry
                // every time, so the next frame is already correct; this only
                // guarantees that frame happens.
                win.dirty_theme = false;
                need = true;
            }
            cc_order = win.take_order();
        }
        if let Some(order) = cc_order {
            // Edit the in-memory copy and mark dirty; the write goes through the
            // debounce queue, since one drag reorders repeatedly.
            self.dock.cfg.cc_order = nooksill_core::control::order_text(&order);
            self.touch_cfg();
            if trace::geom() {
                eprintln!("[cc] 磁贴顺序 -> {}", self.dock.cfg.cc_order);
            }
            need = true;
        }
        // Calendar popup: repaint once after interaction, no timer.
        if let Some(win) = self.calendar_slot.as_mut() {
            if win.due() {
                win.render(&self.gpu)?;
                need = true;
            }
        }
        // Timer panel: one frame per 50 ms while running.
        let now = nooksill_gpu::tool_win::now_ms();
        if let Some(win) = self.tool_slot.as_mut() {
            if win.visible() && win.due() {
                win.render(&self.gpu, now)?;
                need = true;
            }
            if win.dirty {
                win.dirty = false;
                self.tool_save_at = Some(std::time::Instant::now());
            }
        }
        Ok(need)
    }

    /// Publish the rect the scanner should test for occlusion.
    ///
    /// Send only on change: the main loop runs dozens of times per second and
    /// the scanner consumes just the latest value.
    pub(super) fn sync_cover_target(&mut self) {
        // While animating the rect jitters by a pixel per frame without changing
        // the verdict; wait until it settles or the queue fills and the scanner
        // spins on new input.
        if self.dock.anim {
            return;
        }
        // An injected verdict needs no computation.
        let cover = if self.dock.mode == DockMode::SmartHide && self.fake_covered.is_none() {
            Some(self.bar_screen_rect())
        } else {
            None
        };
        // With both the dots and the running tooltip off, skip the running scan
        // (see `ScanInput`).
        let want = ScanInput {
            cover,
            want_running: self.dock.cfg.scan_enabled(),
        };
        if Some(want) != self.scan_sent {
            self.scan_sent = Some(want);
            self.scanner.update(want);
        }
    }
}
