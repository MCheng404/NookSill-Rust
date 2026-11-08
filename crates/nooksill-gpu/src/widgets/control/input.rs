//! Control centre messages: volume, tile clicks and pointer handling.

use super::win::client_pos;
use super::*;

impl CcWindow {
    /// Sets the volume (called on every drag frame).
    pub(super) fn set_volume(&mut self, v: u32) {
        let v = v.min(100);
        if v == self.volume {
            return;
        }
        self.volume = v;
        if let Some(a) = &self.audio {
            a.set_volume(v);
        }
        self.next_repaint = Instant::now();
    }

    /// Clicks one tile.
    pub(super) fn click_tile(&mut self, idx: usize) {
        let Some(tile) = self.tiles.get(idx).copied() else {
            return;
        };
        match tile {
            // These three have no stable public API; open the system panel.
            Tile::Wifi | Tile::Bluetooth | Tile::Hotspot => {
                sysact::open_quick_settings();
                self.hide();
            }
            Tile::Ethernet => {
                // Wired goes to the network connections list, not the Wi-Fi
                // panel: the user wants adapter state, not the quick panel.
                sysact::open_uri("ncpa.cpl");
                self.hide();
            }
            Tile::DarkMode => {
                let next = !self.dark;
                self.dark = sysact::set_dark_mode(next);
                self.dirty_theme = true;
                self.next_repaint = Instant::now();
            }
            Tile::Location => {
                sysact::open_uri("ms-settings:privacy-location");
                self.hide();
            }
            Tile::Settings => {
                sysact::open_uri("ms-settings:");
                self.hide();
            }
            Tile::Mute => {
                self.mute = match &self.audio {
                    Some(a) => a.toggle_mute(),
                    None => !self.mute,
                };
                self.next_repaint = Instant::now();
            }
            Tile::Energy => {
                self.energy = self.power.toggle();
                self.next_repaint = Instant::now();
            }
        }
    }

    pub(super) fn on_message(
        &mut self,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg {
            WM_MOUSEMOVE => {
                let (x, y) = client_pos(lparam, self.ppp);
                let p = egui::pos2(x, y);
                if self.dragging {
                    // Slider tracks the pointer by ratio; 7 px are reserved
                    // at each end for the round knob.
                    let left = self.slider.left() + 30.0;
                    let right = self.slider.right() - 4.0;
                    if right > left {
                        let f = ((x - left) / (right - left)).clamp(0.0, 1.0);
                        self.set_volume((f * 100.0).round() as u32);
                    }
                    return None;
                }
                // Tile drag: starts past a threshold and swaps immediately
                // when it lands on another slot. A drag ghost would be
                // noisier than useful in 3-column tiles.
                if self.press.is_some() && self.drag_tile.is_none() {
                    let dx = x - self.down.0;
                    let dy = y - self.down.1;
                    if dx.abs() > 6.0 || dy.abs() > 6.0 {
                        self.drag_tile = self.press;
                    }
                }
                if let Some(cur) = self.drag_tile {
                    if let Some(to) = self.hits.iter().position(|r| r.contains(p)) {
                        if to != cur && to < self.tiles.len() && cur < self.tiles.len() {
                            self.tiles.swap(cur, to);
                            self.drag_tile = Some(to);
                            self.moved = true;
                            self.next_repaint = Instant::now();
                        }
                    }
                }
                let h = if self.slider.contains(p) {
                    Some(Hit::Slider)
                } else {
                    self.hits.iter().position(|r| r.contains(p)).map(Hit::Tile)
                };
                if self.hover != h {
                    self.hover = h;
                    self.next_repaint = Instant::now();
                }
                // The panel grabs the capture as soon as it appears; during
                // capture no `WM_SETCURSOR` arrives, so the cursor is set here.
                crate::cursor::set_hand_if(self.hover.is_some());
                None
            }
            WM_LBUTTONDOWN => {
                let (x, y) = client_pos(lparam, self.ppp);
                let p = egui::pos2(x, y);
                if self.slider.contains(p) {
                    self.dragging = true;
                    // The drag takes the capture itself, otherwise moving the
                    // cursor out of the panel stops `WM_MOUSEMOVE` and the
                    // slider freezes mid-way.
                    unsafe {
                        SetCapture(self.hwnd);
                    }
                    let left = self.slider.left() + 30.0;
                    let right = self.slider.right() - 4.0;
                    if right > left {
                        let f = ((x - left) / (right - left)).clamp(0.0, 1.0);
                        self.set_volume((f * 100.0).round() as u32);
                    }
                } else {
                    // Press on a tile only records the position; movement
                    // turns it into a drag, otherwise a plain click would
                    // look like a drag that never moved and do nothing.
                    self.down = (x, y);
                    self.press = self.hits.iter().position(|r| r.contains(p));
                    self.drag_tile = None;
                    self.moved = false;
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if self.dragging {
                    self.dragging = false;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    return Some(LRESULT(0));
                }
                let moved = self.moved;
                self.press = None;
                self.drag_tile = None;
                self.moved = false;
                // A drag is not also a click: otherwise every reorder would
                // fire an action whose panel would cover the new arrangement.
                if moved {
                    self.pending_order = Some(self.tiles.clone());
                    self.next_repaint = Instant::now();
                    return Some(LRESULT(0));
                }
                let (x, y) = client_pos(lparam, self.ppp);
                if let Some(Hit::Tile(k)) = self
                    .hits
                    .iter()
                    .position(|r| r.contains(egui::pos2(x, y)))
                    .map(Hit::Tile)
                {
                    self.click_tile(k);
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                crate::cursor::set_hand_if(self.hover.is_some());
                Some(LRESULT(1))
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let _ = BeginPaint(self.hwnd, &mut ps);
                    let _ = EndPaint(self.hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            _ => None,
        }
    }
}
