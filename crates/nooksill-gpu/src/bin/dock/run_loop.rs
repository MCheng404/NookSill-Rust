//! Main message loop and the per-frame layout/reorder steps it drives.

use super::drag::{reorder_bar, BarMove};
use super::misc::{reserve_space, work_rect};
use super::sys::{dump_geometry, pump};
use super::*;

impl App {
    /// Rebind the global hotkey when `[Hotkeys] Toggle` or its switch changes.
    ///
    /// A failed rebind rolls back inside `GlobalHotkey` and is logged once;
    /// marking it applied prevents retrying a taken combination every frame.
    fn sync_global_hotkey(&mut self) {
        let want = (self.dock.cfg.hotkey_combo, self.dock.cfg.hotkey);
        if want == self.hotkey_applied {
            return;
        }
        self.global_hotkey.rebind(want.0, want.1);
        self.hotkey_applied = want;
    }

    /// Follow `[Settings] Monitor` and display changes without a restart.
    ///
    /// Work area, DPI and the AppBar reservation are all per monitor, so all
    /// three are re-derived instead of staying at their boot values; the window
    /// itself is moved by `step_relayout`. A `WM_DISPLAYCHANGE` re-runs this
    /// even when the index did not change: resolution, scaling and a hot-plugged
    /// screen all invalidate the cached work area. Edge changes keep their own
    /// path (`mount_key`), which does not touch the work rect. Returns whether
    /// anything changed.
    fn sync_monitor(&mut self) -> bool {
        let want = (self.dock.cfg.monitor, self.dock.geom_epoch);
        if want == self.monitor_for {
            return false;
        }
        let count = display::monitors().len();
        if self.dock.cfg.monitor < 0 || self.dock.cfg.monitor as usize >= count.max(1) {
            if trace::geom() {
                eprintln!(
                    "[monitor] 序号 {} 越界（当前 {count} 台），改用主显示器",
                    self.dock.cfg.monitor
                );
            }
            self.dock.cfg.monitor = 0;
        }
        let want = (self.dock.cfg.monitor, want.1);
        self.monitor_for = want;
        let taskbar = appbar::taskbar();
        self.work = work_rect(&self.dock.cfg, taskbar);
        // `Dock` keeps its own copy for `calc_metrics`; a stale one lays the bar
        // out for the old screen.
        self.dock.work = self.work;
        // Panels that are not anchored to a card centre themselves here.
        display::set_target_work_area(self.work);
        let dpi = display::monitor_dpi(want.0);
        if dpi != self.dpi {
            self.dpi = dpi;
            self.ppp = dpi as f32 / 96.0;
            self.frame.set_pixels_per_point(self.ppp);
        }
        self.reserved = reserve_space(
            &mut self.appbar,
            &self.dock.cfg,
            self.dock.mode,
            self.dock.reserve_thickness(),
            taskbar,
        );
        // Sizes follow the DPI, so the layout has to run before the window is
        // moved; `step_relayout` covers both a resize and a pure move.
        self.dock.mark_dirty();
        if trace::geom() {
            eprintln!(
                "[monitor] index={} dpi={} work=({},{},{},{})",
                want.0, self.dpi, self.work.left, self.work.top, self.work.right, self.work.bottom
            );
        }
        true
    }

    /// Message loop; returns only on `WM_QUIT`.
    ///
    /// Each step reports whether this round needs a frame. Order matters: the
    /// menu is modal and must sit after clicks but before settings; disk writes
    /// come after settings so this round's changes are already in `cfg`.
    pub(super) fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut msg = MSG::default();
        loop {
            let mut need_frame = false;
            // The experimental blur switch lives in `Config`, but the capture
            // thread and every `FrameRenderer` need it immediately; sync per
            // frame so a settings change applies without a restart.
            nooksill_gpu::frost::set_illusion(self.dock.cfg.get_i32(Key::FROST_ILLUSION) != 0);
            // A settings edit changes the combination without a restart.
            self.sync_global_hotkey();

            // 1. Drain the queue; one frame per message would jitter.
            match pump(&mut msg) {
                Pumped::Quit => return Ok(()),
                Pumped::Frames => need_frame = true,
                // A worker wake only breaks the wait; the dirty flags below
                // decide whether to paint.
                Pumped::Wake | Pumped::Idle => {}
            }

            // 2. Monitor and edge from settings: refresh the work rect, DPI and
            //    AppBar reservation before the layout runs.
            need_frame |= self.sync_monitor();
            // 2.5 Relayout. Both conditions are required: `dirty` (mouse
            //     enter/leave/press changed input state, so `hot` must be
            //     recomputed) and `anim` (the zoom curve is still running).
            need_frame |= self.step_layout();
            // 3. Window size and position follow the item count.
            need_frame |= self.step_relayout()?;
            // 3.5 A bar rebuild suspended during drag; runs before reorder so
            //     the new list takes part in hit tests and painting this frame.
            self.flush_items_stale();
            // 4. Sync drag reordering into the config copy; the write is debounced.
            self.step_reorder();
            // 4.5 Dragging a separator changed IconSize / ZoomSize: mark dirty
            //     for the same debounce window.
            if self.dock.take_sep_dirty() {
                self.touch_cfg();
            }
            // 5. Clicks launch on a worker; `ShellExecuteW` no longer blocks.
            self.step_click();
            // 6. Context menu (modal).
            need_frame |= self.step_menu()?;
            // 6.5 Tray clicks (its menu is modal too).
            need_frame |= self.step_tray()?;
            // 7. Settings window (non-modal).
            need_frame |= self.step_settings()?;
            // 8. Worker results: icons, running state, occlusion. All
            //    `try_recv`, never blocking.
            need_frame |= self.step_icons();
            need_frame |= self.step_scan();
            // 8.5 Resource widget: per-second sampling and the detail panel.
            need_frame |= self.step_sysinfo();
            need_frame |= self.step_resmon()?;
            self.sync_cover_target();
            // 9. Disk writes, sent when the debounce expires; the write runs on
            //    another thread and needs no frame.
            self.step_disk();
            self.step_tool_save();
            // 10. Hide entirely while fullscreen.
            need_frame |= self.step_fullscreen();
            // 11. Auto-hide and smart hide.
            need_frame |= self.step_auto_hide();
            // 12. Dismiss panels on outside click (polls global keys, see `popup`).
            need_frame |= self.step_panel_dismiss();
            // 12.5 Group panel: render a frame and handle its action.
            need_frame |= self.step_group()?;
            // 12.6 Notes panel: render a frame and handle its action.
            need_frame |= self.step_notes()?;
            // 12.6 Dropped files become items.
            need_frame |= self.step_drop();
            // 12.7 Notice toast: keep drawing while alive (it fades out at the
            //      end), clear once expired.
            if self.dock.notice_active() {
                self.dock.mark_dirty();
                need_frame = true;
            } else {
                self.dock.expire_notice();
            }
            // 13. Smoke: paces itself (see `step_smoke`).
            need_frame |= self.step_smoke();
            // egui's own repaint deadline (hover, illusion, animations).
            need_frame |= !self.dock.fully_hidden() && !self.fs_hidden && self.frame.repaint_due();

            // 14. Present. While fully hidden, draw nothing: the content is
            //     offscreen and the smart-hide scanner still wakes the loop
            //     every 100 ms. `anim` is untouched, so the final collapse
            //     frame is still drawn.
            if (need_frame || self.dock.anim) && !self.dock.fully_hidden() {
                self.render()?;
            }
            self.wait();
        }
    }

    /// Real interval since the previous frame, in milliseconds.
    ///
    /// Floor 0.1 ms so two frames in the same millisecond do not divide by zero;
    /// cap 100 ms so a breakpoint or system stall cannot finish an animation in
    /// one frame (`lerp_to` caps at 4 frames as well).
    pub(super) fn frame_dt_ms(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last_layout).as_secs_f32() * 1000.0;
        self.last_layout = now;
        dt.clamp(0.1, 100.0)
    }

    /// Raise the system timer to 1 ms while animating, restore it afterwards.
    ///
    /// `MsgWaitForMultipleObjects` rounds its timeout up to the system tick
    /// (~15.6 ms), so without this a 2 ms pacing has no effect and high-refresh
    /// animations stay at ~64 Hz. Only during animation or smoke: a permanent
    /// 1 ms tick raises idle power draw machine-wide.
    pub(super) fn set_timer_resolution(&mut self, want: bool) {
        if want == self.timer_raised {
            return;
        }
        unsafe {
            if want {
                windows::Win32::Media::timeBeginPeriod(1);
            } else {
                windows::Win32::Media::timeEndPeriod(1);
            }
        }
        self.timer_raised = want;
    }

    /// Advance one layout frame; returns whether a frame is due.
    pub(super) fn step_layout(&mut self) -> bool {
        // Unattended screenshots: `NOOKSILL_FAKE_MOUSE="u,v"` pins the cursor so
        // hover state is reproducible while a user may move the real one.
        if let Some((u, v)) = self.fake_mouse {
            self.dock.mouse = (u, v);
            self.dock.hover = true;
        }
        if !self.dock.take_dirty() && !self.dock.anim {
            return false;
        }
        let dt = self.frame_dt_ms();
        self.dock.update(self.dpi, dt);
        true
    }

    /// Config hot reload: a new item count changes the bar width, so window
    /// size and position both change.
    ///
    /// The offscreen texture and DIB must resize too, or `UpdateLayeredWindow`
    /// commits a stale bitmap and only the top-left part appears.
    pub(super) fn step_relayout(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let (nw, nh) = self.dock.physical_size_i32();
        let resized = nw != self.size.0 || nh != self.size.1;
        if resized {
            self.size = (nw, nh);
            self.frame.resize(&self.gpu, nw as u32, nh as u32)?;
        }
        // Switching monitor or edge keeps the size on a square bar, so the
        // origin has to be compared as well: a resize-only test would leave the
        // window on the old screen.
        if !resized && self.desired_origin() == self.pos {
            return Ok(false);
        }
        self.reposition();
        if trace::geom() {
            eprintln!(
                "[relayout] 窗口 -> ({},{}) {nw}x{nh}",
                self.pos.0, self.pos.1
            );
            // Slot positions follow the item count; report them for automation.
            dump_geometry(&self.dock, self.dpi);
        }
        Ok(true)
    }

    /// Sync drag reordering into the config copy.
    ///
    /// No disk write here: one drag produces several reorders and each full
    /// UTF-16 rewrite stutters, so mark dirty and let `DiskWriter` write. No
    /// extra frame either: `move_item` sets `moved`, and the next `update`
    /// animates into place and asks for a frame.
    pub(super) fn step_reorder(&mut self) {
        let mut cfg_moved = false;
        for (from, to) in self.dock.take_pending_moves() {
            // Pinned swaps and pinning a running app change the config; order
            // between running apps lives in `temp_keys` and is never persisted.
            match self.apply_bar_move(from, to) {
                BarMove::Pinned | BarMove::PinnedNow => cfg_moved = true,
                BarMove::Running | BarMove::None => {}
            }
        }
        // The flag must be taken every round or it accumulates; the write only
        // depends on whether the config really changed.
        let _bar_changed = self.dock.take_order_changed();
        // Dropping on a group merges, dropping outside the bar removes. Must run
        // after the reorder: positions may have changed, so `from` is current.
        if let Some(i) = self.dock.take_pending_remove() {
            if i < self.raw.len() && self.raw.len() > 1 {
                let name = self.raw[i].name.clone();
                self.raw.remove(i);
                // `apply_item_change` rebuilds `dock.items`; otherwise the
                // removed slot stays and clicks land on nothing.
                self.apply_item_change();
                self.status(nooksill_core::i18n::fmt(
                    "Removed {0} from the Dock",
                    &[name.to_string().as_str()],
                ));
                return;
            }
            // A transient running item is not in the config and cannot be
            // deleted, so it should return to the bar. Rebuilding beats leaving
            // a hole, which makes every later click miss.
            if i >= self.raw.len() {
                self.sync_items();
            }
        }
        if let Some((from, to)) = self.dock.take_pending_merge() {
            match group::merge_item(&mut self.raw, from, to) {
                Ok(()) => {
                    self.apply_item_change();
                    self.status(nooksill_core::i18n::t("Added to group"));
                    return;
                }
                Err(e) => {
                    // Dropping a running app on a group also lands here: it has
                    // no config entry. Rebuild to the config order.
                    if from >= self.raw.len() {
                        self.status(nooksill_core::i18n::t(
                            "A running app cannot be added to a group",
                        ));
                    } else {
                        self.status(nooksill_core::i18n::fmt(
                            "Could not group: {0}",
                            &[e.to_string().as_str()],
                        ));
                    }
                    self.sync_items();
                }
            }
        }
        if cfg_moved {
            self.touch_items();
        }
    }

    /// Apply one bar move to the config copy; a thin wrapper over
    /// [`reorder_bar`]. Returns where the move landed.
    pub(super) fn apply_bar_move(&mut self, from: usize, to: usize) -> BarMove {
        let moved = reorder_bar(
            &mut self.raw,
            &mut self.temps,
            &mut self.temp_keys,
            from,
            to,
        );
        if moved == BarMove::PinnedNow {
            // The pinned range grew, so drag bounds must follow now; `sync_items`
            // is suspended for the duration of the drag.
            self.dock.set_pinned(self.raw.len());
            self.status(nooksill_core::i18n::t("Pinned to Dock"));
        }
        if trace::geom() {
            eprintln!(
                "[reorder] {from}->{to} {moved:?} raw={} temps={}",
                self.raw.len(),
                self.temps.len()
            );
        }
        moved
    }

    /// Pin bar item `i` (a running app) into the config.
    ///
    /// Dragging into the pinned range also pins, but that is a gesture; the menu
    /// is the explicit entry, so the user need not guess where to drop.
    pub(super) fn pin_item(&mut self, i: usize) {
        let n = self.raw.len();
        if i < n || i >= n + self.temps.len() {
            return;
        }
        let k = i - n;
        let mut item = self.temps.remove(k);
        if k < self.temp_keys.len() {
            let _ = self.temp_keys.remove(k);
        }
        item.temp = false;
        item.running = true;
        let name = item.name.clone();
        // Append at the end of the pinned range: the "pinned first, running
        // after" invariant must hold or the order cannot be written back. The
        // icon jumps to the tail, but only when pinning from the menu.
        self.raw.push(item);
        self.apply_item_change();
        self.status(nooksill_core::i18n::fmt(
            "Pinned {0}",
            &[name.to_string().as_str()],
        ));
    }
}
