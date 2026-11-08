//! Group panel painter: cells, previews and arrows.

use super::*;

/// Offscreen entry point (`examples/panels_shot.rs`).
///
/// The icon texture map is empty, so this exercises the placeholder branch:
/// offscreen there is no shell icon to fetch.
pub fn paint_preview(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    title: &str,
    kids: &[GroupKid],
    hover: Option<usize>,
) {
    let empty: HashMap<usize, egui::TextureHandle> = HashMap::new();
    let _ = paint(ui, dark, frosted, title, kids, &empty, hover, None);
}

#[expect(
    clippy::too_many_arguments,
    reason = "样式、数据、悬停与释放状态集中在一处绘制，拆开要反复传 rect"
)]
pub(super) fn paint(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    title: &str,
    kids: &[GroupKid],
    icons: &HashMap<usize, egui::TextureHandle>,
    hover: Option<usize>,
    released: Option<usize>,
) -> (Vec<GroupAction>, bool) {
    let rect = ui.max_rect();
    // Shared panel style: base fill, overlay, hairline, three text tiers.
    let style = PanelStyle::current(dark, frosted);
    let (bg, ink, faint, border) = (style.bg(), style.ink(), style.ink_sub(), style.border());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(PanelStyle::R_PANEL), bg);
    painter.rect_filled(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        style.overlay(),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.left() + PAD, rect.top() + PAD + 2.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(15.0),
        ink,
    );

    // "Dissolve group" in the header.
    let dissolve;
    {
        let bw = 84.0;
        let bh = 26.0;
        let br = EguiRect::from_min_size(
            egui::pos2(rect.right() - PAD - bw, rect.top() + PAD),
            egui::vec2(bw, bh),
        );
        let resp = ui.interact(br, egui::Id::unique("group-dissolve"), egui::Sense::click());
        painter.rect_filled(
            br,
            PanelStyle::pill(br.height()),
            faint.linear_multiply(0.18),
        );
        painter.text(
            br.center(),
            Align2::CENTER_CENTER,
            "解散分组",
            FontId::proportional(12.0),
            ink,
        );
        dissolve = resp.clicked();
    }

    // Child grid.
    let mut actions = Vec::new();
    for (k, kid) in kids.iter().enumerate() {
        let Some((cx, cy, cw, ch)) = cell_rect(k, kids.len()) else {
            continue;
        };
        let cell = EguiRect::from_min_size(egui::pos2(cx, cy), egui::vec2(cw, ch));
        let hovered = hover == Some(k);
        if hovered {
            painter.rect_filled(
                cell,
                CornerRadius::same(PanelStyle::R_CARD),
                faint.linear_multiply(0.12),
            );
        }
        // Icon.
        if let Some(tex) = icons.get(&k) {
            painter.image(
                tex.id(),
                cell,
                // UVs are normalized 0..1, not pixel sizes. The id returned
                // by `load_texture` covers the whole atlas region, so
                // writing 0..60 would sample one corner and blow the icon
                // up 60x.
                EguiRect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            // Without a shell icon, draw the built-in glyph for the kind;
            // a name initial is unreadable at 20 px and cannot tell items
            // with the same name apart.
            crate::glyph::paint_kind(
                painter,
                cell,
                kid.kind,
                crate::glyph::IconStyle::new(
                    Color32::WHITE,
                    Color32::from_rgb(46, 123, 246),
                    crate::glyph::IconState::default(),
                ),
                dark,
            );
        }
        // Name.
        painter.text(
            egui::pos2(cell.center().x, cell.bottom() + 16.0),
            Align2::CENTER_CENTER,
            fit_text(painter, &kid.name, cw - 8.0, 11.5),
            FontId::proportional(11.5),
            faint,
        );
        // Running indicator dot.
        if kid.running {
            painter.circle_filled(
                egui::pos2(cell.center().x, cell.bottom() + 30.0),
                3.0,
                Color32::from_rgb(80, 200, 120),
            );
        }
        // The x in the top-right corner on hover removes the child.
        //
        // It must stay fully inside `cell_rect`: the x is only drawn while
        // `hover == Some(k)`, and `hover` comes from `hit_at` using
        // `cell_rect`. Outside the cell the cursor would clear the hover,
        // the x would vanish, and it could never be clicked.
        if hovered {
            let cr = EguiRect::from_min_size(
                egui::pos2(cell.right() - 18.0, cell.top() + 2.0),
                egui::vec2(16.0, 16.0),
            );
            let resp = ui.interact(cr, egui::Id::unique(("group-x", k)), egui::Sense::click());
            painter.circle_filled(cr.center(), 8.0, faint.linear_multiply(0.55));
            painter.line_segment(
                [
                    egui::pos2(cr.center().x - 3.5, cr.center().y - 3.5),
                    egui::pos2(cr.center().x + 3.5, cr.center().y + 3.5),
                ],
                Stroke::new(1.6, ink),
            );
            painter.line_segment(
                [
                    egui::pos2(cr.center().x + 3.5, cr.center().y - 3.5),
                    egui::pos2(cr.center().x - 3.5, cr.center().y + 3.5),
                ],
                Stroke::new(1.6, ink),
            );
            if resp.clicked() {
                actions.push(GroupAction::Remove(k));
            }

            // Reorder arrows in the top-left corner, same rule as the x:
            // fully inside `cell_rect`, else the hover is lost and the
            // button disappears under the cursor.
            if k > 0 {
                let r = EguiRect::from_min_size(
                    egui::pos2(cell.left() + 2.0, cell.top() + 2.0),
                    egui::vec2(16.0, 16.0),
                );
                if arrow_button(ui, painter, r, -1, faint, ink, k) {
                    actions.push(GroupAction::Move(k, -1));
                }
            }
            if k + 1 < kids.len() {
                let r = EguiRect::from_min_size(
                    egui::pos2(cell.left() + 20.0, cell.top() + 2.0),
                    egui::vec2(16.0, 16.0),
                );
                if arrow_button(ui, painter, r, 1, faint, ink, k) {
                    actions.push(GroupAction::Move(k, 1));
                }
            }
        }
    }

    // Released on the icon (not on a small button) means launch. The cell
    // hit at release is used so a click fires once.
    if let Some(k) = released {
        let touched = actions.iter().any(|a| match a {
            GroupAction::Launch(i) | GroupAction::Remove(i) | GroupAction::Move(i, _) => *i == k,
            _ => false,
        });
        if !touched {
            actions.push(GroupAction::Launch(k));
        }
    }

    (actions, dissolve)
}

/// A round arrow button. Returns whether it was clicked this frame.
pub(super) fn arrow_button(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: EguiRect,
    dir: i32,
    bg: Color32,
    ink: Color32,
    kid: usize,
) -> bool {
    let resp = ui.interact(
        rect,
        egui::Id::unique(("group-arrow", kid, dir)),
        egui::Sense::click(),
    );
    painter.circle_filled(rect.center(), 8.0, bg.linear_multiply(0.55));
    let c = rect.center();
    // The sign matters: `dir = -1` draws "<" and `dir = +1` draws ">".
    // Without it both arrows point the same way.
    let d = -3.0 * dir as f32;
    painter.line_segment(
        [egui::pos2(c.x + d, c.y - 3.5), egui::pos2(c.x - d, c.y)],
        Stroke::new(1.6, ink),
    );
    painter.line_segment(
        [egui::pos2(c.x - d, c.y), egui::pos2(c.x + d, c.y + 3.5)],
        Stroke::new(1.6, ink),
    );
    resp.clicked()
}

/// Truncates overlong text and appends an ellipsis.
pub(super) fn fit_text(painter: &egui::Painter, text: &str, max_w: f32, size: f32) -> String {
    let font = FontId::proportional(size);
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
        <= max_w
    {
        return text.to_owned();
    }
    let mut out: Vec<char> = text.chars().collect();
    while !out.is_empty() {
        out.pop();
        let s: String = out.iter().collect();
        if painter
            .layout_no_wrap(format!("{s}…"), font.clone(), Color32::WHITE)
            .size()
            .x
            <= max_w
        {
            return format!("{s}…");
        }
    }
    String::new()
}
