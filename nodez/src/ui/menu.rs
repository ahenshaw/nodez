//! The add-node search popup, Blender's `Shift+A` and its link-drag search.

use egui::{Color32, Context, Id, Key, Pos2, RichText, Vec2};

/// Height of one row in the list.
const ROW_HEIGHT: f32 = 17.0;

use crate::graph::SocketRef;
use crate::template::{NodeLibrary, TemplateId};
use crate::types::DataTypeId;

/// When the menu was opened by dropping a wire, this remembers what the new
/// node has to connect to.
#[derive(Clone, Debug)]
pub(crate) struct LinkFilter {
    pub anchor: SocketRef,
    /// Whether the dragged wire started at an output socket.
    pub anchor_is_output: bool,
    pub ty: DataTypeId,
}

/// The open add-node popup.
#[derive(Clone, Debug)]
pub(crate) struct MenuState {
    /// Where the popup is drawn.
    pub screen_pos: Pos2,
    /// Where the new node is placed, in graph space.
    pub graph_pos: Pos2,
    pub query: String,
    pub focus: bool,
    pub link: Option<LinkFilter>,
    pub highlighted: usize,
}

impl MenuState {
    pub fn new(screen_pos: Pos2, graph_pos: Pos2, link: Option<LinkFilter>) -> Self {
        Self {
            screen_pos,
            graph_pos,
            query: String::new(),
            focus: true,
            link,
            highlighted: 0,
        }
    }
}

/// What the popup did this frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MenuOutcome {
    pub picked: Option<TemplateId>,
    pub close: bool,
}

pub(crate) fn show_menu(
    ctx: &Context,
    id: Id,
    state: &mut MenuState,
    library: &NodeLibrary,
) -> MenuOutcome {
    let mut outcome = MenuOutcome::default();

    // Candidates: everything matching the query, narrowed to what could accept
    // or feed the dragged wire.
    let mut matches = library.search(&state.query);
    if let Some(link) = &state.link {
        matches.retain(|(_, template)| {
            if link.anchor_is_output {
                template
                    .inputs
                    .iter()
                    .any(|s| !s.hidden && library.types.compatible(link.ty, s.ty))
            } else {
                template
                    .outputs
                    .iter()
                    .any(|s| !s.hidden && library.types.compatible(s.ty, link.ty))
            }
        });
    }
    if matches.is_empty() {
        state.highlighted = 0;
    } else {
        state.highlighted = state.highlighted.min(matches.len() - 1);
    }

    ctx.input(|i| {
        if i.key_pressed(Key::Escape) {
            outcome.close = true;
        }
        if i.key_pressed(Key::ArrowDown) && !matches.is_empty() {
            state.highlighted = (state.highlighted + 1) % matches.len();
        }
        if i.key_pressed(Key::ArrowUp) && !matches.is_empty() {
            state.highlighted = (state.highlighted + matches.len() - 1) % matches.len();
        }
        if i.key_pressed(Key::Enter)
            && let Some((template, _)) = matches.get(state.highlighted)
        {
            outcome.picked = Some(*template);
            outcome.close = true;
        }
    });

    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(state.screen_pos)
        .constrain(true);

    let response = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style())
            .fill(Color32::from_rgb(0x2A, 0x2A, 0x2A))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x12, 0x12, 0x12)))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_min_width(240.0);
                ui.set_max_width(320.0);

                if let Some(link) = &state.link {
                    ui.label(
                        RichText::new(format!(
                            "Link {} \u{2192} {}",
                            library.types.name(link.ty),
                            if link.anchor_is_output { "input" } else { "output" }
                        ))
                        .size(11.0)
                        .color(library.types.color(link.ty)),
                    );
                }

                let search = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("Search nodes\u{2026}")
                        .desired_width(f32::INFINITY),
                );
                if state.focus {
                    search.request_focus();
                    state.focus = false;
                }

                ui.add_space(4.0);
                if matches.is_empty() {
                    ui.weak("No matching nodes");
                    return;
                }
                ui.spacing_mut().item_spacing.y = 1.0;

                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let grouped = state.query.trim().is_empty() && state.link.is_none();
                        if grouped {
                            show_grouped(ui, &matches, library, state, &mut outcome);
                        } else {
                            for (index, (template, spec)) in matches.iter().enumerate() {
                                if entry(ui, library, spec, index == state.highlighted, true) {
                                    outcome.picked = Some(*template);
                                    outcome.close = true;
                                }
                            }
                        }
                    });
            });
    });

    // Clicking anywhere outside dismisses the popup.
    if ctx.input(|i| i.pointer.any_pressed()) {
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        if let Some(pos) = pointer
            && !response.response.rect.contains(pos)
        {
            outcome.close = true;
        }
    }

    outcome
}

fn show_grouped(
    ui: &mut egui::Ui,
    matches: &[(TemplateId, &crate::template::NodeTemplate)],
    library: &NodeLibrary,
    state: &MenuState,
    outcome: &mut MenuOutcome,
) {
    let mut index = 0usize;
    for category in library.categories() {
        let in_category: Vec<_> = matches
            .iter()
            .filter(|(_, t)| t.category == category)
            .collect();
        if in_category.is_empty() {
            continue;
        }
        ui.add_space(3.0);
        ui.label(
            RichText::new(category)
                .size(10.0)
                .color(library.category_color(category).unwrap_or(Color32::GRAY)),
        );
        for (template, spec) in in_category {
            if entry(ui, library, spec, index == state.highlighted, false) {
                outcome.picked = Some(*template);
                outcome.close = true;
            }
            index += 1;
        }
    }
}

/// One row of the list. Returns true when clicked.
///
/// Painted by hand rather than with a button so the label stays left-aligned
/// next to its colour bar at any width.
fn entry(
    ui: &mut egui::Ui,
    library: &NodeLibrary,
    template: &crate::template::NodeTemplate,
    highlighted: bool,
    show_category: bool,
) -> bool {
    let color = template
        .header_color
        .or_else(|| library.category_color(&template.category))
        .unwrap_or(Color32::GRAY);

    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), egui::Sense::click());

    let painter = ui.painter();
    if highlighted || response.hovered() {
        let fill = if highlighted {
            ui.visuals().selection.bg_fill
        } else {
            ui.visuals().widgets.hovered.bg_fill
        };
        painter.rect_filled(rect, egui::CornerRadius::same(2), fill);
    }

    let bar = egui::Rect::from_min_size(
        rect.left_top() + Vec2::new(3.0, 3.0),
        Vec2::new(3.0, (rect.height() - 6.0).max(2.0)),
    );
    painter.rect_filled(bar, egui::CornerRadius::same(1), color);

    let font = egui::FontId::proportional(12.0);
    let text_left = rect.left() + 12.0;
    let label_end = painter.text(
        egui::pos2(text_left, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &template.label,
        font.clone(),
        ui.visuals().text_color(),
    );
    if show_category {
        painter.text(
            egui::pos2(label_end.right() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &template.category,
            egui::FontId::proportional(10.0),
            ui.visuals().weak_text_color(),
        );
    }

    if highlighted {
        response.scroll_to_me(None);
    }
    let response = if template.description.is_empty() {
        response
    } else {
        response.on_hover_text(&template.description)
    };
    response.clicked()
}
