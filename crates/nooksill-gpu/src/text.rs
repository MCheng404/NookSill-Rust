//! One-pass text truncation for panel and launchpad labels.
//!
//! egui lays the text out once here; painting the galley avoids the second
//! layout that `Painter::text` would do.

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, FontId, Galley, Painter, Pos2};
use std::sync::Arc;

/// A shrinking card can hand us a zero, negative or non-finite width.
fn safe_width(max_w: f32) -> f32 {
    if max_w.is_finite() {
        max_w.max(1.0)
    } else {
        1.0
    }
}

/// Lay out `text` once, eliding with an ellipsis when it exceeds `max_w`.
#[must_use]
pub fn truncated_galley(
    painter: &Painter,
    text: &str,
    max_w: f32,
    size: f32,
    color: Color32,
) -> Arc<Galley> {
    let width = safe_width(max_w);
    let mut job = LayoutJob::simple(text.to_owned(), FontId::proportional(size), color, width);
    job.wrap = TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

/// Paint truncated text with the same anchoring as `Painter::text`.
pub fn draw_truncated(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    max_w: f32,
    size: f32,
    color: Color32,
) {
    let galley = truncated_galley(painter, text, max_w, size, color);
    let rect = anchor.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{LayerId, Rect, Vec2};

    fn test_painter() -> Painter {
        let ctx = egui::Context::default();
        // Fonts are installed on the first pass; layout before that panics.
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        // Dropping an unapplied TexturesDelta panics in epaint 0.37.
        output.textures_delta.clear();
        Painter::new(
            ctx,
            LayerId::background(),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 1024.0)),
        )
    }

    #[test]
    fn degenerate_widths_are_clamped() {
        assert_eq!(safe_width(-4.0), 1.0);
        assert_eq!(safe_width(0.0), 1.0);
        assert_eq!(safe_width(f32::NAN), 1.0);
        assert_eq!(safe_width(f32::INFINITY), 1.0);
        assert_eq!(safe_width(42.0), 42.0);
    }

    #[test]
    fn long_text_is_elided_and_stays_within_width() {
        let painter = test_painter();
        let galley = truncated_galley(
            &painter,
            "A label long enough to need truncation",
            60.0,
            12.0,
            Color32::WHITE,
        );
        assert!(galley.elided);
        assert!(galley.size().x <= 60.0, "width {}", galley.size().x);
    }

    #[test]
    fn short_text_is_unchanged() {
        let painter = test_painter();
        let galley = truncated_galley(&painter, "Short", 200.0, 12.0, Color32::WHITE);
        assert!(!galley.elided);
        assert_eq!(galley.text(), "Short");
    }
}
