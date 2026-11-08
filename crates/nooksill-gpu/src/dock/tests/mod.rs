//! Dock behaviour tests.

use super::*;

mod anim;
mod drag;
mod hide;
mod hit;
mod paint;

/// Center of slot `i`, window physical pixels.
fn item_center(dock: &Dock, i: usize) -> (f32, f32) {
    let (l, r) = (dock.geom.slot_l[i], dock.geom.slot_r[i]);
    let y = (dock.geom.bar_top + dock.geom.bar_bottom) / 2.0;
    ((l + r) / 2.0, y)
}

fn settled_dock(n: usize) -> Dock {
    let cfg = Config::defaults();
    let items: Vec<DockItem> = (0..n)
        .map(|i| {
            let it = Item::new(ItemKind::File, format!("第{i}项"), format!("C:/x{i}.exe"));
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        })
        .collect();
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    dock
}

/// Dock with three items and room to drag.
fn drag_dock() -> Dock {
    let cfg = Config::defaults();
    let items: Vec<DockItem> = (0..3)
        .map(|i| {
            let it = Item::new(ItemKind::File, format!("第{i}项"), format!("C:/x{i}.exe"));
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        })
        .collect();
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    dock
}

/// Dock with a separator at index 1; returns the separator hit point.
fn dock_with_sep(enabled: bool) -> (Dock, f32, f32) {
    let mut cfg = Config::defaults();
    cfg.apply_i32(Key::SEP_SCALE, i32::from(enabled));
    let kinds = [ItemKind::File, ItemKind::Separator, ItemKind::File];
    let items: Vec<DockItem> = kinds
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let it = Item::new(*k, format!("第{i}项"), format!("C:/x{i}.exe"));
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        })
        .collect();
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    let u = (dock.geom.slot_l[1] + dock.geom.slot_r[1]) * 0.5;
    let v = dock.geom.bar_top + dock.metrics.bar_h * 0.5;
    assert_eq!(dock.hit_at(u, v), Some(1), "测试坐标要真的落在分割线上");
    (dock, u, v)
}

/// Dock mixing pinned items and running apps.
fn dock_with_temps(pinned: usize, total: usize) -> Dock {
    let cfg = Config::defaults();
    let items: Vec<DockItem> = (0..total)
        .map(|i| {
            let it = Item::new(ItemKind::File, format!("第{i}项"), format!("C:/x{i}.exe"));
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        })
        .collect();
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));
    dock.set_pinned(pinned);
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    dock
}
