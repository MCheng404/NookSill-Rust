//! Eager construction of the widget panels.
//!
//! A first click used to pay for a window class, a wgpu pipeline and a CJK font
//! atlas. Preloading moves that work to startup; every slot stays hidden until
//! its card is clicked, so the existing open paths are unchanged.
//!
//! No disk or registry reads here: the ini was parsed once in `boot`, and the
//! cached system theme comes from the main loop's `look_epoch` refresh.

use super::*;

impl App {
    /// Build every idle panel window once. A failure only loses preloading for
    /// that panel: the lazy path still creates it on the next click.
    pub(super) fn preload_panels(&mut self) {
        let (dark, hotspot, order, state) = {
            let cfg = &self.dock.cfg;
            (
                cfg.dock_is_dark(self.system_dark),
                cfg.get_i32(Key::CC_HOTSPOT) != 0,
                nooksill_core::control::parse_order(&cfg.cc_order),
                self.tool_state.clone(),
            )
        };
        let ppp = self.ppp;
        // Preloaded panels are GPU windows; building them here would create the
        // very device the CPU mode exists to avoid, so they stay cold and each
        // one pulls the device up when first opened (ADR-119).
        if !nooksill_gpu::gpu::gpu_accel() {
            return;
        }
        let gpu = match self.gpu() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[preload] {e}");
                return;
            }
        };
        let gpu = &gpu;

        if self.calendar_slot.is_none() {
            let today = nooksill_gpu::calendar_win::today();
            match CalendarWindow::new(gpu, ppp, dark, today) {
                Ok(w) => self.calendar_slot = Some(w),
                Err(e) => eprintln!("[preload] calendar: {e}"),
            }
        }
        if self.cc_slot.is_none() {
            match CcWindow::new(gpu, ppp, dark, hotspot, &order) {
                Ok(w) => self.cc_slot = Some(w),
                Err(e) => eprintln!("[preload] control center: {e}"),
            }
        }
        if self.resmon_slot.is_none() {
            match ResMonWindow::new(gpu, ppp, dark) {
                Ok(w) => self.resmon_slot = Some(w),
                Err(e) => eprintln!("[preload] resource monitor: {e}"),
            }
        }
        if self.tool_slot.is_none() {
            match ToolWindow::new(gpu, ppp, dark, state) {
                Ok(w) => self.tool_slot = Some(w),
                Err(e) => eprintln!("[preload] timer: {e}"),
            }
        }
        if self.note_slot.is_none() {
            match NoteWindow::new(gpu, ppp, dark) {
                Ok(w) => self.note_slot = Some(w),
                Err(e) => eprintln!("[preload] notes: {e}"),
            }
        }
        if self.group_slot.is_none() {
            match GroupWindow::new(gpu, ppp, dark) {
                Ok(w) => self.group_slot = Some(w),
                Err(e) => eprintln!("[preload] group: {e}"),
            }
        }
        if trace::geom() {
            eprintln!("[preload] {}/6 panels ready", self.preloaded_count());
        }
    }

    /// Panels that are already built but hidden; used by the visibility list.
    #[must_use]
    pub(super) fn preloaded_count(&self) -> usize {
        [
            self.calendar_slot.is_some(),
            self.cc_slot.is_some(),
            self.resmon_slot.is_some(),
            self.tool_slot.is_some(),
            self.note_slot.is_some(),
            self.group_slot.is_some(),
        ]
        .iter()
        .filter(|built| **built)
        .count()
    }
}
