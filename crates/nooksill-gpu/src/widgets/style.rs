//! 弹出面板（小组件窗口卡片）的**共享视觉样式**。
//!
//! 之前 `calendar_win` / `cc_win` / `tool_win` / `resmon_win` / `note_win` /
//! `group_win` / `edit_win` 各自复制一套配色与圆角：圆角出现过
//! `16/14/13/10/8/7/6/3/12` 九种值，背景 alpha 有 `250/246/248` 三种硬编码，
//! `tool_win` / `resmon_win` 还把 `argbc(a,r,g,b)`（alpha 在前）当 RGBA 传，
//! 得到 `(255,28,28,34)` 这种"红色 + 近乎全透明"的前景。
//!
//! 本模块给出唯一来源，设计准则：
//! - **无锐角**：所有矩形至少 `R_SMALL` 圆角；胶囊用短边的一半。
//! - **轻柔**：低对比的浅色叠加层，不靠高饱和或硬描边分区。
//! - **可读性强**：文字三级层次全部不透明，浅色模式下 ink 是深色。
//! - **便于区分**：面板底 / 叠加层 / 子卡片 / 悬停四级明度递增。
//! - **部分元素居中**：标题、图标、数值由调用方按 `Align2::CENTER_CENTER` 摆放。

use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind};
use nooksill_core::config::{Config, Key};
use std::sync::atomic::{AtomicU8, Ordering};

/// 面板偏好由主程序每帧同步一次（与 `gpu::MSAA_PREF` 同一套做法）。
///
/// 七个弹出面板的构造函数都只拿 `dark: bool`，没有 `Config`；给每个都加
/// 配置参数要改构造签名、调用点和 `Box::pin` 初始化，收益不抵风险。
static PANEL_OPACITY: AtomicU8 = AtomicU8::new(0);
static PANEL_OVERLAY: AtomicU8 = AtomicU8::new(28);

/// 配置加载或改动后调用。开销是两次表查询 + 两次原子写。
pub fn sync_prefs(cfg: &Config) {
    PANEL_OPACITY.store(
        cfg.get_i32(Key::PANEL_OPACITY).clamp(0, 100) as u8,
        Ordering::Relaxed,
    );
    PANEL_OVERLAY.store(
        cfg.get_i32(Key::PANEL_OVERLAY).clamp(0, 100) as u8,
        Ordering::Relaxed,
    );
}

/// 面板视觉参数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelStyle {
    pub dark: bool,
    /// 底层是否已有**毛玻璃背景**。有则面板底色改成半透明 tint，
    /// 让磨砂透出来，而不是盖一块实色板。
    pub frosted: bool,
    /// 用户指定的不透明度百分比；`0` = 按 `frosted` 自动选。
    opacity: u8,
    /// 叠加层强度 `0..100`。
    overlay: u8,
}

impl PanelStyle {
    /// 面板外框圆角。
    pub const R_PANEL: u8 = 18;
    /// 子卡片圆角。
    pub const R_CARD: u8 = 12;
    /// 输入区 / 进度条圆角。
    pub const R_FIELD: u8 = 10;
    /// 最小圆角：任何矩形都不留锐角。
    pub const R_SMALL: u8 = 8;

    /// 从配置构造（单测 / 离屏用）。生产路径走 [`Self::current`]。
    #[must_use]
    pub fn from_cfg(cfg: &Config, dark: bool, frosted: bool) -> Self {
        Self {
            dark,
            frosted,
            opacity: cfg.get_i32(Key::PANEL_OPACITY).clamp(0, 100) as u8,
            overlay: cfg.get_i32(Key::PANEL_OVERLAY).clamp(0, 100) as u8,
        }
    }

    /// 绘制时取当前偏好。`frosted` 由各窗口的 `FrameRenderer::frost_on()` 给。
    #[must_use]
    pub fn current(dark: bool, frosted: bool) -> Self {
        Self {
            dark,
            frosted,
            opacity: PANEL_OPACITY.load(Ordering::Relaxed),
            overlay: PANEL_OVERLAY.load(Ordering::Relaxed),
        }
    }

    /// 离屏验收 / 预览用：与配置默认值一致。
    #[must_use]
    pub fn preview(dark: bool, frosted: bool) -> Self {
        Self {
            dark,
            frosted,
            opacity: 0,
            overlay: 28,
        }
    }

    /// 面板底色的 alpha。
    ///
    /// `0`（自动）时**跟随毛玻璃**：有磨砂就压到 78%，让背景纹理透出来；
    /// 没有磨砂就用 97%，避免半透明底下是纯黑导致文字发灰。
    fn alpha(&self) -> u8 {
        let pct = if self.opacity > 0 {
            self.opacity as f32
        } else if self.frosted {
            78.0
        } else {
            97.0
        };
        (pct / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8
    }

    /// 面板底色。
    #[must_use]
    pub fn bg(&self) -> Color32 {
        let a = self.alpha();
        if self.dark {
            Color32::from_rgba_unmultiplied(24, 25, 30, a)
        } else {
            Color32::from_rgba_unmultiplied(246, 247, 250, a)
        }
    }

    /// 磨砂之上的**叠加层**：一层低对比雾化色，既保留背景纹理，
    /// 又把文字与背景拉开，保证可读性。非磨砂模式下也画，用来统一质感。
    #[must_use]
    pub fn overlay(&self) -> Color32 {
        let a = (self.overlay as f32 / 100.0 * 64.0).round() as u8;
        if self.dark {
            Color32::from_rgba_unmultiplied(12, 14, 20, a)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, a)
        }
    }

    /// 面板外框细线。
    #[must_use]
    pub fn border(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 30)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 20)
        }
    }

    /// 主文字。浅色模式必须是深色 —— 这里用 RGBA 顺序，别再写成 `argbc`。
    #[must_use]
    pub fn ink(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(240, 240, 245, 255)
        } else {
            Color32::from_rgba_unmultiplied(28, 28, 34, 255)
        }
    }

    /// 次要文字。
    #[must_use]
    pub fn ink_sub(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(162, 166, 180, 255)
        } else {
            Color32::from_rgba_unmultiplied(100, 104, 118, 255)
        }
    }

    /// 更淡的说明文字 / 分隔线旁的文字。
    #[must_use]
    pub fn faint(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(200, 200, 210, 255)
        } else {
            Color32::from_rgba_unmultiplied(90, 90, 100, 255)
        }
    }

    /// 分隔线。
    #[must_use]
    pub fn line(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 38)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 30)
        }
    }

    /// 子卡片底色（比面板底高一档明度，便于区分层级）。
    #[must_use]
    pub fn card(&self) -> Color32 {
        let extra = (self.overlay as f32 / 100.0 * 20.0).round() as u8;
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 20u8.saturating_add(extra))
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 14u8.saturating_add(extra / 2))
        }
    }

    /// 子卡片悬停态。
    #[must_use]
    pub fn card_hover(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 58)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 28)
        }
    }

    /// 胶囊圆角：取短边一半，最大 255（egui 的 `CornerRadius` 是 u8）。
    #[must_use]
    pub fn pill(h: f32) -> CornerRadius {
        CornerRadius::same((h * 0.5).round().clamp(2.0, 255.0) as u8)
    }

    /// 画面板外框：底色 → 叠加层 → 细边框。所有弹出面板走这一条。
    pub fn paint_frame(&self, painter: &Painter, rect: Rect) {
        let radius = CornerRadius::same(Self::R_PANEL);
        painter.rect_filled(rect, radius, self.bg());
        painter.rect_filled(rect, radius, self.overlay());
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, self.border()),
            StrokeKind::Middle,
        );
    }

    /// 画一个胶囊元素（按钮 / 标签 / 开关外框）。
    pub fn paint_pill(&self, painter: &Painter, rect: Rect, fill: Color32) {
        painter.rect_filled(rect, Self::pill(rect.height()), fill);
    }

    /// 画一张子卡片。
    pub fn paint_card(&self, painter: &Painter, rect: Rect, hovered: bool) {
        painter.rect_filled(
            rect,
            CornerRadius::same(Self::R_CARD),
            if hovered {
                self.card_hover()
            } else {
                self.card()
            },
        );
    }
}

impl Default for PanelStyle {
    fn default() -> Self {
        Self::preview(true, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_is_opaque_and_contrasts_with_its_theme() {
        let light = PanelStyle::preview(false, false);
        let [r, g, b, a] = light.ink().to_srgba_unmultiplied();
        assert_eq!(a, 255);
        assert!(r < 80 && g < 80 && b < 80, "浅色模式主文字必须深色");

        let dark = PanelStyle::preview(true, false);
        let [r, g, b, a] = dark.ink().to_srgba_unmultiplied();
        assert_eq!(a, 255);
        assert!(r > 200 && g > 200 && b > 200, "深色模式主文字必须亮色");

        for s in [&light, &dark] {
            for c in [s.ink_sub(), s.faint()] {
                assert_eq!(c.a(), 255, "文字不该半透明");
            }
        }
    }

    #[test]
    fn frost_softens_the_base_but_user_opacity_wins() {
        let solid = PanelStyle::preview(true, false);
        let frosted = PanelStyle::preview(true, true);
        assert!(frosted.bg().a() < solid.bg().a(), "有磨砂时底色应更透");

        let mut custom = PanelStyle::preview(true, false);
        custom.opacity = 40;
        assert_eq!(custom.bg().a(), 102, "用户值必须覆盖自动值");
        assert!(custom.bg().a() < solid.bg().a());
    }

    #[test]
    fn every_radius_is_rounded_and_pill_is_half_height() {
        const {
            assert!(PanelStyle::R_PANEL > 0);
            assert!(PanelStyle::R_CARD > 0);
            assert!(PanelStyle::R_FIELD > 0);
            assert!(PanelStyle::R_SMALL > 0);
        }
        assert_eq!(PanelStyle::pill(30.0).nw, 15);
        assert_eq!(PanelStyle::pill(1000.0).nw, 255, "上限要夹住 u8");
    }
}
