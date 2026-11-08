//! `Config` defaults, clamping and per-key read/write.

use super::*;

impl Config {
    /// Default config.
    ///
    /// Geometry is stored in 96 DPI logical pixels, so the same ini renders
    /// the same apparent size at 100% and 150%; the DPI factor is applied at
    /// render time only (see [`Self::migrate_units`]).
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            icon_size: 48,
            zoom_size: 84,
            spacing: 6,
            margin: 8,
            pad: 8,
            radius: 18,
            opacity: 60,
            blur: 6,

            zoom: true,
            dots: true,
            labels: true,
            bounce: true,
            web_app: true,
            border: true,
            reserve: false,
            preview: true,
            show_running: true,
            lock: false,
            hotkey: true,
            accent_tone: false,
            reflect: false,
            clock: false,
            folder_fan: true,
            min_anim: true,
            notes: false,

            theme: DockTheme::Dark,
            ui_theme: UiTheme::System,
            bg_mode: BgMode::Frost,
            mode: DockMode::Always,
            edge: Edge::Bottom,
            dot_style: DotStyle::Bar,

            monitor: 0,
            plate_radius: 22,
            bg_color1: -1,
            bg_color2: -1,

            numeric: NUMERIC_KEYS.iter().map(|row| row.3).collect(),

            adapter: String::new(),
            cc_order: String::new(),
            runtime_accent: None,
            runtime_icon_accent: None,
        }
    }

    /// Sanitize an adapter name: trim, drop control characters, cap length.
    ///
    /// The name is written into the ini and matched against the backend's
    /// enumerated names; a `\r\n` or an over-long value fails to match
    /// silently. Control characters are dropped because an ini value must not
    /// contain a real newline.
    #[must_use]
    pub fn sanitize_adapter(raw: &str) -> String {
        /// Longest adapter name seen is about 60 chars including the vendor
        /// suffix; 128 leaves room and keeps an upper bound.
        const MAX: usize = 128;
        raw.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(MAX)
            .collect()
    }

    /// Clamp every field into its legal range. Idempotent.
    pub fn clamp(&mut self) {
        for key in Key::all_keys() {
            let v = self.get_i32(key);
            self.apply_i32(key, v);
        }
    }

    /// Read one key (enums return their code, bools 0/1, colors the raw value).
    #[must_use]
    pub fn get_i32(&self, key: Key) -> i32 {
        if let Key::Numeric(i) = key {
            return self.numeric[i as usize];
        }
        match key {
            Key::IconSize => self.icon_size,
            Key::ZoomSize => self.zoom_size,
            Key::Spacing => self.spacing,
            Key::Margin => self.margin,
            Key::Padding => self.pad,
            Key::Radius => self.radius,
            Key::Opacity => self.opacity,
            Key::Blur => self.blur,
            Key::Zoom => i32::from(self.zoom),
            Key::Theme => self.theme.code(),
            Key::Dots => i32::from(self.dots),
            Key::Labels => i32::from(self.labels),
            Key::Bounce => i32::from(self.bounce),
            Key::WebAppMode => i32::from(self.web_app),
            Key::Border => i32::from(self.border),
            Key::BgMode => self.bg_mode.code(),
            Key::Mode => self.mode.code(),
            Key::Reserve => i32::from(self.reserve),
            Key::Edge => self.edge.code(),
            Key::Monitor => self.monitor,
            Key::Preview => i32::from(self.preview),
            Key::ShowRunning => i32::from(self.show_running),
            Key::Lock => i32::from(self.lock),
            Key::Hotkey => i32::from(self.hotkey),
            Key::DotStyle => self.dot_style.code(),
            Key::PlateRadius => self.plate_radius,
            Key::BgColor1 => self.bg_color1,
            Key::BgColor2 => self.bg_color2,
            Key::AccentTone => i32::from(self.accent_tone),
            Key::Reflect => i32::from(self.reflect),
            Key::Clock => i32::from(self.clock),
            Key::FolderFan => i32::from(self.folder_fan),
            Key::MinAnim => i32::from(self.min_anim),
            Key::Notes => i32::from(self.notes),
            Key::UiTheme => self.ui_theme.code(),
            // Numeric keys returned above.
            Key::Numeric(_) => unreachable!("数值键已提前处理"),
        }
    }

    /// Write one key, clamped immediately. Every source (ini, settings UI,
    /// presets) goes through here so no field is left unclamped.
    pub fn apply_i32(&mut self, key: Key, v: i32) {
        if let Key::Numeric(i) = key {
            let idx = i as usize;
            let v = if key.is_bool() {
                i32::from(v != 0)
            } else {
                let (lo, hi) = key.range();
                v.clamp(lo, hi)
            };
            self.numeric[idx] = v;
            return;
        }
        let v = if key.is_bool() {
            i32::from(v != 0)
        } else if key.is_enum() {
            // Enums are not numerically clamped: 99 clamped to 2 would silently
            // become "follow system". Out-of-range values fall back to the
            // default variant in `from_code`.
            v
        } else {
            let (lo, hi) = key.range();
            v.clamp(lo, hi)
        };
        match key {
            Key::IconSize => self.icon_size = v,
            Key::ZoomSize => self.zoom_size = v,
            Key::Spacing => self.spacing = v,
            Key::Margin => self.margin = v,
            Key::Padding => self.pad = v,
            Key::Radius => self.radius = v,
            Key::Opacity => self.opacity = v,
            Key::Blur => self.blur = v,
            Key::Zoom => self.zoom = v != 0,
            Key::Theme => self.theme = DockTheme::from_code(v),
            Key::Dots => self.dots = v != 0,
            Key::Labels => self.labels = v != 0,
            Key::Bounce => self.bounce = v != 0,
            Key::WebAppMode => self.web_app = v != 0,
            Key::Border => self.border = v != 0,
            Key::BgMode => self.bg_mode = BgMode::from_code(v),
            Key::Mode => self.mode = DockMode::from_code(v),
            Key::Reserve => self.reserve = v != 0,
            Key::Edge => self.edge = Edge::from_code(v),
            Key::Monitor => self.monitor = v,
            Key::Preview => self.preview = v != 0,
            Key::ShowRunning => self.show_running = v != 0,
            Key::Lock => self.lock = v != 0,
            Key::Hotkey => self.hotkey = v != 0,
            Key::DotStyle => self.dot_style = DotStyle::from_code(v),
            Key::PlateRadius => self.plate_radius = v,
            Key::BgColor1 => self.bg_color1 = v,
            Key::BgColor2 => self.bg_color2 = v,
            Key::AccentTone => self.accent_tone = v != 0,
            Key::Reflect => self.reflect = v != 0,
            Key::Clock => self.clock = v != 0,
            Key::FolderFan => self.folder_fan = v != 0,
            Key::MinAnim => self.min_anim = v != 0,
            Key::Notes => self.notes = v != 0,
            Key::UiTheme => self.ui_theme = UiTheme::from_code(v),
            // Numeric keys returned above.
            Key::Numeric(_) => unreachable!("数值键已提前处理"),
        }
    }

    /// Resolve the dock theme: `System` follows the system setting.
    #[must_use]
    pub fn dock_is_dark(&self, system_dark: bool) -> bool {
        match self.theme {
            DockTheme::Dark => true,
            DockTheme::Light => false,
            DockTheme::System => system_dark,
        }
    }

    /// Resolve the settings window theme. Note `UiTheme` inverts `DockTheme`.
    #[must_use]
    pub fn ui_is_dark(&self, system_dark: bool) -> bool {
        match self.ui_theme {
            UiTheme::Light => false,
            UiTheme::Dark => true,
            UiTheme::System => system_dark,
        }
    }

    /// Whether the frosted background is on (`BgMode == 2`).
    #[must_use]
    pub fn frost_enabled(&self) -> bool {
        self.bg_mode == BgMode::Frost
    }

    /// Whether the running-app scan is on (`Dots` or `ShowRunning`).
    #[must_use]
    pub fn scan_enabled(&self) -> bool {
        self.dots || self.show_running
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::defaults()
    }
}
