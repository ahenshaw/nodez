//! The editor widget.
//!
//! [`NodeEditor`] owns view state (pan, zoom, selection, whatever drag is in
//! flight) and borrows the [`Graph`] and [`NodeLibrary`] for the duration of a
//! frame, so the same graph can be shown in more than one editor.

mod draw;
mod geometry;
mod menu;
mod style;
mod widgets;

pub use geometry::{
    NodeGeometry, RowGeometry, RowKind, SocketGeometry, Viewport, free_position, node_size,
};
pub use style::EditorStyle;

use std::collections::HashSet;

use egui::{
    Align2, CornerRadius, FontId, Id, Key, Modifiers, PointerButton, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::graph::{
    ConnectError, Connection, ConnectionId, Graph, NodeData, NodeId, SocketKind, SocketRef,
};
use crate::template::{NodeLibrary, TemplateId, Widget};
use crate::types::DataTypeId;

use draw::{NodeChromeState, SocketState};
use menu::{LinkFilter, MenuState};

/// Something the editor did, reported back so the host app can react.
#[derive(Clone, Debug)]
pub enum EditorAction {
    NodeAdded(NodeId),
    NodesRemoved(Vec<NodeId>),
    NodesMoved(Vec<NodeId>),
    Connected(ConnectionId),
    Disconnected(Connection),
    /// An inline socket value was edited.
    InputChanged { node: NodeId, socket: String },
    /// A node parameter was edited.
    ParamChanged { node: NodeId, param: String },
    NodeRenamed(NodeId),
    SelectionChanged,
    /// A drag was released on an incompatible socket.
    ConnectionRejected(ConnectError),
}

impl EditorAction {
    /// Whether this action changed the graph's contents, as opposed to just the
    /// view or the selection.
    pub fn is_edit(&self) -> bool {
        !matches!(
            self,
            Self::SelectionChanged | Self::ConnectionRejected(_) | Self::NodesMoved(_)
        )
    }
}

/// What [`NodeEditor::show`] returns.
#[derive(Debug)]
pub struct EditorResponse {
    /// The response for the editor's background area.
    pub response: Response,
    /// True when the graph's contents changed this frame.
    pub changed: bool,
    /// Everything that happened, in order.
    pub actions: Vec<EditorAction>,
}

impl EditorResponse {
    /// Whether any action matches a predicate.
    pub fn any(&self, mut f: impl FnMut(&EditorAction) -> bool) -> bool {
        self.actions.iter().any(&mut f)
    }
}

/// What a bare scroll — no ctrl/cmd, no pinch — does in the editor.
///
/// Zoom is always available on ctrl/cmd + scroll and on a pinch gesture,
/// whichever of these is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScrollMode {
    /// Pan on a trackpad, zoom on a mouse wheel.
    ///
    /// Telling the two apart depends on what the windowing system reports, and
    /// it does not always report enough: X11 in particular often delivers a
    /// trackpad as plain wheel notches, indistinguishable from a mouse. Expose
    /// the other two variants as a preference if that matters to your users.
    #[default]
    Auto,
    /// Always pan.
    Pan,
    /// Always zoom.
    Zoom,
}

/// What the pointer is currently doing.
#[derive(Clone, Debug)]
enum Interaction {
    Idle,
    /// Dragging the selection with the mouse held down.
    DragNodes { moved: bool },
    /// Blender's `G`: nodes follow the pointer until a click confirms.
    Grab {
        start: Pos2,
        origins: Vec<(NodeId, Pos2)>,
    },
    /// Pulling a wire out of a socket.
    DragLink {
        anchor: SocketRef,
        anchor_is_output: bool,
        ty: DataTypeId,
        cursor: Pos2,
    },
    BoxSelect { start: Pos2, additive: bool },
    /// Ctrl-drag across wires to sever them.
    CutLinks { start: Pos2, cursor: Pos2 },
    Resize { node: NodeId, start_width: f32, start_x: f32 },
}

impl Interaction {
    fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// View state: where the camera is, what is selected, what drag is in flight.
#[derive(Clone, Debug)]
pub struct EditorState {
    /// Graph-space offset of the view. Increasing `pan` moves content right.
    pub pan: Vec2,
    pub zoom: f32,
    /// Every selected node.
    pub selection: HashSet<NodeId>,
    /// The last node clicked, drawn with a brighter outline.
    pub active: Option<NodeId>,
    interaction: Interaction,
    menu: Option<MenuState>,
    renaming: Option<NodeId>,
    rename_buffer: String,
    /// Set when a node should be raised at the end of the frame.
    raise: Option<NodeId>,
    /// Whether the scroll in flight came from a trackpad rather than a wheel.
    ///
    /// Remembered across frames because egui smooths a scroll out over several
    /// of them, and only the first carries the originating event.
    scroll_is_trackpad: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            selection: HashSet::new(),
            active: None,
            interaction: Interaction::Idle,
            menu: None,
            renaming: None,
            rename_buffer: String::new(),
            raise: None,
            scroll_is_trackpad: false,
        }
    }
}

impl EditorState {
    pub fn selection(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.selection.iter().copied()
    }

    pub fn is_selected(&self, node: NodeId) -> bool {
        self.selection.contains(&node)
    }

    pub fn select_only(&mut self, node: NodeId) {
        self.selection.clear();
        self.selection.insert(node);
        self.active = Some(node);
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.active = None;
    }

    /// Cancel any drag, grab or popup in progress.
    pub fn cancel_interaction(&mut self) {
        self.interaction = Interaction::Idle;
        self.menu = None;
        self.renaming = None;
    }

    /// Centre the view on a graph-space point.
    pub fn center_on(&mut self, screen: Rect, point: Pos2) {
        self.pan = screen.size() * 0.5 / self.zoom - point.to_vec2();
    }

    /// Fit a graph-space rectangle into the view, with a little margin.
    pub fn fit(&mut self, screen: Rect, bounds: Rect, style: &EditorStyle) {
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return;
        }
        let margin = 40.0;
        let available = vec2(
            (screen.width() - margin * 2.0).max(1.0),
            (screen.height() - margin * 2.0).max(1.0),
        );
        let zoom = (available.x / bounds.width())
            .min(available.y / bounds.height())
            .clamp(style.min_zoom, style.max_zoom);
        self.zoom = zoom;
        self.center_on(screen, bounds.center());
    }
}

/// A Blender-style node editor.
#[derive(Debug)]
pub struct NodeEditor {
    pub state: EditorState,
    pub style: EditorStyle,
    /// The id the editor uses for its popups. Change it if you show two editors.
    pub id: Option<Id>,
    /// What a bare scroll does.
    pub scroll_mode: ScrollMode,
    /// The rect the editor was last drawn in, used when framing the view.
    last_screen: Rect,
}

impl Default for NodeEditor {
    fn default() -> Self {
        Self {
            state: EditorState::default(),
            style: EditorStyle::default(),
            id: None,
            scroll_mode: ScrollMode::default(),
            last_screen: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
        }
    }
}

impl NodeEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style: EditorStyle) -> Self {
        Self {
            style,
            ..Self::default()
        }
    }

    /// The middle of what the editor is currently showing, in graph space.
    ///
    /// Useful for placing a node the user asked for from outside the canvas,
    /// where there is no cursor position to use.
    pub fn view_center(&self) -> egui::Pos2 {
        Viewport {
            screen: self.last_screen,
            pan: self.state.pan,
            zoom: self.state.zoom,
        }
        .visible_graph_rect()
        .center()
    }

    /// Frame the whole graph in the view.
    ///
    /// Call this after loading or generating a graph. `screen` is the rect the
    /// editor is drawn in; if you do not have it yet, pass the panel's rect.
    pub fn fit_to_graph<N: NodeData>(
        &mut self,
        screen: Rect,
        graph: &Graph<N>,
        library: &NodeLibrary,
    ) {
        let style = &self.style;
        let Some(bounds) = graph.bounds(|node| node_size(graph, library, node, style).y) else {
            return;
        };
        // `bounds` used each node's stored width; widen by the drawn width.
        let mut rect = bounds;
        for node in graph.nodes() {
            let size = node_size(graph, library, node, style);
            rect = rect.union(Rect::from_min_size(node.position, size));
        }
        self.state.fit(screen, rect, &self.style);
    }

    /// Draw the editor, filling the available space.
    pub fn show<N: NodeData>(
        &mut self,
        ui: &mut Ui,
        library: &NodeLibrary,
        graph: &mut Graph<N>,
    ) -> EditorResponse {
        let rect = ui.available_rect_before_wrap();
        self.show_in(ui, rect, library, graph)
    }

    /// Draw the editor in an explicit rectangle.
    pub fn show_in<N: NodeData>(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        library: &NodeLibrary,
        graph: &mut Graph<N>,
    ) -> EditorResponse {
        let mut actions = Vec::new();
        self.last_screen = rect;
        let background = ui.allocate_rect(rect, Sense::click_and_drag());
        let base_id = ui.id().with("nodez");
        let editor_id = self.id.unwrap_or(base_id);

        self.handle_view_input(ui, &background, rect);

        let viewport = Viewport {
            screen: rect,
            pan: self.state.pan,
            zoom: self.state.zoom,
        };
        let zoom = viewport.zoom;
        let painter = ui.painter().clone().with_clip_rect(rect);

        draw::paint_grid(&painter, &viewport, &self.style);

        // Lay every node out first, so wires can be drawn behind them.
        let geoms: Vec<geometry::NodeGeometry> = graph
            .nodes_in_draw_order()
            .map(|node| geometry::node_geometry(graph, library, node, &self.style, &viewport))
            .collect();
        let visible: Vec<usize> = (0..geoms.len())
            .filter(|&i| geoms[i].rect.intersects(rect))
            .collect();

        let pointer = ui.input(|i| i.pointer.hover_pos());
        let hovered_socket = pointer.and_then(|p| self.socket_at(&geoms, p, zoom));
        let hovered_wire = match (&pointer, &hovered_socket, &self.state.interaction) {
            (Some(p), None, Interaction::Idle) => self.wire_at(graph, &geoms, *p, zoom),
            _ => None,
        };

        let mut ops: Vec<Op> = Vec::new();

        self.paint_wires(&painter, graph, library, &geoms, zoom, hovered_wire);

        // Node bodies, their widgets and their sockets.
        let original_style = ui.style().clone();
        let widget_style =
            style::scaled_style(&style::node_widget_visuals(&self.style, &original_style), zoom);
        ui.set_style(widget_style);
        let mut changed = false;
        for &index in &visible {
            changed |= self.show_node(
                ui,
                &painter,
                base_id,
                graph,
                library,
                &geoms[index],
                &viewport,
                hovered_socket.as_ref(),
                &mut ops,
                &mut actions,
            );
        }
        ui.set_style(original_style);

        self.paint_overlays(&painter, graph, library, &geoms, &viewport, hovered_socket.as_ref());
        self.handle_background(
            &background,
            &geoms,
            &viewport,
            hovered_wire,
            &mut ops,
            &mut actions,
        );
        self.finish_link_drag(ui, hovered_socket.as_ref(), &viewport, &mut ops);
        self.handle_cut(ui, graph, &geoms, &viewport, &mut ops);
        self.handle_keyboard(ui, &background, graph, &viewport, &mut ops);
        self.run_grab(ui, graph, &mut actions);

        if let Some(menu_action) = self.show_menu(ui, editor_id, library) {
            ops.push(menu_action);
        }

        changed |= self.apply(ops, graph, library, &mut actions);

        if let Some(node) = self.state.raise.take() {
            graph.raise_node(node);
        }
        if !self.state.interaction.is_idle() {
            ui.ctx().request_repaint();
        }

        EditorResponse {
            response: background,
            changed,
            actions,
        }
    }

    // ------------------------------------------------------------- view

    fn handle_view_input(&mut self, ui: &Ui, background: &Response, rect: Rect) {
        let hovering = background.contains_pointer();

        // Middle-drag pans, wherever the pointer is, so nodes do not block it.
        if hovering || !self.state.interaction.is_idle() {
            let (middle_down, delta) =
                ui.input(|i| (i.pointer.middle_down(), i.pointer.delta()));
            if middle_down {
                self.state.pan += delta / self.state.zoom;
            }
        }

        if !hovering {
            return;
        }

        // egui has already done most of the interpreting for us: ctrl/cmd +
        // scroll and pinch both arrive as `zoom_delta`, with `smooth_scroll_delta`
        // zeroed so the two can never fight, and shift / alt have already been
        // applied as axis remapping on the scroll. What is left to decide is
        // what a *bare* scroll means, and that depends on the device.
        let (scroll, zoom_delta, modifiers, pointer, wheel_event) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.modifiers,
                i.pointer.hover_pos(),
                i.events.iter().find_map(|event| match event {
                    egui::Event::MouseWheel {
                        unit, delta, phase, ..
                    } => Some((*unit, *delta, *phase)),
                    _ => None,
                }),
            )
        });
        if let Some((unit, delta, phase)) = wheel_event {
            self.state.scroll_is_trackpad = is_trackpad(unit, delta, phase);
        }

        let anchor = pointer.unwrap_or_else(|| rect.center());
        if (zoom_delta - 1.0).abs() > 1e-4 {
            self.zoom_about(rect, anchor, zoom_delta);
        }

        if scroll == Vec2::ZERO {
            return;
        }
        let pans = match self.scroll_mode {
            ScrollMode::Pan => true,
            ScrollMode::Zoom => false,
            ScrollMode::Auto => self.state.scroll_is_trackpad,
        };
        // An axis modifier always pans, whatever the mode: egui has moved the
        // delta onto that axis, so there would be nothing left to zoom with.
        if pans || modifiers.shift || modifiers.alt {
            self.state.pan += scroll / self.state.zoom;
        } else {
            // A bare wheel zooms, as in Blender's node editor.
            self.zoom_about(rect, anchor, (scroll.y * self.style.zoom_speed).exp());
        }
    }

    /// Scale the view by `factor`, keeping the graph point under `anchor` put.
    fn zoom_about(&mut self, rect: Rect, anchor: Pos2, factor: f32) {
        let before = Viewport {
            screen: rect,
            pan: self.state.pan,
            zoom: self.state.zoom,
        }
        .to_graph(anchor);
        self.state.zoom =
            (self.state.zoom * factor).clamp(self.style.min_zoom, self.style.max_zoom);
        self.state.pan = (anchor - rect.min) / self.state.zoom - before.to_vec2();
    }

    // -------------------------------------------------------------- wires

    fn paint_wires<N: NodeData>(
        &self,
        painter: &egui::Painter,
        graph: &Graph<N>,
        library: &NodeLibrary,
        geoms: &[geometry::NodeGeometry],
        zoom: f32,
        hovered_wire: Option<ConnectionId>,
    ) {
        for conn in graph.connections() {
            let (Some(from), Some(to)) = (
                find_socket(geoms, &conn.from, SocketKind::Output),
                find_link_end(geoms, conn),
            ) else {
                continue;
            };
            let highlighted = hovered_wire == Some(conn.id);
            draw::paint_wire(
                painter,
                from.center,
                to.center,
                library.types.color(from.ty),
                library.types.color(to.ty),
                &self.style,
                zoom,
                highlighted,
            );
        }
    }

    /// The dragged wire, the box-select rectangle and the link-cut line all sit
    /// above the nodes.
    fn paint_overlays<N: NodeData>(
        &self,
        painter: &egui::Painter,
        graph: &Graph<N>,
        library: &NodeLibrary,
        geoms: &[geometry::NodeGeometry],
        viewport: &Viewport,
        hovered_socket: Option<&HoveredSocket>,
    ) {
        match &self.state.interaction {
            Interaction::DragLink {
                anchor,
                anchor_is_output,
                ty,
                cursor,
            } => {
                let kind = if *anchor_is_output {
                    SocketKind::Output
                } else {
                    SocketKind::Input
                };
                let Some(socket) = find_socket(geoms, anchor, kind) else {
                    return;
                };
                let anchor_color = library.types.color(*ty);
                let (target_color, valid) = match hovered_socket {
                    Some(target) => {
                        let ok = self
                            .link_would_connect(graph, library, anchor, *anchor_is_output, target)
                            .is_ok();
                        (
                            if ok {
                                library.types.color(target.ty)
                            } else {
                                self.style.wire_invalid
                            },
                            ok,
                        )
                    }
                    None => (self.style.wire_dragging, true),
                };
                let end = hovered_socket
                    .filter(|_| valid)
                    .map_or(*cursor, |s| s.center);
                let (a, b) = if *anchor_is_output {
                    (socket.center, end)
                } else {
                    (end, socket.center)
                };
                let (ca, cb) = if *anchor_is_output {
                    (anchor_color, target_color)
                } else {
                    (target_color, anchor_color)
                };
                draw::paint_wire(painter, a, b, ca, cb, &self.style, viewport.zoom, false);
            }
            Interaction::BoxSelect { start, .. } => {
                let Some(cursor) = painter
                    .ctx()
                    .input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()))
                else {
                    return;
                };
                let rect = Rect::from_two_pos(*start, cursor);
                painter.rect_filled(rect, CornerRadius::ZERO, self.style.box_select_fill);
                painter.rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    self.style.box_select_stroke,
                    StrokeKind::Middle,
                );
            }
            Interaction::CutLinks { start, cursor } => {
                painter.line_segment(
                    [*start, *cursor],
                    Stroke::new(1.5, self.style.wire_invalid),
                );
            }
            _ => {}
        }
    }

    // --------------------------------------------------------------- node

    #[allow(clippy::too_many_arguments)]
    fn show_node<N: NodeData>(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        base_id: Id,
        graph: &mut Graph<N>,
        library: &NodeLibrary,
        geom: &geometry::NodeGeometry,
        viewport: &Viewport,
        hovered_socket: Option<&HoveredSocket>,
        ops: &mut Vec<Op>,
        actions: &mut Vec<EditorAction>,
    ) -> bool {
        let zoom = viewport.zoom;
        let Some((title, template_id, collapsed, muted)) = graph
            .node(geom.id)
            .map(|n| (n.title.clone(), n.template, n.collapsed, n.muted))
        else {
            return false;
        };
        let Some(template) = library.get(template_id) else {
            return false;
        };
        let header_color = library.header_color(template);

        let node_id = base_id.with(("node", geom.id));
        let response = ui.interact(geom.rect, node_id, Sense::click_and_drag());

        draw::paint_node_chrome(
            painter,
            geom.rect,
            geom.header,
            header_color,
            &self.style,
            zoom,
            NodeChromeState {
                selected: self.state.is_selected(geom.id),
                active: self.state.active == Some(geom.id),
                muted,
                hovered: response.hovered(),
            },
        );

        // Header: collapse arrow, then the title (or its rename field).
        let arrow = Rect::from_min_size(
            geom.header.min + vec2(2.0 * zoom, 0.0),
            vec2(geom.header.height(), geom.header.height()),
        );
        draw::paint_collapse_arrow(painter, arrow, collapsed, self.style.header_text);
        let arrow_response = ui.interact(arrow, node_id.with("arrow"), Sense::click());
        if arrow_response.clicked() {
            ops.push(Op::ToggleCollapse(geom.id));
        }

        let title_rect = Rect::from_min_max(
            pos2(arrow.right() + 2.0 * zoom, geom.header.top()),
            pos2(geom.header.right() - 4.0 * zoom, geom.header.bottom()),
        );
        if self.state.renaming == Some(geom.id) {
            let edit = ui.put(
                title_rect,
                egui::TextEdit::singleline(&mut self.state.rename_buffer)
                    .font(FontId::proportional(
                        (self.style.header_font_size * zoom).max(4.0),
                    ))
                    .desired_width(title_rect.width()),
            );
            edit.request_focus();
            let commit = ui.input(|i| i.key_pressed(Key::Enter)) || edit.lost_focus();
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            if cancel {
                self.state.renaming = None;
            } else if commit {
                ops.push(Op::Rename(geom.id, self.state.rename_buffer.clone()));
                self.state.renaming = None;
            }
        } else {
            draw::paint_clipped_text(
                painter,
                title_rect,
                pos2(title_rect.left(), title_rect.center().y),
                Align2::LEFT_CENTER,
                &title,
                FontId::proportional((self.style.header_font_size * zoom).max(4.0)),
                self.style.header_text,
            );
            if response.double_clicked() && geom.header.contains(response.interact_pointer_pos().unwrap_or_default())
            {
                self.state.renaming = Some(geom.id);
                self.state.rename_buffer = title.clone();
            }
        }

        // Selection and dragging.
        let modifiers = ui.input(|i| i.modifiers);
        if response.clicked() {
            self.click_node(geom.id, modifiers, actions);
        }
        if response.drag_started_by(PointerButton::Primary) {
            if !self.state.is_selected(geom.id) {
                self.click_node(geom.id, modifiers, actions);
            }
            self.state.interaction = Interaction::DragNodes { moved: false };
        }
        if let Interaction::DragNodes { moved } = &mut self.state.interaction
            && response.dragged_by(PointerButton::Primary)
        {
            let delta = response.drag_delta() / zoom;
            if delta != Vec2::ZERO {
                *moved = true;
                let selection: Vec<_> = self.state.selection.iter().copied().collect();
                for id in selection {
                    if let Some(node) = graph.node_mut(id) {
                        node.position += delta;
                    }
                }
            }
        }
        if response.drag_stopped()
            && let Interaction::DragNodes { moved } = self.state.interaction
        {
            if moved {
                actions.push(EditorAction::NodesMoved(
                    self.state.selection.iter().copied().collect(),
                ));
            }
            self.state.interaction = Interaction::Idle;
        }

        self.node_context_menu(&response, geom.id, ops);

        // Resize grip on the right edge.
        let grip = geom.resize_handle(5.0 * zoom);
        let grip_response = ui.interact(grip, node_id.with("resize"), Sense::drag());
        if grip_response.hovered() || matches!(self.state.interaction, Interaction::Resize { node, .. } if node == geom.id)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if grip_response.drag_started() {
            let width = graph.node(geom.id).map_or(150.0, |n| n.width);
            self.state.interaction = Interaction::Resize {
                node: geom.id,
                start_width: width,
                start_x: grip_response.interact_pointer_pos().map_or(0.0, |p| p.x),
            };
        }
        if let Interaction::Resize {
            node,
            start_width,
            start_x,
        } = self.state.interaction
            && node == geom.id
            && grip_response.dragged()
            && let Some(p) = grip_response.interact_pointer_pos()
            && let Some(n) = graph.node_mut(node)
        {
            n.width = (start_width + (p.x - start_x) / zoom)
                .clamp(self.style.min_node_width, self.style.max_node_width);
        }
        if grip_response.drag_stopped() {
            self.state.interaction = Interaction::Idle;
        }

        // One rail per multi-input, behind its slots.
        let mut rails: Vec<(usize, f32, f32, crate::types::DataTypeId)> = Vec::new();
        for socket in geom.sockets.iter().filter(|s| s.slot.is_some()) {
            match rails.iter_mut().find(|(index, ..)| *index == socket.index) {
                Some((_, top, bottom, _)) => {
                    *top = top.min(socket.center.y);
                    *bottom = bottom.max(socket.center.y);
                }
                None => rails.push((socket.index, socket.center.y, socket.center.y, socket.ty)),
            }
        }
        for (_, top, bottom, ty) in rails {
            if bottom > top {
                draw::paint_multi_track(
                    painter,
                    geom.body.left(),
                    top,
                    bottom,
                    library.types.color(ty),
                    &self.style,
                    zoom,
                );
            }
        }

        // Sockets are allocated before the body widgets so a widget wins in the
        // small region where their hit-boxes overlap.
        let mut changed = false;
        for socket in &geom.sockets {
            let radius = self.style.socket_radius * zoom + self.style.socket_grab_padding;
            // Bias the hit-box outwards, away from the node body.
            let bias = if socket.kind.is_input() { -0.3 } else { 0.3 } * radius;
            let hit = Rect::from_center_size(
                socket.center + vec2(bias, 0.0),
                Vec2::splat(radius * 2.0),
            );
            // The slot is part of the id: a multi-input draws several
            // attachment points that share a socket index.
            let salt = (
                "socket",
                geom.id,
                socket.kind.is_output(),
                socket.index,
                socket.slot,
            );
            let socket_response = ui.interact(hit, base_id.with(salt), Sense::click_and_drag());
            if socket_response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if socket_response.drag_started_by(PointerButton::Primary) {
                self.start_link_drag(graph, library, geom.id, socket, ops);
            }
            if socket_response.double_clicked() {
                ops.push(Op::DisconnectSocket(SocketRef::new(
                    geom.id,
                    socket.name.clone(),
                )));
            }

            let state = self.socket_state(graph, library, socket, geom.id, hovered_socket);
            if socket.is_free_slot {
                draw::paint_free_slot(
                    painter,
                    socket.center,
                    library.types.color(socket.ty),
                    &self.style,
                    zoom,
                    state,
                );
            } else {
                draw::paint_socket(
                    painter,
                    socket.center,
                    library.types.color(socket.ty),
                    library.types.shape(socket.ty),
                    &self.style,
                    zoom,
                    state,
                );
            }
            if socket_response.hovered() {
                let spec = if socket.kind.is_input() {
                    template.inputs.get(socket.index)
                } else {
                    template.outputs.get(socket.index)
                };
                if let Some(spec) = spec {
                    let mut text = format!(
                        "{} \u{2014} {}",
                        spec.display(),
                        library.types.name(socket.ty)
                    );
                    if !spec.description.is_empty() {
                        text.push('\n');
                        text.push_str(&spec.description);
                    }
                    socket_response.on_hover_text(text);
                }
            }
        }

        if geom.collapsed || zoom < self.style.detail_cutoff {
            return changed;
        }

        // Body rows: labels drawn by us, editors by egui.
        let label_font = FontId::proportional((self.style.body_font_size * zoom).max(4.0));
        for row in &geom.rows {
            match row.kind {
                RowKind::Output(index) => {
                    let Some(spec) = template.outputs.get(index) else {
                        continue;
                    };
                    draw::paint_clipped_text(
                        painter,
                        row.rect,
                        pos2(row.rect.right(), row.rect.center().y),
                        Align2::RIGHT_CENTER,
                        spec.display(),
                        label_font.clone(),
                        self.style.body_text,
                    );
                }
                RowKind::Input(index) => {
                    let Some(spec) = template.inputs.get(index) else {
                        continue;
                    };
                    if row.linked || spec.multi || spec.widget == Widget::None {
                        let y = if spec.multi {
                            row.rect.top() + self.style.multi_slot_height * zoom * 0.5
                        } else {
                            row.rect.center().y
                        };
                        draw::paint_clipped_text(
                            painter,
                            row.rect,
                            pos2(row.rect.left(), y),
                            Align2::LEFT_CENTER,
                            spec.display(),
                            label_font.clone(),
                            self.style.body_text,
                        );
                        continue;
                    }
                    let mut value = graph
                        .node(geom.id)
                        .and_then(|n| n.input_value(&spec.name))
                        .unwrap_or_else(|| spec.default.clone());
                    let widget_rect = self.split_row(
                        painter,
                        row.rect,
                        spec.display(),
                        &spec.widget,
                        &label_font,
                        true,
                    );
                    if widgets::value_widget(
                        ui,
                        widget_rect,
                        ("in", geom.id, index),
                        &spec.widget,
                        &mut value,
                        spec.display(),
                        &self.style,
                        zoom,
                    ) {
                        if let Some(node) = graph.node_mut(geom.id) {
                            node.set_input_value(&spec.name, value);
                        }
                        actions.push(EditorAction::InputChanged {
                            node: geom.id,
                            socket: spec.name.clone(),
                        });
                        changed = true;
                    }
                }
                RowKind::Param(index) => {
                    let Some(spec) = template.params.get(index) else {
                        continue;
                    };
                    let mut value = graph
                        .node(geom.id)
                        .and_then(|n| n.param(&spec.name))
                        .unwrap_or_else(|| spec.default.clone());
                    let widget_rect = self.split_row(
                        painter,
                        row.rect,
                        spec.display(),
                        &spec.widget,
                        &label_font,
                        spec.show_label,
                    );
                    if widgets::value_widget(
                        ui,
                        widget_rect,
                        ("param", geom.id, index),
                        &spec.widget,
                        &mut value,
                        spec.display(),
                        &self.style,
                        zoom,
                    ) {
                        if let Some(node) = graph.node_mut(geom.id) {
                            node.set_param(&spec.name, value);
                        }
                        actions.push(EditorAction::ParamChanged {
                            node: geom.id,
                            param: spec.name.clone(),
                        });
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    /// Draw a row's label where the widget does not draw its own, and return
    /// the rect left for the widget.
    fn split_row(
        &self,
        painter: &egui::Painter,
        row: Rect,
        label: &str,
        widget: &Widget,
        font: &FontId,
        show_label: bool,
    ) -> Rect {
        let self_labelling = matches!(
            widget,
            Widget::Checkbox | Widget::Text { .. } | Widget::Vec2 { .. } | Widget::Vec3 { .. }
        );
        if !show_label || self_labelling {
            return row;
        }
        let label_width = (row.width() * 0.45).min(90.0);
        draw::paint_clipped_text(
            painter,
            Rect::from_min_max(row.min, pos2(row.left() + label_width, row.bottom())),
            pos2(row.left(), row.center().y),
            Align2::LEFT_CENTER,
            label,
            font.clone(),
            self.style.body_text,
        );
        Rect::from_min_max(pos2(row.left() + label_width + 2.0, row.top()), row.max)
    }

    fn node_context_menu(&mut self, response: &Response, node: NodeId, ops: &mut Vec<Op>) {
        response.context_menu(|ui| {
            if ui.button("Rename\u{2026}").clicked() {
                self.state.renaming = Some(node);
                self.state.rename_buffer.clear();
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                ops.push(Op::Duplicate);
                ui.close();
            }
            if ui.button("Collapse / Expand").clicked() {
                ops.push(Op::ToggleCollapse(node));
                ui.close();
            }
            if ui.button("Mute / Unmute").clicked() {
                ops.push(Op::ToggleMute(node));
                ui.close();
            }
            ui.separator();
            if ui.button("Disconnect all").clicked() {
                ops.push(Op::DisconnectNode(node));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                ops.push(Op::DeleteSelected);
                ui.close();
            }
        });
    }

    fn click_node(&mut self, node: NodeId, modifiers: Modifiers, actions: &mut Vec<EditorAction>) {
        if modifiers.shift || modifiers.ctrl || modifiers.command {
            if !self.state.selection.insert(node) {
                self.state.selection.remove(&node);
            }
        } else {
            self.state.selection.clear();
            self.state.selection.insert(node);
        }
        self.state.active = Some(node);
        self.state.raise = Some(node);
        actions.push(EditorAction::SelectionChanged);
    }

    // ------------------------------------------------------------- sockets

    fn socket_at(
        &self,
        geoms: &[geometry::NodeGeometry],
        pointer: Pos2,
        zoom: f32,
    ) -> Option<HoveredSocket> {
        let radius = self.style.socket_radius * zoom + self.style.socket_grab_padding;
        let mut best: Option<(f32, HoveredSocket)> = None;
        for geom in geoms.iter().rev() {
            for socket in &geom.sockets {
                let distance = (socket.center - pointer).length();
                if distance > radius {
                    continue;
                }
                if best.as_ref().is_none_or(|(d, _)| distance < *d) {
                    best = Some((
                        distance,
                        HoveredSocket {
                            node: geom.id,
                            kind: socket.kind,
                            name: socket.name.clone(),
                            ty: socket.ty,
                            center: socket.center,
                            slot: socket.slot,
                        },
                    ));
                }
            }
        }
        best.map(|(_, s)| s)
    }

    /// The wire nearest the pointer, within a few points of it.
    fn wire_at<N: NodeData>(
        &self,
        graph: &Graph<N>,
        geoms: &[geometry::NodeGeometry],
        pointer: Pos2,
        zoom: f32,
    ) -> Option<ConnectionId> {
        let threshold = (self.style.wire_width * zoom).max(2.0) + 3.0;
        let mut best: Option<(f32, ConnectionId)> = None;
        for conn in graph.connections() {
            let (Some(from), Some(to)) = (
                find_socket(geoms, &conn.from, SocketKind::Output),
                find_link_end(geoms, conn),
            ) else {
                continue;
            };
            let points =
                geometry::wire_control_points(from.center, to.center, &self.style, zoom);
            let distance = geometry::distance_to_bezier(&points, pointer, 32);
            if distance <= threshold && best.as_ref().is_none_or(|(d, _)| distance < *d) {
                best = Some((distance, conn.id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn socket_state<N: NodeData>(
        &self,
        graph: &Graph<N>,
        library: &NodeLibrary,
        socket: &geometry::SocketGeometry,
        node: NodeId,
        hovered: Option<&HoveredSocket>,
    ) -> SocketState {
        let is_hovered = hovered
            .is_some_and(|h| h.node == node && h.kind == socket.kind && h.name == socket.name);
        match &self.state.interaction {
            Interaction::DragLink {
                anchor,
                anchor_is_output,
                ty,
                ..
            } => {
                if anchor.node == node && anchor.socket == socket.name {
                    return SocketState::Candidate;
                }
                let compatible = if *anchor_is_output {
                    socket.kind.is_input()
                        && node != anchor.node
                        && library.types.compatible(*ty, socket.ty)
                        && !graph.reaches(node, anchor.node)
                } else {
                    socket.kind.is_output()
                        && node != anchor.node
                        && library.types.compatible(socket.ty, *ty)
                        && !graph.reaches(anchor.node, node)
                };
                if compatible {
                    SocketState::Candidate
                } else {
                    SocketState::Rejected
                }
            }
            _ if is_hovered => SocketState::Hovered,
            _ => SocketState::Normal,
        }
    }

    fn start_link_drag<N: NodeData>(
        &mut self,
        graph: &Graph<N>,
        library: &NodeLibrary,
        node: NodeId,
        socket: &geometry::SocketGeometry,
        ops: &mut Vec<Op>,
    ) {
        // Dragging off a wired input picks the wire up rather than starting a
        // second one, matching Blender.
        let existing = socket.kind.is_input().then(|| match socket.slot {
            Some(slot) => graph
                .links_into(node, &socket.name)
                .find(|c| c.order == slot),
            None => graph.link_into(node, &socket.name),
        });
        if let Some(Some(conn)) = existing {
            let ty = graph
                .node(conn.from.node)
                .and_then(|n| library.get(n.template))
                .and_then(|t| t.output_spec(&conn.from.socket))
                .map_or(socket.ty, |s| s.ty);
            ops.push(Op::Disconnect(conn.id));
            self.state.interaction = Interaction::DragLink {
                anchor: conn.from.clone(),
                anchor_is_output: true,
                ty,
                cursor: socket.center,
            };
            return;
        }
        self.state.interaction = Interaction::DragLink {
            anchor: SocketRef::new(node, socket.name.clone()),
            anchor_is_output: socket.kind.is_output(),
            ty: socket.ty,
            cursor: socket.center,
        };
    }

    /// Whether the wire in flight could land on `target`.
    fn link_would_connect<N: NodeData>(
        &self,
        graph: &Graph<N>,
        library: &NodeLibrary,
        anchor: &SocketRef,
        anchor_is_output: bool,
        target: &HoveredSocket,
    ) -> Result<(SocketRef, SocketRef), ConnectError> {
        let (from, to) = if anchor_is_output {
            if !target.kind.is_input() {
                return Err(ConnectError::SelfLink);
            }
            (
                anchor.clone(),
                SocketRef::new(target.node, target.name.clone()),
            )
        } else {
            if !target.kind.is_output() {
                return Err(ConnectError::SelfLink);
            }
            (
                SocketRef::new(target.node, target.name.clone()),
                anchor.clone(),
            )
        };
        match graph.can_connect(library, &from, &to) {
            Ok(()) | Err(ConnectError::AlreadyConnected) => Ok((from, to)),
            Err(e) => Err(e),
        }
    }

    fn finish_link_drag(
        &mut self,
        ui: &Ui,
        hovered: Option<&HoveredSocket>,
        viewport: &Viewport,
        ops: &mut Vec<Op>,
    ) {
        let Interaction::DragLink {
            anchor,
            anchor_is_output,
            ty,
            cursor,
        } = &mut self.state.interaction
        else {
            return;
        };
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
            *cursor = p;
        }
        let released = ui.input(|i| i.pointer.any_released() || !i.pointer.primary_down());
        if !released {
            return;
        }

        let anchor = anchor.clone();
        let anchor_is_output = *anchor_is_output;
        let ty = *ty;
        let cursor = *cursor;
        self.state.interaction = Interaction::Idle;

        match hovered {
            Some(target) => ops.push(Op::ConnectFromDrag {
                anchor,
                anchor_is_output,
                target_node: target.node,
                target_socket: target.name.clone(),
                target_is_output: target.kind.is_output(),
                target_slot: target.slot,
            }),
            // Dropping in empty space opens the search menu, filtered to nodes
            // that can take the wire — Blender's link-drag search.
            None => {
                self.state.menu = Some(MenuState::new(
                    cursor,
                    viewport.to_graph(cursor),
                    Some(LinkFilter {
                        anchor,
                        anchor_is_output,
                        ty,
                    }),
                ));
            }
        }
    }

    // -------------------------------------------------------- background

    /// The background response carries its own `Context`, so this does not need
    /// the `Ui`.
    fn handle_background(
        &mut self,
        background: &Response,
        geoms: &[geometry::NodeGeometry],
        viewport: &Viewport,
        hovered_wire: Option<ConnectionId>,
        ops: &mut Vec<Op>,
        actions: &mut Vec<EditorAction>,
    ) {
        let modifiers = background.ctx.input(|i| i.modifiers);

        // Double-clicking a noodle severs it.
        if let Some(id) = hovered_wire {
            background.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            if background.double_clicked() {
                ops.push(Op::Disconnect(id));
                return;
            }
        }

        if background.drag_started_by(PointerButton::Primary)
            && self.state.interaction.is_idle()
            && let Some(start) = background
                .ctx
                .input(|i| i.pointer.press_origin())
                .or_else(|| background.interact_pointer_pos())
        {
            self.state.interaction = if modifiers.ctrl || modifiers.command {
                Interaction::CutLinks {
                    start,
                    cursor: start,
                }
            } else {
                Interaction::BoxSelect {
                    start,
                    additive: modifiers.shift,
                }
            };
        }

        if let Interaction::BoxSelect { start, additive } = self.state.interaction
            && (background.drag_stopped() || background.ctx.input(|i| !i.pointer.primary_down()))
        {
            let cursor = background
                .ctx
                .input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()))
                .unwrap_or(start);
            let rect = Rect::from_two_pos(start, cursor);
            if !additive {
                self.state.selection.clear();
            }
            for geom in geoms {
                if rect.intersects(geom.rect) {
                    self.state.selection.insert(geom.id);
                }
            }
            // Keep an active node so the host app has something to inspect.
            if !self.state.selection.contains(&self.state.active.unwrap_or(NodeId(u64::MAX))) {
                self.state.active = self.state.selection.iter().copied().min();
            }
            self.state.interaction = Interaction::Idle;
            actions.push(EditorAction::SelectionChanged);
        }

        if background.clicked() && self.state.interaction.is_idle() && !modifiers.shift {
            self.state.clear_selection();
            actions.push(EditorAction::SelectionChanged);
        }

        if background.secondary_clicked()
            && let Some(pos) = background.interact_pointer_pos()
        {
            self.state.menu = Some(MenuState::new(pos, viewport.to_graph(pos), None));
        }
    }

    fn handle_cut<N: NodeData>(
        &mut self,
        ui: &Ui,
        graph: &Graph<N>,
        geoms: &[geometry::NodeGeometry],
        viewport: &Viewport,
        ops: &mut Vec<Op>,
    ) {
        let Interaction::CutLinks { start, cursor } = &mut self.state.interaction else {
            return;
        };
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
            *cursor = p;
        }
        if ui.input(|i| i.pointer.primary_down()) {
            return;
        }

        let (start, end) = (*start, *cursor);
        self.state.interaction = Interaction::Idle;
        for conn in graph.connections() {
            let (Some(from), Some(to)) = (
                find_socket(geoms, &conn.from, SocketKind::Output),
                find_link_end(geoms, conn),
            ) else {
                continue;
            };
            let points = geometry::wire_control_points(
                from.center,
                to.center,
                &self.style,
                viewport.zoom,
            );
            if cut_crosses_bezier(&points, start, end) {
                ops.push(Op::Disconnect(conn.id));
            }
        }
    }

    // -------------------------------------------------------- keyboard

    fn handle_keyboard<N: NodeData>(
        &mut self,
        ui: &Ui,
        background: &Response,
        graph: &Graph<N>,
        viewport: &Viewport,
        ops: &mut Vec<Op>,
    ) {
        // Never steal keys from a focused text field.
        if ui.memory(|m| m.focused()).is_some() || self.state.menu.is_some() {
            return;
        }
        if !background.contains_pointer() && !background.has_focus() {
            return;
        }

        // Modifiers are read from each key event rather than from the
        // end-of-frame state: a quick `Shift+A` can release shift in the same
        // frame, which would otherwise look like a bare `A`.
        let presses: Vec<(Key, Modifiers)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        let pointer = ui.input(|i| i.pointer.hover_pos());

        for (key, modifiers) in presses {
            match key {
                Key::A if modifiers.shift => {
                    let pos = pointer.unwrap_or_else(|| viewport.screen.center());
                    self.state.menu = Some(MenuState::new(pos, viewport.to_graph(pos), None));
                }
                Key::A if modifiers.alt => self.state.clear_selection(),
                Key::A => self.state.selection = graph.node_ids().collect(),
                Key::D if modifiers.shift => ops.push(Op::Duplicate),
                Key::G if self.state.interaction.is_idle() => {
                    if let Some(p) = pointer {
                        let origins: Vec<_> = self
                            .state
                            .selection
                            .iter()
                            .filter_map(|id| graph.node(*id).map(|n| (n.id, n.position)))
                            .collect();
                        if !origins.is_empty() {
                            self.state.interaction = Interaction::Grab { start: p, origins };
                        }
                    }
                }
                Key::H => ops.push(Op::ToggleCollapseSelected),
                Key::M => ops.push(Op::ToggleMuteSelected),
                Key::X | Key::Delete => ops.push(Op::DeleteSelected),
                Key::Home => ops.push(Op::FrameAll),
                Key::Period => ops.push(Op::FrameSelected),
                Key::Escape => {
                    if let Interaction::Grab { origins, .. } = &self.state.interaction {
                        ops.push(Op::RestorePositions(origins.clone()));
                    }
                    self.state.cancel_interaction();
                }
                _ => {}
            }
        }
    }

    /// Modal `G`: the selection follows the pointer until a click confirms.
    fn run_grab<N: NodeData>(
        &mut self,
        ui: &Ui,
        graph: &mut Graph<N>,
        actions: &mut Vec<EditorAction>,
    ) {
        let Interaction::Grab { start, origins } = &self.state.interaction else {
            return;
        };
        let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let delta = (cursor - *start) / self.state.zoom;
        for (id, origin) in origins {
            if let Some(node) = graph.node_mut(*id) {
                node.position = *origin + delta;
            }
        }
        let (confirm, cancel) = ui.input(|i| {
            (
                i.pointer.button_clicked(PointerButton::Primary) || i.key_pressed(Key::Enter),
                i.pointer.button_clicked(PointerButton::Secondary),
            )
        });
        if cancel {
            for (id, origin) in origins {
                if let Some(node) = graph.node_mut(*id) {
                    node.position = *origin;
                }
            }
            self.state.interaction = Interaction::Idle;
        } else if confirm {
            actions.push(EditorAction::NodesMoved(
                origins.iter().map(|(id, _)| *id).collect(),
            ));
            self.state.interaction = Interaction::Idle;
        }
    }

    // ------------------------------------------------------------ menu

    fn show_menu(
        &mut self,
        ui: &Ui,
        editor_id: Id,
        library: &NodeLibrary,
    ) -> Option<Op> {
        let state = self.state.menu.as_mut()?;
        let outcome = menu::show_menu(ui.ctx(), editor_id.with("add-menu"), state, library);
        let graph_pos = state.graph_pos;
        let link = state.link.clone();
        if outcome.close {
            self.state.menu = None;
        }
        outcome.picked.map(|template| Op::AddNode {
            template,
            position: graph_pos,
            link,
        })
    }

    // ------------------------------------------------------------- ops

    fn apply<N: NodeData>(
        &mut self,
        ops: Vec<Op>,
        graph: &mut Graph<N>,
        library: &NodeLibrary,
        actions: &mut Vec<EditorAction>,
    ) -> bool {
        let mut changed = false;
        for op in ops {
            changed |= self.apply_one(op, graph, library, actions);
        }
        changed
    }

    fn apply_one<N: NodeData>(
        &mut self,
        op: Op,
        graph: &mut Graph<N>,
        library: &NodeLibrary,
        actions: &mut Vec<EditorAction>,
    ) -> bool {
        match op {
            Op::Disconnect(id) => {
                if let Some(conn) = graph.disconnect(id) {
                    actions.push(EditorAction::Disconnected(conn));
                    return true;
                }
                false
            }
            Op::DisconnectSocket(socket) => {
                let removed = graph.disconnect_socket(&socket);
                let any = !removed.is_empty();
                actions.extend(removed.into_iter().map(EditorAction::Disconnected));
                any
            }
            Op::DisconnectNode(node) => {
                let removed = graph.disconnect_node(node);
                let any = !removed.is_empty();
                actions.extend(removed.into_iter().map(EditorAction::Disconnected));
                any
            }
            Op::ConnectFromDrag {
                anchor,
                anchor_is_output,
                target_node,
                target_socket,
                target_is_output,
                target_slot,
            } => {
                if anchor_is_output == target_is_output {
                    actions.push(EditorAction::ConnectionRejected(ConnectError::SelfLink));
                    return false;
                }
                let (from, to) = if anchor_is_output {
                    (anchor, SocketRef::new(target_node, target_socket))
                } else {
                    (SocketRef::new(target_node, target_socket), anchor)
                };
                let result = match target_slot {
                    // Dropping on a particular slot says where in the order it
                    // belongs, not merely that it belongs.
                    Some(slot) if anchor_is_output => {
                        graph.connect_at(library, from, to, slot)
                    }
                    _ => graph.connect(library, from, to),
                };
                match result {
                    Ok(id) => {
                        actions.push(EditorAction::Connected(id));
                        true
                    }
                    Err(ConnectError::AlreadyConnected) => false,
                    Err(e) => {
                        actions.push(EditorAction::ConnectionRejected(e));
                        false
                    }
                }
            }
            Op::AddNode {
                template,
                position,
                link,
            } => {
                let id = graph.add_node(library, template, position);
                // Centre the new node on the click, like Blender's add menu,
                // then step it clear of anything already there.
                if let Some(node) = graph.node_mut(id) {
                    let offset = vec2(node.width * 0.5, 0.0);
                    node.position -= offset;
                }
                let placed = graph.node(id).map(|node| {
                    (
                        node.position,
                        geometry::node_size(graph, library, node, &self.style),
                    )
                });
                if let Some((preferred, size)) = placed {
                    let free = geometry::free_position(
                        graph,
                        library,
                        &self.style,
                        size,
                        preferred,
                        Some(id),
                    );
                    if let Some(node) = graph.node_mut(id) {
                        node.position = free;
                    }
                }
                actions.push(EditorAction::NodeAdded(id));
                self.state.select_only(id);
                self.state.raise = Some(id);
                if let Some(link) = link {
                    self.auto_connect(graph, library, id, &link, actions);
                }
                true
            }
            Op::DeleteSelected => {
                let ids: Vec<_> = self.state.selection.iter().copied().collect();
                if ids.is_empty() {
                    return false;
                }
                for id in &ids {
                    graph.remove_node(*id);
                }
                self.state.clear_selection();
                actions.push(EditorAction::NodesRemoved(ids));
                true
            }
            Op::Duplicate => {
                if self.state.selection.is_empty() {
                    return false;
                }
                let mapping = graph.duplicate_subgraph(&self.state.selection, vec2(30.0, 30.0));
                if mapping.is_empty() {
                    return false;
                }
                self.state.selection = mapping.values().copied().collect();
                self.state.active = self.state.selection.iter().copied().next();
                for id in mapping.values() {
                    actions.push(EditorAction::NodeAdded(*id));
                }
                true
            }
            Op::ToggleCollapse(id) => {
                if let Some(node) = graph.node_mut(id) {
                    node.collapsed = !node.collapsed;
                    return true;
                }
                false
            }
            Op::ToggleCollapseSelected => {
                let collapse = self
                    .state
                    .selection
                    .iter()
                    .filter_map(|id| graph.node(*id))
                    .any(|n| !n.collapsed);
                let ids: Vec<_> = self.state.selection.iter().copied().collect();
                for id in ids {
                    if let Some(node) = graph.node_mut(id) {
                        node.collapsed = collapse;
                    }
                }
                !self.state.selection.is_empty()
            }
            Op::ToggleMute(id) => {
                if let Some(node) = graph.node_mut(id) {
                    node.muted = !node.muted;
                    return true;
                }
                false
            }
            Op::ToggleMuteSelected => {
                let mute = self
                    .state
                    .selection
                    .iter()
                    .filter_map(|id| graph.node(*id))
                    .any(|n| !n.muted);
                let ids: Vec<_> = self.state.selection.iter().copied().collect();
                for id in ids {
                    if let Some(node) = graph.node_mut(id) {
                        node.muted = mute;
                    }
                }
                !self.state.selection.is_empty()
            }
            Op::Rename(id, title) => {
                if title.trim().is_empty() {
                    return false;
                }
                if let Some(node) = graph.node_mut(id) {
                    node.title = title;
                    actions.push(EditorAction::NodeRenamed(id));
                    return true;
                }
                false
            }
            Op::RestorePositions(origins) => {
                for (id, position) in origins {
                    if let Some(node) = graph.node_mut(id) {
                        node.position = position;
                    }
                }
                false
            }
            Op::FrameAll => {
                self.frame(graph, library, None);
                false
            }
            Op::FrameSelected => {
                let selection = self.state.selection.clone();
                self.frame(graph, library, Some(&selection));
                false
            }
        }
    }

    /// Wire a freshly added node to whatever the dropped wire came from.
    fn auto_connect<N: NodeData>(
        &self,
        graph: &mut Graph<N>,
        library: &NodeLibrary,
        node: NodeId,
        link: &LinkFilter,
        actions: &mut Vec<EditorAction>,
    ) {
        let Some(template) = graph.node(node).and_then(|n| library.get(n.template)) else {
            return;
        };
        let socket = if link.anchor_is_output {
            template
                .inputs
                .iter()
                .find(|s| !s.hidden && library.types.compatible(link.ty, s.ty))
                .map(|s| s.name.clone())
        } else {
            template
                .outputs
                .iter()
                .find(|s| !s.hidden && library.types.compatible(s.ty, link.ty))
                .map(|s| s.name.clone())
        };
        let Some(socket) = socket else { return };
        let (from, to) = if link.anchor_is_output {
            (link.anchor.clone(), SocketRef::new(node, socket))
        } else {
            (SocketRef::new(node, socket), link.anchor.clone())
        };
        match graph.connect(library, from, to) {
            Ok(id) => actions.push(EditorAction::Connected(id)),
            Err(e) => actions.push(EditorAction::ConnectionRejected(e)),
        }
    }

    fn frame<N: NodeData>(
        &mut self,
        graph: &Graph<N>,
        library: &NodeLibrary,
        subset: Option<&HashSet<NodeId>>,
    ) {
        let style = &self.style;
        let mut bounds: Option<Rect> = None;
        for node in graph.nodes() {
            if subset.is_some_and(|s| !s.contains(&node.id)) {
                continue;
            }
            let size = node_size(graph, library, node, style);
            let rect = Rect::from_min_size(node.position, size);
            bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
        }
        if let Some(bounds) = bounds {
            let screen = self.last_screen;
            self.state.fit(screen, bounds, &self.style);
        }
    }
}

/// A socket the pointer is over.
#[derive(Clone, Debug)]
struct HoveredSocket {
    node: NodeId,
    kind: SocketKind,
    name: String,
    ty: DataTypeId,
    center: Pos2,
    /// Which attachment point of a multi-input, if any.
    slot: Option<u32>,
}

/// Structural edits are queued during drawing and applied at the end of the
/// frame, so the layout the user clicked on stays valid while it is handled.
#[derive(Clone, Debug)]
enum Op {
    AddNode {
        template: TemplateId,
        position: Pos2,
        link: Option<LinkFilter>,
    },
    ConnectFromDrag {
        anchor: SocketRef,
        anchor_is_output: bool,
        target_node: NodeId,
        target_socket: String,
        target_is_output: bool,
        /// Which attachment point of a multi-input the wire was dropped on.
        target_slot: Option<u32>,
    },
    Disconnect(ConnectionId),
    DisconnectSocket(SocketRef),
    DisconnectNode(NodeId),
    DeleteSelected,
    Duplicate,
    ToggleCollapse(NodeId),
    ToggleCollapseSelected,
    ToggleMute(NodeId),
    ToggleMuteSelected,
    Rename(NodeId, String),
    RestorePositions(Vec<(NodeId, Pos2)>),
    FrameAll,
    FrameSelected,
}

/// Guess whether a scroll event came from a trackpad rather than a wheel.
///
/// In descending order of reliability: pixel-precise deltas only come from a
/// smooth device; touch phases only come from a touch surface; and a device
/// reporting a *fraction* of a line is scrolling smoothly, whatever it calls
/// itself. A wheel notch is a whole number of lines, so it falls through.
fn is_trackpad(unit: egui::MouseWheelUnit, delta: Vec2, phase: egui::TouchPhase) -> bool {
    match unit {
        egui::MouseWheelUnit::Point => true,
        egui::MouseWheelUnit::Page => false,
        egui::MouseWheelUnit::Line => {
            phase != egui::TouchPhase::Move
                || delta.x.fract() != 0.0
                || delta.y.fract() != 0.0
        }
    }
}

fn find_socket<'a>(
    geoms: &'a [geometry::NodeGeometry],
    socket: &SocketRef,
    kind: SocketKind,
) -> Option<&'a geometry::SocketGeometry> {
    geoms
        .iter()
        .find(|g| g.id == socket.node)?
        .socket_named(kind, &socket.socket)
}

/// Where a particular link meets its input socket.
fn find_link_end<'a>(
    geoms: &'a [geometry::NodeGeometry],
    conn: &Connection,
) -> Option<&'a geometry::SocketGeometry> {
    geoms
        .iter()
        .find(|g| g.id == conn.to.node)?
        .socket_slot(SocketKind::Input, &conn.to.socket, conn.order)
}

/// Whether the cut stroke crosses a wire.
fn cut_crosses_bezier(points: &[Pos2; 4], start: Pos2, end: Pos2) -> bool {
    const SAMPLES: usize = 24;
    let mut previous = points[0];
    for i in 1..=SAMPLES {
        let current = geometry::bezier_point(points, i as f32 / SAMPLES as f32);
        if segments_intersect(previous, current, start, end) {
            return true;
        }
        previous = current;
    }
    false
}

fn segments_intersect(a1: Pos2, a2: Pos2, b1: Pos2, b2: Pos2) -> bool {
    let d = |p: Pos2, q: Pos2, r: Pos2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let d1 = d(b1, b2, a1);
    let d2 = d(b1, b2, a2);
    let d3 = d(a1, a2, b1);
    let d4 = d(a1, a2, b2);
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}
