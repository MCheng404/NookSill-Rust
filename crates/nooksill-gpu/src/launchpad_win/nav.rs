//! Grid geometry, pagination and keyboard/mouse selection.

use super::*;

impl LaunchpadWindow {
    /// Number of cells per page.
    pub(super) fn per(&self) -> usize {
        let (cols, rows) = self.grid_dims();
        cols * rows
    }

    pub(super) fn grid_dims(&self) -> (usize, usize) {
        // The column and row counts come from the config (`LpCols` / `LpRows`)
        // and are pushed in by the caller through `set_grid`; the default is
        // 7 x 4.
        self.grid
    }

    /// Sets the grid size from the config; the main loop calls this before
    /// opening.
    pub fn set_grid(&mut self, cols: usize, rows: usize) {
        self.grid = (cols.clamp(3, 12), rows.clamp(2, 8));
        self.next_repaint = Instant::now();
    }

    /// Whether the filtered view is active: a category or a search query.
    ///
    /// The filtered view lists apps only, never folders: categories classify
    /// apps, and folders have no category, so mixing them in only confuses.
    pub(super) fn filtered(&self) -> bool {
        self.cat != 0 || !self.query.is_empty()
    }

    /// The app indices shown in the filtered view.
    pub(super) fn shown(&self) -> Vec<usize> {
        self.lp.visible_in(self.cat, &self.query)
    }

    /// How many cells are shown in total.
    pub(super) fn count(&self) -> usize {
        if self.filtered() {
            self.shown().len()
        } else {
            self.lp.len()
        }
    }

    pub(super) fn pages(&self) -> usize {
        Launchpad::pages(self.count(), self.per())
    }

    /// How many cells the current page holds.
    pub(super) fn page_len(&self) -> usize {
        let per = self.per().max(1);
        (self.count().saturating_sub(self.page * per)).min(per)
    }

    /// Moves the keyboard selection.
    ///
    /// Left and right turn the page at the edges (the launchpad habit), while
    /// up and down stay inside the page: paging vertically would need an
    /// extra rule for which column to land on, and that is easier to get
    /// lost in.
    pub(super) fn move_sel(&mut self, dx: i32, dy: i32) {
        let len = self.page_len();
        if len == 0 {
            self.sel = None;
            self.next_repaint = Instant::now();
            return;
        }
        let cols = self.grid.0.max(1);
        let k = self.sel.unwrap_or(0).min(len - 1);
        let (col, row) = (k % cols, k / cols);
        if dx != 0 {
            let next = k as i32 + dx;
            if next < 0 {
                // Left edge: last cell of the previous page.
                if self.page > 0 {
                    self.page -= 1;
                    self.sel = Some(self.page_len().saturating_sub(1));
                }
            } else if next >= len as i32 {
                // Right edge: first cell of the next page.
                if self.page + 1 < self.pages() {
                    self.page += 1;
                    self.sel = Some(0);
                }
            } else {
                self.sel = Some(next as usize);
            }
        } else {
            let nr = (row as i32 + dy).clamp(0, self.grid.1 as i32 - 1) as usize;
            let nk = nr * cols + col;
            if nk < len {
                self.sel = Some(nk);
            }
        }
        self.next_repaint = Instant::now();
    }

    /// Enter: opens the selected cell; with nothing selected it opens the
    /// first match of the filtered view.
    pub(super) fn enter_sel(&mut self) {
        if let Some(k) = self.sel {
            if k < self.page_len() {
                self.activate(k);
                return;
            }
        }
        if self.filtered() && self.page_len() > 0 {
            self.activate(0);
        }
    }

    pub(super) fn turn_page(&mut self, delta: i32) {
        if self.open_folder.is_some() {
            return;
        }
        let pages = self.pages();
        let p = self.page as i32 + delta;
        if p >= 0 && (p as usize) < pages {
            self.page = p as usize;
            self.next_repaint = Instant::now();
        }
    }

    /// The page number can go out of range after cells were removed.
    pub(super) fn clamp_page(&mut self) {
        let pages = self.pages();
        if self.page >= pages {
            self.page = pages.saturating_sub(1);
        }
    }

    pub(super) fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        let p = Pos2::new(x, y);
        self.hits
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(h, _)| *h)
    }
}
