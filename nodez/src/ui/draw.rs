//! Painting: the grid, node chrome, sockets and noodles.

use egui::epaint::{CubicBezierShape, PathStroke};
use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    pos2, vec2};

use crate::types::SocketShape;

use super::geometry::{Viewport, wire_control_points};
use super::style::EditorStyle;

/// How a socket is being drawn right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum SocketState {
    #[default]
    Normal,
    /// The pointer is over it.
    Hovered,
    /// It would accept the wire currently being dragged.
    Candidate,
    /// It would refuse the wire currently being dragged.
    Rejected,
}

pub(crate) fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

pub(crate) fn dim(color: Color32, factor: f32) -> Color32 {
    lerp_color(color, Color32::from_rgb(0x20, 0x20, 0x20), 1.0 - factor)
}

/// Blender's endless dotted canvas: fine lines, with every Nth line brighter.
pub(crate) fn paint_grid(painter: &Painter, viewport: &Viewport, style: &EditorStyle) {
    painter.rect_filled(viewport.screen, CornerRadius::ZERO, style.background);
    if !style.show_grid {
        return;
    }

    let spacing = viewport.scale(style.grid_spacing);
    let subdivisions = style.grid_subdivisions.max(1) as f32;
    // Drop a level of detail rather than drawing a solid wall of lines.
    let (spacing, step) = if spacing < 4.0 {
        (spacing * subdivisions, style.grid_subdivisions * style.grid_subdivisions)
    } else {
        (spacing, style.grid_subdivisions)
    };
    if spacing < 3.0 {
        return;
    }

    let visible = viewport.visible_graph_rect();
    let step = step.max(1) as i64;
    let grid = style.grid_spacing * (spacing / viewport.scale(style.grid_spacing));

    let first_x = (visible.left() / grid).floor() as i64;
    let last_x = (visible.right() / grid).ceil() as i64;
    for i in first_x..=last_x {
        let x = viewport.to_screen(pos2(i as f32 * grid, 0.0)).x;
        let major = i.rem_euclid(step) == 0;
        painter.line_segment(
            [pos2(x, viewport.screen.top()), pos2(x, viewport.screen.bottom())],
            Stroke::new(1.0, if major { style.grid_major } else { style.grid_minor }),
        );
    }

    let first_y = (visible.top() / grid).floor() as i64;
    let last_y = (visible.bottom() / grid).ceil() as i64;
    for i in first_y..=last_y {
        let y = viewport.to_screen(pos2(0.0, i as f32 * grid)).y;
        let major = i.rem_euclid(step) == 0;
        painter.line_segment(
            [pos2(viewport.screen.left(), y), pos2(viewport.screen.right(), y)],
            Stroke::new(1.0, if major { style.grid_major } else { style.grid_minor }),
        );
    }
}

/// Paint one noodle, fading from the source socket's colour to the target's.
#[allow(clippy::too_many_arguments)] // a painting helper; each argument is a distinct visual input
pub(crate) fn paint_wire(
    painter: &Painter,
    from: Pos2,
    to: Pos2,
    from_color: Color32,
    to_color: Color32,
    style: &EditorStyle,
    zoom: f32,
    highlighted: bool,
) {
    let points = wire_control_points(from, to, style, zoom);
    let width = (style.wire_width * zoom).max(1.0);

    // A dark backing line reads as an outline against both nodes and canvas.
    let outline_width = width + style.wire_outline_extra_width * zoom.max(0.5);
    painter.add(CubicBezierShape::from_points_stroke(
        points,
        false,
        Color32::TRANSPARENT,
        PathStroke::new(outline_width, style.wire_outline),
    ));

    let (a, b) = if highlighted {
        (
            lerp_color(from_color, style.wire_highlight, 0.65),
            lerp_color(to_color, style.wire_highlight, 0.65),
        )
    } else {
        (from_color, to_color)
    };

    let direction = to - from;
    let length_sq = direction.length_sq().max(1.0);
    let stroke = if a == b {
        PathStroke::new(width, a)
    } else {
        PathStroke::new_uv(width, move |_bbox, pos| {
            let t = ((pos - from).dot(direction) / length_sq).clamp(0.0, 1.0);
            lerp_color(a, b, t)
        })
    };
    painter.add(CubicBezierShape::from_points_stroke(
        points,
        false,
        Color32::TRANSPARENT,
        stroke,
    ));
}

/// Paint a socket in the shape its data type asked for.
pub(crate) fn paint_socket(
    painter: &Painter,
    center: Pos2,
    color: Color32,
    shape: SocketShape,
    style: &EditorStyle,
    zoom: f32,
    state: SocketState,
) {
    let radius = (style.socket_radius * zoom).max(2.0);
    let (fill, outline_color, outline_width) = match state {
        SocketState::Normal => (
            color,
            style.socket_outline,
            style.socket_outline_width * zoom,
        ),
        SocketState::Hovered => (
            lerp_color(color, Color32::WHITE, 0.35),
            style.socket_outline,
            style.socket_outline_width * zoom,
        ),
        SocketState::Candidate => (
            lerp_color(color, Color32::WHITE, 0.2),
            style.socket_candidate_outline,
            (style.socket_outline_width * 1.8 * zoom).max(1.5),
        ),
        SocketState::Rejected => (
            dim(color, style.socket_rejected_dim),
            style.socket_outline,
            style.socket_outline_width * zoom,
        ),
    };
    let outline = Stroke::new(outline_width.max(0.75), outline_color);
    let radius = if state == SocketState::Candidate {
        radius * 1.25
    } else {
        radius
    };

    match shape {
        SocketShape::Circle => {
            painter.circle(center, radius, fill, outline);
        }
        SocketShape::Square => {
            painter.rect(
                Rect::from_center_size(center, vec2(radius * 1.9, radius * 1.9)),
                CornerRadius::same(1),
                fill,
                outline,
                StrokeKind::Middle,
            );
        }
        SocketShape::Diamond | SocketShape::DiamondDot => {
            let r = radius * 1.25;
            let points = vec![
                pos2(center.x, center.y - r),
                pos2(center.x + r, center.y),
                pos2(center.x, center.y + r),
                pos2(center.x - r, center.y),
            ];
            painter.add(Shape::convex_polygon(points, fill, outline));
            if shape == SocketShape::DiamondDot {
                painter.circle_filled(center, radius * 0.4, style.socket_outline);
            }
        }
    }
}

/// A multi-input socket: one pill spanning its attachment points, so a column
/// of links reads as a single socket.
///
/// The pill is solid where links land and has a hole punched at the free point
/// on the end, which says there is room for one more without needing a separate
/// marker beside it.
#[allow(clippy::too_many_arguments)] // a painting helper; each argument is a distinct visual input
pub(crate) fn paint_multi_socket(
    painter: &Painter,
    x: f32,
    top: f32,
    bottom: f32,
    free: Option<Pos2>,
    color: Color32,
    style: &EditorStyle,
    zoom: f32,
) {
    let radius = (style.socket_radius * zoom).max(2.0);
    let rect = Rect::from_min_max(pos2(x - radius, top - radius), pos2(x + radius, bottom + radius));
    let corner = radius.round().clamp(0.0, 255.0) as u8;
    painter.rect(
        rect,
        CornerRadius::same(corner),
        color,
        Stroke::new(
            (style.socket_outline_width * zoom).max(0.75),
            style.socket_outline,
        ),
        StrokeKind::Middle,
    );
    if let Some(center) = free {
        painter.circle(
            center,
            radius * 0.55,
            style.node_fill,
            Stroke::new((style.socket_outline_width * zoom * 0.8).max(0.5), style.socket_outline),
        );
    }
}

/// Mark one attachment point of a multi-input while a wire is over it.
pub(crate) fn paint_slot_highlight(
    painter: &Painter,
    center: Pos2,
    style: &EditorStyle,
    zoom: f32,
    state: SocketState,
) {
    let radius = (style.socket_radius * zoom).max(2.0);
    let ring = match state {
        SocketState::Candidate => Some((
            radius * 1.35,
            Stroke::new((1.8 * zoom).max(1.5), style.socket_candidate_outline),
        )),
        SocketState::Hovered => Some((radius * 1.2, Stroke::new(zoom.max(1.0), Color32::WHITE))),
        _ => None,
    };
    if let Some((radius, stroke)) = ring {
        painter.circle_stroke(center, radius, stroke);
    }
}

/// State flags that change how a node's chrome is drawn.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NodeChromeState {
    pub selected: bool,
    pub active: bool,
    pub muted: bool,
    pub hovered: bool,
}

/// Paint a node's shadow, body, header and outline.
pub(crate) fn paint_node_chrome(
    painter: &Painter,
    rect: Rect,
    header: Rect,
    header_color: Color32,
    style: &EditorStyle,
    zoom: f32,
    state: NodeChromeState,
) {
    let radius_px = (style.node_corner_radius * zoom).round().clamp(0.0, 255.0) as u8;
    let radius = CornerRadius::same(radius_px);

    painter.rect_filled(
        rect.translate(style.node_shadow_offset * zoom),
        radius,
        style.node_shadow,
    );
    painter.rect_filled(rect, radius, style.node_fill);

    // The header shares the node's top corners and is square along the bottom.
    let header_radius = if header.height() >= rect.height() - 0.5 {
        radius
    } else {
        CornerRadius {
            nw: radius_px,
            ne: radius_px,
            sw: 0,
            se: 0,
        }
    };
    let header_fill = if state.hovered {
        lerp_color(header_color, Color32::WHITE, 0.06)
    } else {
        header_color
    };
    painter.rect_filled(header, header_radius, header_fill);

    if state.muted {
        painter.rect_filled(rect, radius, style.muted_tint);
    }

    let (outline_color, outline_width) = if state.active {
        (style.node_active_outline, style.node_selected_outline_width)
    } else if state.selected {
        (
            style.node_selected_outline,
            style.node_selected_outline_width,
        )
    } else {
        (style.node_outline, style.node_outline_width)
    };
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new((outline_width * zoom).max(1.0), outline_color),
        StrokeKind::Middle,
    );
}

/// The little triangle at the left of the header that collapses the node.
pub(crate) fn paint_collapse_arrow(painter: &Painter, rect: Rect, collapsed: bool, color: Color32) {
    let c = rect.center();
    let r = rect.height() * 0.28;
    let points = if collapsed {
        vec![
            pos2(c.x - r * 0.7, c.y - r),
            pos2(c.x + r * 0.8, c.y),
            pos2(c.x - r * 0.7, c.y + r),
        ]
    } else {
        vec![
            pos2(c.x - r, c.y - r * 0.7),
            pos2(c.x + r, c.y - r * 0.7),
            pos2(c.x, c.y + r * 0.8),
        ]
    };
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Draw text clipped to `rect`, anchored at `anchor`.
pub(crate) fn paint_clipped_text(
    painter: &Painter,
    rect: Rect,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    color: Color32,
) {
    if rect.width() <= 1.0 || font.size < 3.0 {
        return;
    }
    painter
        .with_clip_rect(rect.intersect(painter.clip_rect()))
        .text(pos, anchor, text, font, color);
}
