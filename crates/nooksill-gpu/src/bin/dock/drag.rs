//! Bar reordering rules and drop-target hit tests.

use super::*;

/// Where a bar reorder landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BarMove {
    /// No move: same slot, out of range, or a pinned item dragged into the
    /// running segment.
    None,
    /// Swap between pinned items; persisted.
    Pinned,
    /// Swap between running apps; order lives only in `temp_keys`.
    Running,
    /// A running app dropped into the pinned range is pinned in place;
    /// persisted.
    PinnedNow,
}

/// Apply one bar reorder across the pinned (`raw`) and running (`temps`)
/// lists.
///
/// Index space is `raw` first, `temps` after. A running-order swap must also be
/// written back to `temp_keys`; editing `temps` alone lets the next scan
/// (~1.2 s) overwrite the order the user just dragged. Dropping a running item
/// into `raw` pins it in place. The reverse direction is blocked by
/// [`Dock::clamp_segment`] and falls through to [`BarMove::None`].
pub(super) fn reorder_bar(
    raw: &mut Vec<Item>,
    temps: &mut Vec<Item>,
    temp_keys: &mut Vec<String>,
    from: usize,
    to: usize,
) -> BarMove {
    if from == to {
        return BarMove::None;
    }
    let n = raw.len();
    if from < n && to < n {
        let item = raw.remove(from);
        raw.insert(to, item);
        return BarMove::Pinned;
    }
    if from >= n && to >= n {
        let (k, t) = (from - n, to - n);
        // `temps` and `temp_keys` are parallel lists; both indices must be in
        // range.
        if k < temps.len() && t < temps.len() && k < temp_keys.len() && t < temp_keys.len() {
            let temp = temps.remove(k);
            temps.insert(t, temp);
            let key = temp_keys.remove(k);
            temp_keys.insert(t, key);
            return BarMove::Running;
        }
        return BarMove::None;
    }
    if from >= n && to < n {
        // Running app into the pinned range: move it into the config list at
        // the drop slot.
        let k = from - n;
        if k < temps.len() {
            let mut item = temps.remove(k);
            if k < temp_keys.len() {
                let _ = temp_keys.remove(k);
            }
            item.temp = false;
            raw.insert(to, item);
            return BarMove::PinnedNow;
        }
    }
    BarMove::None
}

/// Whether the cursor is over something clickable (item icon or widget card).
///
/// `WM_SETCURSOR` runs without capture and `WM_MOUSEMOVE` while dragging; both
/// must give the same answer.
pub(super) fn dock_on_item(dock: &Dock, u: f32, v: f32) -> bool {
    dock.hit_at(u, v).is_some() || dock.widget_at(u, v).is_some()
}

/// Cursor shape: up/down arrows on separators, hand elsewhere.
///
/// Must stay the single source for both `WM_MOUSEMOVE` and `WM_SETCURSOR`: the
/// system sends no `WM_SETCURSOR` while the mouse is captured, so duplicate
/// logic reverts to the arrow mid-drag.
pub(super) fn dock_cursor(dock: &Dock, u: f32, v: f32) {
    if dock.sep_cursor(u, v) {
        nooksill_gpu::cursor::set(nooksill_gpu::cursor::size_ns());
    } else {
        nooksill_gpu::cursor::set_hand_if(dock_on_item(dock, u, v));
    }
}
