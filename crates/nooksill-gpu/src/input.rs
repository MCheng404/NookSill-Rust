//! Win32 messages to `egui::RawInput`.
//!
//! Without eframe/winit this translation is manual. Only what the dock and the
//! settings window use is covered: pointer, buttons, wheel and a few keys.

use std::time::Instant;

use egui::{Event, ImeEvent, Modifiers, PointerButton, Pos2, RawInput, Vec2};

#[derive(Debug)]
pub struct Input {
    events: Vec<Event>,
    pointer: Pos2,
    modifiers: Modifiers,
    start: Instant,
    hovering: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            pointer: Pos2::ZERO,
            modifiers: Modifiers::default(),
            start: Instant::now(),
            hovering: false,
        }
    }
}

impl Input {
    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        self.hovering = true;
        self.pointer = Pos2::new(x, y);
        self.events.push(Event::PointerMoved(self.pointer));
    }

    /// Cursor left the window. egui only clears hover on `PointerGone`,
    /// otherwise buttons stay highlighted.
    pub fn pointer_gone(&mut self) {
        self.hovering = false;
        self.events.push(Event::PointerGone);
    }

    #[must_use]
    pub const fn hovering(&self) -> bool {
        self.hovering
    }

    pub fn pointer_button(&mut self, pressed: bool, button: PointerButton) {
        self.events.push(Event::PointerButton {
            pos: self.pointer,
            button,
            pressed,
            modifiers: self.modifiers,
        });
    }

    /// `delta_y` is in lines (Win32 `WHEEL_DELTA` 120 equals one line).
    pub fn wheel(&mut self, delta_y: f32) {
        self.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: Vec2::new(0.0, delta_y),
            // A mouse wheel has no touchpad-style phase or source.
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: self.modifiers,
        });
    }

    pub fn key(&mut self, key: egui::Key, pressed: bool) {
        self.events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: self.modifiers,
        });
    }

    /// Insert text (`WM_CHAR`, or an IME commit).
    ///
    /// `TextEdit` inserts only through `Event::Text`; `Event::Key` just moves the
    /// caret and deletes. Anything else gives a focused field that ignores
    /// typing.
    pub fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.events.push(Event::Text(s.to_owned()));
    }

    /// IME composition text (pinyin or candidates).
    ///
    /// `TextEdit` inserts the preedit into its buffer, underlines it and marks the
    /// cursor `ImeComposition`; the next Preedit or Commit removes it, which also
    /// keeps it scrolling with the caret. An empty string ends the composition.
    pub fn ime_preedit(&mut self, text: &str) {
        self.events.push(Event::Ime(ImeEvent::Preedit {
            text: text.to_owned(),
            // `GCS_COMPATTR` / `GCS_CURSORPOS` could give a selection, but
            // highlighting the whole string is enough.
            active_range_chars: None,
        }));
    }

    /// Text committed by the IME.
    ///
    /// Must be `ImeEvent::Commit`, not `Event::Text`: Commit drops the previous
    /// preedit first, otherwise the pinyin stays in the buffer.
    pub fn ime_commit(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.events
            .push(Event::Ime(ImeEvent::Commit(text.to_owned())));
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// Build this frame's `RawInput`; the event queue is drained.
    pub fn take(&mut self, size_px: (u32, u32), pixels_per_point: f32) -> RawInput {
        let logical = Vec2::new(
            size_px.0 as f32 / pixels_per_point,
            size_px.1 as f32 / pixels_per_point,
        );
        let mut raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, logical)),
            time: Some(self.start.elapsed().as_secs_f64()),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        // egui 0.37 reads the native scale from
        // `ViewportInfo::native_pixels_per_point`. Leaving it unset makes egui
        // assume 1.0, so the UI covers only 1/ppp of the surface and text
        // turns blurry.
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(pixels_per_point),
                ..Default::default()
            },
        );
        raw
    }
}

/// VK code to egui key. Only the keys the settings window needs are mapped.
#[must_use]
pub fn map_vk(vk: u32) -> Option<egui::Key> {
    use egui::Key;
    Some(match vk {
        0x1B => Key::Escape,
        0x0D => Key::Enter,
        0x09 => Key::Tab,
        0x20 => Key::Space,
        0x25 => Key::ArrowLeft,
        0x26 => Key::ArrowUp,
        0x27 => Key::ArrowRight,
        0x28 => Key::ArrowDown,
        0x08 => Key::Backspace,
        0x2E => Key::Delete,
        0x24 => Key::Home,
        0x23 => Key::End,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preedit and commit must stay distinct events: `TextEdit` draws the
    /// underline from Preedit and clears it on Commit.
    #[test]
    fn ime_preedit_and_commit_are_distinct_events() {
        let mut input = Input::default();
        input.ime_preedit("nihao");
        input.ime_commit("你好");
        let raw = input.take((100, 100), 1.0);
        assert_eq!(raw.events.len(), 2);
        assert!(matches!(
            &raw.events[0],
            Event::Ime(ImeEvent::Preedit { text, .. }) if text == "nihao"
        ));
        assert!(matches!(
            &raw.events[1],
            Event::Ime(ImeEvent::Commit(text)) if text == "你好"
        ));
    }

    /// Cancelling sends an empty Preedit to clear the text, but an empty commit
    /// emits nothing.
    #[test]
    fn empty_preedit_is_sent_but_empty_commit_is_not() {
        let mut input = Input::default();
        input.ime_preedit("");
        input.ime_commit("");
        let raw = input.take((100, 100), 1.0);
        assert_eq!(raw.events.len(), 1);
        assert!(matches!(
            &raw.events[0],
            Event::Ime(ImeEvent::Preedit { text, .. }) if text.is_empty()
        ));
    }

    /// The queue drains on take, otherwise the same pinyin repeats every frame.
    #[test]
    fn take_drains_events() {
        let mut input = Input::default();
        input.text("a");
        assert_eq!(input.take((10, 10), 1.0).events.len(), 1);
        assert!(input.take((10, 10), 1.0).events.is_empty());
    }

    /// Runs egui for real to prove `Preedit -> Commit` clears the preedit text.
    ///
    /// `filtered_events` may drop IME events and `owns_ime_events` requires focus,
    /// so both must hold. ASCII only: egui 0.37 panics on missing CJK glyphs.
    #[test]
    fn preedit_then_commit_leaves_only_the_final_text() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("ime-test");
        let mut text = "ab".to_owned();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 120.0));

        // Frame 1: create the widget and focus it (`owns_ime_events` is
        // has_focus). There is no texture backend here, so discard the delta or
        // dropping `FullOutput` panics.
        let mut out1 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                ui.ctx().memory_mut(|m| m.request_focus(id));
                ui.add(egui::TextEdit::singleline(&mut text).id(id));
            },
        );
        out1.textures_delta.clear();

        // Frame 2: one Win32 message carrying both the commit and a new
        // composition.
        let events = vec![
            Event::Ime(ImeEvent::Preedit {
                text: "pinyin".to_owned(),
                active_range_chars: None,
            }),
            Event::Ime(ImeEvent::Commit("hi".to_owned())),
        ];
        let mut out2 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut text).id(id));
            },
        );
        out2.textures_delta.clear();
        assert_eq!(text, "abhi", "预编辑的拼音必须被 Commit 清掉");
    }
}
