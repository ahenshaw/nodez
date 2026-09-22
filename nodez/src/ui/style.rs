//! Colors and metrics for the editor, defaulting to Blender's dark theme.

use egui::{Color32, Stroke, Vec2};

/// Everything the editor needs to draw itself.
///
/// All lengths are in unzoomed points; the editor multiplies them by the
/// current zoom.
#[derive(Clone, Debug)]
pub struct EditorStyle {
    // ------------------------------------------------------------ canvas
    pub background: Color32,
    /// Fine grid lines.
    pub grid_minor: Color32,
    /// Every `grid_subdivisions`-th line.
    pub grid_major: Color32,
    /// Spacing of the fine grid in graph units.
    pub grid_spacing: f32,
    pub grid_subdivisions: u32,
    pub show_grid: bool,

    // ------------------------------------------------------------- nodes
    pub node_fill: Color32,
    pub node_outline: Color32,
    pub node_outline_width: f32,
    /// Outline of nodes in the selection.
    pub node_selected_outline: Color32,
    /// Outline of the active node, the last one clicked.
    pub node_active_outline: Color32,
    /// Mark on a node with an input that has to be wired and is not: its
    /// outline, a halo behind the socket, and the row's label.
    ///
    /// Three marks from one color because they answer different questions.
    /// Zoomed out, node bodies are not drawn at all and the outline is all
    /// there is; up close, the outline says which node and the row says which
    /// input. The socket's fill is left alone — that color means its data
    /// type, and it is the only thing that does.
    pub missing_input: Color32,
    /// Whether to mark them at all.
    pub show_missing_inputs: bool,
    pub node_selected_outline_width: f32,
    pub node_corner_radius: f32,
    pub node_shadow: Color32,
    pub node_shadow_offset: Vec2,
    /// Header fill when the template and its category name no color.
    pub header_fill: Color32,
    pub header_height: f32,
    pub header_text: Color32,
    pub header_font_size: f32,
    /// Overlay tint on a muted node.
    pub muted_tint: Color32,
    pub min_node_width: f32,
    pub max_node_width: f32,

    // ------------------------------------------------------------- body
    pub body_text: Color32,
    pub body_font_size: f32,
    pub row_height: f32,
    pub row_spacing: f32,
    pub body_margin: Vec2,

    // ----------------------------------------------------------- sockets
    pub socket_radius: f32,
    pub socket_outline: Color32,
    pub socket_outline_width: f32,
    /// Extra pick radius around a socket, in screen points.
    pub socket_grab_padding: f32,
    /// Vertical spacing between the attachment points of a multi-input socket.
    pub multi_slot_height: f32,
    /// How far the rail behind a multi-input socket's slots is tinted toward
    /// the socket color. 0 is the node fill, 1 is the socket color itself.
    pub multi_slot_track: f32,
    /// Outline drawn around a socket that would accept the wire being dragged.
    pub socket_candidate_outline: Color32,
    /// Tint applied to sockets that would refuse the wire being dragged.
    pub socket_rejected_dim: f32,

    // ----------------------------------------------------------- noodles
    pub wire_width: f32,
    /// Dark backing line drawn under each wire, as Blender does.
    pub wire_outline: Color32,
    pub wire_outline_extra_width: f32,
    /// How much of its width a wire gives up on the way to the input it
    /// feeds, as a fraction.
    ///
    /// Zero draws a wire of one width, which says nothing about which way
    /// anything flows and is what this used to do. One narrows it to a point
    /// where it arrives. In between, the wire is plainly wider where it
    /// leaves an output than where it meets an input, and a graph can be read
    /// without following any wire to its end.
    pub wire_taper: f32,
    /// Horizontal pull of the bezier control points, as a fraction of the
    /// horizontal distance between the two sockets.
    pub wire_curvature: f32,
    pub wire_min_curve: f32,
    pub wire_max_curve: f32,
    /// Wire color while a drag is in flight.
    pub wire_dragging: Color32,
    /// Wire color when a drag would be refused.
    pub wire_invalid: Color32,
    /// Highlight for wires touching a selected node.
    pub wire_highlight: Color32,

    // -------------------------------------------------------- selection
    pub box_select_fill: Color32,
    pub box_select_stroke: Stroke,

    // ------------------------------------------------------------- zoom
    pub min_zoom: f32,
    pub max_zoom: f32,
    /// Zoom multiplier per unit of scroll; one wheel notch is about 50 units.
    pub zoom_speed: f32,
    /// Below this zoom the node bodies are not drawn, only their headers.
    pub detail_cutoff: f32,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self::blender_dark()
    }
}

impl EditorStyle {
    /// The default look: Blender 4.x's dark node editor.
    pub fn blender_dark() -> Self {
        Self {
            background: Color32::from_rgb(0x1D, 0x1D, 0x1D),
            grid_minor: Color32::from_rgb(0x25, 0x25, 0x25),
            grid_major: Color32::from_rgb(0x2E, 0x2E, 0x2E),
            grid_spacing: 20.0,
            grid_subdivisions: 5,
            show_grid: true,

            node_fill: Color32::from_rgba_premultiplied(0x25, 0x25, 0x25, 0xF7),
            node_outline: Color32::from_rgb(0x0D, 0x0D, 0x0D),
            node_outline_width: 1.0,
            node_selected_outline: Color32::from_rgb(0xED, 0x72, 0x1E),
            node_active_outline: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            missing_input: Color32::from_rgb(0xE0, 0x6C, 0x3C),
            show_missing_inputs: true,
            node_selected_outline_width: 1.5,
            node_corner_radius: 5.0,
            node_shadow: Color32::from_black_alpha(0x50),
            node_shadow_offset: Vec2::new(2.0, 3.0),
            header_fill: Color32::from_rgb(0x4B, 0x4B, 0x4B),
            header_height: 22.0,
            header_text: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            header_font_size: 12.0,
            muted_tint: Color32::from_rgba_unmultiplied(0xC0, 0x30, 0x30, 0x40),
            min_node_width: 80.0,
            max_node_width: 480.0,

            body_text: Color32::from_rgb(0xD0, 0xD0, 0xD0),
            body_font_size: 11.0,
            row_height: 19.0,
            row_spacing: 3.0,
            body_margin: Vec2::new(8.0, 5.0),

            socket_radius: 4.5,
            socket_outline: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            socket_outline_width: 1.0,
            socket_grab_padding: 6.0,
            multi_slot_height: 13.0,
            multi_slot_track: 0.78,
            socket_candidate_outline: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            socket_rejected_dim: 0.25,

            wire_width: 2.0,
            wire_outline: Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 0xC0),
            wire_outline_extra_width: 1.6,
            wire_taper: 0.6,
            wire_curvature: 0.5,
            wire_min_curve: 30.0,
            wire_max_curve: 180.0,
            wire_dragging: Color32::from_rgb(0xE0, 0xE0, 0xE0),
            wire_invalid: Color32::from_rgb(0xD0, 0x40, 0x40),
            wire_highlight: Color32::from_rgb(0xED, 0x72, 0x1E),

            box_select_fill: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 0x18),
            box_select_stroke: Stroke::new(1.0, Color32::from_rgb(0xD0, 0xD0, 0xD0)),

            min_zoom: 0.2,
            max_zoom: 2.5,
            zoom_speed: 0.002,
            detail_cutoff: 0.3,
        }
    }

    /// A lighter variant for apps that are not dark-themed.
    pub fn light() -> Self {
        Self {
            background: Color32::from_rgb(0xDE, 0xDE, 0xDE),
            grid_minor: Color32::from_rgb(0xD4, 0xD4, 0xD4),
            grid_major: Color32::from_rgb(0xC6, 0xC6, 0xC6),
            node_fill: Color32::from_rgba_premultiplied(0xEC, 0xEC, 0xEC, 0xF4),
            node_outline: Color32::from_rgb(0x8A, 0x8A, 0x8A),
            header_fill: Color32::from_rgb(0xBB, 0xBB, 0xBB),
            header_text: Color32::from_rgb(0x10, 0x10, 0x10),
            body_text: Color32::from_rgb(0x1A, 0x1A, 0x1A),
            node_active_outline: Color32::from_rgb(0x20, 0x20, 0x20),
            missing_input: Color32::from_rgb(0xC0, 0x44, 0x18),
            wire_dragging: Color32::from_rgb(0x30, 0x30, 0x30),
            box_select_fill: Color32::from_rgba_unmultiplied(0x00, 0x00, 0x00, 0x18),
            box_select_stroke: Stroke::new(1.0, Color32::from_rgb(0x30, 0x30, 0x30)),
            ..Self::blender_dark()
        }
    }
}

/// Scale an egui style so widgets drawn inside a node match the editor zoom.
pub(crate) fn scaled_style(base: &egui::Style, zoom: f32) -> egui::Style {
    let mut style = base.clone();
    for font in style.text_styles.values_mut() {
        font.size = (font.size * zoom).max(1.0);
    }
    let spacing = &mut style.spacing;
    spacing.item_spacing *= zoom;
    spacing.button_padding *= zoom;
    spacing.interact_size *= zoom;
    spacing.indent *= zoom;
    spacing.slider_width *= zoom;
    spacing.slider_rail_height *= zoom;
    spacing.combo_width *= zoom;
    spacing.text_edit_width *= zoom;
    spacing.icon_width *= zoom;
    spacing.icon_width_inner *= zoom;
    spacing.icon_spacing *= zoom;

    for visuals in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visuals.corner_radius = egui::CornerRadius::same(
            (f32::from(visuals.corner_radius.nw) * zoom).round().clamp(0.0, 255.0) as u8,
        );
        visuals.bg_stroke.width *= zoom;
        visuals.fg_stroke.width *= zoom;
        visuals.expansion *= zoom;
    }
    style
}

/// The widget palette used inside node bodies: flat, dark, Blender-ish.
pub(crate) fn node_widget_visuals(style: &EditorStyle, base: &egui::Style) -> egui::Style {
    let mut out = base.clone();
    let dark = style.background.r() < 128;
    let (fill, hover, active, text) = if dark {
        (
            Color32::from_rgb(0x36, 0x36, 0x36),
            Color32::from_rgb(0x42, 0x42, 0x42),
            Color32::from_rgb(0x4C, 0x4C, 0x4C),
            style.body_text,
        )
    } else {
        (
            Color32::from_rgb(0xD8, 0xD8, 0xD8),
            Color32::from_rgb(0xCA, 0xCA, 0xCA),
            Color32::from_rgb(0xBC, 0xBC, 0xBC),
            style.body_text,
        )
    };

    out.visuals.override_text_color = Some(text);
    out.visuals.extreme_bg_color = fill;
    out.visuals.selection.bg_fill = Color32::from_rgb(0x47, 0x72, 0xB3);

    let radius = egui::CornerRadius::same(3);
    for (visuals, bg) in [
        (&mut out.visuals.widgets.noninteractive, fill),
        (&mut out.visuals.widgets.inactive, fill),
        (&mut out.visuals.widgets.hovered, hover),
        (&mut out.visuals.widgets.active, active),
        (&mut out.visuals.widgets.open, hover),
    ] {
        visuals.bg_fill = bg;
        visuals.weak_bg_fill = bg;
        visuals.corner_radius = radius;
        visuals.bg_stroke = Stroke::NONE;
        visuals.fg_stroke = Stroke::new(1.0, text);
        visuals.expansion = 0.0;
    }
    out.spacing.button_padding = Vec2::new(4.0, 1.0);
    out.spacing.item_spacing = Vec2::new(4.0, 2.0);
    out.spacing.interact_size = Vec2::new(0.0, style.row_height);
    out.spacing.icon_width = style.row_height * 0.8;
    out.spacing.icon_width_inner = style.row_height * 0.5;
    out
}
