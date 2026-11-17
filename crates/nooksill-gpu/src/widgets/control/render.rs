//! Control centre painting.

use super::paint::draw_panel;
use super::*;

impl CcWindow {
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let tiles = self.tiles.clone();
        let volume = self.volume;
        let mute = self.mute;
        let has_audio = self.audio.is_some();
        let bright = self.bright_percent;
        let bright_ready = self.brightness.is_some();
        let toggles = self.toggles.clone();
        let hover = self.hover;
        let icon_accent = self.icon_accent;
        let battery = Battery::read();
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        let mut hits = Vec::new();
        let mut slider = EguiRect::NOTHING;
        let mut bright_slider = EguiRect::NOTHING;
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            let (h, s, bs) = draw_panel(
                ui,
                dark,
                frosted,
                &tiles,
                volume,
                mute,
                has_audio,
                battery,
                &toggles,
                bright,
                bright_ready,
                hover,
                icon_accent,
            );
            hits = h;
            slider = s;
            bright_slider = bs;
        })?;
        self.hits = hits;
        self.slider = slider;
        self.bright_slider = bright_slider;
        // Same as the timer panel: push the alarm out after painting and wake
        // on events only, otherwise `due()` stays true and the whole Dock
        // submits at full rate while the panel is open.
        self.next_repaint = Instant::now() + Duration::from_secs(3600);
        Ok(())
    }
}
