//! Painting: the grid, node chrome, sockets and noodles.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    Vec2, pos2, vec2};

use crate::types::SocketShape;

use super::geometry::{Viewport, bezier_point, wire_path};
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

/// Paint one noodle: fading from the source socket's color to the target's,
/// and narrowing as it goes.
#[allow(clippy::too_many_arguments)] // a painting helper; each argument is a distinct visual input
pub(crate) fn paint_wire(
    painter: &Painter,
    from: Pos2,
    to: Pos2,
    waypoints: &[Pos2],
    from_color: Color32,
    to_color: Color32,
    style: &EditorStyle,
    zoom: f32,
    highlighted: bool,
) {
    let points = flatten(&wire_path(from, to, waypoints, style, zoom));
    if points.len() < 2 {
        return;
    }
    let width = (style.wire_width * zoom).max(1.0);

    let (a, b) = if highlighted {
        (
            lerp_color(from_color, style.wire_highlight, 0.65),
            lerp_color(to_color, style.wire_highlight, 0.65),
        )
    } else {
        (from_color, to_color)
    };

    // The gradient is placed along the straight run from socket to socket, so
    // it stays put as a wire is rerouted.
    let direction = to - from;
    let length_sq = direction.length_sq().max(1.0);
    let shade = move |p: Pos2| {
        if a == b {
            a
        } else {
            let t = ((p - from).dot(direction) / length_sq).clamp(0.0, 1.0);
            lerp_color(a, b, t)
        }
    };

    // Widest where it leaves the output, narrowest where it arrives, which is
    // the whole point: which way a wire flows can be seen without following
    // it to either end.
    //
    // The swing is either side of the width asked for rather than all of it
    // below: a wire is two pixels across and taking a fraction off that is a
    // difference nobody can see. Fattening the near end as much as the far
    // end is thinned buys twice the contrast and leaves the graph weighing
    // the same, since the width asked for is still the average.
    let taper = style.wire_taper.clamp(0.0, 1.0);
    let narrowing = move |full: f32| move |along: f32| full * (1.0 + taper - 2.0 * taper * along);

    // A dark backing ribbon reads as an outline against both nodes and
    // canvas. One ribbon for the whole wire rather than one per curve, so a
    // bend cannot lay its outline over the stretch before it.
    let outline = width + style.wire_outline_extra_width * zoom.max(0.5);
    paint_ribbon(painter, &points, narrowing(outline), |_| style.wire_outline);
    paint_ribbon(painter, &points, narrowing(width), shade);
}

/// A wire's curves as one run of points, close enough together to read as a
/// curve.
///
/// Steps to suit each curve's size: the fillet at a corner needs a handful
/// where a sweep across the canvas needs plenty.
fn flatten(path: &[[Pos2; 4]]) -> Vec<Pos2> {
    let mut points: Vec<Pos2> = Vec::new();
    for curve in path {
        let rough = curve[0].distance(curve[1]) + curve[1].distance(curve[2])
            + curve[2].distance(curve[3]);
        let steps = ((rough / 5.0).ceil() as usize).clamp(2, 32);
        for step in 0..=steps {
            let p = bezier_point(curve, step as f32 / steps as f32);
            if points.last().is_none_or(|last| last.distance(p) > 0.05) {
                points.push(p);
            }
        }
    }
    points
}

/// Paint a run of points as a ribbon whose width may change along it.
///
/// A stroke in egui has one width for its whole length, so a wire that
/// narrows cannot be one. This lays down three quads per step instead: a core
/// at full color, and a band either side fading to nothing, which is the same
/// way egui's own tessellator keeps an edge from looking like stairs.
///
/// `width` is given how far along the wire a point is, from 0 at the output
/// to 1 at the input, and answers with how wide to draw it there.
fn paint_ribbon(
    painter: &Painter,
    points: &[Pos2],
    width: impl Fn(f32) -> f32,
    color: impl Fn(Pos2) -> Color32,
) {
    /// How wide the soft edge is. One pixel, the same as egui's own.
    const FEATHER: f32 = 1.0;

    // How far along each point is by distance rather than by index, so a
    // crowd of short steps around a corner does not spend more of the taper
    // than one long straight run.
    let mut along = Vec::with_capacity(points.len());
    let mut run = 0.0;
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            run += points[i - 1].distance(*p);
        }
        along.push(run);
    }
    let total = run.max(0.001);

    let mut mesh = egui::Mesh::default();
    for (i, p) in points.iter().enumerate() {
        // Square to the way the wire is going, averaged across a corner so
        // the two sides of it meet.
        let back = if i > 0 { *p - points[i - 1] } else { Vec2::ZERO };
        let on = if i + 1 < points.len() {
            points[i + 1] - *p
        } else {
            Vec2::ZERO
        };
        let heading = back + on;
        let heading = if heading.length_sq() > f32::EPSILON {
            heading.normalized()
        } else {
            vec2(1.0, 0.0)
        };
        let out = vec2(-heading.y, heading.x);

        // Thinner than a pixel is drawn as a pixel that is barely there,
        // rather than as geometry too small to land on one. Without this the
        // thin end of a wire stops getting thinner and starts getting
        // blurrier, which is the opposite of the point.
        let w = width(along[i] / total).max(0.0);
        let (h, fade) = if w < 1.0 { (0.5, w) } else { (w * 0.5, 1.0) };
        let shade = color(*p).gamma_multiply(fade);
        let base = mesh.vertices.len() as u32;
        // Premultiplied, so a transparent edge is transparent black and
        // cannot leave a halo of the wire's own color.
        mesh.colored_vertex(*p - out * (h + FEATHER), Color32::TRANSPARENT);
        mesh.colored_vertex(*p - out * h, shade);
        mesh.colored_vertex(*p + out * h, shade);
        mesh.colored_vertex(*p + out * (h + FEATHER), Color32::TRANSPARENT);

        if i > 0 {
            let previous = base - 4;
            for band in 0..3 {
                mesh.add_triangle(previous + band, previous + band + 1, base + band);
                mesh.add_triangle(previous + band + 1, base + band + 1, base + band);
            }
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// What a socket looks like, apart from how it is being interacted with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SocketLook {
    /// The data type's color and shape. The fill is never anything else: it
    /// is the only thing that says what travels down the wire.
    pub color: Color32,
    pub shape: SocketShape,
    /// An input that has to be wired and is not, marked by a halo behind it.
    pub missing: bool,
}

/// Paint a socket in the shape its data type asked for.
pub(crate) fn paint_socket(
    painter: &Painter,
    center: Pos2,
    look: SocketLook,
    style: &EditorStyle,
    zoom: f32,
    state: SocketState,
) {
    let SocketLook {
        color,
        shape,
        missing,
    } = look;
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

    // A halo behind the socket rather than a colored ring on it. Recoloring
    // the outline works until the data type's own color is near the mark's --
    // an image socket is already orange -- and then the mark disappears into
    // the one socket it is about. Behind it, with the socket's own dark ring
    // still drawn on top, there is always a line between the two colors.
    if missing && state == SocketState::Normal {
        paint_socket_shape(
            painter,
            center,
            radius + (2.0 * zoom).max(1.5),
            shape,
            style.missing_input,
            Stroke::NONE,
            style,
        );
    }
    paint_socket_shape(painter, center, radius, shape, fill, outline, style);
}

/// One socket, in the shape its data type asked for.
fn paint_socket_shape(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    shape: SocketShape,
    fill: Color32,
    outline: Stroke,
    style: &EditorStyle,
) {
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

/// The rail behind a multi-input socket's attachment points, so a column of
/// slots reads as one socket rather than several.
pub(crate) fn paint_multi_track(
    painter: &Painter,
    x: f32,
    top: f32,
    bottom: f32,
    color: Color32,
    style: &EditorStyle,
    zoom: f32,
) {
    let half = (style.socket_radius * zoom).max(2.0) * 0.55;
    let rect = Rect::from_min_max(pos2(x - half, top), pos2(x + half, bottom));
    let radius = (half * 2.0).round().clamp(0.0, 255.0) as u8;
    painter.rect_filled(
        rect,
        CornerRadius::same(radius),
        lerp_color(style.node_fill, color, style.multi_slot_track),
    );
}

/// The empty attachment point at the end of a multi-input: a hollow ring,
/// saying "drop here to add one" without looking like a live connection.
pub(crate) fn paint_free_slot(
    painter: &Painter,
    center: Pos2,
    color: Color32,
    style: &EditorStyle,
    zoom: f32,
    state: SocketState,
) {
    let radius = (style.socket_radius * zoom).max(2.0) * 0.72;
    let (radius, stroke) = match state {
        SocketState::Candidate => (
            radius * 1.4,
            Stroke::new((1.6 * zoom).max(1.5), style.socket_candidate_outline),
        ),
        SocketState::Rejected => (radius, Stroke::new(zoom.max(1.0), dim(color, 0.3))),
        _ => (radius, Stroke::new(zoom.max(1.0), color)),
    };
    painter.circle(center, radius, style.background, stroke);
}

/// State flags that change how a node's chrome is drawn.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NodeChromeState {
    pub selected: bool,
    pub active: bool,
    pub muted: bool,
    pub hovered: bool,
    /// The node has an input that has to be wired and is not.
    pub missing: bool,
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

    // Outside the node's own outline rather than in place of it, so being
    // selected never hides being unfinished — and so this is still legible
    // when the body is too small to draw and the outline is the whole node.
    if state.missing {
        let width = (style.node_selected_outline_width * zoom).max(1.0);
        painter.rect_stroke(
            rect.expand(width),
            radius,
            Stroke::new(width, style.missing_input),
            StrokeKind::Middle,
        );
    }
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
