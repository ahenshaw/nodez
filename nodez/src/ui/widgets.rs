//! Mapping a [`Widget`] spec onto real egui widgets inside a node body.

use egui::{Align, Color32, Layout, Rect, Ui, UiBuilder};

use crate::template::Widget;
use crate::value::Value;

use super::style::EditorStyle;

/// Run `add` in a child `Ui` confined to `rect` with a stable id.
fn cell<R>(
    ui: &mut Ui,
    rect: Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .id_salt(salt)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            add(ui)
        },
    )
    .inner
}

/// Draw the editor for one value. Returns whether the value changed.
///
/// `rect` is the full row; the caller has already drawn any label that belongs
/// outside the widget.
#[allow(clippy::too_many_arguments)] // a layout helper; each argument is a distinct input
pub(crate) fn value_widget(
    ui: &mut Ui,
    rect: Rect,
    salt: impl std::hash::Hash + Copy + std::fmt::Debug,
    widget: &Widget,
    value: &mut Value,
    label: &str,
    style: &EditorStyle,
    zoom: f32,
) -> bool {
    if rect.width() < 4.0 || rect.height() < 4.0 {
        return false;
    }
    match widget {
        Widget::None => false,

        Widget::Checkbox => {
            let mut current = value.as_bool().unwrap_or(false);
            let changed = cell(ui, rect, salt, |ui| {
                ui.add(egui::Checkbox::new(&mut current, label)).changed()
            });
            if changed {
                *value = Value::Bool(current);
            }
            changed
        }

        Widget::Int {
            min,
            max,
            speed,
            suffix,
        } => {
            let mut current = value.as_i64().unwrap_or(0);
            let changed = cell(ui, rect, salt, |ui| {
                ui.add(
                    egui::DragValue::new(&mut current)
                        .speed(*speed)
                        .range(*min..=*max)
                        .suffix(suffix.clone()),
                )
                .changed()
            });
            if changed {
                *value = Value::Int(current);
            }
            changed
        }

        Widget::Float {
            min,
            max,
            speed,
            suffix,
        } => {
            let mut current = value.as_f64().unwrap_or(0.0);
            let changed = cell(ui, rect, salt, |ui| {
                ui.add(
                    egui::DragValue::new(&mut current)
                        .speed(*speed)
                        .range(*min..=*max)
                        .suffix(suffix.clone())
                        .max_decimals(4),
                )
                .changed()
            });
            if changed {
                *value = Value::Float(current);
            }
            changed
        }

        Widget::Slider { min, max } => {
            let mut current = value.as_f64().unwrap_or(0.0);
            let changed = cell(ui, rect, salt, |ui| {
                ui.spacing_mut().slider_width = ui.available_width() - 40.0 * zoom;
                ui.add(egui::Slider::new(&mut current, *min..=*max).max_decimals(3))
                    .changed()
            });
            if changed {
                *value = Value::Float(current);
            }
            changed
        }

        Widget::Text { multiline, hint } => {
            let mut current = value.as_str().unwrap_or_default().to_owned();
            let changed = cell(ui, rect, salt, |ui| {
                let hint = if hint.is_empty() { label } else { hint.as_str() };
                if *multiline {
                    ui.add_sized(
                        rect.size(),
                        egui::TextEdit::multiline(&mut current)
                            .hint_text(hint),
                    )
                    .changed()
                } else {
                    ui.add_sized(
                        rect.size(),
                        egui::TextEdit::singleline(&mut current)
                            .hint_text(hint),
                    )
                    .changed()
                }
            });
            if changed {
                *value = Value::Text(current);
            }
            changed
        }

        Widget::Combo { options } => {
            let current = value
                .as_str()
                .map(str::to_owned)
                .or_else(|| options.first().cloned())
                .unwrap_or_default();
            let mut picked = current.clone();
            cell(ui, rect, salt, |ui| {
                egui::ComboBox::from_id_salt(("nodez-combo", salt))
                    .width(rect.width())
                    .selected_text(current.clone())
                    .show_ui(ui, |ui| {
                        for option in options {
                            ui.selectable_value(&mut picked, option.clone(), option);
                        }
                    });
            });
            if picked != current {
                *value = Value::Choice(picked);
                true
            } else {
                false
            }
        }

        Widget::Color { alpha } => {
            let [r, g, b, a] = value.as_color().unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let mut color = Color32::from_rgba_unmultiplied(
                to_u8(r),
                to_u8(g),
                to_u8(b),
                to_u8(a),
            );
            let alpha_mode = if *alpha {
                egui::color_picker::Alpha::OnlyBlend
            } else {
                egui::color_picker::Alpha::Opaque
            };
            let changed = cell(ui, rect, salt, |ui| {
                let size = egui::vec2(rect.width(), rect.height());
                ui.spacing_mut().interact_size = size;
                egui::color_picker::color_edit_button_srgba(ui, &mut color, alpha_mode).changed()
            });
            if changed {
                *value = Value::Color([
                    to_f32(color.r()),
                    to_f32(color.g()),
                    to_f32(color.b()),
                    to_f32(color.a()),
                ]);
            }
            changed
        }

        Widget::Vec2 { speed } => {
            let mut current = value.as_vec2().unwrap_or([0.0; 2]);
            let changed = vector_rows(ui, rect, salt, &mut current, &["X", "Y"], *speed, style, zoom);
            if changed {
                *value = Value::Vec2(current);
            }
            changed
        }

        Widget::Vec3 { speed } => {
            let mut current = value.as_vec3().unwrap_or([0.0; 3]);
            let changed =
                vector_rows(ui, rect, salt, &mut current, &["X", "Y", "Z"], *speed, style, zoom);
            if changed {
                *value = Value::Vec3(current);
            }
            changed
        }
    }
}

#[allow(clippy::too_many_arguments)] // a layout helper; each argument is a distinct input
fn vector_rows(
    ui: &mut Ui,
    rect: Rect,
    salt: impl std::hash::Hash + Copy + std::fmt::Debug,
    values: &mut [f32],
    labels: &[&str],
    speed: f64,
    style: &EditorStyle,
    zoom: f32,
) -> bool {
    let count = values.len().min(labels.len());
    if count == 0 {
        return false;
    }
    let row_height = style.row_height * zoom;
    let spacing = style.row_spacing * zoom;
    let mut changed = false;
    for i in 0..count {
        let top = rect.top() + (row_height + spacing) * i as f32;
        let row = Rect::from_min_max(
            egui::pos2(rect.left(), top),
            egui::pos2(rect.right(), top + row_height),
        );
        let mut component = f64::from(values[i]);
        let row_changed = cell(ui, row, (salt, i), |ui| {
            ui.add(
                egui::DragValue::new(&mut component)
                    .speed(speed)
                    .prefix(format!("{}  ", labels[i]))
                    .max_decimals(4),
            )
            .changed()
        });
        if row_changed {
            values[i] = component as f32;
            changed = true;
        }
    }
    changed
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn to_f32(v: u8) -> f32 {
    f32::from(v) / 255.0
}
