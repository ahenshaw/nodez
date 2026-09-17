//! The graph model: nodes, connections, and the editing operations that keep
//! them consistent.

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

/// One node instance.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Node {
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
    /// Values for unconnected input sockets, keyed by socket name.
    pub input_values: BTreeMap<String, Value>,
    /// Values for non-socket parameters, keyed by parameter name.
    pub params: BTreeMap<String, Value>,
}

impl Node {
    /// Value of an unconnected input socket.
    pub fn input_value(&self, socket: &str) -> Option<&Value> {
        self.input_values.get(socket)
    }

    pub fn set_input_value(&mut self, socket: impl Into<String>, value: impl Into<Value>) {
        self.input_values.insert(socket.into(), value.into());
    }

    pub fn param(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }

    pub fn set_param(&mut self, name: impl Into<String>, value: impl Into<Value>) {
        self.params.insert(name.into(), value.into());
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
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    connections: BTreeMap<ConnectionId, Connection>,
    /// Back-to-front draw order.
    order: Vec<NodeId>,
    next_node: u64,
    next_connection: u64,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

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

        let mut node = Node {
            id,
            template,
            title: spec.label.clone(),
            position,
            width: spec.width,
            collapsed: false,
            muted: false,
            input_values: BTreeMap::new(),
            params: BTreeMap::new(),
        };
        for socket in &spec.inputs {
            if socket.widget != crate::Widget::None {
                node.input_values
                    .insert(socket.name.clone(), socket.default.clone());
            }
        }
        for param in &spec.params {
            node.params.insert(param.name.clone(), param.default.clone());
        }

        self.nodes.insert(id, node);
        self.order.push(id);
        id
    }

    /// Insert a pre-built node, for instance when pasting. The node is given a
    /// fresh id, which is returned.
    pub fn insert_node(&mut self, mut node: Node) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        node.id = id;
        self.nodes.insert(id, node);
        self.order.push(id);
        id
    }

    /// Remove a node and every connection touching it.
    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
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

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
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
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.nodes.values_mut()
    }

    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    /// Nodes in back-to-front draw order.
    pub fn nodes_in_draw_order(&self) -> impl Iterator<Item = &Node> {
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
    pub fn nodes_of_template(&self, template: TemplateId) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(move |n| n.template == template)
    }

    // ---------------------------------------------------------- connections

    pub fn connections(&self) -> impl Iterator<Item = &Connection> {
        self.connections.values()
    }

    pub fn connection(&self, id: ConnectionId) -> Option<&Connection> {
        self.connections.get(&id)
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

        let id = ConnectionId(self.next_connection);
        self.next_connection += 1;
        self.connections.insert(id, Connection { id, from, to });
        Ok(id)
    }

    pub fn disconnect(&mut self, id: ConnectionId) -> Option<Connection> {
        self.connections.remove(&id)
    }

    /// Remove every wire touching a socket, returning them.
    pub fn disconnect_socket(&mut self, socket: &SocketRef) -> Vec<Connection> {
        let ids: Vec<_> = self
            .connections
            .values()
            .filter(|c| &c.from == socket || &c.to == socket)
            .map(|c| c.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.connections.remove(&id))
            .collect()
    }

    /// Remove every wire touching a node, returning them.
    pub fn disconnect_node(&mut self, node: NodeId) -> Vec<Connection> {
        let ids: Vec<_> = self
            .connections
            .values()
            .filter(|c| c.from.node == node || c.to.node == node)
            .map(|c| c.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.connections.remove(&id))
            .collect()
    }

    // ---------------------------------------------------------- maintenance

    /// Drop nodes whose template is gone and wires whose sockets no longer
    /// exist or no longer typecheck, then fill in any missing values from the
    /// template defaults.
    ///
    /// Call this after loading a saved graph against an edited library.
    pub fn validate(&mut self, library: &NodeLibrary) -> Repairs {
        let mut repairs = Repairs::default();

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
            node.input_values
                .retain(|name, _| tpl.input_spec(name).is_some_and(|s| s.widget != crate::Widget::None));
            for socket in &tpl.inputs {
                if socket.widget != crate::Widget::None {
                    node.input_values
                        .entry(socket.name.clone())
                        .or_insert_with(|| socket.default.clone());
                }
            }
            node.params.retain(|name, _| tpl.param_spec(name).is_some());
            for param in &tpl.params {
                node.params
                    .entry(param.name.clone())
                    .or_insert_with(|| param.default.clone());
            }
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
                },
            );
        }
        mapping
    }

    /// The bounding box of a set of nodes in graph space, using each node's
    /// stored width and the supplied height lookup.
    pub fn bounds(&self, height_of: impl Fn(&Node) -> f32) -> Option<egui::Rect> {
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
