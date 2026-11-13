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
mod nav;
mod options;
mod performance;
mod position;
mod preview;
mod update;
mod widgets;

use egui::Ui;
use nooksill_core::config::Config;
use nooksill_core::item::Item;

pub use nav::{NavGroup, NavItem};
pub(crate) use widgets::Ctx;
pub use widgets::{Modal, ModalAction};

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
    UpdatePolicy,
    UpdateNetwork,
    About,
}

impl Page {
    pub const ALL: [Page; 14] = [
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
        Page::UpdatePolicy,
        Page::UpdateNetwork,
        Page::About,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Page::General => nooksill_core::i18n::t("General"),
            Page::Appearance => nooksill_core::i18n::t("Appearance"),
            Page::Position => nooksill_core::i18n::t("Position"),
            Page::Icons => nooksill_core::i18n::t("Icons"),
            Page::Interaction => nooksill_core::i18n::t("Interaction"),
            Page::Extras => nooksill_core::i18n::t("Extras"),
            Page::Effect => nooksill_core::i18n::t("Effects"),
            Page::Performance => nooksill_core::i18n::t("Performance"),
            Page::Items => nooksill_core::i18n::t("Items"),
            Page::Library => nooksill_core::i18n::t("Icon Library"),
            Page::Add => nooksill_core::i18n::t("Add"),
            Page::UpdatePolicy => nooksill_core::i18n::t("Update policy"),
            Page::UpdateNetwork => nooksill_core::i18n::t("Network"),
            Page::About => nooksill_core::i18n::t("About"),
        }
    }

    /// One-line description under the page header.
    #[must_use]
    pub fn subtitle(self) -> &'static str {
        match self {
            Page::General => {
                nooksill_core::i18n::t("Startup behaviour, layout lock and global hotkeys.")
            }
            Page::Appearance => {
                nooksill_core::i18n::t("Bar size, background material and colours.")
            }
            Page::Position => nooksill_core::i18n::t("Dock edge, monitor and visibility mode."),
            Page::Icons => {
                nooksill_core::i18n::t("Icon plates, separators, running dots and labels.")
            }
            Page::Interaction => {
                nooksill_core::i18n::t("Hover zoom, folder fan-out and animation.")
            }
            Page::Extras => nooksill_core::i18n::t("Clock, reflection, widgets and browser."),
            Page::Effect => nooksill_core::i18n::t("Ambient smoke around the bar."),
            Page::Performance => {
                nooksill_core::i18n::t("GPU selection, live usage and costly feature switches.")
            }
            Page::Items => nooksill_core::i18n::t("Items on the Dock: rename, reorder and group."),
            Page::Library => nooksill_core::i18n::t("Apply icons in bulk from a folder."),
            Page::Add => {
                nooksill_core::i18n::t("Add things to the Dock, import and export the config.")
            }
            Page::UpdatePolicy => {
                nooksill_core::i18n::t("Release channel, update popup and automatic updates.")
            }
            Page::UpdateNetwork => {
                nooksill_core::i18n::t("DNS checks, mirror selection and experimental switches.")
            }
            Page::About => nooksill_core::i18n::t("Version, project URL, author and licence."),
        }
    }
}

/// UI-only state, kept out of `Config`: page position and live stats must not
/// reach the INI file.
#[derive(Clone, Debug)]
pub struct SettingsState {
    pub page: Page,
    /// Open group; while set, the sidebar shows its content tabs instead.
    pub sub: Option<NavGroup>,
    /// Primary page to restore when the back row is pressed.
    pub return_page: Page,
    /// The one modal on screen, drawn above a softened settings surface.
    pub modal: Option<Modal>,
    /// The update popup has already been offered in this window session.
    pub update_prompted: bool,
    /// The "ready to install" popup has already been offered this session.
    pub update_ready_prompted: bool,
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
    /// Text in the global hotkey field; initialised from `cfg` on first paint.
    pub hotkey_input: String,
    /// Whether the last hotkey text failed to parse.
    pub hotkey_error: bool,
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
            sub: None,
            return_page: Page::General,
            modal: None,
            update_prompted: false,
            update_ready_prompted: false,
            monitors: vec![nooksill_core::i18n::t("Primary monitor").to_owned()],
            auto_start: false,
            config_dir: String::new(),
            stats: nooksill_core::i18n::t("No samples yet").to_owned(),
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
            hotkey_input: String::new(),
            hotkey_error: false,
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
    /// Version text of the offered release, for the popup.
    pub version: String,
    /// Release notes, trimmed by the popup.
    pub notes: String,
    /// The offered release belongs to the beta channel.
    pub prerelease: bool,
    /// Bytes received by the download in progress.
    pub bytes_done: u64,
    /// Asset size from release metadata; 0 means unknown.
    pub bytes_total: u64,
}

impl UpdateView {
    /// `None` outside an active download, or when the server gave no size.
    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        if !self.busy || self.bytes_total == 0 {
            return None;
        }
        Some((self.bytes_done as f32 / self.bytes_total as f32).clamp(0.0, 1.0))
    }
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
    normalize_nav(state);
    update::maybe_prompt(state, cfg);
    update::maybe_ready_prompt(state, cfg);
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
        let rows = match ctx.state.sub {
            Some(group) => group.tabs().len() + 1,
            None => NavItem::PRIMARY.len(),
        };
        let nav_h = if rows > widgets::MAX_LABELS {
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
        // Above the scrolling cards, so the sketch stays visible while the
        // settings below it are scrolled.
        if ctx.state.sub.is_some_and(NavGroup::shows_preview) {
            preview::preview(ctx.cfg, &mut content_ui);
        }
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
                Page::UpdatePolicy => update::policy_page(&mut ctx, ui),
                Page::UpdateNetwork => update::network_page(&mut ctx, ui),
                Page::About => about::page(&mut ctx, ui),
            });

        // Bubbles last: every row's hover and click state is settled by now, so
        // the bubble never lags a frame behind. The modal goes above both.
        widgets::render_tip(ui, ctx.state);
        widgets::render_modal(ui, ctx.cfg, ctx.state, ctx.out);
    }
    out
}

/// A page owned by a group is always shown with that group's tab strip, even
/// when the host sets `page` directly (headless tests do this).
fn normalize_nav(state: &mut SettingsState) {
    match state.sub {
        None => {
            if let Some(group) = NavGroup::containing(state.page) {
                state.return_page = Page::General;
                state.sub = Some(group);
            }
        }
        Some(group) => {
            if !group.tabs().contains(&state.page) {
                state.sub = NavGroup::containing(state.page);
            }
        }
    }
}
