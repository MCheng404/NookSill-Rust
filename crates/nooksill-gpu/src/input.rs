//! Win32 消息 → `egui::RawInput`。
//!
//! 这是"不用 eframe"的代价所在：winit 帮你做的输入翻译，这里要自己写。
//! M0 只覆盖 Dock/设置界面真正用得到的部分：鼠标移动、左右键、滚轮、
//! Esc/Enter/Tab 这几个键。IME 留到 M1（见 PORTING.md 的风险项）。

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

    /// 光标离开窗口。egui 需要 `PointerGone` 才会清掉悬停态，
    /// 否则按钮会一直停在 hover 高亮。
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

    /// `delta_y` 单位是"行"（Win32 的 `WHEEL_DELTA` 120 = 1 行）。
    pub fn wheel(&mut self, delta_y: f32) {
        self.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: Vec2::new(0.0, delta_y),
            // 鼠标滚轮没有触控板那样的 phase/source 语义。
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

    /// 插入一段文字（`WM_CHAR`，或输入法上屏的那串）。
    ///
    /// egui 的 `TextEdit` **只认 `Event::Text`**：`Event::Key` 只用来移动
    /// 光标与删除，敲键盘不会插入字符。所以整条文本输入链路必须走这里，
    /// 否则输入框能拿到焦点却打不进字（设置窗口此前就是这个状态）。
    pub fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.events.push(Event::Text(s.to_owned()));
    }

    /// IME 组合中的文字（拼音 / 候选）。
    ///
    /// egui 的 `TextEdit` 会自己把 preedit 插进 buffer、画下划线、把光标
    /// 标成 `ImeComposition`；下一次 Preedit / Commit 会先删掉它。这是
    /// "组合串回显"的正确做法 —— 在窗口里另画一层文字既会在多行框里
    /// 错位，也不会跟着光标滚动。
    ///
    /// 传空串表示组合结束，egui 会清掉预编辑文本。
    pub fn ime_preedit(&mut self, text: &str) {
        self.events.push(Event::Ime(ImeEvent::Preedit {
            text: text.to_owned(),
            // Win32 的 `GCS_COMPATTR` / `GCS_CURSORPOS` 能给出选区，
            // 但整串候选高亮已经够用，不必为了它再取一次 IME 上下文。
            active_range_chars: None,
        }));
    }

    /// IME 上屏的文字。
    ///
    /// **必须走 `ImeEvent::Commit` 而不是 `Event::Text`**：Commit 会先
    /// 清掉上一帧的 preedit 再插入结果；用 `Event::Text` 会把拼音留在
    /// 正文里（"nihao你好"）。
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

    /// 取出本帧的 `RawInput`。事件队列在此清空。
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
        // egui 0.37 用 `ViewportInfo::native_pixels_per_point` 告知原生缩放。
        // **不设的话 egui 按 1.0 缩放**：界面只占表面的 1/ppp，
        // 文字被放大后发虚（M0 的设置界面就有这个问题）。
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

/// VK 码 → egui 键。只映射设置界面需要的那些，其余返回 `None`。
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

    /// 组合串与上屏串必须是**两类**事件：TextEdit 靠 Preedit 画下划线、
    /// 靠 Commit 清理 preedit。混成 `Event::Text` 会把拼音留在正文里。
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

    /// 组合取消要发一条空的 Preedit（清掉预编辑文本），但空的 Commit
    /// 不该产生事件 —— egui 对空 Commit 有"无组合时忽略"的分支。
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

    /// 事件队列取出后清空，否则同一串拼音会每帧重复插入。
    #[test]
    fn take_drains_events() {
        let mut input = Input::default();
        input.text("a");
        assert_eq!(input.take((10, 10), 1.0).events.len(), 1);
        assert!(input.take((10, 10), 1.0).events.is_empty());
    }

    /// 用 egui 真跑一帧，证明 `Preedit → Commit` 能清掉预编辑文本。
    ///
    /// 这一条比"事件队列里有两个事件"扎实得多：`TextEdit` 的
    /// `filtered_events(&event_filter)` 可能把 IME 事件过滤掉，
    /// `owns_ime_events` 又要求 widget 有焦点 —— 任何一个不成立，
    /// 组合串回显都是空谈。
    ///
    /// 用 ASCII 是刻意的：没装 CJK 字体时 egui 0.37 缺字形直接 panic。
    #[test]
    fn preedit_then_commit_leaves_only_the_final_text() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("ime-test");
        let mut text = "ab".to_owned();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 120.0));

        // 第 1 帧：先建 widget 并给它焦点（`owns_ime_events` == has_focus）。
        // 测试没有真正的纹理后端，`FullOutput` 被丢弃时 `TexturesDelta`
        // 会以"有未应用的 delta"panic —— 这里显式消费掉。
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

        // 第 2 帧：一条 Win32 消息里同时带上屏结果与新组合串。
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
