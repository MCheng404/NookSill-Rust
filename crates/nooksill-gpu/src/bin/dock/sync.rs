//! Config/bar synchronisation, disk flushes and background worker steps.

use super::misc::{
    request_missing_icons, temp_name, CFG_SAVE_DEBOUNCE_MS, MAX_TEMP, TOOL_SAVE_DEBOUNCE_MS,
};
use super::*;

impl App {
    /// Single finish path after an item list change: swap the displayed list,
    /// schedule the write, request missing icons.
    ///
    /// The write is debounced and a failure never rolls back memory: better
    /// "applied but unsaved" than "the click did nothing".
    pub(super) fn apply_item_change(&mut self) {
        self.sync_items();
        request_missing_icons(&self.icons, &self.dock, &self.raw);
        self.touch_items();
    }

    /// Bar item `i`.
    ///
    /// The first `raw.len()` entries are pinned, the rest are transient. Every
    /// index lookup must go through here; `raw.get(i)` alone misses transients.
    pub(super) fn item_at(&self, i: usize) -> Option<&Item> {
        self.raw
            .get(i)
            .or_else(|| self.temps.get(i.checked_sub(self.raw.len())?))
    }

    /// Rebuild the bar list from pinned plus transient items.
    ///
    /// `replace_items` clears hover and drag state and sets `anim`, so rebuild
    /// only when the list really changed; a 1.2 s rebuild keeps breaking hover.
    /// While the mouse is down, always defer: a rebuild would clear `drag` in
    /// the middle of a swap.
    pub(super) fn sync_items(&mut self) {
        if self.dock.press_active() {
            self.items_stale = true;
            return;
        }
        self.sync_items_now();
    }

    /// Rebuild immediately, ignoring drag state; only for
    /// [`Self::flush_items_stale`].
    pub(super) fn sync_items_now(&mut self) {
        let all: Vec<DockItem> = self
            .raw
            .iter()
            .chain(self.temps.iter())
            .map(DockItem::from_item)
            .collect();
        if trace::geom() {
            eprintln!(
                "[items] 重建 raw={} temps={} stale={}",
                self.raw.len(),
                self.temps.len(),
                self.items_stale
            );
        }
        self.dock.replace_items(all);
        // Swaps are only allowed within one segment, so `Dock` needs the boundary.
        self.dock.set_pinned(self.raw.len());
        self.items_stale = false;
        // Keys of slots that no longer exist can never match again.
        self.icons.retain(self.raw.len() + self.temps.len());
        self.request_temp_icons();
    }

    /// Apply the rebuild a drag deferred.
    pub(super) fn flush_items_stale(&mut self) {
        if self.items_stale && !self.dock.press_active() {
            self.sync_items_now();
        }
    }

    /// Fetch icons for transient items; their indices start at `raw.len()`.
    pub(super) fn request_temp_icons(&self) {
        for (k, t) in self.temps.iter().enumerate() {
            let i = self.raw.len() + k;
            if self.dock.has_icon(i) {
                continue;
            }
            self.icons.submit(IconRequest {
                index: i,
                kind: t.kind,
                icon_src: String::new(),
                path: t.path.clone(),
                fit: t.fit,
                kids: Vec::new(),
            });
        }
    }

    /// Sync unpinned running apps; returns whether the list changed.
    ///
    /// Append-only, because `EnumWindows` returns Z-order and re-sorting each
    /// round would interleave icons and re-upload textures. Pinned apps never
    /// repeat (compared by `running_key`), and the list is capped at
    /// [`MAX_TEMP`] since the bar shrinks with the item count.
    pub(super) fn sync_temps(&mut self, running: &std::collections::HashSet<String>) -> bool {
        let want = if self.dock.cfg.get_i32(Key::ShowRunning) != 0 {
            // Exe paths of pinned items; transient entries must exclude them or
            // the same app shows up twice.
            let pinned: std::collections::HashSet<String> =
                self.raw.iter().filter_map(running::running_key).collect();
            running::temp_keys(running, &pinned, &self.temp_keys, MAX_TEMP, self.self_exe())
        } else {
            // Turning the switch off also drops the existing list.
            Vec::new()
        };
        if want == self.temp_keys {
            return false;
        }
        self.temps = want
            .iter()
            .map(|exe| {
                let mut it = Item::new(ItemKind::File, temp_name(exe), exe.clone());
                // A transient entry is running by definition, so light the dot.
                it.running = true;
                it
            })
            .collect();
        self.temp_keys = want;
        self.sync_items();
        true
    }

    /// The dock's own exe path in lowercase; transient entries skip it.
    pub(super) fn self_exe(&self) -> &'static str {
        static SELF_EXE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SELF_EXE.get_or_init(|| {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        })
    }

    /// Mark `[Items]` dirty.
    ///
    /// Stores a copy: the write happens on another thread, and `raw` may change
    /// again before it runs.
    pub(super) fn touch_items(&mut self) {
        self.dirty_items = Some(self.raw.clone());
        self.dirty_at.get_or_insert_with(Instant::now);
    }

    /// Mark `[Settings]` dirty.
    pub(super) fn touch_cfg(&mut self) {
        self.dirty_cfg = Some(Box::new(self.dock.cfg.clone()));
        self.dirty_at.get_or_insert_with(Instant::now);
    }

    /// Hand pending content to the writer once the debounce expires.
    ///
    /// A slider drag emits dozens of changes and each write is a full UTF-16
    /// rewrite; 400 ms means "save when the hand stops". Items and config share
    /// the window, so one flush writes the file once.
    pub(super) fn step_disk(&mut self) {
        for result in self.disk.drain() {
            match result {
                Ok(()) => {
                    // Do not report success: flashing on every change is noise,
                    // only failures are worth a message.
                    if trace::geom() {
                        eprintln!("[disk] 已写回 {}", self.paths.ini.display());
                    }
                }
                Err(e) => eprintln!("保存失败（改动仍在本进程内生效）：{e}"),
            }
        }

        let Some(at) = self.dirty_at else {
            return;
        };
        if at.elapsed() < Duration::from_millis(CFG_SAVE_DEBOUNCE_MS) {
            return;
        }
        self.dirty_at = None;
        let items = self.dirty_items.take();
        let config = self.dirty_cfg.take();
        let notes = self.dirty_notes.take();
        if items.is_some() || config.is_some() || notes.is_some() {
            self.disk.submit(WriteJob {
                items,
                config,
                // Config and item writes leave `[Tool]` alone: each section has
                // its own dirty flag, and writing together would also refresh
                // the stopwatch on every slider move.
                tool: None,
                notes,
            });
        }
    }

    /// Persist timer panel changes, also debounced.
    ///
    /// A separate path rather than `dirty_at`: checking a box is a one-off action
    /// and sharing the window would delay it by 400 ms, which feels lost.
    pub(super) fn step_tool_save(&mut self) {
        let Some(at) = self.tool_save_at else {
            return;
        };
        if at.elapsed() < Duration::from_millis(TOOL_SAVE_DEBOUNCE_MS) {
            return;
        }
        self.tool_save_at = None;
        let Some(win) = self.tool_slot.as_ref() else {
            return;
        };
        let now = nooksill_gpu::tool_win::now_ms();
        self.disk.submit(WriteJob {
            items: None,
            config: None,
            tool: Some((Box::new(win.state.clone()), now)),
            notes: None,
        });
    }

    /// Write everything still held by the debounce window and wait for the
    /// disk. Called once on the quit path.
    ///
    /// Without it an edit followed by an immediate quit is lost: the debounce
    /// only submits after 400 ms, and the writer thread is never waited for.
    pub(super) fn flush_pending_writes(&mut self) {
        let items = self.dirty_items.take();
        let config = self.dirty_cfg.take();
        let notes = self.dirty_notes.take();
        let tool = self.tool_save_at.take().and_then(|_| {
            self.tool_slot.as_ref().map(|win| {
                (
                    Box::new(win.state.clone()),
                    nooksill_gpu::tool_win::now_ms(),
                )
            })
        });
        self.dirty_at = None;
        let pending = items.is_some() || config.is_some() || notes.is_some() || tool.is_some();
        let job = WriteJob {
            items,
            config,
            tool,
            notes,
        };
        let result = self.disk.shutdown(pending.then_some(job));
        if let Err(e) = result {
            eprintln!("退出前保存失败：{e}");
        } else if trace::geom() {
            eprintln!("[disk] 退出前已写回 {}", self.paths.ini.display());
        }
    }

    /// Take finished icons from the worker and upload them as textures.
    ///
    /// Pixel work is done; only `load_texture` remains, and that needs a thread
    /// holding an `egui::Context`.
    pub(super) fn step_icons(&mut self) -> bool {
        let ready = self.icons.drain();
        if ready.is_empty() {
            return false;
        }
        let ctx = self.frame.ctx();
        for icon in &ready {
            // The queue recorded the index at request time; items may have been
            // added or removed, so check the path and never attach an icon to
            // the wrong item.
            let same = self
                .dock
                .items
                .get(icon.index)
                .is_some_and(|it| it.path == icon.path);
            if same {
                self.dock.install_icon(icon.index, ctx, icon);
            }
        }
        true
    }

    /// Take scanner results: running state and occlusion.
    pub(super) fn step_scan(&mut self) -> bool {
        let mut need_frame = false;
        // Disabling both the dots and the running icons stops the scan, so the
        // "nothing is running" round never arrives and the last transient icons
        // would stay on the bar for good.
        if !self.dock.cfg.scan_enabled() && !self.temps.is_empty() {
            self.temps.clear();
            self.temp_keys.clear();
            self.sync_items();
            need_frame = true;
        }
        for out in self.scanner.drain() {
            if let Some(running) = out.running {
                if self.sync_temps(&running) {
                    need_frame = true;
                }
                if running::refresh_running(&mut self.raw, &running) {
                    // `Dock` and `raw` are separate lists; the dot is drawn on
                    // the former.
                    for (slot, item) in self.dock.items.iter_mut().zip(self.raw.iter()) {
                        slot.running = item.running;
                    }
                    need_frame = true;
                }
            }
            if let Some(covered) = out.covered {
                self.covered = covered;
                // Occlusion only affects the smart-hide verdict, so no frame is
                // needed; `step_auto_hide` reads it every 100 ms.
            }
        }
        need_frame
    }

    /// Refresh the resource widget sampling.
    ///
    /// Once per second, not per frame: `GetSystemTimes` and
    /// `GlobalMemoryStatusEx` are not cheap, and CPU usage only matters at
    /// second granularity. Idle sessions render almost no frames, so a timer is
    /// the right trigger.
    pub(super) fn step_sysinfo(&mut self) -> bool {
        // No resource widget in the strip, so skip sampling.
        if self
            .dock
            .metrics
            .widgets
            .iter()
            .all(|(k, _)| k != nooksill_core::layout::WidgetKind::Resource)
        {
            return false;
        }
        if self.last_sysinfo.elapsed() < std::time::Duration::from_secs(1) {
            return false;
        }
        self.last_sysinfo = std::time::Instant::now();
        let before = self.dock.sysinfo();
        self.dock.refresh_sysinfo();
        let after = self.dock.sysinfo();
        // Skip the repaint when the numbers did not change; idle CPU is 0 most
        // of the time.
        before != after
    }
}
