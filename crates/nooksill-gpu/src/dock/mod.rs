//! Dock body: layout state machine and egui painting.
//!
//! Geometry is produced by `core::layout` in physical pixels; painting converts
//! to logical points once, in `Dock::projection`.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect as EguiRect, Stroke, StrokeKind};
use std::time::{Duration, Instant};

use crate::worker::ReadyIcon;
use nooksill_core::clock::ClockLine;
use nooksill_core::color;
use nooksill_core::config::{Config, DockMode, Edge, Key};
use nooksill_core::icon::BBox;
use nooksill_core::item::{Item, ItemKind};
use nooksill_core::layout::{self, Metrics, Rect as LRect, SlotKind};

use egui::epaint::Direction;

/// Per-frame coordinate transform.
#[derive(Copy, Clone, Debug)]
struct Proj {
    /// Physical pixels to logical points.
    k: f32,
    /// Hidden-state horizontal offset, physical pixels.
    tx: f32,
    /// Hidden-state vertical offset, physical pixels.
    ty: f32,
}

/// An icon uploaded to the GPU.
///
/// Kept at source resolution; only the sample `bbox` is stored and scaling is
/// left to the GPU bilinear filter, since the size changes every frame.
#[derive(Clone)]
pub struct IconTex {
    tex: egui::TextureHandle,
    /// Source size, used to map `bbox` to UV.
    src_w: u32,
    src_h: u32,
    bbox: BBox,
    /// Fraction of the slot the content fills.
    fill: f32,
}

impl std::fmt::Debug for IconTex {
    // `TextureHandle` has no `Debug`; hand-write one so outer structs can print.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconTex")
            .field("src", &format_args!("{}x{}", self.src_w, self.src_h))
            .field("bbox", &self.bbox)
            .field("fill", &self.fill)
            .finish_non_exhaustive()
    }
}

impl IconTex {
    /// Normalized UV of the sample area.
    fn uv(&self) -> EguiRect {
        let (w, h) = (self.src_w as f32, self.src_h as f32);
        EguiRect::from_min_max(
            Pos2::new(self.bbox.x0 as f32 / w, self.bbox.y0 as f32 / h),
            Pos2::new(self.bbox.x1 as f32 / w, self.bbox.y1 as f32 / h),
        )
    }

    /// Sample area aspect, as fractions of the longer side.
    fn aspect(&self) -> (f32, f32) {
        let bw = self.bbox.width() as f32;
        let bh = self.bbox.height() as f32;
        let mx = bw.max(bh).max(1.0);
        (bw / mx, bh / mx)
    }
}

/// Runtime state of one drag reorder.
///
/// Kept together because the fields are only valid as a set; split apart, a
/// half-started drag is easy to reach.
#[derive(Clone, Debug, PartialEq)]
pub struct DragState {
    /// Current index in `items`; updated as the item is reordered.
    pub index: usize,
    /// Cursor position, physical pixels in client space.
    pub pt: (f32, f32),
    /// Whether the press has crossed the drag threshold.
    ///
    /// A press alone is not a drag; 6 px of movement is required so a click
    /// does not jitter first.
    pub active: bool,
    /// Press point, physical pixels, for the threshold check.
    press_pt: (f32, f32),
}

/// Drag threshold in physical pixels.
const DRAG_THRESHOLD: f32 = 6.0;

/// Separator scaling rate limit, matching the upstream 16 ms `sepTick`.
const SEP_THROTTLE: Duration = Duration::from_millis(16);

/// State of a separator resize drag.
#[derive(Clone, Copy, Debug)]
struct SepDrag {
    /// Client y at press, physical pixels.
    y0: f32,
    /// Icon size at press; the scale ratio is always relative to it.
    icon0: i32,
    /// Zoom size at press.
    zoom0: i32,
    /// Last time the size was actually recomputed.
    last: Instant,
}

/// Runtime state of one dock item.
#[derive(Clone, Debug)]
pub struct DockItem {
    /// Display name, used by the hover bubble.
    pub name: String,
    /// Item path; used only to carry loaded icon textures across a reload.
    pub path: String,
    pub kind: ItemKind,
    /// Current edge length, driven toward the target by the magnify animation.
    pub cur: f32,
    /// Drag slide offset.
    pub slide: f32,
    /// Whether the app is running; controls the running indicator dot.
    pub running: bool,
    /// Bounce start time; `None` while not bouncing.
    pub bounce_at: Option<Instant>,
    /// Slot left edge from the previous frame, the base for `slide`.
    pub prev_l: f32,
}

impl DockItem {
    /// Current bounce offset in logical pixels; `0` when not moving.
    ///
    /// Shared by `paint_icons` and `paint_drag_ghost` so both stay in sync.
    #[must_use]
    pub fn bounce_offset(&self, s: f32) -> f32 {
        let Some(t) = self.bounce_at else {
            return 0.0;
        };
        layout::bounce_offset(
            u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX),
            s,
        )
    }

    /// Build from a config item. `cur` starts at 0 and grows via animation.
    #[must_use]
    pub fn from_item(item: &Item) -> Self {
        Self {
            name: item.name.clone(),
            path: item.path.clone(),
            kind: item.kind,
            cur: 0.0,
            slide: 0.0,
            running: item.running,
            bounce_at: None,
            prev_l: 0.0,
        }
    }

    fn is_sep(&self) -> bool {
        self.kind == ItemKind::Separator
    }

    fn slot_kind(&self) -> SlotKind {
        if self.is_sep() {
            SlotKind::Separator
        } else {
            SlotKind::Icon
        }
    }
}

/// Derived geometry, all in physical pixels.
#[derive(Clone, Debug, Default)]
pub struct DockGeom {
    /// Bar left edge.
    pub bar_l: f32,
    /// Bar width.
    pub bar_w: f32,
    /// Bar top edge.
    pub bar_top: f32,
    /// Bar bottom edge.
    pub bar_bottom: f32,
    /// Icon bottom; icons grow upward from here.
    pub icon_bottom: f32,
    /// Left edge of each slot.
    pub slot_l: Vec<f32>,
    /// Right edge of each slot.
    pub slot_r: Vec<f32>,
    /// Actual (magnified) centre of each slot.
    pub act_c: Vec<f32>,
    /// Base (unmagnified) centre of each slot.
    pub base_c: Vec<f32>,
    /// Left edge of the reserved external-drop slot; `None` when absent.
    pub insert_l: Option<f32>,
}

/// One batch of paths dropped from the shell.
///
/// Taken together; splitting the fields can mix this batch's paths with the
/// previous batch's drop point.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DroppedBatch {
    pub paths: Vec<String>,
    /// Insert before this index in `raw`; `None` appends at the end.
    pub index: Option<usize>,
    /// `dock.items` index of the group the drop landed on.
    pub group: Option<usize>,
}

/// Shared visual constants for widget cards.
///
/// Colours and ratios live here because per-card copies drifted apart.
struct WidgetStyle;

impl WidgetStyle {
    /// Foreground colour for card content.
    #[must_use]
    fn ink(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgba_unmultiplied(235, 240, 240, 246)
        } else {
            Color32::from_rgba_unmultiplied(32, 30, 40, 240)
        }
    }

    /// Secondary text colour, dimmer but still readable.
    #[must_use]
    fn ink_sub(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgba_unmultiplied(190, 220, 220, 228)
        } else {
            Color32::from_rgba_unmultiplied(58, 56, 68, 220)
        }
    }

    /// Hairline between the time and date groups.
    #[must_use]
    fn rule(dark: bool) -> Color32 {
        if dark {
            argbc(90, 255, 255, 255)
        } else {
            argbc(95, 60, 60, 70)
        }
    }

    /// Card padding in logical points, horizontal and vertical.
    const PAD_X: f32 = 6.0;
    const PAD_Y: f32 = 4.0;
    /// Icon size as a fraction of the card short side, per icon kind.
    const ICON_TRAY: f32 = 0.30;
    const ICON_CTRL: f32 = 0.62;
    const ICON_TOOL: f32 = 0.46;
    /// Font size as a fraction of card height, with a logical-pixel floor.
    const FONT_RATIO: f32 = 0.27;
    const FONT_MIN: f32 = 9.0;
    /// Do not draw below this text width; avoids negative font sizes.
    const MIN_TEXT_WIDTH: f32 = 8.0;
}

/// The dock itself.
#[derive(Debug)]
pub struct Dock {
    /// Pending widget-card click; cards are hit-tested separately.
    pub widget_click: Option<layout::WidgetKind>,
    /// CPU sampler; needs two samples to produce a delta.
    cpu_sampler: nooksill_platform::CpuSampler,
    /// Last system sample.
    sysinfo: nooksill_platform::SysInfo,
    pub cfg: Config,
    pub items: Vec<DockItem>,
    /// Work area in physical pixels, used for available sizes.
    pub work: LRect,
    /// Cursor position inside the window, physical pixels.
    ///
    /// Same unit as `metrics` and `geom`; logical points here would skew hits
    /// at 150% DPI.
    pub mouse: (f32, f32),
    /// Whether the cursor is inside the window.
    pub hover: bool,
    /// Index of the hovered item.
    pub hot: Option<usize>,
    /// Hide progress: 0 fully shown, 1 fully hidden.
    pub hide_cur: f32,
    /// Whether this frame still animates; keeps the frame loop running.
    pub anim: bool,
    pub metrics: Metrics,
    pub geom: DockGeom,
    /// Actual centres from the previous frame, for `map_actual_to_base`.
    prev_act_c: Vec<f32>,
    /// Icon texture per item index; `None` draws the fallback glyph.
    icons: Vec<Option<IconTex>>,
    /// Set by input events; otherwise the main loop only relayouts while
    /// animating.
    dirty: bool,
    /// Item index hit on left press; used for drag and click pair checks.
    press: Option<usize>,
    /// Path of the pressed item; release checks identity by path, not by a
    /// fresh hit test.
    press_path: Option<String>,
    /// Separator resize drag in progress; `None` when idle.
    sep: Option<SepDrag>,
    /// Separator drag changed `IconSize`/`ZoomSize`; the main loop must persist.
    sep_dirty: bool,
    /// Display mode: always shown, auto-hide, or smart-hide when covered.
    pub mode: DockMode,
    /// Target hidden state, not the animated value.
    pub hide_target: bool,
    /// Hidden by the user hotkey. Outranks auto-hide and ignores the cursor.
    pub force_hide: bool,
    /// Last time the cursor was near (monotonic ms).
    last_near_ms: u64,
    /// Delay before hiding after the cursor leaves, ms.
    pub hide_delay_ms: u64,
    /// Continuous hover required to show again, ms; 0 keeps instant behaviour.
    pub show_delay_ms: u64,
    /// Start time of the current hover while hidden.
    show_pending_ms: Option<u64>,
    /// Current DPI, written every frame by `update`.
    dpi: i32,
    /// Active drag state; `None` when no item is held.
    drag: Option<DragState>,
    /// A reorder happened this frame; `update` adds the slide.
    moved: bool,
    /// Item order changed; the main loop persists it.
    order_changed: bool,
    /// Reorder queue (from -> to), applied to the config copy by the main loop.
    ///
    /// A queue, not a single value: several moves can happen in one frame.
    pending_moves: Vec<(usize, usize)>,
    /// Number of leading pinned items; the rest are running apps.
    pinned: usize,
    /// Pending click as `(index, force_new_instance)`.
    click: Option<(usize, bool)>,
    /// Pending context-menu request in client physical pixels.
    ///
    /// Deferred so the modal menu does not block `WndProc`.
    menu_at: Option<(f32, f32)>,
    /// Whether the press was Shift or middle-button, deciding the click force.
    press_force: bool,
    /// Group under the cursor during a drag; no reorder while set.
    merge_target: Option<usize>,
    /// Pending merge (dragged -> group), applied by the main loop.
    pending_merge: Option<(usize, usize)>,
    /// Cursor left the dock window; release removes the item.
    drop_outside: bool,
    /// Collapse the dragged item to preview removal.
    remove_preview: Option<usize>,
    /// Live insert index for an external drag.
    external_drop: Option<usize>,
    /// Layout changed by insert or collapse; add a slide next frame.
    slot_shift: bool,
    /// Slot left edges from the previous frame, for the shift animation.
    prev_slot_l: Vec<f32>,
    /// Pending removal of the dragged item index.
    pending_remove: Option<usize>,
    /// Paths dropped from Explorer, drained by the main loop.
    dropped_paths: Vec<String>,
    /// Insert index for this batch; `None` appends at the end.
    dropped_at: Option<usize>,
    /// `dock.items` index of the group the drop landed on.
    dropped_group: Option<usize>,
    /// Tray icon left-clicked; toggle visibility.
    tray_toggle: bool,
    /// Tray icon right-clicked; open the menu.
    tray_menu: bool,
    /// Short Dock-side notice as `(text, start time)`.
    notice: Option<(String, Instant)>,
    /// System appearance change counter, bumped on `WM_SETTINGCHANGE`.
    pub look_epoch: u32,
    /// Display topology counter, bumped on `WM_DISPLAYCHANGE`.
    ///
    /// Resolution, scaling and monitor hot-plug all invalidate the cached work
    /// area; the main loop re-derives it once per bump.
    pub geom_epoch: u32,
}

mod chrome;

use self::chrome::{argbc, clock_line_size, it_visible, r8, rounded_gradient, to_rgb, BarGradient};
mod input;
mod items;
mod paint;
mod paint_cards;
mod state;

#[cfg(test)]
mod tests;
