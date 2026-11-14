//! Launchpad contents: entries, icon requests and frame composition.

use super::paint::draw;
use super::*;

impl LaunchpadWindow {
    /// Opens a cell: an app launches, a folder expands.
    pub(super) fn activate(&mut self, k: usize) {
        let idx = self.top_index(k);
        if !self.filtered() {
            match self.lp.top().get(idx).copied() {
                Some(Top::App(i)) => {
                    let path = self.lp.apps()[i].path.clone();
                    nooksill_platform::sysact::open_uri(&path);
                    self.close();
                }
                Some(Top::Folder(f)) => {
                    self.open_folder = Some(f);
                    self.next_repaint = Instant::now();
                }
                None => {}
            }
        } else {
            if let Some(&ai) = self.shown().get(idx) {
                let path = self.lp.apps()[ai].path.clone();
                nooksill_platform::sysact::open_uri(&path);
                self.close();
            }
        }
    }

    /// Adds the dropped files to the launchpad.
    pub(super) fn add_paths(&mut self, paths: Vec<String>) {
        for p in paths {
            let name = Path::new(&p)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone());
            if self.lp.add(&name, &p).is_some() {
                self.dirty_save = true;
            }
        }
        self.next_repaint = Instant::now();
    }

    /// Queues icon loads for the currently visible cells (one batch per
    /// frame).
    pub(super) fn request_icons(&mut self) {
        let Some(loader) = &self.loader else { return };
        let per = self.per();
        let page = self.page;
        let count = self.count();

        // Collect the app indices shown on this page.
        let mut want: Vec<usize> = Vec::new();
        if !self.filtered() {
            for k in 0..per {
                let idx = page * per + k;
                if idx >= count {
                    break;
                }
                match self.lp.top()[idx] {
                    Top::App(i) => want.push(i),
                    Top::Folder(f) => {
                        for &ai in &self.lp.folders()[f].apps {
                            want.push(ai);
                        }
                    }
                }
            }
        } else {
            let hits = self.shown();
            for k in 0..per {
                if let Some(&ai) = hits.get(page * per + k) {
                    want.push(ai);
                }
            }
        }

        for ai in want {
            let app = &self.lp.apps()[ai];
            if self.icons.contains_key(&ai) || self.asked.contains_key(&app.path) {
                continue;
            }
            self.asked.insert(app.path.clone(), ai);
            loader.submit(IconRequest {
                index: ai,
                kind: nooksill_core::item::ItemKind::File,
                path: app.path.clone(),
                icon_src: String::new(),
                fit: 0,
                kids: Vec::new(),
            });
        }
    }

    /// Draws one frame.
    pub fn render(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        self.request_icons();
        // The icon thread's results must be uploaded **inside** the frame.
        //
        // Calling `load_texture` before or after `ctx.run` panics ("Dropped
        // TexturesDelta with 1 unapplied deltas"): it only queues the
        // texture in the context, and the queue is claimed by the
        // `textures_delta` of the next `run`. Outside the frame the queued
        // delta lives on until nobody claims it and the Drop check catches
        // it; inside the frame it enters this frame's delta and is uploaded
        // with it.
        let ready = self
            .loader
            .as_ref()
            .map(IconLoader::drain)
            .unwrap_or_default();
        let mut icons = self.icons.clone();
        let pos = self.pos;
        let hwnd = self.hwnd;
        let dark = self.dark;
        let (w, h) = self.size;
        let page = self.page;
        let query = self.query.clone();
        // The composing pinyin is passed separately: it has to be displayed
        // but must not filter, since matching "nihao" against Chinese names
        // would give an empty list.
        let composing = self.ime.composing().to_owned();
        let open_folder = self.open_folder;
        let hover = self.hover;
        let dragging = self.dragging;
        let drag_src = self.drag_src;
        let cur = self.cur;
        let lp = self.lp.clone();
        let grid = self.grid;
        let mut hits: HitList = Vec::new();
        let mut icons_rects: HitList = Vec::new();
        let apps = lp.apps().to_vec();
        let rename_buf = self.rename_buf.clone();
        // An out of range selection counts as none: paging and searching
        // change the cell count.
        let sel = self.sel.filter(|k| *k < self.page_len());
        let menu_items = self.menu_items();
        let ov = Overlay {
            menu_items: &menu_items,
            menu_at: self.menu_at,
            sel,
            cat: self.cat,
            renaming: self.renaming,
            rename_buf: &rename_buf,
        };

        let frost = self.renderer.frost_on();
        let raw = self.input.take(self.renderer.size(), self.ppp);
        self.renderer.render(gpu, raw, hwnd, pos, |ui| {
            let ctx = ui.ctx().clone();
            for r in &ready {
                // The entry may have been removed while the load was queued:
                // check the path.
                if apps.get(r.index).map(|a| a.path.as_str()) != Some(r.path.as_str()) {
                    continue;
                }
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [r.width as usize, r.height as usize],
                    &r.rgba,
                );
                let tex = ctx.load_texture(
                    format!("lp-icon-{}", r.index),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                icons.insert(r.index, tex);
            }
            let out = draw(
                ui,
                dark,
                (w, h),
                &lp,
                grid,
                page,
                &query,
                &composing,
                open_folder,
                hover,
                if dragging { drag_src } else { None },
                cur,
                &icons,
                ov,
                frost,
            );
            hits = out.0;
            icons_rects = out.1;
        })?;
        self.icons = icons;
        self.hits = hits;
        self.icon_rects = icons_rects;
        // The launchpad has no permanent animation: searching, paging and
        // hovering all set the alarm back to now through their events.
        // Push it away after drawing, otherwise `due()` is always true while
        // visible and the Dock is dragged into a permanent 60 fps.
        self.next_repaint = Instant::now() + Duration::from_secs(3600);
        Ok(())
    }
}
