//! Items page: manage what sits on the dock bar.
//!
//! Three cards: dock items, groups, and creation. The item list is a snapshot
//! synced by the host program, which owns the real state.

use egui::{Align, Id, Layout, RichText, Sense, Ui};

use nooksill_core::item::{Item, ItemKind};

use super::widgets::{card, small_button, Ctx};
use super::{Command, ItemEdit};
use crate::text_field::{text_field, FieldOpts, FIELD_H};

/// Badge text for an item kind.
fn kind_tag(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::File => nooksill_core::i18n::t("App"),
        ItemKind::Folder => nooksill_core::i18n::t("Folder"),
        ItemKind::Url => nooksill_core::i18n::t("Web page"),
        ItemKind::Separator => nooksill_core::i18n::t("Separator"),
        ItemKind::Group => nooksill_core::i18n::t("Group"),
        ItemKind::Trash => nooksill_core::i18n::t("Recycle Bin"),
        ItemKind::Note => nooksill_core::i18n::t("Notes"),
    }
}

/// Second line of an item row: path or child count.
fn detail(item: &Item) -> String {
    match item.kind {
        ItemKind::Separator => nooksill_core::i18n::t("Bar separator").to_owned(),
        ItemKind::Group => {
            nooksill_core::i18n::fmt("{0} items", &[item.kids.len().to_string().as_str()])
        }
        ItemKind::Trash => nooksill_core::i18n::t("System Recycle Bin").to_owned(),
        _ => {
            if item.path.is_empty() {
                nooksill_core::i18n::t("(No target)").to_owned()
            } else {
                item.path.clone()
            }
        }
    }
}

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    items_card(ctx, ui);
    groups_card(ctx, ui);
    create_card(ctx, ui);
}

/// Dock items: rename, edit path and arguments, reorder, remove.
fn items_card(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Dock items"),
        nooksill_core::i18n::t(
            "Everything on the bar is here. Changes apply and save immediately.",
        ),
        |ctx, ui| {
            if ctx.state.items.is_empty() {
                ui.label(
                    RichText::new(nooksill_core::i18n::t(
                        "No items yet. Add some on the Add page or drop files onto the Dock.",
                    ))
                    .small()
                    .weak(),
                );
                return;
            }
            // Drawn in reverse: the list reads top-to-bottom as left-to-right
            // on the bar, so the top row is the item closest to the dock's
            // left end.
            let n = ctx.state.items.len();
            for i in (0..n).rev() {
                let item = ctx.state.items[i].clone();
                item_row(ctx, ui, i, &item);
            }
        },
    );
}

/// One item row.
fn item_row(ctx: &mut Ctx<'_>, ui: &mut Ui, index: usize, item: &Item) {
    let editing = ctx.state.editing == Some(index);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width() - 190.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("[{}]", kind_tag(item.kind)))
                        .small()
                        .weak(),
                );
                let name = if item.name.is_empty() {
                    nooksill_core::i18n::t("(Untitled)")
                } else {
                    item.name.as_str()
                };
                ui.label(RichText::new(name).strong());
            });
            ui.label(RichText::new(detail(item)).small().weak());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if small_button(
                ui,
                nooksill_core::i18n::t("Delete"),
                nooksill_core::i18n::t("Remove this item from the Dock"),
            ) {
                ctx.out.command = Some(Command::RemoveItem(index));
            }
            if small_button(
                ui,
                nooksill_core::i18n::t("Edit"),
                nooksill_core::i18n::t("Rename, change the target and arguments"),
            ) {
                begin_edit(ctx, index, item);
            }
            if small_button(ui, "↓", nooksill_core::i18n::t("Move down")) {
                ctx.out.command = Some(Command::MoveItem(index, 1));
            }
            if small_button(ui, "↑", nooksill_core::i18n::t("Move up")) {
                ctx.out.command = Some(Command::MoveItem(index, -1));
            }
        });
    });
    ui.add_space(6.0);

    if editing {
        edit_form(ctx, ui, index, item);
    }
}

/// Enter edit mode, copying the current values into the buffers.
fn begin_edit(ctx: &mut Ctx<'_>, index: usize, item: &Item) {
    ctx.state.editing = Some(index);
    ctx.state.edit_name = item.name.clone();
    ctx.state.edit_path = item.path.clone();
    ctx.state.edit_args = item.args.clone();
    ctx.state.edit_group = None;
}

/// Edit form: name, path, arguments and target group.
fn edit_form(ctx: &mut Ctx<'_>, ui: &mut Ui, index: usize, item: &Item) {
    let groups: Vec<(usize, String)> = ctx
        .state
        .items
        .iter()
        .enumerate()
        .filter(|(i, it)| it.kind == ItemKind::Group && *i != index)
        .map(|(i, it)| (i, it.name.clone()))
        .collect();

    ui.push_id(index, |ui| {
        egui::Grid::new("item-edit")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(nooksill_core::i18n::t("Name"));
                let w = ui.available_width().max(140.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, FIELD_H), Sense::hover());
                text_field(
                    ui,
                    rect,
                    Id::unique("item-edit-name"),
                    &mut ctx.state.edit_name,
                    &FieldOpts {
                        hint: nooksill_core::i18n::t("Shown in the hover bubble and group panel"),
                        ..FieldOpts::default()
                    },
                );
                ui.end_row();

                ui.label(nooksill_core::i18n::t("Target"));
                let w = ui.available_width().max(140.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, FIELD_H), Sense::hover());
                let path_hint = match item.kind {
                    ItemKind::Url => "https://example.com",
                    ItemKind::Trash => nooksill_core::i18n::t("The Recycle Bin needs no target"),
                    _ => nooksill_core::i18n::t("Program path or folder path"),
                };
                text_field(
                    ui,
                    rect,
                    Id::unique("item-edit-path"),
                    &mut ctx.state.edit_path,
                    &FieldOpts {
                        hint: path_hint,
                        ..FieldOpts::default()
                    },
                );
                ui.end_row();

                ui.label(nooksill_core::i18n::t("Arguments"));
                let w = ui.available_width().max(140.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, FIELD_H), Sense::hover());
                text_field(
                    ui,
                    rect,
                    Id::unique("item-edit-args"),
                    &mut ctx.state.edit_args,
                    &FieldOpts {
                        hint: nooksill_core::i18n::t("Extra command-line arguments (optional)"),
                        ..FieldOpts::default()
                    },
                );
                ui.end_row();

                let group_label = ui.label(nooksill_core::i18n::t("Assign to group"));
                egui::ComboBox::from_id_salt("item-edit-group")
                    .selected_text(match ctx.state.edit_group {
                        None => nooksill_core::i18n::t("Keep on the bar").to_owned(),
                        Some(g) => groups.iter().find(|(i, _)| *i == g).map_or_else(
                            || nooksill_core::i18n::t("Keep on the bar").to_owned(),
                            |(_, n)| n.clone(),
                        ),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut ctx.state.edit_group,
                            None,
                            nooksill_core::i18n::t("Keep on the bar"),
                        );
                        for (i, name) in &groups {
                            ui.selectable_value(&mut ctx.state.edit_group, Some(*i), name);
                        }
                    })
                    .response
                    // AccessKit does not adopt the adjacent label as the
                    // widget name, so screen readers and kittest need this.
                    .labelled_by(group_label.id);
                ui.end_row();
            });

        // Icon row stays outside the Grid: a path plus two buttons would widen
        // column two, and the other rows' `available_width() - 60` would go
        // negative on the next frame and panic.
        //
        // Only formats the shell can decode are offered: there is no PNG or
        // JPEG decoder, so listing them would let the user pick a dead path.
        ui.horizontal(|ui| {
            ui.label(nooksill_core::i18n::t("Icon:"));
            let shown = if item.icon_src.is_empty() {
                nooksill_core::i18n::t("Default icon").to_owned()
            } else {
                let short: String = item
                    .icon_src
                    .chars()
                    .rev()
                    .take(20)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("…{short}")
            };
            ui.label(RichText::new(shown).small().weak());
            if small_button(
                ui,
                nooksill_core::i18n::t("Change…"),
                nooksill_core::i18n::t("Pick an .ico / .exe / .dll as the icon"),
            ) {
                ctx.out.command = Some(Command::PickIcon(index));
            }
            if !item.icon_src.is_empty()
                && small_button(
                    ui,
                    nooksill_core::i18n::t("Reset to default"),
                    nooksill_core::i18n::t("Clear the custom icon"),
                )
            {
                ctx.out.command = Some(Command::ResetIcon(index));
            }
        });

        ui.horizontal(|ui| {
            if small_button(
                ui,
                nooksill_core::i18n::t("Save"),
                nooksill_core::i18n::t("Write this item back"),
            ) {
                let edit = ItemEdit {
                    name: ctx.state.edit_name.trim().to_owned(),
                    path: ctx.state.edit_path.trim().to_owned(),
                    args: ctx.state.edit_args.trim().to_owned(),
                    group: ctx.state.edit_group,
                };
                ctx.out.command = Some(Command::EditItem(index, edit));
                ctx.state.editing = None;
            }
            if small_button(
                ui,
                nooksill_core::i18n::t("Cancel"),
                nooksill_core::i18n::t("Discard changes"),
            ) {
                ctx.state.editing = None;
            }
        });
    });
    ui.add_space(8.0);
}

/// Groups: move items in and out, dissolve.
fn groups_card(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Group"),
        nooksill_core::i18n::t(
            "Collect scattered items into a group; a group occupies one icon on the bar.",
        ),
        |ctx, ui| {
            let groups: Vec<(usize, Item)> = ctx
                .state
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| it.kind == ItemKind::Group)
                .map(|(i, it)| (i, it.clone()))
                .collect();
            if groups.is_empty() {
                ui.label(
                    RichText::new(nooksill_core::i18n::t(
                        "No groups yet. Create one in New below.",
                    ))
                    .small()
                    .weak(),
                );
                return;
            }
            for (gi, group) in groups {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&group.name).strong());
                    ui.label(
                        RichText::new(nooksill_core::i18n::fmt(
                            "{0} items",
                            &[group.kids.len().to_string().as_str()],
                        ))
                        .small()
                        .weak(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small_button(
                            ui,
                            nooksill_core::i18n::t("Dissolve"),
                            nooksill_core::i18n::t(
                                "Put all children back on the bar and remove the group",
                            ),
                        ) {
                            ctx.out.command = Some(Command::DissolveGroup(gi));
                        }
                    });
                });
                if group.kids.is_empty() {
                    ui.label(
                        RichText::new(nooksill_core::i18n::t("Empty group. Select an item above and set its Group field to this one."))
                            .small()
                            .weak(),
                    );
                } else {
                    for (ki, kid) in group.kids.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("· {}", display_name(kid))).small());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if small_button(
                                    ui,
                                    nooksill_core::i18n::t("Move out"),
                                    nooksill_core::i18n::t("Put this child back on the bar"),
                                ) {
                                    ctx.out.command = Some(Command::ExtractKid(gi, ki));
                                }
                                // Words instead of arrows: the item rows above
                                // already use arrows, and both lists share the
                                // page.
                                if ki + 1 < group.kids.len()
                                    && small_button(
                                        ui,
                                        nooksill_core::i18n::t("Move later"),
                                        nooksill_core::i18n::t("Move one slot later in the group"),
                                    )
                                {
                                    ctx.out.command = Some(Command::MoveKid(gi, ki, 1));
                                }
                                if ki > 0
                                    && small_button(
                                        ui,
                                        nooksill_core::i18n::t("Move earlier"),
                                        nooksill_core::i18n::t(
                                            "Move one slot earlier in the group",
                                        ),
                                    )
                                {
                                    ctx.out.command = Some(Command::MoveKid(gi, ki, -1));
                                }
                            });
                        });
                    }
                }
                ui.add_space(6.0);
            }
        },
    );
}

/// Creation: group, separator, trash.
fn create_card(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(ctx, ui, nooksill_core::i18n::t("New"), "", |ctx, ui| {
        ui.horizontal_wrapped(|ui| {
            if small_button(
                ui,
                nooksill_core::i18n::t("New group…"),
                nooksill_core::i18n::t("Create an empty group, then move items into it"),
            ) {
                ctx.out.command = Some(Command::NewGroup);
            }
            if small_button(
                ui,
                nooksill_core::i18n::t("Add separator"),
                nooksill_core::i18n::t("Append a separator to the bar"),
            ) {
                ctx.out.command = Some(Command::AddSeparator);
            }
            if small_button(
                ui,
                nooksill_core::i18n::t("Add Recycle Bin"),
                nooksill_core::i18n::t("Add a Recycle Bin entry"),
            ) {
                ctx.out.command = Some(Command::AddTrash);
            }
        });
    });
}

/// Display name of an item, falling back to the kind tag.
fn display_name(item: &Item) -> String {
    if item.name.is_empty() {
        kind_tag(item.kind).to_owned()
    } else {
        item.name.clone()
    }
}
