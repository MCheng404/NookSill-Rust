//! Timer / stopwatch / todo list: pure logic plus `[Tool]` section IO.
//!
//! Time arrives as a millisecond integer from the caller; `Instant::now` is
//! never called here, so every timing path is unit-testable with fixed numbers.

use crate::ini::Ini;

/// Name of the `[Tool]` section.
pub const SECTION: &str = "Tool";

/// Maximum number of todos; upstream stores them in `todo[16]`.
pub const MAX_TODOS: usize = 16;

/// Text limit per todo, mirroring `WCHAR todo[16][96]`.
pub const MAX_TODO_LEN: usize = 96;

/// Default countdown duration in milliseconds (`TOOL_TOTAL_DEF`).
pub const DEFAULT_TOTAL_MS: i64 = 5 * 60 * 1000;

/// Timer mode.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ToolMode {
    /// Counts down from the total.
    #[default]
    Countdown,
    /// Counts up from zero.
    Stopwatch,
}

impl ToolMode {
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::Countdown => 0,
            Self::Stopwatch => 1,
        }
    }

    #[must_use]
    pub const fn from_code(v: i32) -> Self {
        if v == 1 {
            Self::Stopwatch
        } else {
            Self::Countdown
        }
    }

    /// Label of the panel toggle button.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Countdown => "计时",
            Self::Stopwatch => "秒表",
        }
    }
}

/// One todo entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Todo {
    pub text: String,
    pub done: bool,
}

impl Todo {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let mut text = text.into();
        text.truncate(MAX_TODO_LEN);
        Self {
            text: text.trim().to_owned(),
            done: false,
        }
    }

    /// Serialize to one `[Tool]` line: `{done}|{text}`.
    #[must_use]
    pub fn to_line(&self) -> String {
        format!("{}|{}", i32::from(self.done), self.text)
    }

    /// Parse one `[Tool]` line.
    ///
    /// Strips exactly one `0|` / `1|` prefix. A loop would eat user text
    /// that legitimately starts with a digit and a pipe.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        let rest = line.trim_end_matches(['\r', '\n']);
        // Only a leading 0 or 1 is the flag; any other head is user text.
        let (done, text) = match rest.split_once('|') {
            Some((head, tail)) => match head.trim() {
                "0" => (false, tail),
                "1" => (true, tail),
                _ => (false, rest),
            },
            None => (false, rest),
        };
        let mut text = text.to_owned();
        text.truncate(MAX_TODO_LEN);
        Self {
            text: text.trim().to_owned(),
            done,
        }
    }
}

/// Full timer / stopwatch / todo state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolState {
    pub mode: ToolMode,
    pub todos: Vec<Todo>,
    /// Countdown total in milliseconds. Unused by the stopwatch.
    pub total_ms: i64,
    /// Remaining milliseconds while paused; a running countdown subtracts elapsed time.
    remain_ms: i64,
    /// Accumulated milliseconds while the stopwatch is paused.
    elapsed_ms: i64,
    /// Whether the timer is running.
    pub running: bool,
    /// Start of the current run, in the caller's clock.
    start_ms: i64,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            mode: ToolMode::Countdown,
            todos: Vec::new(),
            total_ms: DEFAULT_TOTAL_MS,
            remain_ms: DEFAULT_TOTAL_MS,
            elapsed_ms: 0,
            running: false,
            start_ms: 0,
        }
    }
}

impl ToolState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Milliseconds to display right now.
    ///
    /// A running timer adds or subtracts the elapsed time. A countdown never
    /// returns a negative value.
    #[must_use]
    pub fn display_ms(&self, now_ms: i64) -> i64 {
        let run = if self.running {
            (now_ms - self.start_ms).max(0)
        } else {
            0
        };
        match self.mode {
            ToolMode::Countdown => (self.remain_ms - run).max(0),
            ToolMode::Stopwatch => self.elapsed_ms + run,
        }
    }

    /// Whether a running countdown has already reached zero.
    #[must_use]
    pub fn is_finished(&self, now_ms: i64) -> bool {
        self.mode == ToolMode::Countdown && self.running && self.display_ms(now_ms) == 0
    }

    /// Advance one frame; stop the timer when the countdown hits zero.
    ///
    /// Stopping is explicit here rather than a side effect of drawing, so a
    /// countdown ends even when nothing is painted.
    pub fn tick(&mut self, now_ms: i64) {
        if self.is_finished(now_ms) {
            self.running = false;
            self.remain_ms = 0;
        }
    }

    /// Start or resume.
    pub fn start(&mut self, now_ms: i64) {
        if self.running {
            return;
        }
        // A finished countdown does not restart; it would hit zero again at once.
        if self.mode == ToolMode::Countdown && self.remain_ms <= 0 {
            return;
        }
        self.running = true;
        self.start_ms = now_ms;
    }

    /// Pause and freeze the current display value.
    pub fn pause(&mut self, now_ms: i64) {
        if !self.running {
            return;
        }
        let cur = self.display_ms(now_ms);
        match self.mode {
            ToolMode::Countdown => self.remain_ms = cur,
            ToolMode::Stopwatch => self.elapsed_ms = cur,
        }
        self.running = false;
    }

    /// Toggle between running and paused (the panel button).
    pub fn toggle_run(&mut self, now_ms: i64) {
        if self.running {
            self.pause(now_ms);
        } else {
            self.start(now_ms);
        }
    }

    /// Reset: the countdown returns to its total, the stopwatch to zero. Stops.
    pub fn reset(&mut self) {
        self.running = false;
        match self.mode {
            ToolMode::Countdown => self.remain_ms = self.total_ms,
            ToolMode::Stopwatch => self.elapsed_ms = 0,
        }
    }

    /// Switch mode. Stops the timer but keeps each mode's own value.
    pub fn set_mode(&mut self, mode: ToolMode) {
        if self.mode == mode {
            return;
        }
        self.running = false;
        self.mode = mode;
    }

    /// Append a todo. Returns `false` when the list is full or the text is empty.
    pub fn add_todo(&mut self, text: &str) -> bool {
        let todo = Todo::new(text);
        if todo.text.is_empty() || self.todos.len() >= MAX_TODOS {
            return false;
        }
        self.todos.push(todo);
        true
    }

    /// Toggle the done flag. Returns `false` for an out-of-range index.
    pub fn toggle_todo(&mut self, i: usize) -> bool {
        match self.todos.get_mut(i) {
            Some(t) => {
                t.done = !t.done;
                true
            }
            None => false,
        }
    }

    /// Remove one todo. Returns `false` for an out-of-range index.
    pub fn remove_todo(&mut self, i: usize) -> bool {
        if i >= self.todos.len() {
            return false;
        }
        self.todos.remove(i);
        true
    }

    /// Number of unfinished todos (the dots on the strip card).
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.todos.iter().filter(|t| !t.done).count()
    }

    /// Read from the `[Tool]` section. Missing keys fall back to the defaults.
    #[must_use]
    pub fn load(ini: &Ini) -> Self {
        let mut out = Self::default();
        let n = ini
            .get_i32(SECTION, "N")
            .unwrap_or(0)
            .clamp(0, MAX_TODOS as i32);
        for i in 0..n {
            let line = ini.get(SECTION, &format!("T{i}")).unwrap_or_default();
            if line.is_empty() {
                continue;
            }
            out.todos.push(Todo::parse_line(line));
        }
        // The ini stores seconds in `Total` / `Remain`; scale back to milliseconds.
        if let Some(secs) = ini.get_i32(SECTION, "Total") {
            out.total_ms = i64::from(secs.max(1)) * 1000;
        }
        out.remain_ms = ini
            .get_i32(SECTION, "Remain")
            .map_or(out.total_ms, |s| i64::from(s.max(0)) * 1000);
        out.elapsed_ms = 0;
        out.running = false;
        out
    }

    /// Write back to the `[Tool]` section.
    ///
    /// Surplus `T{i}` keys are removed, otherwise the file keeps entries the
    /// `N` count no longer covers.
    pub fn save(&self, ini: &mut Ini, now_ms: i64) {
        let old_n = ini.get_i32(SECTION, "N").unwrap_or(0).max(0);
        ini.set_i32(SECTION, "N", self.todos.len() as i32);
        for (i, todo) in self.todos.iter().enumerate() {
            ini.set(SECTION, &format!("T{i}"), todo.to_line());
        }
        for i in self.todos.len() as i32..old_n {
            ini.remove_key(SECTION, &format!("T{i}"));
        }
        ini.set_i32(SECTION, "Total", (self.total_ms / 1000) as i32);
        ini.set_i32(SECTION, "Remain", (self.display_ms(now_ms) / 1000) as i32);
    }
}

/// Format milliseconds for the large panel text.
///
/// The stopwatch carries two decimals, the countdown stops at seconds.
#[must_use]
pub fn format_ms(ms: i64, mode: ToolMode) -> String {
    let ms = ms.max(0);
    match mode {
        ToolMode::Countdown => format!("{:02}:{:02}", ms / 60_000, (ms / 1000) % 60),
        ToolMode::Stopwatch => format!(
            "{:02}:{:02}.{:02}",
            ms / 60_000,
            (ms / 1000) % 60,
            (ms % 1000) / 10
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_line_roundtrip() {
        let t = Todo {
            text: "买菜".to_owned(),
            done: true,
        };
        assert_eq!(t.to_line(), "1|买菜");
        assert_eq!(Todo::parse_line("1|买菜"), t);

        let t = Todo {
            text: "写代码".to_owned(),
            done: false,
        };
        assert_eq!(t.to_line(), "0|写代码");
        assert_eq!(Todo::parse_line("0|写代码"), t);
    }

    /// Regression: the `0|` prefix must be stripped or every save/load cycle
    /// adds another layer.
    #[test]
    fn todo_parse_strips_zero_prefix() {
        assert!(!Todo::parse_line("0|买菜").done);
        assert_eq!(Todo::parse_line("0|买菜").text, "买菜");
        // A file with a doubled prefix keeps one layer but stops growing.
        assert!(!Todo::parse_line("0|0|买菜").done);
        assert_eq!(Todo::parse_line("0|0|买菜").text, "0|买菜");
        // Saving again must not add another layer.
        let once = Todo::parse_line("0|0|买菜");
        assert_eq!(once.to_line(), "0|0|买菜", "存量不再增长");
    }

    /// A stripping loop would eat user text: "1|foo" must survive verbatim.
    #[test]
    fn todo_parse_does_not_eat_user_text() {
        let t = Todo {
            text: "1|foo".to_owned(),
            done: false,
        };
        assert_eq!(t.to_line(), "0|1|foo");
        let back = Todo::parse_line("0|1|foo");
        assert!(!back.done);
        assert_eq!(back.text, "1|foo", "用户文本不能被当成前缀");
    }

    /// A pipe inside the text must not be eaten.
    #[test]
    fn todo_parse_keeps_pipes_in_text() {
        assert_eq!(Todo::parse_line("a|b").text, "a|b");
        assert_eq!(Todo::parse_line("2|x").text, "2|x");
    }

    #[test]
    fn countdown_goes_down_while_running() {
        let mut s = ToolState::new();
        s.total_ms = 60_000;
        s.reset();
        assert_eq!(s.display_ms(0), 60_000);

        s.start(1_000);
        assert_eq!(s.display_ms(1_000), 60_000);
        assert_eq!(s.display_ms(11_000), 50_000);

        s.pause(11_000);
        assert_eq!(s.display_ms(99_999), 50_000, "暂停后不再走");
        s.start(20_000);
        assert_eq!(s.display_ms(25_000), 45_000);
    }

    #[test]
    fn countdown_stops_at_zero() {
        let mut s = ToolState::new();
        s.total_ms = 2_000;
        s.reset();
        s.start(0);
        assert_eq!(s.display_ms(5_000), 0, "不会变成负数");
        assert!(s.is_finished(5_000));
        s.tick(5_000);
        assert!(!s.running, "归零后应当停表");
        // Starting again after the end must not restart the timer.
        s.start(6_000);
        assert!(!s.running);
    }

    #[test]
    fn stopwatch_goes_up() {
        let mut s = ToolState::new();
        s.set_mode(ToolMode::Stopwatch);
        s.reset();
        s.start(1_000);
        assert_eq!(s.display_ms(1_000), 0);
        assert_eq!(s.display_ms(3_500), 2_500);
        s.pause(3_500);
        assert_eq!(s.display_ms(9_000), 2_500);
        s.start(9_000);
        assert_eq!(s.display_ms(10_000), 3_500);
    }

    /// Switching mode stops the timer but keeps each side's value.
    #[test]
    fn mode_switch_keeps_each_value() {
        let mut s = ToolState::new();
        s.total_ms = 10_000;
        s.reset();
        s.start(0);
        s.pause(4_000);
        assert_eq!(s.display_ms(4_000), 6_000);

        s.set_mode(ToolMode::Stopwatch);
        assert!(!s.running);
        s.start(4_000);
        s.pause(7_000);
        assert_eq!(s.display_ms(7_000), 3_000);

        // Switching back keeps the 6 seconds that were left.
        s.set_mode(ToolMode::Countdown);
        assert_eq!(s.display_ms(7_000), 6_000);
    }

    #[test]
    fn reset_restores_starting_value() {
        let mut s = ToolState::new();
        s.total_ms = 30_000;
        s.reset();
        s.start(0);
        s.pause(5_000);
        s.reset();
        assert_eq!(s.display_ms(5_000), 30_000);
        assert!(!s.running);

        s.set_mode(ToolMode::Stopwatch);
        s.start(0);
        s.pause(1_500);
        s.reset();
        assert_eq!(s.display_ms(1_500), 0);
    }

    #[test]
    fn todos_are_bounded_and_editable() {
        let mut s = ToolState::new();
        assert!(s.add_todo("  第一条  "));
        assert_eq!(s.todos[0].text, "第一条", "两端空白应当去掉");
        assert!(!s.add_todo("   "), "空文本不该进去");

        for i in 1..MAX_TODOS {
            assert!(s.add_todo(&format!("第 {i} 条")));
        }
        assert_eq!(s.todos.len(), MAX_TODOS);
        assert!(!s.add_todo("再来一条"), "超过上限要拒绝");

        assert!(s.toggle_todo(0));
        assert!(s.todos[0].done);
        assert_eq!(s.pending_count(), MAX_TODOS - 1);

        assert!(s.remove_todo(0));
        assert_eq!(s.todos.len(), MAX_TODOS - 1);
        assert!(!s.remove_todo(999), "越界要返回 false");
    }

    #[test]
    fn tool_section_roundtrip() {
        let mut s = ToolState::new();
        s.total_ms = 90_000;
        s.reset();
        s.add_todo("买菜");
        s.add_todo("写代码");
        s.toggle_todo(1);
        s.start(0);
        s.pause(30_000);

        let mut ini = Ini::new();
        s.save(&mut ini, 30_000);

        assert_eq!(ini.get(SECTION, "N"), Some("2"));
        assert_eq!(ini.get(SECTION, "T0"), Some("0|买菜"));
        assert_eq!(ini.get(SECTION, "T1"), Some("1|写代码"));
        assert_eq!(ini.get(SECTION, "Total"), Some("90"));
        assert_eq!(ini.get(SECTION, "Remain"), Some("60"));

        let back = ToolState::load(&ini);
        assert_eq!(back.todos, s.todos);
        assert_eq!(back.total_ms, 90_000);
        assert_eq!(back.display_ms(0), 60_000);
        // The running state is not persisted: a restart always starts stopped.
        assert!(!back.running);
    }

    /// Removing a todo must drop the surplus `T{i}` keys from the file.
    #[test]
    fn save_drops_stale_todo_keys() {
        let mut s = ToolState::new();
        s.add_todo("a");
        s.add_todo("b");
        s.add_todo("c");
        let mut ini = Ini::new();
        s.save(&mut ini, 0);
        assert_eq!(ini.get(SECTION, "T2"), Some("0|c"));

        s.remove_todo(2);
        s.remove_todo(1);
        s.save(&mut ini, 0);
        assert_eq!(ini.get(SECTION, "N"), Some("1"));
        assert_eq!(ini.get(SECTION, "T0"), Some("0|a"));
        assert!(ini.get(SECTION, "T1").is_none(), "旧键要删掉");
        assert!(ini.get(SECTION, "T2").is_none(), "旧键要删掉");
    }

    #[test]
    fn format_matches_upstream() {
        assert_eq!(format_ms(0, ToolMode::Countdown), "00:00");
        assert_eq!(format_ms(65_000, ToolMode::Countdown), "01:05");
        assert_eq!(format_ms(65_430, ToolMode::Stopwatch), "01:05.43");
    }
}
