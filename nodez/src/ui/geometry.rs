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

/// One laid-out socket.
#[derive(Clone, Debug)]
pub struct SocketGeometry {
    pub kind: SocketKind,
    /// Index into the template's `inputs` or `outputs`.
    pub index: usize,
    pub name: String,
    pub ty: DataTypeId,
    /// Screen-space centre.
    pub center: Pos2,
    pub linked: bool,
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
        });
    }

    for (i, param) in template.params.iter().enumerate() {
        let lines = param.widget.rows().max(1.0);
        rows.push(RowPlan {
            kind: RowKind::Param(i),
            height: lines * style.row_height + (lines - 1.0) * style.row_spacing,
        });
    }

    for (i, socket) in template.inputs.iter().enumerate() {
        if socket.hidden {
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
                });
                linked
            }
            RowKind::Input(index) => {
                let socket = &template.inputs[index];
                let linked = graph.is_input_linked(node.id, &socket.name);
                sockets.push(SocketGeometry {
                    kind: SocketKind::Input,
                    index,
                    name: socket.name.clone(),
                    ty: socket.ty,
                    center: pos2(body.left(), socket_y),
                    linked,
                });
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
    let dx = (to.x - from.x).abs();
    let dy = (to.y - from.y).abs();
    let pull = (dx * style.wire_curvature)
        .max(viewport_scaled(style.wire_min_curve, zoom) + dy * 0.15)
        .min(viewport_scaled(style.wire_max_curve, zoom));
    [
        from,
        pos2(from.x + pull, from.y),
        pos2(to.x - pull, to.y),
        to,
    ]
}

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
