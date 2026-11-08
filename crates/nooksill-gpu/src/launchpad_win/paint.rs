//! The launchpad grid painter.

use super::paint_folder::{
    draw_folder_icon, draw_folder_panel, draw_letter_tile, fit_size, fit_text, hit_cell_in, measure,
};
use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "argument group is the API boundary"
)]
pub(super) fn draw(
    ui: &mut egui::Ui,
    dark: bool,
    (w, h): (f32, f32),
    lp: &Launchpad,
    (cols, rows): (usize, usize),
    page: usize,
    query: &str,
    composing: &str,
    open_folder: Option<usize>,
    hover: Option<Hit>,
    drag_src: Option<usize>,
    cur: (f32, f32),
    icons: &HashMap<usize, egui::TextureHandle>,
    ov: Overlay<'_>,
    frost: bool,
) -> (HitList, HitList) {
    let painter = ui.painter().clone();
    let rect = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(w, h));

    // Backing plate: with frost on, only a translucent tint is painted so
    // the blurred desktop drawn by the `frost` module shows through; without
    // it (or after a failed capture) the near opaque 246/255 fill is kept,
    // because the real desktop behind would only reveal the window outline
    // and look unfinished.
    let bg = if frost {
        crate::frost::panel_bg(dark, true)
    } else if dark {
        Color32::from_rgba_unmultiplied(12, 13, 18, 246)
    } else {
        Color32::from_rgba_unmultiplied(238, 241, 248, 246)
    };
    painter.rect_filled(rect, CornerRadius::ZERO, bg);

    let tc = if dark {
        Color32::from_rgba_unmultiplied(240, 240, 245, 255)
    } else {
        Color32::from_rgba_unmultiplied(28, 28, 34, 255)
    };
    let sub = if dark {
        Color32::from_rgba_unmultiplied(200, 202, 214, 255)
    } else {
        Color32::from_rgba_unmultiplied(90, 94, 108, 255)
    };
    let accent = ui.visuals().selection.bg_fill;
    let hover_bg = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 46)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 34)
    };

    let mut hits: HitList = Vec::new();
    let mut icon_rects: HitList = Vec::new();

    // Search box, which turns into the rename field in place: a fixed
    // position does not collide with the tab bar and saves an extra overlay
    // to draw.
    let renaming = ov.renaming.is_some();
    let search = EguiRect::from_min_size(
        Pos2::new((w - SEARCH_W) * 0.5, 40.0),
        egui::vec2(SEARCH_W, SEARCH_H),
    );
    painter.rect_filled(
        search,
        CornerRadius::same((SEARCH_H * 0.5) as u8),
        if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 70)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 150)
        },
    );
    if renaming {
        // An accent ring marks the input state and tells it apart from the
        // search box used for filtering.
        painter.rect_stroke(
            search,
            CornerRadius::same((SEARCH_H * 0.5) as u8),
            Stroke::new(1.6, accent),
            egui::StrokeKind::Inside,
        );
    }
    // Search box: the committed text plus the pinyin being composed. The
    // pinyin is underlined so the user can tell the IME is still working
    // instead of assuming the app froze.
    let text = if renaming {
        // The caret is drawn as a bar: there is no native text field here to
        // blink in.
        format!("{}|", ov.rename_buf)
    } else {
        format!("{query}{composing}")
    };
    if renaming {
        // The name can be long (40 characters max), so the whole line
        // shrinks instead: overflowing the rounded box looks worse than a
        // small font.
        let size = fit_size(ui, &text, SEARCH_W - 28.0, 14.0);
        let full = measure(ui, &text, size);
        painter.text(
            egui::pos2(search.center().x - full * 0.5, search.center().y),
            Align2::LEFT_CENTER,
            &text,
            FontId::proportional(size),
            tc,
        );
        // The hint goes outside the box on the right: the name already fills
        // the inside, so anything more would collide.
        painter.text(
            egui::pos2(search.right() + 14.0, search.center().y),
            Align2::LEFT_CENTER,
            "回车确认 · Esc 取消",
            FontId::proportional(11.5),
            sub,
        );
    } else if text.is_empty() {
        painter.text(
            search.center(),
            Align2::CENTER_CENTER,
            "搜索应用…（可直接打中文）",
            FontId::proportional(13.0),
            sub,
        );
    } else {
        let w_full = measure(ui, &text, 13.0);
        let left = search.center().x - w_full * 0.5;
        painter.text(
            egui::pos2(left, search.center().y),
            Align2::LEFT_CENTER,
            &text,
            FontId::proportional(13.0),
            tc,
        );
        if !composing.is_empty() {
            let w_done = measure(ui, query, 13.0);
            let y = search.center().y + 8.0;
            painter.line_segment(
                [egui::pos2(left + w_done, y), egui::pos2(left + w_full, y)],
                Stroke::new(1.0, accent),
            );
        }
    }

    // Tab bar (all, tag groups, plus), right below the search box: a
    // category is another way of filtering, so the two belong together.
    let nt = 1 + lp.tags().len();
    let tab_h = 28.0;
    let tab_gap = 8.0;
    let tab_y = search.bottom() + 8.0;
    let cell_w = (SEARCH_W - tab_gap * nt as f32) / (nt + 1) as f32;
    for t in 0..=nt {
        let tx = search.left() + t as f32 * (cell_w + tab_gap);
        let r = EguiRect::from_min_size(Pos2::new(tx, tab_y), egui::vec2(cell_w, tab_h));
        let on = ov.cat == t && t <= lp.tags().len();
        let adding = t == nt;
        let label = if adding {
            "＋".to_owned()
        } else if t == 0 {
            "全部".to_owned()
        } else {
            lp.tags()[t - 1].clone()
        };
        let fill = if on {
            accent
        } else if hover == Some(Hit::Tab(t)) {
            hover_bg
        } else if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 45)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 26)
        };
        painter.rect_filled(r, CornerRadius::same(9), fill);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            fit_text(ui, &label, cell_w - 8.0, 12.5),
            FontId::proportional(12.5),
            if on { Color32::WHITE } else { sub },
        );
        // The group being renamed gets a ring, so the user knows which one
        // is being edited.
        if t >= 1 && t <= lp.tags().len() && ov.renaming == Some(RenameTarget::Tag(t - 1)) {
            painter.rect_stroke(
                r,
                CornerRadius::same(9),
                Stroke::new(2.0, accent),
                egui::StrokeKind::Inside,
            );
        }
        hits.push((Hit::Tab(t), r));
    }

    // A-Z sort button, left of the search box.
    let az = EguiRect::from_min_size(
        Pos2::new(search.left() - SEARCH_H - 12.0, search.top()),
        egui::vec2(SEARCH_H, SEARCH_H),
    );
    if az.left() > 8.0 {
        painter.rect_filled(
            az,
            CornerRadius::same((SEARCH_H * 0.5) as u8),
            if hover == Some(Hit::SortAz) {
                hover_bg
            } else if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 60)
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, 40)
            },
        );
        painter.text(
            az.center(),
            Align2::CENTER_CENTER,
            "A–Z",
            FontId::proportional(13.0),
            tc,
        );
        hits.push((Hit::SortAz, az));
    }

    // Plus button.
    let plus = EguiRect::from_min_size(Pos2::new(w - 84.0, 40.0), egui::vec2(SEARCH_H, SEARCH_H));
    hits.push((Hit::Plus, plus));
    painter.rect_filled(
        plus,
        CornerRadius::same((SEARCH_H * 0.5) as u8),
        if hover == Some(Hit::Plus) {
            hover_bg
        } else if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 60)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 40)
        },
    );
    painter.text(
        plus.center(),
        Align2::CENTER_CENTER,
        "+",
        FontId::proportional(22.0),
        tc,
    );

    // Grid.
    let per = cols * rows;
    let cw = (w - 2.0 * SIDE) / cols as f32;
    let ch = (h - TOP_H - BOT_H) / rows as f32;
    let icon_s = 88.0_f32.min(cw * 0.56).min(ch * 0.5);
    let gx = SIDE;
    let gy = TOP_H;

    // The filtered view applies with a search query **or** a category. Only
    // when both are empty are top level entries and folders shown, otherwise
    // just apps (categories classify apps, folders have none).
    let filtering = ov.cat != 0 || !query.is_empty();
    let filtered: Vec<usize> = if filtering {
        lp.visible_in(ov.cat, query)
    } else {
        Vec::new()
    };
    let count = if filtering { filtered.len() } else { lp.len() };

    for k in 0..per {
        let idx = page * per + k;
        if idx >= count {
            break;
        }
        let col = k % cols;
        let row = k / cols;
        let cell = EguiRect::from_min_size(
            Pos2::new(gx + col as f32 * cw, gy + row as f32 * ch),
            egui::vec2(cw, ch),
        );
        if hover == Some(Hit::Cell(k)) && drag_src.is_none() {
            painter.rect_filled(
                EguiRect::from_min_max(
                    cell.min + egui::vec2(4.0, 2.0),
                    cell.max - egui::vec2(4.0, 8.0),
                ),
                CornerRadius::same(16),
                hover_bg,
            );
        }
        // The cell being renamed gets a ring. Without it the user would only
        // see a new input box on top and not know which app or folder is
        // being renamed.
        if !filtering && ov.renaming == Some(RenameTarget::Cell(idx)) {
            painter.rect_stroke(
                EguiRect::from_min_max(
                    cell.min + egui::vec2(4.0, 2.0),
                    cell.max - egui::vec2(4.0, 8.0),
                ),
                CornerRadius::same(16),
                Stroke::new(2.0, accent),
                egui::StrokeKind::Inside,
            );
        }

        let icon_rect = EguiRect::from_min_size(
            Pos2::new(
                cell.center().x - icon_s * 0.5,
                cell.top() + (ch - icon_s - 34.0) * 0.5,
            ),
            egui::vec2(icon_s, icon_s),
        );
        icon_rects.push((Hit::Cell(k), icon_rect));
        // The cell body goes into `hits` too; `icon_rects` only answers
        // "is the pointer on the icon". Missing this shows up as clicks on
        // the icon doing nothing and right click not opening a menu, because
        // `hit_at` only searches `hits`.
        hits.push((Hit::Cell(k), cell));

        let (name, app_idx, is_folder, folder) = if !filtering {
            match lp.top()[idx] {
                Top::App(i) => (lp.apps()[i].name.clone(), Some(i), false, None),
                Top::Folder(f) => (lp.folders()[f].name.clone(), None, true, Some(f)),
            }
        } else {
            let ai = filtered[idx];
            (lp.apps()[ai].name.clone(), Some(ai), false, None)
        };

        if is_folder {
            draw_folder_icon(&painter, &icon_rect, lp, folder.unwrap_or(0), icons);
        } else if let Some(i) = app_idx {
            if let Some(tex) = icons.get(&i) {
                painter.image(
                    tex.id(),
                    icon_rect,
                    EguiRect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                draw_letter_tile(&painter, &icon_rect, &name, dark);
            }
        }

        // Name, truncated when too wide. The dragged one is drawn dimmer.
        let ghost = drag_src == Some(k);
        painter.text(
            Pos2::new(cell.center().x, icon_rect.bottom() + 8.0),
            Align2::CENTER_TOP,
            fit_text(ui, &name, cw - 12.0, 11.5),
            FontId::proportional(11.5),
            if ghost { sub } else { tc },
        );

        // The keyboard selection is a halo around the icon. Hover uses a
        // background instead, so the two do not conflict.
        if ov.sel == Some(k) && drag_src.is_none() {
            let halo = icon_rect.expand(icon_s * 0.1);
            painter.rect_stroke(
                halo,
                CornerRadius::same((icon_s * 0.24) as u8),
                Stroke::new(2.5, accent),
                egui::StrokeKind::Outside,
            );
        }
    }

    // Empty state.
    if count == 0 {
        painter.text(
            Pos2::new(w * 0.5, h * 0.5 - 20.0),
            Align2::CENTER_CENTER,
            if !query.is_empty() {
                "没有找到匹配的应用"
            } else if ov.cat != 0 {
                "这一组里还没有应用 —— 右键图标可以把它移过来"
            } else {
                "起动台是空的 —— 把文件、快捷方式或文件夹拖进来"
            },
            FontId::proportional(14.0),
            sub,
        );
    }

    // Page dots.
    let pages = Launchpad::pages(count, per);
    if pages > 1 {
        let start = w * 0.5 - (pages - 1) as f32 * 11.0;
        for p in 0..pages.min(40) {
            painter.circle_filled(
                Pos2::new(start + p as f32 * 22.0, h - 104.0),
                4.0,
                if p == page { tc } else { sub },
            );
        }
    }

    // The dragged icon follows the pointer.
    if let Some(k) = drag_src {
        let idx = page * per + k;
        if filtering {
            // In the filtered view a cell is the app itself, so its icon is
            // drawn directly.
            if let Some(&ai) = filtered.get(idx) {
                let s = icon_s * 1.1;
                let r = EguiRect::from_min_size(
                    Pos2::new(cur.0 - s * 0.5, cur.1 - s * 0.5),
                    egui::vec2(s, s),
                );
                if let Some(tex) = icons.get(&ai) {
                    painter.image(
                        tex.id(),
                        r,
                        EguiRect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::from_rgba_unmultiplied(255, 255, 255, 220),
                    );
                } else {
                    draw_letter_tile(&painter, &r, &lp.apps()[ai].name, dark);
                }
            }
        } else {
            if let Some(t) = lp.top().get(idx).copied() {
                let s = icon_s * 1.1;
                let r = EguiRect::from_min_size(
                    Pos2::new(cur.0 - s * 0.5, cur.1 - s * 0.5),
                    egui::vec2(s, s),
                );
                match t {
                    Top::App(i) => {
                        if let Some(tex) = icons.get(&i) {
                            painter.image(
                                tex.id(),
                                r,
                                EguiRect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                Color32::from_rgba_unmultiplied(255, 255, 255, 220),
                            );
                        } else {
                            draw_letter_tile(&painter, &r, &lp.apps()[i].name, dark);
                        }
                    }
                    Top::Folder(f) => draw_folder_icon(&painter, &r, lp, f, icons),
                }
                // Outline the drop target when the pointer is on its icon.
                if let Some(Hit::Cell(tk)) = hit_cell_in(per, cols, gx, gy, cw, ch, cur) {
                    if tk < count && tk != k {
                        let cell = EguiRect::from_min_size(
                            Pos2::new(gx + (tk % cols) as f32 * cw, gy + (tk / cols) as f32 * ch),
                            egui::vec2(cw, ch),
                        );
                        if cell.contains(Pos2::new(cur.0, cur.1)) {
                            let halo = EguiRect::from_min_size(
                                Pos2::new(
                                    gx + (tk % cols) as f32 * cw + cw * 0.5 - icon_s * 0.62,
                                    gy + (tk / cols) as f32 * ch + (ch - icon_s - 34.0) * 0.5
                                        - icon_s * 0.12,
                                ),
                                egui::vec2(icon_s * 1.24, icon_s * 1.24),
                            );
                            painter.rect_stroke(
                                halo,
                                CornerRadius::same((icon_s * 0.3) as u8),
                                Stroke::new(2.5, accent),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }
                }
            }
        }
    }

    // Folder panel.
    if let Some(fi) = open_folder {
        draw_folder_panel(
            ui,
            dark,
            (w, h),
            lp,
            fi,
            cols,
            cw,
            ch,
            icon_s,
            icons,
            hover,
            &mut hits,
        );
    }

    // Context menu and rename field, drawn in the same window instead of a
    // second HWND: hit testing, IME and repaints then all go through one
    // path, and no extra window class and message loop are needed.
    let mut overlay_hits: HitList = Vec::new();
    if !ov.menu_items.is_empty() {
        let labels = ov.menu_items;
        let row_h = 28.0;
        let pad = 6.0;
        // The width follows the longest item: the labels are named by the
        // user ("office", "design assets", ...), and a fixed 170 would
        // truncate long names.
        let widest = labels
            .iter()
            .map(|l| measure(ui, l, 12.5))
            .fold(170.0_f32 - 34.0, f32::max);
        let mw = (widest + 34.0).max(170.0).min(w - 24.0);
        let mh = labels.len() as f32 * row_h + pad * 2.0;
        let mx = ov.menu_at.0.min(w - mw - 8.0).max(8.0);
        let my = ov.menu_at.1.min(h - mh - 8.0).max(8.0);
        let panel = EguiRect::from_min_size(Pos2::new(mx, my), egui::vec2(mw, mh));
        painter.rect_filled(
            panel,
            CornerRadius::same(10),
            if dark {
                Color32::from_rgba_unmultiplied(46, 47, 54, 252)
            } else {
                Color32::from_rgba_unmultiplied(252, 252, 255, 252)
            },
        );
        painter.rect_stroke(
            panel,
            CornerRadius::same(10),
            Stroke::new(1.0, sub),
            egui::StrokeKind::Inside,
        );
        for (i, label) in labels.iter().enumerate() {
            let r = EguiRect::from_min_size(
                Pos2::new(mx + 4.0, my + pad + i as f32 * row_h),
                egui::vec2(mw - 8.0, row_h),
            );
            if hover == Some(Hit::MenuItem(i)) {
                painter.rect_filled(r, CornerRadius::same(6), hover_bg);
            }
            painter.text(
                egui::pos2(r.left() + 10.0, r.center().y),
                Align2::LEFT_CENTER,
                label.as_str(),
                FontId::proportional(12.5),
                if label == "从起动台移除" {
                    Color32::from_rgba_unmultiplied(230, 74, 70, 255)
                } else {
                    tc
                },
            );
            overlay_hits.push((Hit::MenuItem(i), r));
        }
    }
    // The overlay hit areas come first: they float above the grid.
    overlay_hits.extend(hits);
    (overlay_hits, icon_rects)
}
