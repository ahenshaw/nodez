//! The graph model: nodes, connections, and the editing operations that keep
//! them consistent.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use egui::{Pos2, pos2};

use crate::template::{NodeLibrary, NodeTemplate, TemplateId};
use crate::types::DataTypeId;
use crate::value::Value;

/// Handle to a node. Ids are never reused within a graph.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(pub u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

/// Handle to a connection. Ids are never reused within a graph.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConnectionId(pub u64);

/// Which side of a node a socket lives on.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SocketKind {
    Input,
    Output,
}

impl SocketKind {
    pub fn is_input(self) -> bool {
        self == Self::Input
    }

    pub fn is_output(self) -> bool {
        self == Self::Output
    }
}

/// Addresses one socket of one node, by the socket's stable name.
///
/// Naming rather than indexing keeps saved graphs valid when a template gains,
/// loses or reorders sockets.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SocketRef {
    pub node: NodeId,
    pub socket: String,
}

impl SocketRef {
    pub fn new(node: NodeId, socket: impl Into<String>) -> Self {
        Self {
            node,
            socket: socket.into(),
        }
    }
}

impl fmt::Display for SocketRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.node, self.socket)
    }
}

impl From<(NodeId, &str)> for SocketRef {
    fn from((node, socket): (NodeId, &str)) -> Self {
        Self::new(node, socket)
    }
}

impl From<(NodeId, String)> for SocketRef {
    fn from((node, socket): (NodeId, String)) -> Self {
        Self::new(node, socket)
    }
}

impl From<&SocketRef> for SocketRef {
    fn from(value: &SocketRef) -> Self {
        value.clone()
    }
}

/// What a graph stores for each node, beyond its position and title.
///
/// [`DynNode`] is the default and the one the editor, serde and
/// [`Graph::validate`] were built around: maps of [`Value`] keyed by socket
/// name. A domain that would rather store its own Rust types implements this
/// trait for them and uses [`Graph<MyNode>`](Graph) instead.
///
/// Values come back as [`Cow`] so that a payload holding real `Value`s can lend
/// them, while one backed by typed fields — which has none to lend — can build
/// one and hand it over. Nobody pays for the other's representation.
pub trait NodeData: Clone {
    /// A fresh instance of a template, carrying its default values.
    fn new(template: &NodeTemplate) -> Self;

    /// The inline value of an input socket, if it has one.
    fn input_value(&self, socket: &str) -> Option<Cow<'_, Value>>;

    fn set_input_value(&mut self, socket: &str, value: Value);

    /// The value of a non-socket parameter.
    fn param(&self, name: &str) -> Option<Cow<'_, Value>>;

    fn set_param(&mut self, name: &str, value: Value);

    /// Bring the payload back in line with a template that has since changed.
    fn reconcile(&mut self, _template: &NodeTemplate) {}
}

/// The default node payload: values keyed by socket and parameter name.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DynNode {
    /// Values for unconnected input sockets, keyed by socket name.
    pub input_values: BTreeMap<String, Value>,
    /// Values for non-socket parameters, keyed by parameter name.
    pub params: BTreeMap<String, Value>,
}

impl NodeData for DynNode {
    fn new(template: &NodeTemplate) -> Self {
        let mut data = Self::default();
        for socket in &template.inputs {
            if socket.widget != crate::Widget::None {
                data.input_values
                    .insert(socket.name.clone(), socket.default.clone());
            }
        }
        for param in &template.params {
            data.params.insert(param.name.clone(), param.default.clone());
        }
        data
    }

    fn input_value(&self, socket: &str) -> Option<Cow<'_, Value>> {
        self.input_values.get(socket).map(Cow::Borrowed)
    }

    fn set_input_value(&mut self, socket: &str, value: Value) {
        self.input_values.insert(socket.to_owned(), value);
    }

    fn param(&self, name: &str) -> Option<Cow<'_, Value>> {
        self.params.get(name).map(Cow::Borrowed)
    }

    fn set_param(&mut self, name: &str, value: Value) {
        self.params.insert(name.to_owned(), value);
    }

    fn reconcile(&mut self, template: &NodeTemplate) {
        self.input_values.retain(|name, _| {
            template
                .input_spec(name)
                .is_some_and(|s| s.widget != crate::Widget::None)
        });
        for socket in &template.inputs {
            if socket.widget != crate::Widget::None {
                self.input_values
                    .entry(socket.name.clone())
                    .or_insert_with(|| socket.default.clone());
            }
        }
        self.params.retain(|name, _| template.param_spec(name).is_some());
        for param in &template.params {
            self.params
                .entry(param.name.clone())
                .or_insert_with(|| param.default.clone());
        }
    }
}

/// One node instance: where it sits, what it is, and its payload.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Node<N = DynNode> {
    pub id: NodeId,
    pub template: TemplateId,
    /// Shown in the header. Starts as the template label and can be renamed.
    pub title: String,
    /// Top-left corner in graph space.
    pub position: Pos2,
    pub width: f32,
    /// Blender's `H`: draw the node as a header-only pill.
    pub collapsed: bool,
    /// Blender's `M`: keep the node but exclude it from evaluation.
    pub muted: bool,
    /// Whatever this domain stores per node.
    pub data: N,
}

impl<N: NodeData> Node<N> {
    /// Value of an unconnected input socket.
    pub fn input_value(&self, socket: &str) -> Option<Cow<'_, Value>> {
        self.data.input_value(socket)
    }

    pub fn set_input_value(&mut self, socket: impl AsRef<str>, value: impl Into<Value>) {
        self.data.set_input_value(socket.as_ref(), value.into());
    }

    pub fn param(&self, name: &str) -> Option<Cow<'_, Value>> {
        self.data.param(name)
    }

    pub fn set_param(&mut self, name: impl AsRef<str>, value: impl Into<Value>) {
        self.data.set_param(name.as_ref(), value.into());
    }
}

/// One wire, always from an output socket to an input socket.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Connection {
    pub id: ConnectionId,
    /// The output socket the wire leaves.
    pub from: SocketRef,
    /// The input socket the wire enters.
    pub to: SocketRef,
    /// Where this link sits among those entering `to`.
    ///
    /// Only meaningful for a multi-input socket, where order is part of the
    /// meaning: the parts of a joined string, the services in a stack. Stored
    /// rather than inferred from creation order, so unplugging a link and
    /// plugging it back does not move it to the end.
    #[cfg_attr(feature = "serde", serde(default))]
    pub order: u32,
    /// Points in graph space the wire bends through, source to target.
    ///
    /// Purely how the wire is drawn. Nothing in traversal or evaluation reads
    /// them, so a routed graph folds to exactly what an unrouted one does.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Vec::is_empty"))]
    pub waypoints: Vec<egui::Pos2>,
}

/// Why a connection was refused.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("no such node: {0}")]
    NoSuchNode(NodeId),
    #[error("node {0} has no template in this library")]
    NoSuchTemplate(NodeId),
    #[error("no output socket named `{socket}` on {node}")]
    NoSuchOutput { node: NodeId, socket: String },
    #[error("no input socket named `{socket}` on {node}")]
    NoSuchInput { node: NodeId, socket: String },
    #[error("a node cannot be wired to itself")]
    SelfLink,
    #[error("incompatible sockets: {from_type} cannot drive {to_type}")]
    TypeMismatch {
        from_type: String,
        to_type: String,
        from: DataTypeId,
        to: DataTypeId,
    },
    #[error("that link would create a cycle")]
    WouldCycle,
    #[error("those sockets are already connected")]
    AlreadyConnected,
}

/// The graph was expected to be acyclic but is not.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("the graph contains a cycle through {}", format_cycle(.0))]
pub struct CycleError(pub Vec<NodeId>);

fn format_cycle(nodes: &[NodeId]) -> String {
    nodes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// A directed acyclic graph of nodes and typed connections.
///
/// The graph enforces its own invariants on every edit: types must be
/// compatible, single-link inputs hold at most one wire, and no edit may
/// introduce a cycle. That means traversal code can rely on the graph being a
/// DAG without re-checking.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Graph<N = DynNode> {
    nodes: BTreeMap<NodeId, Node<N>>,
    connections: BTreeMap<ConnectionId, Connection>,
    /// Back-to-front draw order.
    order: Vec<NodeId>,
    next_node: u64,
    next_connection: u64,
    /// What each [`TemplateId`] in use here is called, filled in by
    /// [`Graph::name_templates`] on the way out to a file.
    ///
    /// A `TemplateId` is a position in a library, and a position only means
    /// anything to the library that handed it out. Saved without these, a
    /// graph is only readable by a library whose templates are registered in
    /// exactly the same order — which rules out a library that can gain a
    /// template, and so rules out reusable node groups.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "BTreeMap::is_empty")
    )]
    templates: BTreeMap<u32, String>,
}

impl<N> Default for Graph<N> {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            connections: BTreeMap::new(),
            order: Vec::new(),
            next_node: 0,
            next_connection: 0,
            templates: BTreeMap::new(),
        }
    }
}

impl Graph<DynNode> {
    /// An empty graph with the default payload.
    ///
    /// A default type parameter does not take part in inference at a call site
    /// like `Graph::new()`, so this constructor is pinned to [`DynNode`]. A
    /// graph with its own payload is built with `Graph::<MyNode>::default()`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<N: NodeData> Graph<N> {
    // ---------------------------------------------------------------- nodes

    /// Add a node of the given template at `position`, filling in the
    /// template's default socket and parameter values.
    pub fn add_node(
        &mut self,
        library: &NodeLibrary,
        template: TemplateId,
        position: Pos2,
    ) -> NodeId {
        let spec = library.expect(template);
        let id = NodeId(self.next_node);
        self.next_node += 1;

        let node = Node {
            id,
            template,
            title: spec.label.clone(),
            position,
            width: spec.width,
            collapsed: false,
            muted: false,
            data: N::new(spec),
        };

        self.nodes.insert(id, node);
        self.order.push(id);
        id
    }

    /// Insert a pre-built node, for instance when pasting. The node is given a
    /// fresh id, which is returned.
    pub fn insert_node(&mut self, mut node: Node<N>) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        node.id = id;
        self.nodes.insert(id, node);
        self.order.push(id);
        id
    }

    /// Remove a node and every connection touching it.
    pub fn remove_node(&mut self, id: NodeId) -> Option<Node<N>> {
        let node = self.nodes.remove(&id)?;
        self.connections
            .retain(|_, c| c.from.node != id && c.to.node != id);
        self.order.retain(|&n| n != id);
        Some(node)
    }

    /// Copy a node, including its values, offset by `offset`.
    pub fn duplicate_node(&mut self, id: NodeId, offset: egui::Vec2) -> Option<NodeId> {
        let mut node = self.nodes.get(&id)?.clone();
        node.position += offset;
        Some(self.insert_node(node))
    }

    pub fn node(&self, id: NodeId) -> Option<&Node<N>> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node<N>> {
        self.nodes.get_mut(&id)
    }

    pub fn contains_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Nodes in creation order.
    pub fn nodes(&self) -> impl Iterator<Item = &Node<N>> {
        self.nodes.values()
    }

    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut Node<N>> {
        self.nodes.values_mut()
    }

    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    /// Nodes in back-to-front draw order.
    pub fn nodes_in_draw_order(&self) -> impl Iterator<Item = &Node<N>> {
        self.order.iter().filter_map(|id| self.nodes.get(id))
    }

    /// Move a node to the front of the draw order.
    pub fn raise_node(&mut self, id: NodeId) {
        if let Some(pos) = self.order.iter().position(|&n| n == id) {
            let id = self.order.remove(pos);
            self.order.push(id);
        }
    }

    /// The template backing a node.
    pub fn template_of<'a>(&self, library: &'a NodeLibrary, id: NodeId) -> Option<&'a NodeTemplate> {
        library.get(self.nodes.get(&id)?.template)
    }

    /// Every node using the given template.
    pub fn nodes_of_template(&self, template: TemplateId) -> impl Iterator<Item = &Node<N>> {
        self.nodes.values().filter(move |n| n.template == template)
    }

    // ---------------------------------------------------------- connections

    pub fn connections(&self) -> impl Iterator<Item = &Connection> {
        self.connections.values()
    }

    pub fn connection(&self, id: ConnectionId) -> Option<&Connection> {
        self.connections.get(&id)
    }

    /// Straighten every wire, dropping whatever routing they carry.
    ///
    /// The other half of [`crate::route_links`]: this is how a graph goes back
    /// to plain noodles. Returns how many wires were carrying a bend.
    pub fn clear_routing(&mut self) -> usize {
        let mut cleared = 0;
        for conn in self.connections.values_mut() {
            if !conn.waypoints.is_empty() {
                conn.waypoints.clear();
                cleared += 1;
            }
        }
        cleared
    }

    /// Mutable access to one connection, for editing how its wire is drawn.
    ///
    /// The endpoints are the graph's own business; change those through
    /// [`Graph::connect`] and [`Graph::disconnect`] so its bookkeeping keeps up.
    pub fn connection_mut(&mut self, id: ConnectionId) -> Option<&mut Connection> {
        self.connections.get_mut(&id)
    }

    /// Check a prospective connection without performing it.
    ///
    /// Note that an existing wire on a single-link input is *not* an error:
    /// [`Graph::connect`] replaces it, exactly as Blender does.
    pub fn can_connect(
        &self,
        library: &NodeLibrary,
        from: &SocketRef,
        to: &SocketRef,
    ) -> Result<(), ConnectError> {
        if from.node == to.node {
            return Err(ConnectError::SelfLink);
        }
        let from_node = self
            .nodes
            .get(&from.node)
            .ok_or(ConnectError::NoSuchNode(from.node))?;
        let to_node = self
            .nodes
            .get(&to.node)
            .ok_or(ConnectError::NoSuchNode(to.node))?;
        let from_tpl = library
            .get(from_node.template)
            .ok_or(ConnectError::NoSuchTemplate(from.node))?;
        let to_tpl = library
            .get(to_node.template)
            .ok_or(ConnectError::NoSuchTemplate(to.node))?;

        let out = from_tpl
            .output_spec(&from.socket)
            .ok_or_else(|| ConnectError::NoSuchOutput {
                node: from.node,
                socket: from.socket.clone(),
            })?;
        let inp = to_tpl
            .input_spec(&to.socket)
            .ok_or_else(|| ConnectError::NoSuchInput {
                node: to.node,
                socket: to.socket.clone(),
            })?;

        if !library.types.compatible(out.ty, inp.ty) {
            return Err(ConnectError::TypeMismatch {
                from_type: library.types.name(out.ty).to_owned(),
                to_type: library.types.name(inp.ty).to_owned(),
                from: out.ty,
                to: inp.ty,
            });
        }

        if self
            .connections
            .values()
            .any(|c| &c.from == from && &c.to == to)
        {
            return Err(ConnectError::AlreadyConnected);
        }

        if self.reaches(to.node, from.node) {
            return Err(ConnectError::WouldCycle);
        }

        Ok(())
    }

    /// Wire an output socket to an input socket.
    ///
    /// If the input is single-link and already wired, the old wire is removed
    /// first. Returns the new connection's id.
    pub fn connect(
        &mut self,
        library: &NodeLibrary,
        from: impl Into<SocketRef>,
        to: impl Into<SocketRef>,
    ) -> Result<ConnectionId, ConnectError> {
        let from = from.into();
        let to = to.into();
        self.can_connect(library, &from, &to)?;

        let multi = self
            .nodes
            .get(&to.node)
            .and_then(|n| library.get(n.template))
            .and_then(|t| t.input_spec(&to.socket))
            .is_some_and(|s| s.multi);
        if !multi {
            self.connections.retain(|_, c| c.to != to);
        }

        let order = self.links_into(to.node, &to.socket).count() as u32;
        Ok(self.insert_connection(from, to, order))
    }

    /// Wire an output to an input at a given position among that input's links.
    ///
    /// Links at or after `index` shift down. Out-of-range indices append.
    pub fn connect_at(
        &mut self,
        library: &NodeLibrary,
        from: impl Into<SocketRef>,
        to: impl Into<SocketRef>,
        index: u32,
    ) -> Result<ConnectionId, ConnectError> {
        let from = from.into();
        let to = to.into();
        self.can_connect(library, &from, &to)?;

        let multi = self.is_multi(library, &to);
        if !multi {
            self.connections.retain(|_, c| c.to != to);
            return Ok(self.insert_connection(from, to, 0));
        }

        let index = index.min(self.links_into(to.node, &to.socket).count() as u32);
        for conn in self.connections.values_mut() {
            if conn.to == to && conn.order >= index {
                conn.order += 1;
            }
        }
        Ok(self.insert_connection(from, to, index))
    }

    /// Move a link to a different position among its socket's links.
    ///
    /// Returns false if there is no such link. Other links close up or make
    /// room around it.
    pub fn reorder_link(&mut self, link: ConnectionId, index: u32) -> bool {
        let Some(conn) = self.connections.get(&link) else {
            return false;
        };
        let (to, from_index) = (conn.to.clone(), conn.order);
        let last = self
            .links_into(to.node, &to.socket)
            .count()
            .saturating_sub(1) as u32;
        let index = index.min(last);
        if index == from_index {
            return true;
        }

        for conn in self.connections.values_mut() {
            if conn.to != to || conn.id == link {
                continue;
            }
            if from_index < index && (from_index + 1..=index).contains(&conn.order) {
                conn.order -= 1;
            } else if index < from_index && (index..from_index).contains(&conn.order) {
                conn.order += 1;
            }
        }
        if let Some(conn) = self.connections.get_mut(&link) {
            conn.order = index;
        }
        true
    }

    fn insert_connection(&mut self, from: SocketRef, to: SocketRef, order: u32) -> ConnectionId {
        let id = ConnectionId(self.next_connection);
        self.next_connection += 1;
        self.connections.insert(
            id,
            Connection {
                id,
                from,
                to,
                order,
                waypoints: Vec::new(),
            },
        );
        id
    }

    fn is_multi(&self, library: &NodeLibrary, socket: &SocketRef) -> bool {
        self.nodes
            .get(&socket.node)
            .and_then(|n| library.get(n.template))
            .and_then(|t| t.input_spec(&socket.socket))
            .is_some_and(|s| s.multi)
    }

    /// Close up the gaps left in a socket's ordering after a link is removed.
    fn compact_orders(&mut self, sockets: &[SocketRef]) {
        for socket in sockets {
            let mut links: Vec<(ConnectionId, u32)> = self
                .connections
                .values()
                .filter(|c| &c.to == socket)
                .map(|c| (c.id, c.order))
                .collect();
            links.sort_by_key(|(id, order)| (*order, *id));
            for (position, (id, _)) in links.into_iter().enumerate() {
                if let Some(conn) = self.connections.get_mut(&id) {
                    conn.order = position as u32;
                }
            }
        }
    }

    pub fn disconnect(&mut self, id: ConnectionId) -> Option<Connection> {
        let removed = self.connections.remove(&id)?;
        self.compact_orders(std::slice::from_ref(&removed.to));
        Some(removed)
    }

    /// Remove every wire touching a socket, returning them.
    pub fn disconnect_socket(&mut self, socket: &SocketRef) -> Vec<Connection> {
        let ids: Vec<_> = self
            .connections
            .values()
            .filter(|c| &c.from == socket || &c.to == socket)
            .map(|c| c.id)
            .collect();
        let removed: Vec<Connection> = ids
            .into_iter()
            .filter_map(|id| self.connections.remove(&id))
            .collect();
        let touched: Vec<SocketRef> = removed.iter().map(|c| c.to.clone()).collect();
        self.compact_orders(&touched);
        removed
    }

    /// Remove every wire touching a node, returning them.
    pub fn disconnect_node(&mut self, node: NodeId) -> Vec<Connection> {
        let ids: Vec<_> = self
            .connections
            .values()
            .filter(|c| c.from.node == node || c.to.node == node)
            .map(|c| c.id)
            .collect();
        let removed: Vec<Connection> = ids
            .into_iter()
            .filter_map(|id| self.connections.remove(&id))
            .collect();
        let touched: Vec<SocketRef> = removed.iter().map(|c| c.to.clone()).collect();
        self.compact_orders(&touched);
        removed
    }

    // ---------------------------------------------------------- maintenance

    /// Drop nodes whose template is gone and wires whose sockets no longer
    /// exist or no longer typecheck, then fill in any missing values from the
    /// template defaults.
    ///
    /// Call this after loading a saved graph against an edited library.
    /// Record what every template in use here is called, so the graph can be
    /// read back by a library that has since gained or lost templates.
    ///
    /// Call it before serializing; [`Graph::validate`] is what reads the
    /// names again, and clears them once it has. `EditorApp`'s JSON files do
    /// both for you.
    pub fn name_templates(&mut self, library: &NodeLibrary) {
        self.templates = self
            .nodes
            .values()
            .filter_map(|node| {
                let template = library.get(node.template)?;
                Some((node.template.0, template.id.clone()))
            })
            .collect();
    }

    pub fn validate(&mut self, library: &NodeLibrary) -> Repairs {
        let mut repairs = Repairs::default();

        // First, what the ids in this graph were called when it was written.
        // A file from a library with different templates, or the same ones in
        // a different order, is readable exactly as far as the names match.
        if !self.templates.is_empty() {
            let named = std::mem::take(&mut self.templates);
            for node in self.nodes.values_mut() {
                let Some(name) = named.get(&node.template.0) else {
                    continue;
                };
                // Left alone when the name is gone: the stale sweep below is
                // what removes it, and it already knows how to say so.
                if let Some(now) = library.id(name) {
                    node.template = now;
                }
            }
        }

        let stale: Vec<_> = self
            .nodes
            .values()
            .filter(|n| library.get(n.template).is_none())
            .map(|n| n.id)
            .collect();
        for id in stale {
            self.remove_node(id);
            repairs.removed_nodes += 1;
        }

        for node in self.nodes.values_mut() {
            let Some(tpl) = library.get(node.template) else {
                continue;
            };
            node.data.reconcile(tpl);
        }

        let bad: Vec<_> = self
            .connections
            .values()
            .filter(|c| {
                let ok = self
                    .nodes
                    .get(&c.from.node)
                    .and_then(|n| library.get(n.template))
                    .and_then(|t| t.output_spec(&c.from.socket))
                    .zip(
                        self.nodes
                            .get(&c.to.node)
                            .and_then(|n| library.get(n.template))
                            .and_then(|t| t.input_spec(&c.to.socket)),
                    )
                    .is_some_and(|(o, i)| library.types.compatible(o.ty, i.ty));
                !ok
            })
            .map(|c| c.id)
            .collect();
        for id in bad {
            self.connections.remove(&id);
            repairs.removed_connections += 1;
        }

        // Orders may have gaps or duplicates after a repair, or come from a
        // file written before they were stored at all.
        let sockets: Vec<SocketRef> = {
            let mut seen: Vec<SocketRef> = Vec::new();
            for conn in self.connections.values() {
                if !seen.contains(&conn.to) {
                    seen.push(conn.to.clone());
                }
            }
            seen
        };
        self.compact_orders(&sockets);

        // Re-establish the draw order over exactly the surviving nodes.
        self.order.retain(|id| self.nodes.contains_key(id));
        for id in self.nodes.keys() {
            if !self.order.contains(id) {
                self.order.push(*id);
            }
        }

        // Saved ids must not be handed out again.
        self.next_node = self
            .nodes
            .keys()
            .map(|n| n.0 + 1)
            .max()
            .unwrap_or(0)
            .max(self.next_node);
        self.next_connection = self
            .connections
            .keys()
            .map(|c| c.0 + 1)
            .max()
            .unwrap_or(0)
            .max(self.next_connection);

        repairs
    }

    /// Copy a set of nodes, keeping the wires that run between them.
    ///
    /// Returns a map from original id to copy id.
    /// Copy every node and wire of another graph into this one, offset, and
    /// say where each of its nodes landed.
    ///
    /// Unlike [`Graph::duplicate_subgraph`] the source is a different graph,
    /// so nothing here needs to be looked up against a library: the wires
    /// being copied were valid where they came from and connect the same two
    /// sockets when they arrive.
    pub fn absorb(&mut self, other: &Graph<N>, offset: egui::Vec2) -> HashMap<NodeId, NodeId>
    where
        N: Clone,
    {
        let mut mapping = HashMap::new();
        for id in other.nodes.keys().copied() {
            let Some(node) = other.nodes.get(&id) else {
                continue;
            };
            let mut copy = node.clone();
            copy.position += offset;
            mapping.insert(id, self.insert_node(copy));
        }
        for link in other.connections.values() {
            let (Some(&from), Some(&to)) =
                (mapping.get(&link.from.node), mapping.get(&link.to.node))
            else {
                continue;
            };
            self.insert_connection(
                SocketRef::new(from, link.from.socket.clone()),
                SocketRef::new(to, link.to.socket.clone()),
                link.order,
            );
        }
        mapping
    }

    /// The same graph carrying a different payload.
    ///
    /// A payload is values keyed by the names in a template, and [`NodeData`]
    /// is the whole of how they are read and written — so a graph can be
    /// moved from one payload to another by copying what its templates name.
    /// Node and connection ids come across unchanged, which is what lets a
    /// selection survive the trip.
    ///
    /// What a payload keeps that its template does not mention does not come
    /// across. That is the same bargain serialization makes.
    pub fn convert<M: NodeData>(&self, library: &NodeLibrary) -> Graph<M> {
        let mut out = Graph::<M> {
            next_node: self.next_node,
            next_connection: self.next_connection,
            connections: self.connections.clone(),
            templates: self.templates.clone(),
            order: self.order.clone(),
            ..Default::default()
        };
        for (id, node) in &self.nodes {
            let Some(template) = library.get(node.template) else {
                continue;
            };
            let mut data = M::new(template);
            for socket in &template.inputs {
                if let Some(value) = node.data.input_value(&socket.name) {
                    data.set_input_value(&socket.name, value.into_owned());
                }
            }
            for param in &template.params {
                if let Some(value) = node.data.param(&param.name) {
                    data.set_param(&param.name, value.into_owned());
                }
            }
            out.nodes.insert(
                *id,
                Node {
                    id: *id,
                    template: node.template,
                    title: node.title.clone(),
                    position: node.position,
                    width: node.width,
                    collapsed: node.collapsed,
                    muted: node.muted,
                    data,
                },
            );
        }
        out
    }

    /// Join two sockets without asking a library whether they may be joined.
    ///
    /// For rewiring links that were already valid — putting a group's
    /// interior back into the graph — where the answer is known and the
    /// library has nothing to add.
    pub(crate) fn rejoin(&mut self, from: SocketRef, to: SocketRef, order: u32) -> ConnectionId {
        self.insert_connection(from, to, order)
    }

    pub fn duplicate_subgraph(
        &mut self,
        nodes: &HashSet<NodeId>,
        offset: egui::Vec2,
    ) -> HashMap<NodeId, NodeId> {
        let mut mapping = HashMap::new();
        let mut sources: Vec<_> = nodes.iter().copied().collect();
        sources.sort_unstable();
        for id in sources {
            if let Some(new_id) = self.duplicate_node(id, offset) {
                mapping.insert(id, new_id);
            }
        }

        let internal: Vec<_> = self
            .connections
            .values()
            .filter(|c| nodes.contains(&c.from.node) && nodes.contains(&c.to.node))
            .cloned()
            .collect();
        for conn in internal {
            let (Some(&from), Some(&to)) =
                (mapping.get(&conn.from.node), mapping.get(&conn.to.node))
            else {
                continue;
            };
            let id = ConnectionId(self.next_connection);
            self.next_connection += 1;
            self.connections.insert(
                id,
                Connection {
                    id,
                    from: SocketRef::new(from, conn.from.socket.clone()),
                    to: SocketRef::new(to, conn.to.socket.clone()),
                    order: conn.order,
                    // The copy sits at an offset, so its bends do too.
                    waypoints: conn.waypoints.iter().map(|p| *p + offset).collect(),
                },
            );
        }
        mapping
    }

    /// The bounding box of a set of nodes in graph space, using each node's
    /// stored width and the supplied height lookup.
    pub fn bounds(&self, height_of: impl Fn(&Node<N>) -> f32) -> Option<egui::Rect> {
        let mut bounds: Option<egui::Rect> = None;
        for node in self.nodes.values() {
            let rect = egui::Rect::from_min_size(
                node.position,
                egui::vec2(node.width, height_of(node)),
            );
            bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
        }
        bounds
    }

    /// Shift every node by `delta`.
    pub fn translate(&mut self, delta: egui::Vec2) {
        for node in self.nodes.values_mut() {
            node.position += delta;
        }
    }

    /// Snap every node position to a grid.
    pub fn snap_to_grid(&mut self, spacing: f32) {
        for node in self.nodes.values_mut() {
            node.position = pos2(
                (node.position.x / spacing).round() * spacing,
                (node.position.y / spacing).round() * spacing,
            );
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.connections.clear();
        self.order.clear();
        self.templates.clear();
    }

    /// Whether `target` is reachable from `start` by following outputs.
    pub(crate) fn reaches(&self, start: NodeId, target: NodeId) -> bool {
        if start == target {
            return true;
        }
        let mut seen = HashSet::from([start]);
        let mut stack = vec![start];
        while let Some(current) = stack.pop() {
            for conn in self.connections.values() {
                if conn.from.node != current {
                    continue;
                }
                if conn.to.node == target {
                    return true;
                }
                if seen.insert(conn.to.node) {
                    stack.push(conn.to.node);
                }
            }
        }
        false
    }
}

/// What [`Graph::validate`] had to throw away.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Repairs {
    pub removed_nodes: usize,
    pub removed_connections: usize,
}

impl Repairs {
    pub fn is_clean(self) -> bool {
        self.removed_nodes == 0 && self.removed_connections == 0
    }
}
