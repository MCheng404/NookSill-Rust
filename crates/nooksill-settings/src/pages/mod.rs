//! Settings pages: 12 pure-egui pages, no window or GPU dependency.
//!
//! Layout is a left navigation column plus a right stack of cards. Ranges, key
//! names and clamping all come from `core::config`; this layer only renders.

mod about;
mod about_art;
mod add;
mod appearance;
mod effect;
mod extras;
mod general;
mod icons;
mod interaction;
mod items;
mod library;
mod options;
mod performance;
mod position;
mod widgets;

use egui::Ui;
use nooksill_core::config::Config;
use nooksill_core::item::Item;

pub(crate) use widgets::Ctx;

/// Settings pages, in sidebar order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    General,
    Appearance,
    Position,
    Icons,
    Interaction,
    Extras,
    Effect,
    Performance,
    Items,
    Library,
    Add,
    About,
}

impl Page {
    pub const ALL: [Page; 12] = [
        Page::General,
        Page::Appearance,
        Page::Position,
        Page::Icons,
        Page::Interaction,
        Page::Extras,
        Page::Effect,
        Page::Performance,
        Page::Items,
        Page::Library,
        Page::Add,
        Page::About,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Page::General => "常规",
            Page::Appearance => "外观",
            Page::Position => "位置",
            Page::Icons => "图标",
            Page::Interaction => "交互",
            Page::Extras => "附件",
            Page::Effect => "效果",
            Page::Performance => "性能",
            Page::Items => "条目",
            Page::Library => "图标库",
            Page::Add => "添加",
            Page::About => "关于",
        }
    }

    /// One-line description under the page header.
    #[must_use]
    pub const fn subtitle(self) -> &'static str {
        match self {
            Page::General => "启动行为、锁定与全局快捷键。",
            Page::Appearance => "栏体尺寸、背景材质与配色。",
            Page::Position => "停靠边、显示器与显示模式。",
            Page::Icons => "底盘、分割线、运行指示与标签。",
            Page::Interaction => "悬停放大、文件夹展开与动画。",
            Page::Extras => "时钟、倒影、小组件与浏览器。",
            Page::Effect => "栏体周围的环境烟雾。",
            Page::Performance => "显卡选择、实时占用与高耗能功能开关。",
            Page::Items => "Dock 上的条目：改名、排序、分组。",
            Page::Library => "从文件夹批量设置图标。",
            Page::Add => "往 Dock 里加东西，以及配置的导入导出。",
            Page::About => "版本、项目地址、作者与许可。",
        }
    }
}

/// UI-only state, kept out of `Config`: page position and live stats must not
/// reach the INI file.
#[derive(Clone, Debug)]
pub struct SettingsState {
    pub page: Page,
    /// Monitor names, filled by the platform layer.
    pub monitors: Vec<String>,
    /// Real autostart state read from the registry, not the config value.
    pub auto_start: bool,
    pub config_dir: String,
    /// Stats text on the performance page, refreshed by the GPU layer.
    pub stats: String,
    /// Selectable adapters, excluding "auto", enumerated by the platform layer.
    pub adapters: Vec<String>,
    /// Renderer actually in use, filled by the GPU layer; not user-editable.
    pub active_adapter: String,
    /// Warning when the requested adapter differs from the active one.
    pub adapter_note: String,
    /// Bottom status bar text.
    pub status: String,
    /// The About page's hidden entry is unlocked (version clicked 5 times).
    /// Resets when the window closes.
    pub advanced: bool,
    /// Click counter for the version label.
    pub version_clicks: u8,
    /// Updater state, refreshed by the GPU layer from a background thread.
    pub update: UpdateView,
    /// Address typed into the "add web app" field.
    pub url_input: String,
    /// Icon folder selected on the library page (empty = none).
    pub icon_dir: String,
    /// Top bar search text; when non-empty only matching nav labels are shown.
    pub nav_filter: String,
    /// Currently expanded help bubble, pinned by click or opened by 3 s hover.
    pub tip: Option<Tip>,
    /// Hovered tip dot with its start instant; the 3 s delay spans frames.
    pub tip_hover: Option<(egui::Id, std::time::Instant)>,
    /// Card heights of the previous frame, keyed by base top-left, for magnets.
    pub card_h_prev: Vec<(egui::Pos2, f32)>,
    /// Card heights of this frame, becoming `card_h_prev` next frame.
    pub card_h: Vec<(egui::Pos2, f32)>,
    /// Row rects of the previous frame keyed by base top-left; the hover band
    /// is drawn below row content.
    pub row_bands_prev: Vec<(egui::Pos2, egui::Rect)>,
    /// Row rects of this frame, becoming `row_bands_prev` next frame.
    pub row_bands: Vec<(egui::Pos2, egui::Rect)>,

    /// Copy of the dock items, synced by the host program, which owns them.
    pub items: Vec<Item>,
    /// Index of the item being edited; `None` when nothing is open.
    pub editing: Option<usize>,
    /// Name being edited.
    pub edit_name: String,
    /// Path being edited (file, folder or URL).
    pub edit_path: String,
    /// Launch arguments being edited.
    pub edit_args: String,
    /// Group index to merge into; `None` keeps the item on the bar.
    pub edit_group: Option<usize>,
}

/// An expanded help bubble.
#[derive(Clone, Debug)]
pub struct Tip {
    /// Owning dot; reusing it keeps the bubble alive so a click toggles it
    /// instead of rebuilding.
    pub id: egui::Id,
    pub text: String,
    /// Rect of the dot; the bubble anchors below it, or above when it does
    /// not fit.
    pub anchor: egui::Rect,
    /// Pinned bubbles survive the cursor moving away.
    pub pinned: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            page: Page::General,
            monitors: vec!["主显示器".to_owned()],
            auto_start: false,
            config_dir: String::new(),
            stats: "尚无采样".to_owned(),
            adapters: Vec::new(),
            active_adapter: String::new(),
            adapter_note: String::new(),
            status: String::new(),
            nav_filter: String::new(),
            tip: None,
            tip_hover: None,
            card_h_prev: Vec::new(),
            card_h: Vec::new(),
            row_bands_prev: Vec::new(),
            advanced: false,
            version_clicks: 0,
            update: UpdateView::default(),
            url_input: String::new(),
            icon_dir: String::new(),
            row_bands: Vec::new(),
            items: Vec::new(),
            editing: None,
            edit_name: String::new(),
            edit_path: String::new(),
            edit_args: String::new(),
            edit_group: None,
        }
    }
}

/// Editable fields of an item; name, path and arguments are applied together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemEdit {
    pub name: String,
    pub path: String,
    pub args: String,
    /// Group index to merge into; `None` keeps the item on the bar.
    pub group: Option<usize>,
}

/// Actions the host program must perform (dialogs, registry, item edits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Close,
    ResetDefaults,
    LowPower,
    Recommended,
    OpenConfigDir,
    ToggleAutostart(bool),
    PickIconDir,
    AutoMatch,
    AddUrl,
    AddFile,
    AddFolder,
    NewGroup,
    AddSeparator,
    AddTrash,
    ExportAll,
    Import,
    /// Open a link in the default browser.
    OpenUrl(&'static str),
    /// Check for updates on a background thread; the result lands in
    /// [`UpdateView`].
    CheckUpdate,
    /// Download and verify a new version.
    DownloadUpdate,
    /// Replace this binary with the downloaded one and restart.
    ApplyUpdate,
    /// Change name, path, arguments and group of item `index`.
    EditItem(usize, ItemEdit),
    /// Remove item `index`.
    RemoveItem(usize),
    /// Move item `index` up (`-1`) or down (`+1`).
    MoveItem(usize, i32),
    /// Move child `kid` of group `index` back onto the bar.
    ExtractKid(usize, usize),
    /// Dissolve group `index`, flattening its children onto the bar.
    DissolveGroup(usize),
    /// Pick a custom icon file for item `index`.
    PickIcon(usize),
    /// Clear the custom icon of item `index`.
    ResetIcon(usize),
    /// Move child `kid` of group `group` up (`-1`) or down (`+1`).
    MoveKid(usize, usize, i32),
    /// Add a note item to the dock.
    AddNote,
    /// Open the notes panel.
    OpenNotes,
}

/// The updater as seen by the UI.
///
/// Strings and flags only: this layer does not know the network types, the GPU
/// layer translates `update::Release` into them.
#[derive(Clone, Debug, Default)]
pub struct UpdateView {
    /// One status line ("checking", "up to date", "found 1.0.1", or an error).
    pub status: String,
    /// A newer version can be downloaded.
    pub available: bool,
    /// Downloaded and verified, waiting for a restart.
    pub ready: bool,
    /// A request is in flight; buttons are disabled.
    pub busy: bool,
}

/// Result of one frame.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub changed: bool,
    pub command: Option<Command>,
}

/// Render the whole settings UI: navigation column plus card content.
///
/// The status line is drawn by the window layer, not here.
pub fn settings_ui(ui: &mut Ui, cfg: &mut Config, state: &mut SettingsState) -> Outcome {
    let mut out = Outcome::default();
    {
        let mut ctx = Ctx {
            cfg,
            state,
            out: &mut out,
        };

        // Split the rect by hand instead of using `SidePanel`, which lays out
        // against the root UI's available area and would fight the title bar
        // drawn by an outer child UI.
        //
        // Must start from `available_rect_before_wrap()`: `max_rect()` does not
        // shrink with content, so the columns would cover the title bar.
        let rect = ui.available_rect_before_wrap();
        // Navigation height is the window height; past `MAX_LABELS` rows it is
        // capped and the rest scrolls.
        let nav_h = if Page::ALL.len() > widgets::MAX_LABELS {
            (widgets::MAX_LABELS as f32 * (widgets::NAV_ITEM_H + 3.0) + 20.0).min(rect.height())
        } else {
            rect.height()
        };
        let nav_rect = egui::Rect::from_min_size(rect.min, egui::vec2(widgets::NAV_W, nav_h));
        let content_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + widgets::NAV_W + 14.0, rect.top()),
            egui::pos2(rect.right(), rect.bottom()),
        );

        // Translucent sidebar fill plus a 1 px stroke: alpha alone reads as
        // shapeless over the frosted backdrop.
        let dark = ui.visuals().dark_mode;
        ui.painter().rect_filled(
            nav_rect,
            egui::CornerRadius::same(18),
            widgets::sidebar_fill(ui),
        );
        ui.painter().rect_stroke(
            nav_rect,
            egui::CornerRadius::same(18),
            egui::Stroke::new(
                1.0,
                if dark {
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
                } else {
                    egui::Color32::from_rgba_unmultiplied(23, 26, 35, 14)
                },
            ),
            egui::StrokeKind::Inside,
        );

        // Inset by 6 px so the selection pill does not touch the edge.
        let mut nav_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(nav_rect.shrink2(egui::vec2(6.0, 6.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        widgets::sidebar(&mut nav_ui, ctx.state);

        // Card heights and row rects describe the previous frame, so they must
        // be cleared before drawing: the first frame of a new page would
        // otherwise compute magnets and hover bands from the old page's
        // geometry.
        ctx.state.card_h_prev = std::mem::take(&mut ctx.state.card_h);
        ctx.state.row_bands_prev = std::mem::take(&mut ctx.state.row_bands);

        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let page = ctx.state.page;
        widgets::page_header(&mut content_ui, page);
        content_ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut content_ui, |ui| match page {
                Page::General => general::page(&mut ctx, ui),
                Page::Appearance => appearance::page(&mut ctx, ui),
                Page::Position => position::page(&mut ctx, ui),
                Page::Icons => icons::page(&mut ctx, ui),
                Page::Interaction => interaction::page(&mut ctx, ui),
                Page::Extras => extras::page(&mut ctx, ui),
                Page::Effect => effect::page(&mut ctx, ui),
                Page::Performance => performance::page(&mut ctx, ui),
                Page::Items => items::page(&mut ctx, ui),
                Page::Library => library::page(&mut ctx, ui),
                Page::Add => add::page(&mut ctx, ui),
                Page::About => about::page(&mut ctx, ui),
            });

        // Bubbles last: every row's hover and click state is settled by now, so
        // the bubble never lags a frame behind.
        widgets::render_tip(ui, ctx.state);
    }
    out
}
