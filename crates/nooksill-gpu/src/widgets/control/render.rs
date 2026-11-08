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
        let energy = self.energy;
        let has_audio = self.audio.is_some();
        let hover = self.hover;
        let icon_accent = self.icon_accent;
        let battery = Battery::read();
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        let mut hits = Vec::new();
        let mut slider = EguiRect::NOTHING;
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            let (h, s) = draw_panel(
                ui,
                dark,
                frosted,
                &tiles,
                volume,
                mute,
                energy,
                has_audio,
                battery,
                hover,
                icon_accent,
            );
            hits = h;
            slider = s;
        })?;
        self.hits = hits;
        self.slider = slider;
        // Same as the timer panel: push the alarm out after painting and wake
        // on events only, otherwise `due()` stays true and the whole Dock
        // submits at full rate while the panel is open.
        self.next_repaint = Instant::now() + Duration::from_secs(3600);
        Ok(())
    }
}
