//! 小组件模块：每个弹出面板一个独立 `.rs`，便于单独维护。
//!
//! 拆分前这些文件散在 `src/` 根目录（`calendar_win.rs` / `cc_win.rs` /
//! `tool_win.rs` / `resmon_win.rs` / `note_win.rs` / `group_win.rs` /
//! `edit_win.rs`），加上共享的 `panel_style.rs`；想改某一个面板得在一堆
//! 无关模块里翻。现在物理位置统一到 `src/widgets/`，命名也改成业务名
//! 而不是 `*_win` 后缀。
//!
//! **兼容性**：`lib.rs` 用 `pub use ... as calendar_win` 之类保留了旧路径，
//! `bin/dock.rs` 与 `examples/*.rs` 的引用无需改动。新增小组件时：
//! 1. 在这里加一行 `pub mod`；
//! 2. 在 `lib.rs` 按需加一条旧名重导出（或直接用 `widgets::` 新路径）。

pub mod calendar;
pub mod control;
pub mod editor;
pub mod group;
pub mod notes;
pub mod resource;
pub mod style;
pub mod timer;

/// 所有弹出面板共用的视觉样式。
pub use style::PanelStyle;
