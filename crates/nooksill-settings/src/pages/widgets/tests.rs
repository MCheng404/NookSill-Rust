//! Unit tests for the pure geometry here; widget behaviour is covered by
//! `tests/settings_ui.rs` through kittest.

use super::motion::magnet_pull;

/// Full pull inside the rect, none past the radius, capped magnitude.
#[test]
fn 磁吸位移的分档与方向() {
    let base = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(160.0, 40.0));
    let c = base.center();

    // Inside: full pull, pointing from the centre to the pointer.
    let inside = magnet_pull(base, Some(egui::pos2(c.x - 30.0, c.y + 5.0)), 2.5, 56.0);
    assert!(
        inside.x < -2.0 && inside.y > 0.0,
        "应朝左下的光标偏移: {inside:?}"
    );
    assert!(
        inside.length() <= 2.5 + 1e-6,
        "幅度不得超过上限: {inside:?}"
    );

    // Exactly on the radius: zero.
    let edge = magnet_pull(base, Some(egui::pos2(base.left() - 56.0, c.y)), 2.5, 56.0);
    assert_eq!(edge, egui::Vec2::ZERO, "半径之外不应偏移");

    // Inside the radius but outside the rect: decays with distance.
    let near = magnet_pull(base, Some(egui::pos2(base.left() - 28.0, c.y)), 2.5, 56.0);
    assert!(
        near.x < -1.0 && near.x > -2.5,
        "一半距离应约为一半幅度: {near:?}"
    );

    // No pointer: no offset.
    assert_eq!(magnet_pull(base, None, 2.5, 56.0), egui::Vec2::ZERO);
}
