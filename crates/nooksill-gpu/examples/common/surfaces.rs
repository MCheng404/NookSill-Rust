//! Shared window set, layered-window creation and sampling for the memory
//! probes, so the two probes cannot drift apart or away from the product.

use nooksill_gpu::mem::MemStats;
use nooksill_gpu::{calendar_win, cc_win, group_win, note_win, resmon_win, settings_win, tool_win};

/// The six preloaded panels plus the settings window, in logical pixels.
pub fn windows() -> Vec<(&'static str, (f32, f32))> {
    let tiles = nooksill_core::control::tiles(true, false).len();
    let order = nooksill_core::resource::ResKind::DEFAULT_ORDER;
    vec![
        ("calendar", calendar_win::SIZE),
        ("control", cc_win::logical_size(tiles)),
        (
            "resource",
            (resmon_win::PANEL_W, resmon_win::logical_height(&order)),
        ),
        ("timer", tool_win::SIZE),
        ("notes", note_win::SIZE),
        (
            "group",
            group_win::logical_size(8, &group_win::FanSpec::default()),
        ),
        ("settings", settings_win::LOGICAL_SIZE),
    ]
}

/// Physical pixel sizes at the given scale.
pub fn physical(ppp: f32) -> Vec<(String, u32, u32)> {
    windows()
        .into_iter()
        .map(|(name, (lw, lh))| {
            (
                name.to_owned(),
                (lw * ppp).round() as u32,
                (lh * ppp).round() as u32,
            )
        })
        .collect()
}

/// Median of several samples: one read is a coin flip under driver jitter.
pub fn sample() -> MemStats {
    let mut ws: Vec<usize> = Vec::new();
    let mut private: Vec<usize> = Vec::new();
    for _ in 0..7 {
        let s = MemStats::now();
        ws.push(s.working_set);
        private.push(s.private);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    ws.sort_unstable();
    private.sort_unstable();
    MemStats {
        working_set: ws[ws.len() / 2],
        peak_working_set: ws[ws.len() / 2],
        private: private[private.len() / 2],
    }
}

/// Bytes to MiB. `MemStats` divides by 1024*1024 too, so its "MB" is MiB.
pub fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
