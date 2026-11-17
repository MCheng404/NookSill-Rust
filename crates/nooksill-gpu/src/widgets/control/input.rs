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
            // WiFi goes through the WLAN software radio; the hardware switch
            // still wins when it is off.
            Tile::Wifi => {
                let on = self.toggles.wifi.as_ref().is_some_and(|w| w.on);
                if nooksill_platform::radio::set_wifi(!on) {
                    self.refresh_toggles();
                }
            }
            // Bluetooth has no Win32 switch: the WinRT radio stack may ask
            // for permission on the first toggle.
            Tile::Bluetooth => {
                let on = self.toggles.bt_on.unwrap_or(false);
                if nooksill_platform::radio::set_bluetooth(!on) {
                    self.refresh_toggles();
                }
            }
            // Hotspot has no stable public API; open the system panel.
            Tile::Hotspot => {
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
        }
    }

    /// Toggles mute from the speaker icon.
    pub(super) fn toggle_mute(&mut self) {
        self.mute = match &self.audio {
            Some(a) => a.toggle_mute(),
            None => !self.mute,
        };
        self.next_repaint = Instant::now();
    }

    /// Re-reads the radio and location state after a toggle or on open.
    pub(super) fn refresh_toggles(&mut self) {
        self.toggles.wifi = nooksill_platform::radio::wifi_state();
        self.toggles.bt_on = nooksill_platform::radio::bluetooth_state();
        self.toggles.location_on = nooksill_platform::radio::location_state();
        self.next_repaint = Instant::now();
    }

    /// Maps an x position inside the brightness strip to 0..100.
    fn bright_at(&mut self, x: f32) {
        let left = self.bright_slider.left() + 30.0;
        let right = self.bright_slider.right() - 4.0;
        if right <= left {
            return;
        }
        let f = ((x - left) / (right - left)).clamp(0.0, 1.0);
        self.set_brightness((f * 100.0).round() as u32);
    }

    /// Caches the new level and writes it unless the throttle holds it back.
    fn set_brightness(&mut self, percent: u32) {
        let percent = percent.min(100);
        self.bright_percent = percent;
        self.pending_bright = Some(percent);
        self.next_repaint = Instant::now();
        // A monitor may have been connected since the panel was opened; retry
        // once on interaction instead of leaving the row permanently dead.
        if self.brightness.is_none() {
            self.brightness = nooksill_platform::brightness::Brightness::probe();
        }
        if self.bright_write.elapsed() >= BRIGHT_THROTTLE {
            self.flush_brightness();
        }
    }

    /// Writes the newest pending level; also called on release so the last
    /// position is never lost to the throttle.
    fn flush_brightness(&mut self) {
        if let (Some(b), Some(percent)) = (self.brightness.as_mut(), self.pending_bright.take()) {
            b.set(percent);
            self.bright_write = Instant::now();
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
                if self.dragging_bright {
                    self.bright_at(x);
                    return None;
                }
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
                let h = if self.bright_slider.contains(p) {
                    Some(Hit::Bright)
                } else if self.slider.contains(p) {
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
                // Brightness is checked first: its strip sits below the
                // volume strip and never overlaps it.
                if self.bright_slider.contains(p) {
                    self.dragging_bright = true;
                    // Same capture rule as the volume drag: without it the
                    // stream stops as soon as the cursor leaves the panel.
                    unsafe {
                        SetCapture(self.hwnd);
                    }
                    self.bright_at(x);
                    self.flush_brightness();
                    return Some(LRESULT(0));
                }
                // The speaker icon sits at the left end of the slider strip;
                // clicking it mutes instead of starting a drag at ~0%.
                if self.slider.contains(p) && x < self.slider.left() + VOLUME_ICON_W {
                    self.toggle_mute();
                    return Some(LRESULT(0));
                }
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
                if self.dragging_bright {
                    self.dragging_bright = false;
                    // The throttle may still hold the last value.
                    self.flush_brightness();
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    return Some(LRESULT(0));
                }
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
