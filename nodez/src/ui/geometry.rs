//! Node layout: where every row, socket and widget sits.
//!
//! Layout is computed analytically rather than by egui's layout pass, so wires
//! can be drawn behind nodes in the same frame the nodes are laid out, and so a
//! graph can be measured without an `egui::Ui` at hand.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::graph::{Graph, Node, NodeData, NodeId, SocketKind};
use crate::template::{NodeLibrary, NodeTemplate, Widget};
use crate::types::DataTypeId;

use super::style::EditorStyle;

/// Maps between graph space and screen space.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    /// The editor's rectangle on screen.
    pub screen: Rect,
    /// Graph-space offset of the view.
    pub pan: Vec2,
    pub zoom: f32,
}

impl Viewport {
    pub fn to_screen(&self, p: Pos2) -> Pos2 {
        self.screen.min + (p.to_vec2() + self.pan) * self.zoom
    }

    pub fn to_graph(&self, p: Pos2) -> Pos2 {
        (((p - self.screen.min) / self.zoom) - self.pan).to_pos2()
    }

    /// Scale a length from graph space to screen space.
    pub fn scale(&self, len: f32) -> f32 {
        len * self.zoom
    }

    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }

    pub fn rect_to_graph(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_graph(r.min), self.to_graph(r.max))
    }

    /// The graph-space region currently visible.
    pub fn visible_graph_rect(&self) -> Rect {
        self.rect_to_graph(self.screen)
    }
}

/// What a body row holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// Index into the template's `outputs`.
    Output(usize),
    /// Index into the template's `params`.
    Param(usize),
    /// Index into the template's `inputs`.
    Input(usize),
}

/// One laid-out row of a node body.
#[derive(Clone, Debug)]
pub struct RowGeometry {
    pub kind: RowKind,
    /// Screen-space rect, already inset by the body margin.
    pub rect: Rect,
    /// Whether this row's socket has a wire attached.
    pub linked: bool,
}

/// One laid-out socket, or one attachment point of a multi-input socket.
#[derive(Clone, Debug)]
pub struct SocketGeometry {
    pub kind: SocketKind,
    /// Index into the template's `inputs` or `outputs`.
    pub index: usize,
    pub name: String,
    pub ty: DataTypeId,
    /// Screen-space center.
    pub center: Pos2,
    pub linked: bool,
    /// Which attachment point this is, for a multi-input socket.
    ///
    /// A multi-input shows one slot per link plus a free one at the bottom, so
    /// where a wire is dropped decides where it lands in the order.
    pub slot: Option<u32>,
    /// True for the empty slot at the end of a multi-input.
    pub is_free_slot: bool,
}

/// A fully laid-out node, in screen space.
#[derive(Clone, Debug)]
pub struct NodeGeometry {
    pub id: NodeId,
    /// The whole node, header included.
    pub rect: Rect,
    pub header: Rect,
    pub body: Rect,
    pub collapsed: bool,
    pub rows: Vec<RowGeometry>,
    pub sockets: Vec<SocketGeometry>,
}

impl NodeGeometry {
    pub fn socket(&self, kind: SocketKind, index: usize) -> Option<&SocketGeometry> {
        self.sockets
            .iter()
            .find(|s| s.kind == kind && s.index == index)
    }

    pub fn socket_named(&self, kind: SocketKind, name: &str) -> Option<&SocketGeometry> {
        self.sockets
            .iter()
            .find(|s| s.kind == kind && s.name == name)
    }

    /// The attachment point a particular link uses, for a multi-input socket.
    /// Falls back to the socket itself when it has no slots.
    pub fn socket_slot(
        &self,
        kind: SocketKind,
        name: &str,
        slot: u32,
    ) -> Option<&SocketGeometry> {
        self.sockets
            .iter()
            .find(|s| s.kind == kind && s.name == name && s.slot == Some(slot))
            .or_else(|| self.socket_named(kind, name))
    }

    pub fn row(&self, kind: RowKind) -> Option<&RowGeometry> {
        self.rows.iter().find(|r| r.kind == kind)
    }

    /// The grab strip along the node's right edge used for resizing.
    pub fn resize_handle(&self, width: f32) -> Rect {
        Rect::from_min_max(
            pos2(self.rect.right() - width, self.rect.top()),
            self.rect.max,
        )
    }
}

/// A row's kind and its height in graph units.
struct RowPlan {
    kind: RowKind,
    height: f32,
    /// Attachment points, for a multi-input socket. One otherwise.
    slots: u32,
}

fn row_plan<N: NodeData>(
    graph: &Graph<N>,
    template: &NodeTemplate,
    node: &Node<N>,
    style: &EditorStyle,
) -> Vec<RowPlan> {
    let mut rows = Vec::new();

    // Blender's order: outputs on top, then properties, then inputs.
    for (i, socket) in template.outputs.iter().enumerate() {
        if socket.hidden {
            continue;
        }
        rows.push(RowPlan {
            kind: RowKind::Output(i),
            height: style.row_height,
            slots: 1,
        });
    }

    for (i, param) in template.params.iter().enumerate() {
        let lines = param.widget.rows().max(1.0);
        rows.push(RowPlan {
            kind: RowKind::Param(i),
            height: lines * style.row_height + (lines - 1.0) * style.row_spacing,
            slots: 1,
        });
    }

    for (i, socket) in template.inputs.iter().enumerate() {
        if socket.hidden {
            continue;
        }
        // A multi-input is as tall as its links plus the free slot below them.
        if socket.multi {
            let slots = graph.links_into(node.id, &socket.name).count() as u32 + 1;
            rows.push(RowPlan {
                kind: RowKind::Input(i),
                height: (slots as f32 * style.multi_slot_height).max(style.row_height),
                slots,
            });
            continue;
        }
        let linked = graph.is_input_linked(node.id, &socket.name);
        let lines = if linked || socket.widget == Widget::None {
            1.0
        } else {
            socket.widget.rows().max(1.0)
        };
        rows.push(RowPlan {
            kind: RowKind::Input(i),
            height: lines * style.row_height + (lines - 1.0) * style.row_spacing,
            slots: 1,
        });
    }

    rows
}

/// The size a node will be drawn at, in graph units.
///
/// Pass this to [`crate::layout::layered`] so automatic layout agrees with what
/// the editor draws.
pub fn node_size<N: NodeData>(
    graph: &Graph<N>,
    library: &NodeLibrary,
    node: &Node<N>,
    style: &EditorStyle,
) -> Vec2 {
    let width = node
        .width
        .clamp(style.min_node_width, style.max_node_width);
    let Some(template) = library.get(node.template) else {
        return vec2(width, style.header_height);
    };
    if node.collapsed {
        return vec2(collapsed_width(width, style), style.header_height);
    }

    let rows = row_plan(graph, template, node, style);
    let mut height = style.header_height + style.body_margin.y * 2.0;
    for (i, row) in rows.iter().enumerate() {
        height += row.height;
        if i + 1 < rows.len() {
            height += style.row_spacing;
        }
    }
    vec2(width, height)
}

fn collapsed_width(width: f32, style: &EditorStyle) -> f32 {
    (width * 0.55).max(style.min_node_width)
}

/// Lay a node out in screen space.
/// Where one of a node's wires attaches, in graph space.
///
/// Routing needs this rather than the node's edge: a wire leaves a socket, and
/// on a tall node the difference is most of its height.
///
/// `slot` picks the attachment point on a multi-input, which is the link's
/// [`crate::Connection::order`]. Passing `None` takes the first, which is only
/// right for an output or a single-link input.
pub fn socket_anchor<N: NodeData>(
    graph: &Graph<N>,
    library: &NodeLibrary,
    node: &Node<N>,
    style: &EditorStyle,
    kind: SocketKind,
    socket: &str,
    slot: Option<u32>,
) -> Option<Pos2> {
    // An identity viewport leaves the geometry in graph space.
    let viewport = Viewport {
        screen: Rect::from_min_size(Pos2::ZERO, Vec2::ZERO),
        pan: Vec2::ZERO,
        zoom: 1.0,
    };
    let geometry = node_geometry(graph, library, node, style, &viewport);
    match slot {
        Some(slot) => geometry.socket_slot(kind, socket, slot),
        None => geometry.socket_named(kind, socket),
    }
    .map(|socket| socket.center)
}

pub(crate) fn node_geometry<N: NodeData>(
    graph: &Graph<N>,
    library: &NodeLibrary,
    node: &Node<N>,
    style: &EditorStyle,
    viewport: &Viewport,
) -> NodeGeometry {
    let size = node_size(graph, library, node, style);
    let min = viewport.to_screen(node.position);
    let rect = Rect::from_min_size(min, size * viewport.zoom);
    let header = Rect::from_min_size(
        rect.min,
        vec2(rect.width(), viewport.scale(style.header_height)),
    );
    let body = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);

    let Some(template) = library.get(node.template) else {
        return NodeGeometry {
            id: node.id,
            rect,
            header,
            body,
            collapsed: node.collapsed,
            rows: Vec::new(),
            sockets: Vec::new(),
        };
    };

    if node.collapsed {
        return collapsed_geometry(graph, template, node, rect, header, body, viewport);
    }

    let rows = row_plan(graph, template, node, style);
    let margin = style.body_margin * viewport.zoom;
    let mut y = body.top() + margin.y;
    let content_left = body.left() + margin.x;
    let content_right = body.right() - margin.x;

    let mut row_geoms = Vec::with_capacity(rows.len());
    let mut sockets = Vec::new();

    for (i, row) in rows.iter().enumerate() {
        let height = viewport.scale(row.height);
        let rect = Rect::from_min_max(pos2(content_left, y), pos2(content_right, y + height));
        // Sockets attach to the first line of a multi-line row.
        let socket_y = y + viewport.scale(style.row_height) * 0.5;

        let linked = match row.kind {
            RowKind::Output(index) => {
                let socket = &template.outputs[index];
                let linked = graph.is_output_linked(node.id, &socket.name);
                sockets.push(SocketGeometry {
                    kind: SocketKind::Output,
                    index,
                    name: socket.name.clone(),
                    ty: socket.ty,
                    center: pos2(body.right(), socket_y),
                    linked,
                    slot: None,
                    is_free_slot: false,
                });
                linked
            }
            RowKind::Input(index) => {
                let socket = &template.inputs[index];
                let linked = graph.is_input_linked(node.id, &socket.name);
                if row.slots > 1 || socket.multi {
                    let step = viewport.scale(style.multi_slot_height);
                    for slot in 0..row.slots {
                        sockets.push(SocketGeometry {
                            kind: SocketKind::Input,
                            index,
                            name: socket.name.clone(),
                            ty: socket.ty,
                            center: pos2(body.left(), y + step * (slot as f32 + 0.5)),
                            linked: slot + 1 < row.slots,
                            slot: Some(slot),
                            is_free_slot: slot + 1 == row.slots,
                        });
                    }
                } else {
                    sockets.push(SocketGeometry {
                        kind: SocketKind::Input,
                        index,
                        name: socket.name.clone(),
                        ty: socket.ty,
                        center: pos2(body.left(), socket_y),
                        linked,
                        slot: None,
                        is_free_slot: false,
                    });
                }
                linked
            }
            RowKind::Param(_) => false,
        };

        row_geoms.push(RowGeometry {
            kind: row.kind,
            rect,
            linked,
        });

        y += height;
        if i + 1 < rows.len() {
            y += viewport.scale(style.row_spacing);
        }
    }

    NodeGeometry {
        id: node.id,
        rect,
        header,
        body,
        collapsed: false,
        rows: row_geoms,
        sockets,
    }
}

/// A collapsed node keeps its sockets, fanned along the header edges.
fn collapsed_geometry<N: NodeData>(
    graph: &Graph<N>,
    template: &NodeTemplate,
    node: &Node<N>,
    rect: Rect,
    header: Rect,
    body: Rect,
    viewport: &Viewport,
) -> NodeGeometry {
    let mut sockets = Vec::new();
    let visible_inputs: Vec<_> = template
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.hidden)
        .collect();
    let visible_outputs: Vec<_> = template
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.hidden)
        .collect();

    let spread = (header.height() - viewport.scale(6.0)).max(0.0);
    let fan = |count: usize, i: usize| -> f32 {
        if count <= 1 {
            header.center().y
        } else {
            header.center().y - spread * 0.5 + spread * (i as f32) / (count as f32 - 1.0)
        }
    };

    for (slot, (index, socket)) in visible_inputs.iter().enumerate() {
        sockets.push(SocketGeometry {
            kind: SocketKind::Input,
            index: *index,
            name: socket.name.clone(),
            ty: socket.ty,
            center: pos2(rect.left(), fan(visible_inputs.len(), slot)),
            linked: graph.is_input_linked(node.id, &socket.name),
            slot: None,
            is_free_slot: false,
        });
    }
    for (slot, (index, socket)) in visible_outputs.iter().enumerate() {
        sockets.push(SocketGeometry {
            kind: SocketKind::Output,
            index: *index,
            name: socket.name.clone(),
            ty: socket.ty,
            center: pos2(rect.right(), fan(visible_outputs.len(), slot)),
            linked: graph.is_output_linked(node.id, &socket.name),
            slot: None,
            is_free_slot: false,
        });
    }

    NodeGeometry {
        id: node.id,
        rect,
        header,
        body,
        collapsed: true,
        rows: Vec::new(),
        sockets,
    }
}

/// Find a spot near `preferred` where a node of `size` will not sit on top of
/// anything already placed.
///
/// Candidates step outwards a node at a time, below first, so a run of nodes
/// added from the same place fills the space around it instead of stacking.
/// `ignore` is skipped when testing, for repositioning a node that is already
/// in the graph.
pub fn free_position<N: NodeData>(
    graph: &Graph<N>,
    library: &NodeLibrary,
    style: &EditorStyle,
    size: Vec2,
    preferred: Pos2,
    ignore: Option<NodeId>,
) -> Pos2 {
    let gap = style.row_height;
    let occupied: Vec<Rect> = graph
        .nodes()
        .filter(|node| Some(node.id) != ignore)
        .map(|node| {
            Rect::from_min_size(node.position, node_size(graph, library, node, style))
                .expand(gap * 0.5)
        })
        .collect();

    let is_free = |at: Pos2| {
        let rect = Rect::from_min_size(at, size);
        !occupied.iter().any(|other| other.intersects(rect))
    };
    if is_free(preferred) {
        return preferred;
    }

    let step = size + Vec2::splat(gap);
    for ring in 1..=24 {
        let r = ring as f32;
        // Below, then the diagonals and sides, so a column fills first.
        for (dx, dy) in [
            (0.0, r),
            (r, r),
            (r, 0.0),
            (r, -r),
            (0.0, -r),
            (-r, -r),
            (-r, 0.0),
            (-r, r),
        ] {
            let candidate = preferred + vec2(dx * step.x, dy * step.y);
            if is_free(candidate) {
                return candidate;
            }
        }
    }
    preferred
}

/// Control points for the wire between two sockets.
///
/// Wires leave outputs to the right and enter inputs from the left, so a
/// backwards link bows outwards instead of doubling back through the node.
pub(crate) fn wire_control_points(
    from: Pos2,
    to: Pos2,
    style: &EditorStyle,
    zoom: f32,
) -> [Pos2; 4] {
    let pull = segment_pull(from, to, style, zoom);
    [
        from,
        pos2(from.x + pull, from.y),
        pos2(to.x - pull, to.y),
        to,
    ]
}

/// How far a segment's control points reach out from its ends.
fn segment_pull(from: Pos2, to: Pos2, style: &EditorStyle, zoom: f32) -> f32 {
    let dx = to.x - from.x;
    let dy = (to.y - from.y).abs();
    let pull = (dx.abs() * style.wire_curvature)
        .max(viewport_scaled(style.wire_min_curve, zoom) + dy * 0.15)
        .min(viewport_scaled(style.wire_max_curve, zoom));
    // The floor above answers to the drop, not to the span, so a short wire
    // between two distant heights used to ask for more reach than it had room
    // for. Past a reach of `dx` the two handles cross over and the curve
    // bulges back the way it came: work the derivative through and it is
    // exactly `pull <= dx` that keeps the wire running one way.
    //
    // A wire that already runs backwards has no forward span to protect, and
    // needs its full reach to throw the loop that gets it there.
    if dx > 0.0 { pull.min(dx) } else { pull }
}

/// The cubic segments a wire is drawn from: one when it runs straight to its
/// socket, otherwise a rounded version of the path through its waypoints.
///
/// The corners are filleted rather than passed through: the wire starts
/// turning before each one and has finished turning after it. Bending *at* a
/// corner instead would carry the wire past it — still descending as it met
/// the socket's height — and take a reverse bend to come back.
pub(crate) fn wire_path(
    from: Pos2,
    to: Pos2,
    waypoints: &[Pos2],
    style: &EditorStyle,
    zoom: f32,
) -> Vec<[Pos2; 4]> {
    if waypoints.is_empty() {
        return vec![wire_control_points(from, to, style, zoom)];
    }

    let mut points = Vec::with_capacity(waypoints.len() + 2);
    points.push(from);
    points.extend_from_slice(waypoints);
    points.push(to);

    let spans: Vec<f32> = (0..points.len() - 1)
        .map(|i| (points[i + 1] - points[i]).length())
        .collect();
    // A routed wire tracks its waypoints closely — they were chosen to clear
    // the nodes, and a wide corner would undo that. The shorter of the two
    // spans meeting at a corner also limits it, so neighboring corners cannot
    // eat into each other.
    let cap = viewport_scaled(style.wire_min_curve, zoom);
    let radius = |i: usize| (spans[i - 1].min(spans[i]) * 0.5).min(cap);

    let unit = |v: Vec2| {
        if v.length_sq() < f32::EPSILON {
            Vec2::ZERO
        } else {
            v.normalized()
        }
    };
    let straight = |a: Pos2, b: Pos2| [a, a + (b - a) / 3.0, b - (b - a) / 3.0, b];

    let mut path = Vec::with_capacity(points.len() * 2);
    let mut cursor = points[0];
    for i in 1..points.len() - 1 {
        let corner = points[i];
        let r = radius(i);
        let enter = corner - unit(corner - points[i - 1]) * r;
        let leave = corner + unit(points[i + 1] - corner) * r;

        if (enter - cursor).length() > 0.01 {
            path.push(straight(cursor, enter));
        }
        if r > 0.01 {
            // The circle-ish arc the corner rounds off. Both handles pull
            // toward the corner, so the curve stays inside it.
            path.push([
                enter,
                enter + (corner - enter) * CORNER_HANDLE,
                leave + (corner - leave) * CORNER_HANDLE,
                leave,
            ]);
        }
        cursor = leave;
    }
    let last = points[points.len() - 1];
    if (last - cursor).length() > 0.01 || path.is_empty() {
        path.push(straight(cursor, last));
    }
    path
}

/// Handle length, as a fraction of the corner radius, that makes a cubic sit
/// closest to a quarter circle.
const CORNER_HANDLE: f32 = 0.5523;

fn viewport_scaled(len: f32, zoom: f32) -> f32 {
    len * zoom
}

/// Sample a cubic bezier, used for hit-testing wires.
pub(crate) fn bezier_point(points: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    pos2(
        a * points[0].x + b * points[1].x + c * points[2].x + d * points[3].x,
        a * points[0].y + b * points[1].y + c * points[2].y + d * points[3].y,
    )
}

/// Shortest distance from `p` to a sampled cubic bezier.
pub(crate) fn distance_to_bezier(points: &[Pos2; 4], p: Pos2, samples: usize) -> f32 {
    let mut best = f32::INFINITY;
    let mut previous = points[0];
    for i in 1..=samples {
        let current = bezier_point(points, i as f32 / samples as f32);
        best = best.min(distance_to_segment(p, previous, current));
        previous = current;
    }
    best
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::SocketKind;
    use crate::layout::{LayoutOptions, RouteOptions};
    use crate::template::{NodeLibrary, NodeTemplate, SocketSpec};

    /// Four columns, three nodes deep in the middle two, with wires running
    /// the whole width. Those are the ones that used to cut through whatever
    /// stood in the way.
    fn crowded() -> (NodeLibrary, crate::Graph) {
        let mut library = NodeLibrary::new();
        let ty = library.types.add("T", egui::Color32::GRAY);
        let source = library.register(
            NodeTemplate::new("source", "Source").output(SocketSpec::new("out", ty)),
        );
        let mid = library.register(
            NodeTemplate::new("mid", "Mid")
                .input(SocketSpec::new("a", ty).multi())
                .input(SocketSpec::new("b", ty))
                .output(SocketSpec::new("out", ty)),
        );

        let mut graph = crate::Graph::new();
        let at = pos2(0.0, 0.0);
        let sources: Vec<_> = (0..3).map(|_| graph.add_node(&library, source, at)).collect();
        let col1: Vec<_> = (0..3).map(|_| graph.add_node(&library, mid, at)).collect();
        let col2: Vec<_> = (0..3).map(|_| graph.add_node(&library, mid, at)).collect();
        let sink = graph.add_node(&library, mid, at);

        for (s, m) in sources.iter().zip(&col1) {
            graph.connect(&library, (*s, "out"), (*m, "a")).unwrap();
        }
        for (a, b) in col1.iter().zip(&col2) {
            graph.connect(&library, (*a, "out"), (*b, "a")).unwrap();
        }
        for b in &col2 {
            graph.connect(&library, (*b, "out"), (sink, "a")).unwrap();
        }
        // The long ones: straight from the first column to the last.
        for s in &sources {
            graph.connect(&library, (*s, "out"), (sink, "a")).unwrap();
        }
        (library, graph)
    }

    #[test]
    fn routed_wires_do_not_cross_nodes() {
        let (library, mut graph) = crowded();
        let style = EditorStyle::default();
        let size = |g: &crate::Graph, n: &crate::Node| node_size(g, &library, n, &style);

        crate::layout::layered(&mut graph, &LayoutOptions::default(), size).unwrap();
        crate::layout::route_links(
            &mut graph,
            &RouteOptions {
                curvature: style.wire_curvature,
                min_curve: style.wire_min_curve,
                max_curve: style.wire_max_curve,
                ..RouteOptions::default()
            },
            size,
            |g, socket, kind, slot| {
                let node = g.node(socket.node)?;
                socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .unwrap();

        let rects: Vec<(crate::NodeId, Rect)> = graph
            .nodes()
            .map(|n| (n.id, Rect::from_min_size(n.position, size(&graph, n))))
            .collect();

        let mut crossings = Vec::new();
        for conn in graph.connections() {
            let from_node = graph.node(conn.from.node).unwrap();
            let to_node = graph.node(conn.to.node).unwrap();
            let a = socket_anchor(&graph, &library, from_node, &style, SocketKind::Output,
                &conn.from.socket, None).unwrap();
            let b = socket_anchor(&graph, &library, to_node, &style, SocketKind::Input,
                &conn.to.socket, Some(conn.order)).unwrap();

            // The path exactly as the editor draws it, at zoom 1.
            for points in wire_path(a, b, &conn.waypoints, &style, 1.0) {
                for i in 0..=24 {
                    let p = bezier_point(&points, i as f32 / 24.0);
                    for (id, rect) in &rects {
                        if *id != conn.from.node && *id != conn.to.node && rect.contains(p) {
                            crossings.push((conn.id, *id, p));
                        }
                    }
                }
            }
        }
        assert!(
            crossings.is_empty(),
            "{} wire samples land inside a node: {:?}",
            crossings.len(),
            &crossings[..crossings.len().min(4)]
        );
    }

    /// A rounded corner stays inside the turn. A corner that blends its
    /// tangent instead carries the wire past the socket's height and needs a
    /// reverse bend to come back, which shows up here as a sample outside the
    /// path's own bounding box.
    #[test]
    fn a_routed_wire_never_overshoots_its_path() {
        let (library, mut graph) = crowded();
        let style = EditorStyle::default();
        let size = |g: &crate::Graph, n: &crate::Node| node_size(g, &library, n, &style);

        crate::layout::layered(&mut graph, &LayoutOptions::default(), size).unwrap();
        crate::layout::route_links(
            &mut graph,
            &RouteOptions::default(),
            size,
            |g, socket, kind, slot| {
                let node = g.node(socket.node)?;
                socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .unwrap();

        let mut checked = 0;
        for conn in graph.connections() {
            if conn.waypoints.is_empty() {
                continue;
            }
            let from_node = graph.node(conn.from.node).unwrap();
            let to_node = graph.node(conn.to.node).unwrap();
            let a = socket_anchor(&graph, &library, from_node, &style, SocketKind::Output,
                &conn.from.socket, None).unwrap();
            let b = socket_anchor(&graph, &library, to_node, &style, SocketKind::Input,
                &conn.to.socket, Some(conn.order)).unwrap();

            let mut hull = Rect::from_points(&[a, b]);
            for p in &conn.waypoints {
                hull.extend_with(*p);
            }
            let hull = hull.expand(0.5);
            for points in wire_path(a, b, &conn.waypoints, &style, 1.0) {
                for i in 0..=32 {
                    let p = bezier_point(&points, i as f32 / 32.0);
                    assert!(
                        hull.contains(p),
                        "{:?} swings out to {p:?}, past {hull:?}",
                        conn.id
                    );
                }
            }
            checked += 1;
        }
        assert!(checked > 0, "no wire was routed, so this proves nothing");
    }

    /// The router has to aim at the attachment point the wire is drawn to. On
    /// a multi-input those differ per link, and aiming at the first of them
    /// leaves every other wire a stray diagonal to cover at the end.
    #[test]
    fn routing_meets_a_multi_input_at_the_slot_the_wire_uses() {
        let (library, mut graph) = crowded();
        let style = EditorStyle::default();
        let size = |g: &crate::Graph, n: &crate::Node| node_size(g, &library, n, &style);

        crate::layout::layered(&mut graph, &LayoutOptions::default(), size).unwrap();
        crate::layout::route_links(
            &mut graph,
            &RouteOptions::default(),
            size,
            |g, socket, kind, slot| {
                let node = g.node(socket.node)?;
                socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .unwrap();

        let mut checked = 0;
        for conn in graph.connections() {
            let Some(last) = conn.waypoints.last() else {
                continue;
            };
            // Where the editor will actually draw this wire's end.
            let to_node = graph.node(conn.to.node).unwrap();
            let target = socket_anchor(&graph, &library, to_node, &style, SocketKind::Input,
                &conn.to.socket, Some(conn.order)).unwrap();
            assert!(
                (last.y - target.y).abs() < 0.01,
                "{:?} on slot {} is routed to y {} but drawn to y {}",
                conn.id,
                conn.order,
                last.y,
                target.y
            );
            if conn.order > 0 {
                checked += 1;
            }
        }
        assert!(checked > 0, "no wire past the first slot was routed");
    }

    /// Two wires climbing the same channel must not be drawn one on top of
    /// the other, the same way two crossing at the same height are fanned.
    #[test]
    fn wires_climbing_one_channel_get_their_own_line() {
        let (library, mut graph) = crowded();
        let style = EditorStyle::default();
        let size = |g: &crate::Graph, n: &crate::Node| node_size(g, &library, n, &style);

        crate::layout::layered(&mut graph, &LayoutOptions::default(), size).unwrap();
        crate::layout::route_links(
            &mut graph,
            &RouteOptions::default(),
            size,
            |g, socket, kind, slot| {
                let node = g.node(socket.node)?;
                socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .unwrap();

        // Every stretch of wire that runs straight down.
        let mut climbs = Vec::new();
        for conn in graph.connections() {
            for pair in conn.waypoints.windows(2) {
                if (pair[0].x - pair[1].x).abs() < 0.01 && (pair[0].y - pair[1].y).abs() > 1.0 {
                    climbs.push((
                        conn.id,
                        pair[0].x,
                        pair[0].y.min(pair[1].y),
                        pair[0].y.max(pair[1].y),
                    ));
                }
            }
        }
        assert!(climbs.len() > 1, "this graph needs climbs, or it proves nothing");

        for (i, a) in climbs.iter().enumerate() {
            for b in &climbs[i + 1..] {
                let overlaps = a.2.max(b.2) < a.3.min(b.3);
                assert!(
                    !(overlaps && (a.1 - b.1).abs() < 0.5),
                    "{:?} and {:?} climb the same line at x {}",
                    a.0,
                    b.0,
                    a.1
                );
            }
        }
    }

    #[test]
    fn a_routed_wire_crosses_at_one_height() {
        let (library, mut graph) = crowded();
        let style = EditorStyle::default();
        let size = |g: &crate::Graph, n: &crate::Node| node_size(g, &library, n, &style);

        crate::layout::layered(&mut graph, &LayoutOptions::default(), size).unwrap();
        crate::layout::route_links(
            &mut graph,
            &RouteOptions::default(),
            size,
            |g, socket, kind, slot| {
                let node = g.node(socket.node)?;
                socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .unwrap();

        let routed = graph
            .connections()
            .filter(|c| !c.waypoints.is_empty())
            .count();
        assert!(routed > 0, "this graph needs routing, or the test proves nothing");
        for conn in graph.connections() {
            if conn.waypoints.is_empty() {
                continue;
            }
            // Leaving a channel, the run across, arriving at the other: four
            // corners at the very most, and fewer when a height already
            // matches.
            assert!(
                conn.waypoints.len() <= 4,
                "{:?} bends {} times",
                conn.id,
                conn.waypoints.len()
            );
            // The wire climbs inside the channels, so it only ever occupies
            // two x positions, and crosses everything between at one height.
            let mut xs: Vec<f32> = conn.waypoints.iter().map(|p| p.x).collect();
            xs.dedup();
            assert!(xs.len() <= 2, "{:?} climbs outside a channel: {xs:?}", conn.id);
            assert!(
                xs.windows(2).all(|w| w[0] < w[1]),
                "{:?} doubles back: {xs:?}",
                conn.id
            );
            // Exactly one height is shared by both channels: the run across.
            let shared: Vec<f32> = conn
                .waypoints
                .iter()
                .filter(|p| p.x == xs[0])
                .filter(|p| conn.waypoints.iter().any(|q| q.x != xs[0] && q.y == p.y))
                .map(|p| p.y)
                .collect();
            assert_eq!(shared.len(), 1, "{:?} steps between heights", conn.id);
        }
    }
}
