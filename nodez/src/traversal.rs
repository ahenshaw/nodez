//! Iterators and folds for walking the graph.
//!
//! Everything here is read-only and works on any [`Graph`]; nothing in this
//! module needs the editor to be running. This is the half of the crate you use
//! to turn a graph into a config file.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::{Connection, CycleError, DynNode, Graph, Node, NodeData, NodeId, SocketRef};
use crate::template::{NodeLibrary, NodeTemplate};
use crate::value::Value;

/// Which way to walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Toward the inputs: the nodes this one depends on.
    Upstream,
    /// Toward the outputs: the nodes that depend on this one.
    Downstream,
}

impl Direction {
    pub fn reversed(self) -> Self {
        match self {
            Self::Upstream => Self::Downstream,
            Self::Downstream => Self::Upstream,
        }
    }
}

/// What is feeding one input socket.
#[derive(Clone, Debug)]
pub enum InputSource<'a> {
    /// Nothing is wired in; this is the socket's inline value.
    Literal(Cow<'a, Value>),
    /// One or more wires feed this socket, in connection order.
    Linked(Vec<&'a Connection>),
    /// Nothing is wired in and the socket has no inline value.
    Unset,
}

impl<'a> InputSource<'a> {
    pub fn literal(&self) -> Option<&Value> {
        match self {
            Self::Literal(v) => Some(v),
            _ => None,
        }
    }

    pub fn links(&self) -> &[&'a Connection] {
        match self {
            Self::Linked(links) => links,
            _ => &[],
        }
    }

    pub fn is_linked(&self) -> bool {
        matches!(self, Self::Linked(_))
    }
}

impl<N: NodeData> Graph<N> {
    // ------------------------------------------------------ socket queries

    /// Every wire arriving at a node, in connection order.
    pub fn incoming(&self, node: NodeId) -> impl Iterator<Item = &Connection> {
        self.connections().filter(move |c| c.to.node == node)
    }

    /// Every wire leaving a node, in connection order.
    pub fn outgoing(&self, node: NodeId) -> impl Iterator<Item = &Connection> {
        self.connections().filter(move |c| c.from.node == node)
    }

    /// Every wire arriving at one input socket, in the socket's own order.
    ///
    /// For a multi-input that order is the user's: see [`Connection::order`].
    pub fn links_into(&self, node: NodeId, socket: &str) -> impl Iterator<Item = &Connection> {
        let mut links: Vec<&Connection> = self
            .connections()
            .filter(|c| c.to.node == node && c.to.socket == socket)
            .collect();
        links.sort_by_key(|c| (c.order, c.id.0));
        links.into_iter()
    }

    /// Every wire leaving one output socket.
    pub fn links_from<'a>(
        &'a self,
        node: NodeId,
        socket: &'a str,
    ) -> impl Iterator<Item = &'a Connection> {
        self.connections()
            .filter(move |c| c.from.node == node && c.from.socket == socket)
    }

    /// The single wire arriving at an input socket, if any. For multi-inputs
    /// this is the first one; use [`Graph::links_into`] for all of them.
    pub fn link_into(&self, node: NodeId, socket: &str) -> Option<&Connection> {
        self.connections()
            .find(|c| c.to.node == node && c.to.socket == socket)
    }

    /// The output socket feeding an input socket, if any.
    pub fn source_of(&self, node: NodeId, socket: &str) -> Option<&SocketRef> {
        self.link_into(node, socket).map(|c| &c.from)
    }

    pub fn is_input_linked(&self, node: NodeId, socket: &str) -> bool {
        self.connections()
            .any(|c| c.to.node == node && c.to.socket == socket)
    }

    pub fn is_output_linked(&self, node: NodeId, socket: &str) -> bool {
        self.connections()
            .any(|c| c.from.node == node && c.from.socket == socket)
    }

    /// Whether one input has to be wired and is not.
    ///
    /// Which inputs have to be wired is the schema's to say — see
    /// [`crate::SocketSpec::required`] — and this asks the graph whether they are.
    pub fn is_input_missing(&self, library: &NodeLibrary, node: NodeId, socket: &str) -> bool {
        let Some(node_) = self.node(node) else {
            return false;
        };
        // A muted node is deliberately switched off. Nothing about it is
        // unfinished, and saying so would put a mark on every node somebody
        // parked while they worked on something else.
        if node_.muted {
            return false;
        }
        self.template_of(library, node)
            .and_then(|template| template.input_spec(socket))
            .is_some_and(|spec| spec.required() && !self.is_input_linked(node, socket))
    }

    /// Every input in the graph that has to be wired and is not.
    ///
    /// The same fact the editor marks up, so a generator can refuse a graph
    /// for the reason the editor is already showing rather than for one of
    /// its own.
    pub fn missing_inputs(&self, library: &NodeLibrary) -> Vec<SocketRef> {
        let mut missing = Vec::new();
        for node in self.nodes() {
            if node.muted {
                continue;
            }
            let Some(template) = self.template_of(library, node.id) else {
                continue;
            };
            for socket in template.inputs.iter().filter(|s| s.required()) {
                if !self.is_input_linked(node.id, &socket.name) {
                    missing.push(SocketRef::new(node.id, socket.name.clone()));
                }
            }
        }
        missing
    }

    /// Resolve an input socket to either its inline value or its incoming wires.
    pub fn input_source<'a>(
        &'a self,
        library: &'a NodeLibrary,
        node: NodeId,
        socket: &str,
    ) -> InputSource<'a> {
        let links: Vec<_> = self.links_into(node, socket).collect();
        if !links.is_empty() {
            return InputSource::Linked(links);
        }
        let Some(node) = self.node(node) else {
            return InputSource::Unset;
        };
        if let Some(value) = node.input_value(socket) {
            return InputSource::Literal(value);
        }
        match library
            .get(node.template)
            .and_then(|t| t.input_spec(socket))
            .map(|s| &s.default)
        {
            Some(default) if !default.is_null() => InputSource::Literal(Cow::Borrowed(default)),
            _ => InputSource::Unset,
        }
    }

    /// The effective value of a node parameter, falling back to the template
    /// default when the node has no stored value.
    pub fn param<'a>(
        &'a self,
        library: &'a NodeLibrary,
        node: NodeId,
        name: &str,
    ) -> Option<Cow<'a, Value>> {
        let node = self.node(node)?;
        node.param(name).or_else(|| {
            library
                .get(node.template)
                .and_then(|t| t.param_spec(name))
                .map(|p| Cow::Borrowed(&p.default))
        })
    }

    // --------------------------------------------------- adjacency queries

    /// Immediate upstream nodes, deduplicated, in connection order.
    pub fn predecessors(&self, node: NodeId) -> Vec<NodeId> {
        dedup(self.incoming(node).map(|c| c.from.node))
    }

    /// Immediate downstream nodes, deduplicated, in connection order.
    pub fn successors(&self, node: NodeId) -> Vec<NodeId> {
        dedup(self.outgoing(node).map(|c| c.to.node))
    }

    /// Immediate neighbors in the given direction.
    pub fn neighbors(&self, node: NodeId, direction: Direction) -> Vec<NodeId> {
        match direction {
            Direction::Upstream => self.predecessors(node),
            Direction::Downstream => self.successors(node),
        }
    }

    /// Nodes with no incoming wires: the leaves of the dependency tree, where
    /// evaluation starts.
    pub fn roots(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_ids()
            .filter(move |&id| self.incoming(id).next().is_none())
    }

    /// Nodes with no outgoing wires: the results of the graph.
    pub fn sinks(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_ids()
            .filter(move |&id| self.outgoing(id).next().is_none())
    }

    /// Nodes wired to nothing at all.
    pub fn isolated(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_ids().filter(move |&id| {
            self.incoming(id).next().is_none() && self.outgoing(id).next().is_none()
        })
    }

    pub fn in_degree(&self, node: NodeId) -> usize {
        self.incoming(node).count()
    }

    pub fn out_degree(&self, node: NodeId) -> usize {
        self.outgoing(node).count()
    }

    // ------------------------------------------------------------- walking

    /// Breadth-first walk from `start`, not including `start` itself.
    ///
    /// The same node is yielded at most once.
    pub fn walk(&self, start: NodeId, direction: Direction) -> Walk<'_, N> {
        let mut queue = VecDeque::new();
        let mut seen = HashSet::new();
        seen.insert(start);
        for next in self.neighbors(start, direction) {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
        Walk {
            graph: self,
            direction,
            queue,
            seen,
        }
    }

    /// Every node this one transitively depends on.
    pub fn ancestors(&self, node: NodeId) -> Walk<'_, N> {
        self.walk(node, Direction::Upstream)
    }

    /// Every node that transitively depends on this one.
    pub fn descendants(&self, node: NodeId) -> Walk<'_, N> {
        self.walk(node, Direction::Downstream)
    }

    /// Whether `target` transitively depends on `node`.
    pub fn depends_on(&self, node: NodeId, target: NodeId) -> bool {
        self.ancestors(target).any(|id| id == node)
    }

    /// The weakly connected component containing `node`, including `node`.
    pub fn component_of(&self, node: NodeId) -> HashSet<NodeId> {
        let mut seen = HashSet::from([node]);
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            let neighbors = self
                .predecessors(current)
                .into_iter()
                .chain(self.successors(current));
            for next in neighbors {
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        seen
    }

    /// Every weakly connected component, each sorted by node id.
    pub fn components(&self) -> Vec<Vec<NodeId>> {
        let mut assigned: HashSet<NodeId> = HashSet::new();
        let mut out = Vec::new();
        for id in self.node_ids() {
            if assigned.contains(&id) {
                continue;
            }
            let component = self.component_of(id);
            assigned.extend(component.iter().copied());
            let mut ids: Vec<_> = component.into_iter().collect();
            ids.sort_unstable();
            out.push(ids);
        }
        out
    }

    // ---------------------------------------------------- topological order

    /// Every node, ordered so each comes after everything it depends on.
    ///
    /// Ties are broken by node id, so the order is stable across runs.
    pub fn topological_order(&self) -> Result<Vec<NodeId>, CycleError> {
        self.topological_subset(self.node_ids().collect())
    }

    /// [`Graph::topological_order`] as an iterator.
    pub fn iter_topological(&self) -> Result<Topological<'_, N>, CycleError> {
        Ok(Topological {
            graph: self,
            order: self.topological_order()?.into_iter(),
        })
    }

    /// `node` and everything it transitively depends on, in evaluation order,
    /// with `node` last.
    pub fn dependency_order(&self, node: NodeId) -> Result<Vec<NodeId>, CycleError> {
        let mut subset: HashSet<NodeId> = self.ancestors(node).collect();
        subset.insert(node);
        self.topological_subset(subset)
    }

    /// Topologically order an arbitrary subset, ignoring wires that leave it.
    pub fn topological_subset(&self, subset: HashSet<NodeId>) -> Result<Vec<NodeId>, CycleError> {
        let mut in_degree: HashMap<NodeId, usize> =
            subset.iter().map(|&id| (id, 0usize)).collect();
        let mut edges: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for conn in self.connections() {
            if !subset.contains(&conn.from.node) || !subset.contains(&conn.to.node) {
                continue;
            }
            edges
                .entry(conn.from.node)
                .or_default()
                .push(conn.to.node);
            *in_degree.entry(conn.to.node).or_default() += 1;
        }

        // A sorted ready-set keeps the output deterministic.
        let mut ready: Vec<NodeId> = in_degree
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();
        ready.sort_unstable();

        let mut out = Vec::with_capacity(subset.len());
        while let Some(id) = ready.pop() {
            out.push(id);
            if let Some(targets) = edges.get(&id) {
                let mut newly_ready = Vec::new();
                for &target in targets {
                    let degree = in_degree.get_mut(&target).expect("target is in subset");
                    *degree -= 1;
                    if *degree == 0 {
                        newly_ready.push(target);
                    }
                }
                ready.extend(newly_ready);
                ready.sort_unstable();
                ready.dedup();
            }
        }

        if out.len() == subset.len() {
            Ok(out)
        } else {
            let remaining: HashSet<_> = subset.difference(&out.iter().copied().collect()).copied().collect();
            Err(CycleError(self.trace_cycle(&remaining)))
        }
    }

    /// A cycle in the graph, if there is one. The graph's editing operations
    /// refuse to create cycles, so this only finds ones built by hand.
    pub fn find_cycle(&self) -> Option<Vec<NodeId>> {
        self.topological_order().err().map(|CycleError(c)| c)
    }

    pub fn is_acyclic(&self) -> bool {
        self.topological_order().is_ok()
    }

    /// Longest path from any root, per node. Useful for laying a graph out in
    /// columns. Returns `Err` if the graph has a cycle.
    pub fn depths(&self) -> Result<HashMap<NodeId, usize>, CycleError> {
        let mut depths = HashMap::new();
        for id in self.topological_order()? {
            let depth = self
                .predecessors(id)
                .into_iter()
                .filter_map(|p| depths.get(&p).copied())
                .max()
                .map_or(0, |d: usize| d + 1);
            depths.insert(id, depth);
        }
        Ok(depths)
    }

    /// Walk a cycle inside `remaining` so the error can name it.
    fn trace_cycle(&self, remaining: &HashSet<NodeId>) -> Vec<NodeId> {
        let Some(&start) = remaining.iter().min() else {
            return Vec::new();
        };
        let mut path = vec![start];
        let mut seen = HashSet::from([start]);
        let mut current = start;
        loop {
            let next = self
                .successors(current)
                .into_iter()
                .find(|n| remaining.contains(n));
            let Some(next) = next else { return path };
            if !seen.insert(next) {
                // Trim the tail leading into the loop.
                if let Some(pos) = path.iter().position(|&n| n == next) {
                    path.drain(..pos);
                }
                path.push(next);
                return path;
            }
            path.push(next);
            current = next;
        }
    }

    // ---------------------------------------------------------- evaluation

    /// Fold the graph into a value per node, in dependency order.
    ///
    /// `f` is called once per node, after all of its upstream nodes, and is
    /// handed an [`EvalContext`] holding those upstream results. This is the
    /// building block for turning a graph into text, config or a runtime
    /// structure.
    ///
    /// ```no_run
    /// # use nodez::{Graph, NodeLibrary};
    /// # fn demo(graph: &Graph, library: &NodeLibrary) {
    /// let rendered = graph.evaluate_all::<String, std::convert::Infallible>(library, |ctx| {
    ///     let args: Vec<&String> = ctx.inputs("args").into_iter().map(|l| l.value).collect();
    ///     Ok(format!("{}({})", ctx.template().id, args.len()))
    /// });
    /// # }
    /// ```
    pub fn evaluate_all<T, E>(
        &self,
        library: &NodeLibrary,
        mut f: impl FnMut(EvalContext<'_, T, N>) -> Result<T, E>,
    ) -> Result<HashMap<NodeId, T>, EvalError<E>> {
        let order = self.topological_order().map_err(EvalError::Cycle)?;
        self.evaluate_order(library, &order, &mut f)
    }

    /// Evaluate only what `target` needs, and return its value.
    pub fn evaluate<T, E>(
        &self,
        library: &NodeLibrary,
        target: NodeId,
        mut f: impl FnMut(EvalContext<'_, T, N>) -> Result<T, E>,
    ) -> Result<T, EvalError<E>> {
        if !self.contains_node(target) {
            return Err(EvalError::MissingNode(target));
        }
        let order = self.dependency_order(target).map_err(EvalError::Cycle)?;
        let mut results = self.evaluate_order(library, &order, &mut f)?;
        results.remove(&target).ok_or(EvalError::MissingNode(target))
    }

    fn evaluate_order<T, E>(
        &self,
        library: &NodeLibrary,
        order: &[NodeId],
        f: &mut impl FnMut(EvalContext<'_, T, N>) -> Result<T, E>,
    ) -> Result<HashMap<NodeId, T>, EvalError<E>> {
        let mut results: HashMap<NodeId, T> = HashMap::with_capacity(order.len());
        for &id in order {
            let node = self.node(id).ok_or(EvalError::MissingNode(id))?;
            let template = library
                .get(node.template)
                .ok_or(EvalError::MissingTemplate(id))?;
            let ctx = EvalContext {
                graph: self,
                library,
                node,
                template,
                results: &results,
            };
            let value = f(ctx).map_err(|source| EvalError::Node { node: id, source })?;
            results.insert(id, value);
        }
        Ok(results)
    }

    /// Visit every node in dependency order without accumulating results.
    pub fn for_each_topological<E>(
        &self,
        mut f: impl FnMut(&Node<N>) -> Result<(), E>,
    ) -> Result<(), EvalError<E>> {
        for id in self.topological_order().map_err(EvalError::Cycle)? {
            let node = self.node(id).ok_or(EvalError::MissingNode(id))?;
            f(node).map_err(|source| EvalError::Node { node: id, source })?;
        }
        Ok(())
    }
}

/// A breadth-first walk produced by [`Graph::walk`].
#[derive(Debug)]
pub struct Walk<'a, N = DynNode> {
    graph: &'a Graph<N>,
    direction: Direction,
    queue: VecDeque<NodeId>,
    seen: HashSet<NodeId>,
}

impl<N: NodeData> Iterator for Walk<'_, N> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.queue.pop_front()?;
        for next in self.graph.neighbors(current, self.direction) {
            if self.seen.insert(next) {
                self.queue.push_back(next);
            }
        }
        Some(current)
    }
}

/// A topological walk produced by [`Graph::iter_topological`].
#[derive(Debug)]
pub struct Topological<'a, N = DynNode> {
    graph: &'a Graph<N>,
    order: std::vec::IntoIter<NodeId>,
}

impl<'a, N: NodeData> Iterator for Topological<'a, N> {
    type Item = &'a Node<N>;

    fn next(&mut self) -> Option<&'a Node<N>> {
        loop {
            let id = self.order.next()?;
            if let Some(node) = self.graph.node(id) {
                return Some(node);
            }
        }
    }
}

impl<N: NodeData> ExactSizeIterator for Topological<'_, N> {
    fn len(&self) -> usize {
        self.order.len()
    }
}

/// One upstream result arriving at an input socket.
#[derive(Clone, Copy, Debug)]
pub struct Linked<'a, T> {
    /// The value the upstream node evaluated to.
    pub value: &'a T,
    /// The upstream node.
    pub node: NodeId,
    /// The output socket of that node the wire leaves.
    pub socket: &'a str,
}

/// What an evaluation closure is handed for one node.
pub struct EvalContext<'a, T, N = DynNode> {
    graph: &'a Graph<N>,
    library: &'a NodeLibrary,
    node: &'a Node<N>,
    template: &'a NodeTemplate,
    results: &'a HashMap<NodeId, T>,
}

impl<'a, T, N: NodeData> EvalContext<'a, T, N> {
    pub fn graph(&self) -> &'a Graph<N> {
        self.graph
    }

    pub fn library(&self) -> &'a NodeLibrary {
        self.library
    }

    pub fn node(&self) -> &'a Node<N> {
        self.node
    }

    pub fn template(&self) -> &'a NodeTemplate {
        self.template
    }

    pub fn id(&self) -> NodeId {
        self.node.id
    }

    /// The node's display title, which the user may have renamed.
    pub fn title(&self) -> &'a str {
        &self.node.title
    }

    pub fn is_muted(&self) -> bool {
        self.node.muted
    }

    /// Already-computed results, keyed by node.
    pub fn results(&self) -> &'a HashMap<NodeId, T> {
        self.results
    }

    // ------------------------------------------------------------ linkage

    /// Every upstream result arriving at an input socket, in connection order.
    pub fn inputs(&self, socket: &str) -> Vec<Linked<'a, T>> {
        self.graph
            .links_into(self.node.id, socket)
            .filter_map(|conn| {
                self.results.get(&conn.from.node).map(|value| Linked {
                    value,
                    node: conn.from.node,
                    socket: conn.from.socket.as_str(),
                })
            })
            .collect()
    }

    /// The first upstream result arriving at an input socket.
    pub fn input(&self, socket: &str) -> Option<Linked<'a, T>> {
        self.inputs(socket).into_iter().next()
    }

    pub fn is_linked(&self, socket: &str) -> bool {
        self.graph.is_input_linked(self.node.id, socket)
    }

    // ------------------------------------------------------------- values

    /// The inline value of an input socket, whether or not it is wired.
    pub fn literal(&self, socket: &str) -> Option<Cow<'a, Value>> {
        self.node.input_value(socket).or_else(|| {
            self.template
                .input_spec(socket)
                .map(|s| Cow::Borrowed(&s.default))
                .filter(|v| !v.is_null())
        })
    }

    /// The inline value of an input socket, but only while it is unwired —
    /// mirroring what the editor shows.
    pub fn unlinked_literal(&self, socket: &str) -> Option<Cow<'a, Value>> {
        if self.is_linked(socket) {
            None
        } else {
            self.literal(socket)
        }
    }

    pub fn literal_str(&self, socket: &str) -> Option<Cow<'a, str>> {
        borrowed_str(self.literal(socket)?)
    }

    pub fn literal_f64(&self, socket: &str) -> Option<f64> {
        self.literal(socket).as_deref().and_then(Value::as_f64)
    }

    pub fn literal_i64(&self, socket: &str) -> Option<i64> {
        self.literal(socket).as_deref().and_then(Value::as_i64)
    }

    pub fn literal_bool(&self, socket: &str) -> Option<bool> {
        self.literal(socket).as_deref().and_then(Value::as_bool)
    }

    // --------------------------------------------------------- parameters

    pub fn param(&self, name: &str) -> Option<Cow<'a, Value>> {
        self.node.param(name).or_else(|| {
            self.template
                .param_spec(name)
                .map(|p| Cow::Borrowed(&p.default))
                .filter(|v| !v.is_null())
        })
    }

    pub fn param_str(&self, name: &str) -> Option<Cow<'a, str>> {
        borrowed_str(self.param(name)?)
    }

    pub fn param_f64(&self, name: &str) -> Option<f64> {
        self.param(name).as_deref().and_then(Value::as_f64)
    }

    pub fn param_i64(&self, name: &str) -> Option<i64> {
        self.param(name).as_deref().and_then(Value::as_i64)
    }

    pub fn param_bool(&self, name: &str) -> Option<bool> {
        self.param(name).as_deref().and_then(Value::as_bool)
    }
}

impl<T, N> std::fmt::Debug for EvalContext<'_, T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalContext")
            .field("node", &self.node.id)
            .field("template", &self.template.id)
            .field("computed", &self.results.len())
            .finish()
    }
}

/// Failure from [`Graph::evaluate`] and friends.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum EvalError<E> {
    #[error(transparent)]
    Cycle(CycleError),
    #[error("node {0} is not in the graph")]
    MissingNode(NodeId),
    #[error("node {0} refers to a template that is not in the library")]
    MissingTemplate(NodeId),
    #[error("node {node}: {source}")]
    Node {
        node: NodeId,
        #[source]
        source: E,
    },
}

impl<E> EvalError<E> {
    /// The node-specific error, if that is what this is.
    pub fn into_inner(self) -> Option<E> {
        match self {
            Self::Node { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Keep a borrowed string borrowed; only an owned value has to allocate.
fn borrowed_str(value: Cow<'_, Value>) -> Option<Cow<'_, str>> {
    match value {
        Cow::Borrowed(value) => value.as_str().map(Cow::Borrowed),
        Cow::Owned(value) => value.as_str().map(|s| Cow::Owned(s.to_owned())),
    }
}

fn dedup(ids: impl Iterator<Item = NodeId>) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    ids.filter(|id| seen.insert(*id)).collect()
}
