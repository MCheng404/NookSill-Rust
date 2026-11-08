//! Group panel composition, icon requests and hit testing.

use super::paint::paint;
use super::*;

impl GroupWindow {
    /// Paints one frame.
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        self.next_repaint = None;
        self.request_icons();

        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let title = self.title.clone();
        let kids = self.kids.clone();
        let hover = self.hover;
        let released = self.released;
        let frosted = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);

        // Icon textures must be uploaded in this context. The results are
        // drained first (`drain` does not block) and `load_texture` runs
        // inside the paint closure, so `self.renderer` and `self.loader` are
        // never borrowed mutably at the same time.
        let ready: Vec<crate::worker::ReadyIcon> =
            self.loader.as_ref().map(|l| l.drain()).unwrap_or_default();
        let mut icons = self.icons.clone();
        let mut actions: Vec<GroupAction> = Vec::new();
        let mut dissolve = false;

        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            for r in &ready {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [r.width as usize, r.height as usize],
                    &r.rgba,
                );
                let tex = ui.ctx().load_texture(
                    format!("group-kid-{}", r.index),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                icons.insert(r.index, tex);
            }
            let (kid_actions, dis) =
                paint(ui, dark, frosted, &title, &kids, &icons, hover, released);
            actions = kid_actions;
            dissolve = dis;
        })?;

        self.icons = icons;
        // `released` is a one-shot event: without clearing it, the next
        // repaint caused by a hover or a late icon would launch the same
        // application a second time.
        self.released = None;
        // Actions go straight to the main loop. Hit rects and painting share
        // the same geometry (`cell_rect`), so what is seen is what is hit.
        for action in actions {
            self.action = action;
            self.next_repaint = Some(Instant::now());
        }
        if dissolve {
            self.action = GroupAction::Dissolve;
            self.next_repaint = Some(Instant::now());
        }
        Ok(())
    }

    /// Queues an icon request for every child that has none yet.
    pub(super) fn request_icons(&mut self) {
        let Some(loader) = self.loader.as_ref() else {
            return;
        };
        for (i, kid) in self.kids.iter().enumerate() {
            if self.icons.contains_key(&i) || kid.path.is_empty() {
                continue;
            }
            if !self.asked.insert(kid.path.clone()) {
                continue;
            }
            loader.submit(IconRequest {
                index: i,
                kind: kid.kind,
                path: kid.path.clone(),
                icon_src: String::new(),
                fit: 0,
                kids: Vec::new(),
            });
        }
    }

    /// Hit test: which child is under the cursor.
    pub(super) fn hit_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.kids.len()).find(|&k| match cell_rect(k, self.kids.len()) {
            Some((cx, cy, cw, ch)) => x >= cx && x <= cx + cw && y >= cy && y <= cy + ch,
            None => false,
        })
    }
}
