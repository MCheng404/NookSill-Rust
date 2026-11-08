//! Small helpers: timing constants, icon requests, cursor proximity and raw input.

use super::*;

/// Minimum interval between fullscreen checks, in milliseconds.
///
/// Two read-only queries that trigger no rendering, so this only bounds how
/// long the dock takes to step aside. One second is quick enough for the user
/// and caps idle wakeups at one per second.
pub(super) const FS_CHECK_MS: u32 = 1000;

/// Maximum number of transient (running, unpinned) entries shown.
///
/// The bar shrinks as items grow (floor 25%), so dozens of transient icons
/// would squeeze it into a thin line. 16 covers a typical desktop.
pub(super) const MAX_TEMP: usize = 16;

/// Display name for a transient entry: shell display name, file stem fallback.
pub(super) fn temp_name(exe: &str) -> String {
    if let Some(n) = nooksill_platform::icon::display_name(exe) {
        if !n.is_empty() {
            return n;
        }
    }
    std::path::Path::new(exe)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(exe)
        .to_owned()
}

/// Debounce window between a settings change and the write, in milliseconds.
///
/// A slider drag emits dozens of changes and the ini is rewritten as full
/// UTF-16, so 400 ms means "save when the hand stops".
pub(super) const CFG_SAVE_DEBOUNCE_MS: u64 = 400;

/// Save debounce for the timer panel, in milliseconds.
///
/// Shorter than the config window: checking a todo is one discrete action, but
/// a running stopwatch changes its remaining time every frame.
pub(super) const TOOL_SAVE_DEBOUNCE_MS: u64 = 120;

/// Queue icons for the items that can have one.
///
/// Separators draw a line; a custom `icon_src` and URLs are unsupported (no
/// PNG/JPEG decoder, no network fetch for favicons) and fall back to the
/// initial glyph; everything else goes through the shell image list.
pub(super) fn request_missing_icons(loader: &IconLoader, dock: &Dock, raw: &[Item]) {
    for (i, item) in raw.iter().enumerate() {
        // Separators have no icon, and existing icons are skipped:
        // `replace_items` carries textures over by path, so re-requesting
        // everything flashes the initial glyphs.
        if item.kind == ItemKind::Separator
            || dock.has_icon(i)
            || nooksill_gpu::glyph::Builtin::from_path(&item.path).is_some()
        {
            continue;
        }
        loader.submit(IconRequest {
            index: i,
            kind: item.kind,
            path: item.path.clone(),
            icon_src: item.icon_src.clone(),
            fit: item.fit,
            // A group has no icon of its own; tile its children.
            kids: item
                .kids
                .iter()
                .map(|k| nooksill_gpu::worker::IconKid {
                    kind: k.kind,
                    path: k.path.clone(),
                    icon_src: k.icon_src.clone(),
                })
                .collect(),
        });
    }
}

/// Inputs and result of one proximity test.
///
/// The cursor and hot rect are carried for diagnostics: a bare `near=false`
/// cannot tell "cursor far away" from "hot rect miscalculated".
#[derive(Clone, Copy, Debug)]
pub(super) struct Near {
    pub(super) hit: bool,
    pub(super) cursor: (i32, i32),
    /// Hot rect in screen coordinates.
    pub(super) hot: (f32, f32, f32, f32),
}

/// Whether the cursor counts as touching the dock.
///
/// Only gathers screen and window coordinates; the decision lives in
/// [`layout::near_dock`] so unit tests can pin edge cases.
pub(super) fn cursor_near(dock: &Dock, hwnd: HWND) -> Near {
    let mut p = POINT::default();
    let mut wr = RECT::default();
    if unsafe { GetCursorPos(&raw mut p) }.is_err()
        || unsafe { GetWindowRect(hwnd, &raw mut wr) }.is_err()
    {
        return Near {
            hit: false,
            cursor: (p.x, p.y),
            hot: (0.0, 0.0, 0.0, 0.0),
        };
    }
    let (hx, hy, hw, hh) = dock.hot_phys();
    let hot = (wr.left as f32 + hx, wr.top as f32 + hy, hw, hh);
    Near {
        hit: layout::near_dock(
            dock.cfg.edge,
            (p.x, p.y),
            hot,
            dock.work,
            dock.inner_reach(),
            !dock.hide_target,
        ),
        cursor: (p.x, p.y),
        hot,
    }
}

/// Resolve `.lnk` items to real exe paths, cached in `Item.target`.
///
/// Runs once at startup: resolving initializes COM and creates shell objects,
/// too heavy for the 1.2 s scan loop. Unresolvable targets (documents, URLs,
/// deleted files) stay empty.
pub(super) fn resolve_targets(items: &mut [Item]) {
    let mut resolved = 0usize;
    for item in items.iter_mut() {
        // Only file items have an executable target; separators, URLs, folders
        // and the trash are skipped.
        if item.kind != ItemKind::File {
            continue;
        }
        if let Some(sc) = shortcut::resolve(&item.path) {
            item.target = sc.target;
            resolved += 1;
        }
    }
    if trace::geom() {
        eprintln!("[scan] 解析出 {resolved} 个可执行目标");
    }
}

/// Display name for a URL item: the host without `www.`.
///
/// No page title: that needs the network and HTML parsing and can stall the UI.
pub(super) fn url_host(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        "网页应用".to_owned()
    } else {
        host.to_owned()
    }
}

/// Recommended preset: only switches effects on; sizes, colors and the dock
/// edge stay as the user set them.
pub(super) fn apply_recommended(cfg: &mut Config) {
    for (key, v) in [
        (Key::Labels, 1),
        (Key::Zoom, 1),
        (Key::Reflect, 1),
        (Key::Dots, 1),
        (Key::Bounce, 1),
        (Key::MinAnim, 1),
        (Key::Border, 1),
        (Key::Preview, 1),
    ] {
        cfg.apply_i32(key, v);
    }
}

/// Low power preset: off for everything that costs extra work per frame.
///
/// Frost (`Blur` + `BgMode`) is the only constant cost; the rest only skip
/// occasional extra work.
pub(super) fn apply_low_power(cfg: &mut Config) {
    for (key, v) in [
        (Key::Reflect, 0),
        (Key::Blur, 0),
        (Key::MinAnim, 0),
        (Key::Bounce, 0),
        (Key::Preview, 0),
        (Key::FolderFan, 0),
        (Key::ShowRunning, 0),
        (Key::BgMode, 0),
    ] {
        cfg.apply_i32(key, v);
    }
}

/// Build the single-frame `RawInput` shared by the dock and the menu.
///
/// `screen_rect` must be right or egui lays the bar out in the wrong place.
/// `native_pixels_per_point` drives native scaling; left unset, egui assumes
/// 1.0 and the image covers only 1/ppp of the surface.
pub(super) fn raw_input(size: (u32, u32), ppp: f32, start: Instant) -> egui::RawInput {
    let (w, h) = size;
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32 / ppp, h as f32 / ppp),
        )),
        time: Some(start.elapsed().as_secs_f64()),
        ..Default::default()
    };
    raw.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(ppp),
            ..Default::default()
        },
    );
    raw
}
