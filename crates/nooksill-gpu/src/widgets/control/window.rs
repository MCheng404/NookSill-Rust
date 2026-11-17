//! Control centre lifecycle: creation, placement and pending tile order.

use super::win::{class_pcwstr, physical_size, wndproc};
use super::*;
use crate::widgets::framework::{self, PanelKind};

impl CcWindow {
    /// Creates the window (hidden).
    pub fn new(
        gpu: &Gpu,
        ppp: f32,
        dark: bool,
        hotspot_on: bool,
        order: &[Tile],
    ) -> Result<Pin<Box<Self>>, GpuError> {
        let hinst =
            unsafe { GetModuleHandleW(None) }.map_err(|_| GpuError::Win32("GetModuleHandleW"))?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            lpszClassName: class_pcwstr(),
            // An empty class cursor shows the busy cursor on enter.
            hCursor: crate::cursor::class_cursor(),
            ..Default::default()
        };
        unsafe {
            RegisterClassW(&raw const wc);
        }

        // Order comes from `[widgets] Order`; an empty table means the
        // default order. The network tile follows the real physical link.
        let tiles = control::tiles_ordered(
            order,
            hotspot_on,
            nooksill_platform::adapter::wired_connected(),
        );
        let (pw, ph) = physical_size(ppp, tiles.len());
        let title = windows::core::HSTRING::from(nooksill_core::i18n::t("Control Center"));
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_pcwstr(),
                windows::core::PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                pw,
                ph,
                None,
                None,
                Some(windows::Win32::Foundation::HINSTANCE(hinst.0)),
                None,
            )
        }
        .map_err(|_| GpuError::Win32("CreateWindowExW (control center)"))?;

        let lease = crate::widgets::framework::WindowLease::new(hwnd);
        let renderer = FrameRenderer::new(gpu, pw as u32, ph as u32, ppp)?;
        let hwnd = lease.defuse();
        let audio = Audio::new();
        let volume = audio.as_ref().and_then(Audio::volume).unwrap_or(50);
        let mute = audio.as_ref().map(Audio::muted).unwrap_or(false);

        let mut win = Box::pin(Self {
            hwnd,
            renderer,
            input: crate::input::Input::default(),
            pos: (0, 0),
            ppp,
            visible: false,
            dark,
            dirty_theme: false,
            tiles,
            audio,
            volume,
            mute,
            icon_accent: None,
            hover: None,
            hits: Vec::new(),
            slider: EguiRect::NOTHING,
            dragging: false,
            press: None,
            drag_tile: None,
            down: (0.0, 0.0),
            moved: false,
            pending_order: None,
            toggles: Toggles::default(),
            // Probing DDC/CI is slow and may block: do it on first open, not
            // during preload.
            brightness: None,
            bright_percent: 50,
            bright_slider: EguiRect::NOTHING,
            dragging_bright: false,
            bright_write: Instant::now() - Duration::from_secs(1),
            pending_bright: None,
            next_repaint: Instant::now(),
        });
        win.attach();
        Ok(win)
    }

    pub(super) fn attach(&mut self) {
        unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_USERDATA,
                std::ptr::from_mut::<Self>(self) as isize,
            );
        }
    }

    /// Window handle. The main loop uses it to test whether a click landed
    /// inside the panel.
    #[must_use]
    pub const fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Opens the panel next to the control centre card.
    pub fn show_at(
        &mut self,
        gpu: &Gpu,
        anchor: (f32, f32, f32, f32),
        edge: nooksill_core::config::Edge,
    ) -> Result<(), GpuError> {
        // Re-read the system volume and power plan on every open: they may
        // have changed elsewhere.
        if let Some(a) = &self.audio {
            self.volume = a.volume().unwrap_or(self.volume);
            self.mute = a.muted();
        }
        // The network tile is re-checked on every open because a cable may
        // have been plugged in or pulled out. Only that tile is replaced, so
        // the user's tile order and the window size survive.
        let net = if nooksill_platform::adapter::wired_connected() {
            nooksill_core::control::Tile::Ethernet
        } else {
            nooksill_core::control::Tile::Wifi
        };
        for t in &mut self.tiles {
            if t.is_net() {
                *t = net;
            }
        }
        // Radios are re-read on every open: the user may have toggled them
        // from the system flyout.
        self.refresh_toggles();
        // DDC/CI is probed on every open: monitor handles die on a mode
        // change, and the slider row only exists when the probe answers.
        self.brightness = nooksill_platform::brightness::Brightness::probe();
        if let Some(b) = &self.brightness {
            self.bright_percent = b.percent();
        }
        let (pw, ph) = physical_size(self.ppp, self.tiles.len());
        if (pw as u32, ph as u32) != self.renderer.size() {
            self.renderer.resize(gpu, pw as u32, ph as u32)?;
        }
        let (x, y) = framework::show_panel(
            &mut self.renderer,
            self.hwnd,
            PanelKind::ControlCenter,
            (pw, ph),
            Some(anchor),
            edge,
            self.ppp,
        );
        self.pos = (x, y);
        self.visible = true;
        self.next_repaint = Instant::now();
        Ok(())
    }

    /// Hides the panel. Releases the mouse capture.
    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        framework::hide_panel(&mut self.renderer, self.hwnd);
    }

    #[must_use]
    pub fn due(&mut self) -> bool {
        self.visible && (self.next_repaint <= Instant::now() || self.renderer.frost_pending())
    }

    /// Sets the icon theme color.
    ///
    /// The main loop calls this each frame: the parsed RGB when the accent
    /// toggle is on, `None` otherwise so icons fall back to a neutral ink.
    pub fn set_icon_accent(&mut self, rgb: Option<[u8; 3]>) {
        self.icon_accent = rgb.map(|c| Color32::from_rgb(c[0], c[1], c[2]));
    }

    /// Takes the "tile order changed" notice, produced once per drag.
    ///
    /// The panel never writes config: `Config` belongs to the main loop and
    /// disk writes go through a debounced queue.
    pub fn take_order(&mut self) -> Option<Vec<Tile>> {
        self.pending_order.take()
    }
}
