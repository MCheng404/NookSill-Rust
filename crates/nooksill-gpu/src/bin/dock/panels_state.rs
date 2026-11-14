//! Popup panel bookkeeping and the debug env hooks that open them.

use super::menu_win::frost_spec;
use super::satellite::{ensure_launchpad, ensure_settings};
use super::*;

impl App {
    /// Debug hook: `NOOKSILL_DEBUG_OPEN=cc|launchpad|calendar|tool|resmon|settings`
    /// opens that panel once after startup.
    ///
    /// Unattended checks need to click a card, but a foreground window often
    /// swallows the click, so opening the panel directly isolates its rendering.
    pub(super) fn debug_open_once(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.debug_opened {
            return Ok(false);
        }
        let Ok(what) = std::env::var("NOOKSILL_DEBUG_OPEN") else {
            self.debug_opened = true;
            return Ok(false);
        };
        self.debug_opened = true;
        let kind = match what.as_str() {
            "cc" => nooksill_core::layout::WidgetKind::CtrlCenter,
            "calendar" => nooksill_core::layout::WidgetKind::Clock,
            "tool" => nooksill_core::layout::WidgetKind::Tool,
            "resmon" => nooksill_core::layout::WidgetKind::Resource,
            "launchpad" => {
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
                lw.place_on_target(&self.gpu, self.ppp)?;
                lw.set_grid(
                    self.dock.cfg.get_i32(Key::LP_COLS).max(3) as usize,
                    self.dock.cfg.get_i32(Key::LP_ROWS).max(2) as usize,
                );
                lw.open(frost_spec(&self.dock.cfg, Key::LP_FROST, 0.0));
                return Ok(true);
            }
            "settings" => {
                let sw = ensure_settings(
                    &self.gpu,
                    &mut self.settings_slot,
                    &self.paths,
                    settings_win::effective_ppp(&self.dock.cfg, self.dpi),
                )?;
                sw.state.auto_start = nooksill_platform::theme::get_autostart();
                sw.show(frost_spec(&self.dock.cfg, Key::UI_FROST, 20.0));
                return Ok(true);
            }
            other => {
                eprintln!("NOOKSILL_DEBUG_OPEN={other} 不认识");
                return Ok(false);
            }
        };
        // Same path as a card click: construct and place.
        self.dock.click_widget(kind);
        Ok(true)
    }

    /// Visibility and handle of each of the six panels.
    ///
    /// Every check (any open, hit inside, hide all) walks this one list. The
    /// group panel was missing once, and auto-hide collapsed the bar while the
    /// panel stayed afloat.
    pub(super) fn panels(&self) -> [Option<(bool, HWND)>; 6] {
        [
            self.calendar_slot.as_ref().map(|w| (w.visible(), w.hwnd())),
            self.cc_slot.as_ref().map(|w| (w.visible(), w.hwnd())),
            self.resmon_slot.as_ref().map(|w| (w.visible(), w.hwnd())),
            self.tool_slot.as_ref().map(|w| (w.visible(), w.hwnd())),
            self.group_slot.as_ref().map(|w| (w.visible(), w.hwnd())),
            self.note_slot.as_ref().map(|w| (w.visible(), w.hwnd())),
        ]
    }

    /// Whether any panel is open.
    pub(super) fn panel_open(&self) -> bool {
        self.panels().iter().flatten().any(|(visible, _)| *visible)
    }

    /// Drop the scale-baked panels after a DPI change.
    ///
    /// Each of these fixes its physical size and `pixels_per_point` in `new`,
    /// so it cannot follow the dock to another monitor in place. Persistent
    /// state is handed over first: a pending tile order goes into the config
    /// and the timer snapshot into the disk queue; everything else is rebuilt
    /// from `App` / `raw` when the panel opens again.
    pub(super) fn reset_panels_for_scale(&mut self) {
        if let Some(win) = self.cc_slot.as_mut() {
            if let Some(order) = win.take_order() {
                self.dock.cfg.cc_order = nooksill_core::control::order_text(&order);
                self.touch_cfg();
            }
        }
        if let Some(win) = self.tool_slot.as_ref() {
            let now = nooksill_gpu::tool_win::now_ms();
            self.disk.submit(WriteJob {
                items: None,
                config: None,
                tool: Some((Box::new(win.state.clone()), now)),
                notes: None,
            });
        }
        self.tool_save_at = None;
        // The dropped windows destroy themselves; hiding first keeps a panel
        // from flashing at the old scale for one frame.
        self.hide_panels();
        self.calendar_slot = None;
        self.cc_slot = None;
        self.resmon_slot = None;
        self.tool_slot = None;
        self.group_slot = None;
        self.note_slot = None;
    }

    /// Whether the user is busy with the dock; auto-hide stays expanded then.
    ///
    /// Besides dragging: an open panel puts the cursor off the dock while still
    /// belonging to it, the settings window is a live preview, and the launchpad
    /// covers the dock completely.
    pub(super) fn dock_busy(&self) -> bool {
        self.dock.dragging().is_some()
            || self.panel_open()
            || self.settings_slot.as_ref().is_some_and(|w| w.visible())
            || self.launchpad_slot.as_ref().is_some_and(|w| w.visible())
    }

    /// Whether a screen point lands inside a visible panel.
    pub(super) fn panel_covers(&self, x: i32, y: i32) -> bool {
        self.panels()
            .iter()
            .flatten()
            .any(|(visible, hwnd)| *visible && nooksill_gpu::popup::hits(*hwnd, x, y))
    }

    /// Hide every panel; returns whether anything was hidden.
    pub(super) fn hide_panels(&mut self) -> bool {
        let mut any = false;
        if let Some(w) = self.calendar_slot.as_mut() {
            if w.visible() {
                w.hide();
                any = true;
            }
        }
        if let Some(w) = self.cc_slot.as_mut() {
            if w.visible() {
                w.hide();
                any = true;
            }
        }
        if let Some(w) = self.resmon_slot.as_mut() {
            if w.visible() {
                w.hide();
                any = true;
            }
        }
        if let Some(w) = self.tool_slot.as_mut() {
            if w.visible() {
                w.hide();
                any = true;
            }
        }
        if let Some(w) = self.group_slot.as_mut() {
            if w.visible() {
                w.hide();
                self.group_index = None;
                any = true;
            }
        }
        if let Some(w) = self.note_slot.as_mut() {
            if w.visible() {
                w.hide();
                any = true;
            }
        }
        any
    }

    /// Dismiss on outside click (`[widgets] PanelDismiss`); returns whether it
    /// closed. Three hits are ignored: the dock itself (click handling owns
    /// it), inside a panel, and a disabled setting.
    pub(super) fn step_panel_dismiss(&mut self) -> bool {
        // Just opened: drop the edge from the opening click, otherwise the
        // first cursor move reads as a new click and closes the panel at once.
        let open = self.panel_open();
        if open && !self.panel_was_open {
            self.watcher.reset();
        }
        self.panel_was_open = open;
        // Nothing open: return without even polling button state.
        if !open {
            return false;
        }
        let Some((x, y)) = self.watcher.poll() else {
            return false;
        };
        if self.dock.cfg.get_i32(Key::PANEL_DISMISS) == 0 {
            return false;
        }
        if nooksill_gpu::popup::hits(self.hwnd, x, y) || self.panel_covers(x, y) {
            return false;
        }
        if trace::popup() {
            eprintln!("[popup] 面板外点了 ({x}, {y})，收起");
        }
        self.hide_panels()
    }
}
