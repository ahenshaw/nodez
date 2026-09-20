//! Node groups: a template whose behaviour is a graph.
//!
//! Blender calls them node groups and GNU Radio calls them hier blocks, and
//! underneath they are the same thing — a graph with boundary nodes, standing
//! in the outer graph as one node whose sockets are those boundaries.
//!
//! A group here is a *template*, not a node. [`NodeLibrary`] already gives a
//! template reuse, a place in the add menu, a category, a header color and a
//! size; a group node is then an ordinary [`Node`](crate::Node) whose
//! `template` points at one, and every one of those comes free. It also means
//! a group lives where GNU Radio keeps its hier blocks — in the library, next
//! to the blocks that came with it — rather than in the document, so one
//! group serves every graph that loads the library.
//!
//! The interface is read off the inside rather than declared. A group's
//! inputs are the [`INPUT_PAD`] nodes in it and its outputs are the
//! [`OUTPUT_PAD`] nodes, ordered down the canvas, each contributing the socket
//! it is named after. There is no second place to keep in step.
//!
//! Nothing downstream has to know about any of this: [`Graph::flatten`] puts
//! each group node's interior back into the graph, so evaluation, traversal
//! and every generator go on seeing a flat graph. A hier block is a flat
//! flowgraph once it runs, and a node group is a subtree once it renders, so
//! that is what they are here too.

use std::collections::{HashMap, HashSet};

use crate::graph::{Graph, NodeId, SocketRef};
use crate::template::{NodeLibrary, NodeTemplate, ParamSpec, SocketSpec, TemplateId, Widget};
use crate::types::DataTypeId;

/// Template id of the node that stands for one of a group's inputs.
pub const INPUT_PAD: &str = "group_input";
/// Template id of the node that stands for one of a group's outputs.
pub const OUTPUT_PAD: &str = "group_output";
/// The parameter on a pad that names the socket it becomes.
pub const PAD_NAME: &str = "name";

/// What a group could not be made of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupError {
    /// A group that contains itself, directly or through another.
    Recursive(String),
    /// Two pads of the same kind asking for the same socket name.
    DuplicatePad(String),
    /// No pads at all, so the group would have no way in or out.
    NoPads,
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recursive(id) => write!(f, "group `{id}` would contain itself"),
            Self::DuplicatePad(name) => write!(f, "two pads are both called `{name}`"),
            Self::NoPads => write!(f, "a group needs at least one pad to wire it up by"),
        }
    }
}

impl std::error::Error for GroupError {}

/// Register the two pad templates, and hand back their ids.
///
/// A pad carries one wildcard socket, so it takes whatever is wired to it and
/// the group's outer socket takes that type in turn. `wildcard` has been on
/// [`DataType`](crate::types::DataType) since the beginning, described as
/// being for exactly this.
pub fn register_pads(library: &mut NodeLibrary) -> (TemplateId, TemplateId) {
    let any = pad_type(library);
    let input = library.register(
        NodeTemplate::new(INPUT_PAD, "Group Input")
            .category("Group")
            .description("One of the group's inputs, seen from the inside.")
            .keywords(["pad", "input", "interface"])
            .param(ParamSpec::new(PAD_NAME, Widget::text_hint("name")).show_label(false))
            .output(SocketSpec::new("out", any).label("")),
    );
    let output = library.register(
        NodeTemplate::new(OUTPUT_PAD, "Group Output")
            .category("Group")
            .description("One of the group's outputs, seen from the inside.")
            .keywords(["pad", "output", "interface"])
            .param(ParamSpec::new(PAD_NAME, Widget::text_hint("name")).show_label(false))
            .input(SocketSpec::new("in", any).label("")),
    );
    (input, output)
}

/// The wildcard type a pad's socket speaks, registered if it is not there.
fn pad_type(library: &mut NodeLibrary) -> DataTypeId {
    const ANY: &str = "Any";
    library.types.id(ANY).unwrap_or_else(|| {
        library.types.register(
            crate::types::DataTypeBuilder::new(ANY, egui::Color32::from_rgb(0x9A, 0x9A, 0x9A))
                .wildcard(true)
                .description("Whatever is wired to it."),
        )
    })
}

/// One pad found inside a group: the socket it becomes, and where it sits.
struct Pad {
    name: String,
    at: f32,
}

/// The pads of one kind, in the order they are drawn down the canvas.
///
/// Down the canvas, because that is the order they are read in — a group's
/// sockets come out in the order its pads appear, and moving a pad moves the
/// socket. GNU Radio numbers its pads instead; this is the same idea with the
/// number read off the picture rather than typed in twice.
fn pads(inside: &Graph, library: &NodeLibrary, kind: &str) -> Vec<Pad> {
    let Some(template) = library.id(kind) else {
        return Vec::new();
    };
    let mut found: Vec<Pad> = inside
        .nodes_of_template(template)
        .map(|node| Pad {
            name: node
                .param(PAD_NAME)
                .and_then(|value| value.as_str().map(str::to_owned))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| node.title.clone()),
            at: node.position.y,
        })
        .collect();
    found.sort_by(|one, two| one.at.total_cmp(&two.at));
    found
}

/// What one of a group's sockets carries: whatever the pad is wired to on the
/// inside, or the wildcard when it is wired to nothing yet.
fn pad_socket(inside: &Graph, library: &NodeLibrary, pad: &Pad, into: bool, fallback: DataTypeId) -> DataTypeId {
    let kind = if into { INPUT_PAD } else { OUTPUT_PAD };
    let Some(template) = library.id(kind) else {
        return fallback;
    };
    inside
        .nodes_of_template(template)
        .filter(|node| {
            node.param(PAD_NAME)
                .and_then(|v| v.as_str().map(str::to_owned))
                .is_some_and(|name| name == pad.name)
        })
        .find_map(|node| {
            // An input pad's type is that of the sockets it feeds; an output
            // pad's is that of the socket feeding it.
            let socket = if into {
                let link = inside.connections().find(|c| c.from.node == node.id)?;
                let to = inside.node(link.to.node)?;
                library.get(to.template)?.input_spec(&link.to.socket)?.ty
            } else {
                let link = inside.connections().find(|c| c.to.node == node.id)?;
                let from = inside.node(link.from.node)?;
                library.get(from.template)?.output_spec(&link.from.socket)?.ty
            };
            Some(socket)
        })
        .unwrap_or(fallback)
}

/// The template a graph makes when it is used as a group.
///
/// `id`, `label` and `category` are the group's own; the sockets are read off
/// its pads.
pub fn template_of(
    inside: &Graph,
    library: &NodeLibrary,
    id: &str,
    label: &str,
    category: &str,
) -> Result<NodeTemplate, GroupError> {
    let Some(any) = library.types.id("Any") else {
        return Err(GroupError::NoPads);
    };
    let (ins, outs) = (
        pads(inside, library, INPUT_PAD),
        pads(inside, library, OUTPUT_PAD),
    );
    if ins.is_empty() && outs.is_empty() {
        return Err(GroupError::NoPads);
    }

    let mut seen = HashSet::new();
    let mut template = NodeTemplate::new(id, label).category(category);
    for pad in &ins {
        if !seen.insert((true, pad.name.clone())) {
            return Err(GroupError::DuplicatePad(pad.name.clone()));
        }
        let ty = pad_socket(inside, library, pad, true, any);
        template = template.input(SocketSpec::new(&pad.name, ty));
    }
    for pad in &outs {
        if !seen.insert((false, pad.name.clone())) {
            return Err(GroupError::DuplicatePad(pad.name.clone()));
        }
        let ty = pad_socket(inside, library, pad, false, any);
        template = template.output(SocketSpec::new(&pad.name, ty));
    }
    Ok(template)
}

impl NodeLibrary {
    /// Register a graph as a reusable node group.
    ///
    /// The template's sockets are read off the pads inside, so the interface
    /// is never written down twice. Re-registering the same id replaces the
    /// group and keeps its [`TemplateId`], which is what lets a group be
    /// edited with instances of it already in a graph — they pick the new
    /// interface up, and `Graph::validate` clears away any wire the change
    /// orphaned.
    pub fn register_group(
        &mut self,
        id: &str,
        label: &str,
        category: &str,
        inside: Graph,
    ) -> Result<TemplateId, GroupError> {
        if contains(&inside, self, id) {
            return Err(GroupError::Recursive(id.to_owned()));
        }
        let template = template_of(&inside, self, id, label, category)?;
        let at = self.register(template);
        self.groups.insert(at, inside);
        Ok(at)
    }

    /// The graph behind a template, when the template is a group.
    pub fn group(&self, template: TemplateId) -> Option<&Graph> {
        self.groups.get(&template)
    }

    pub fn group_mut(&mut self, template: TemplateId) -> Option<&mut Graph> {
        self.groups.get_mut(&template)
    }

    pub fn is_group(&self, template: TemplateId) -> bool {
        self.groups.contains_key(&template)
    }

    /// Lift a group's interior out of the library, to be edited and put back
    /// with [`NodeLibrary::register_group`].
    ///
    /// Out rather than borrowed, because an editor showing the interior needs
    /// the library at the same time to know what its nodes are.
    pub fn take_group(&mut self, template: TemplateId) -> Option<Graph> {
        self.groups.remove(&template)
    }
}

/// Whether a graph uses the group called `id`, at any depth.
fn contains(inside: &Graph, library: &NodeLibrary, id: &str) -> bool {
    let Some(looking_for) = library.id(id) else {
        return false;
    };
    let mut seen = HashSet::new();
    let mut waiting = vec![inside];
    while let Some(graph) = waiting.pop() {
        for node in graph.nodes() {
            if node.template == looking_for {
                return true;
            }
            if seen.insert(node.template)
                && let Some(deeper) = library.group(node.template)
            {
                waiting.push(deeper);
            }
        }
    }
    false
}

impl Graph {
    /// Put every group node's interior back into the graph.
    ///
    /// A group node is replaced by a copy of the graph behind it, with the
    /// wires that met its sockets reconnected to whatever the matching pads
    /// were wired to inside. Groups inside groups are flattened too.
    ///
    /// This is what keeps the rest of the crate from having to know groups
    /// exist. Evaluation, traversal and every generator go on seeing one flat
    /// graph — which is what a hier block is once it runs, and what a node
    /// group is once it renders.
    ///
    /// Positions are kept but mean little afterwards: the copies land where
    /// they sat inside the group. Run [`crate::layered`] over the result if
    /// the flattened graph is to be looked at rather than walked.
    pub fn flatten(&self, library: &NodeLibrary) -> Graph {
        let mut out = self.clone();
        // Depth first, so a group inside a group is opened as it is reached.
        // Bounded by the recursion check `register_group` does, which is the
        // only way a group gets into a library.
        loop {
            let next = out
                .nodes()
                .find(|node| library.is_group(node.template))
                .map(|node| node.id);
            let Some(node) = next else {
                return out;
            };
            expand(&mut out, library, node);
        }
    }
}

/// Replace one group node with the graph behind it.
fn expand(graph: &mut Graph, library: &NodeLibrary, at: NodeId) {
    let (Some(node), Some(inside)) = (graph.node(at), graph.node(at).and_then(|n| library.group(n.template)))
    else {
        return;
    };
    let offset = node.position.to_vec2();
    let template = node.template;

    // Copy the interior in, remembering where each of its nodes landed.
    let mapping = graph.absorb(inside, offset);

    // The pads, so the wires that met the group's sockets can be rejoined to
    // whatever those pads stood for.
    let inputs = pad_links(inside, library, INPUT_PAD, &mapping, true);
    let outputs = pad_links(inside, library, OUTPUT_PAD, &mapping, false);
    let _ = template;

    let touching: Vec<_> = graph
        .connections()
        .filter(|c| c.from.node == at || c.to.node == at)
        .map(|c| (c.id, c.from.clone(), c.to.clone(), c.order))
        .collect();
    for (id, from, to, order) in touching {
        graph.disconnect(id);
        if to.node == at {
            // Into the group: rejoin to everything that pad fed inside.
            for inner in inputs.get(&to.socket).into_iter().flatten() {
                graph.rejoin(from.clone(), inner.clone(), order);
            }
        } else {
            // Out of the group: rejoin from whatever fed that pad inside.
            if let Some(inner) = outputs.get(&from.socket).and_then(|v| v.first()) {
                graph.rejoin(inner.clone(), to.clone(), order);
            }
        }
    }

    // The pads and the group node itself have done their work.
    for id in pad_nodes(inside, library, &mapping) {
        graph.remove_node(id);
    }
    graph.remove_node(at);
}

/// Where each pad's wires go once the interior has been copied in: for an
/// input pad, the sockets it feeds; for an output pad, the socket feeding it.
fn pad_links(
    inside: &Graph,
    library: &NodeLibrary,
    kind: &str,
    mapping: &HashMap<NodeId, NodeId>,
    outward: bool,
) -> HashMap<String, Vec<SocketRef>> {
    let Some(template) = library.id(kind) else {
        return HashMap::new();
    };
    let mut out: HashMap<String, Vec<SocketRef>> = HashMap::new();
    for node in inside.nodes_of_template(template) {
        let name = node
            .param(PAD_NAME)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| node.title.clone());
        for link in inside.connections() {
            let (near, far) = if outward {
                (link.from.node, &link.to)
            } else {
                (link.to.node, &link.from)
            };
            if near != node.id {
                continue;
            }
            let Some(&moved) = mapping.get(&far.node) else {
                continue;
            };
            out.entry(name.clone())
                .or_default()
                .push(SocketRef::new(moved, far.socket.clone()));
        }
    }
    out
}

/// The copies of the interior's pad nodes, which are scaffolding and go once
/// the wires are rejoined.
fn pad_nodes(inside: &Graph, library: &NodeLibrary, mapping: &HashMap<NodeId, NodeId>) -> Vec<NodeId> {
    [INPUT_PAD, OUTPUT_PAD]
        .into_iter()
        .filter_map(|kind| library.id(kind))
        .flat_map(|template| inside.nodes_of_template(template))
        .filter_map(|node| mapping.get(&node.id).copied())
        .collect()
}

/// Make a group of a selection: take those nodes out of the graph, register
/// them as a template, and leave one node in their place.
///
/// The pads come from the wires that crossed the selection's edge. One input
/// pad per distinct socket outside that fed in — so two wires from one output
/// share a pad, which is what makes the group take one wire where the
/// selection took two — and one output pad per distinct socket inside that
/// fed out.
///
/// Returns the node that stands in the graph for what was taken out.
pub fn make_group(
    outer: &mut Graph,
    library: &mut NodeLibrary,
    chosen: &HashSet<NodeId>,
    id: &str,
    label: &str,
    category: &str,
) -> Result<NodeId, GroupError> {
    if chosen.is_empty() {
        return Err(GroupError::NoPads);
    }
    let (pad_in, pad_out) = register_pads(library);

    // The selection, copied into a graph of its own. Ids change, so everything
    // after this goes through the mapping.
    let mut inside = Graph::new();
    let mut moved = HashMap::new();
    let mut ids: Vec<NodeId> = chosen.iter().copied().collect();
    ids.sort_unstable();
    let mut middle = egui::Vec2::ZERO;
    for id in &ids {
        let Some(node) = outer.node(*id) else {
            continue;
        };
        middle += node.position.to_vec2();
        moved.insert(*id, inside.insert_node(node.clone()));
    }
    let middle = (middle / ids.len() as f32).to_pos2();

    // The wires, sorted into the three kinds: wholly inside, crossing in, and
    // crossing out.
    let mut links: Vec<_> = outer
        .connections()
        .map(|c| (c.id, c.from.clone(), c.to.clone(), c.order))
        .collect();
    links.sort_by_key(|(id, _, _, _)| id.0);

    let mut ways_in: Vec<(SocketRef, Vec<SocketRef>)> = Vec::new();
    let mut ways_out: Vec<(SocketRef, Vec<(SocketRef, u32)>)> = Vec::new();
    for (id, from, to, order) in &links {
        let (source, target) = (chosen.contains(&from.node), chosen.contains(&to.node));
        match (source, target) {
            (true, true) => {
                let (Some(&a), Some(&b)) = (moved.get(&from.node), moved.get(&to.node)) else {
                    continue;
                };
                inside.rejoin(
                    SocketRef::new(a, from.socket.clone()),
                    SocketRef::new(b, to.socket.clone()),
                    *order,
                );
            }
            (false, true) => {
                let Some(&b) = moved.get(&to.node) else {
                    continue;
                };
                let landing = SocketRef::new(b, to.socket.clone());
                match ways_in.iter_mut().find(|(outside, _)| outside == from) {
                    Some((_, inner)) => inner.push(landing),
                    None => ways_in.push((from.clone(), vec![landing])),
                }
            }
            (true, false) => {
                let Some(&a) = moved.get(&from.node) else {
                    continue;
                };
                let leaving = SocketRef::new(a, from.socket.clone());
                match ways_out.iter_mut().find(|(source, _)| *source == leaving) {
                    Some((_, outside)) => outside.push((to.clone(), *order)),
                    None => ways_out.push((leaving, vec![(to.clone(), *order)])),
                }
            }
            (false, false) => {}
        }
        let _ = id;
    }
    if ways_in.is_empty() && ways_out.is_empty() {
        return Err(GroupError::NoPads);
    }

    // A pad per way in and per way out, named after the socket it stands for
    // and placed beside it so the order reads the way the picture does.
    let mut taken: HashSet<String> = HashSet::new();
    let mut inputs = Vec::new();
    for (outside, landings) in &ways_in {
        let at = landings
            .first()
            .and_then(|l| inside.node(l.node))
            .map_or(middle, |n| n.position);
        let name = spare(
            &mut taken,
            landings
                .first()
                .and_then(|l| socket_label(&inside, library, l, true))
                .unwrap_or_else(|| "in".to_owned()),
        );
        let pad = inside.add_node(library, pad_in, egui::pos2(at.x - 200.0, at.y));
        inside
            .node_mut(pad)
            .expect("just added")
            .set_param(PAD_NAME, crate::value::Value::from(name.as_str()));
        for landing in landings {
            inside.rejoin(SocketRef::new(pad, "out"), landing.clone(), 0);
        }
        inputs.push((name, outside.clone()));
    }
    let mut outputs = Vec::new();
    for (leaving, outsides) in &ways_out {
        let at = inside.node(leaving.node).map_or(middle, |n| n.position);
        let name = spare(
            &mut taken,
            socket_label(&inside, library, leaving, false).unwrap_or_else(|| "out".to_owned()),
        );
        let pad = inside.add_node(library, pad_out, egui::pos2(at.x + 200.0, at.y));
        inside
            .node_mut(pad)
            .expect("just added")
            .set_param(PAD_NAME, crate::value::Value::from(name.as_str()));
        inside.rejoin(leaving.clone(), SocketRef::new(pad, "in"), 0);
        outputs.push((name, outsides.clone()));
    }

    let template = library.register_group(id, label, category, inside)?;

    // Out with the selection, in with the one node that stands for it.
    for id in &ids {
        outer.remove_node(*id);
    }
    let stand_in = outer.add_node(library, template, middle);
    for (name, outside) in inputs {
        outer.rejoin(outside, SocketRef::new(stand_in, name), 0);
    }
    for (name, outsides) in outputs {
        for (target, order) in outsides {
            outer.rejoin(SocketRef::new(stand_in, name.clone()), target, order);
        }
    }
    Ok(stand_in)
}

/// What a socket is called, for naming the pad that stands for it.
fn socket_label(
    graph: &Graph,
    library: &NodeLibrary,
    socket: &SocketRef,
    input: bool,
) -> Option<String> {
    let template = library.get(graph.node(socket.node)?.template)?;
    let spec = if input {
        template.input_spec(&socket.socket)?
    } else {
        template.output_spec(&socket.socket)?
    };
    Some(spec.display().to_owned())
}

/// A name nothing else has taken yet.
fn spare(taken: &mut HashSet<String>, want: String) -> String {
    let mut name = want.clone();
    let mut n = 2;
    while !taken.insert(name.clone()) {
        name = format!("{want} {n}");
        n += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::Widget;
    use crate::value::Value;
    use egui::{Color32, pos2};

    /// Text in, text out, and a Join in the middle: enough to make a group of.
    fn library() -> (NodeLibrary, DataTypeId) {
        let mut library = NodeLibrary::new();
        let text = library.types.add("Text", Color32::from_rgb(0xA1, 0xA1, 0xA1));
        library.register(
            NodeTemplate::new("text", "Text")
                .input(SocketSpec::new("value", text).editable(Widget::text()))
                .output(SocketSpec::new("out", text)),
        );
        library.register(
            NodeTemplate::new("join", "Join")
                .input(SocketSpec::new("a", text))
                .input(SocketSpec::new("b", text))
                .output(SocketSpec::new("out", text)),
        );
        library.register(NodeTemplate::new("sink", "Sink").input(SocketSpec::new("value", text)));
        register_pads(&mut library);
        (library, text)
    }

    /// Two inputs into a Join, out again: the inside of a group.
    fn inside(library: &NodeLibrary) -> Graph {
        let mut graph = Graph::new();
        let add = |graph: &mut Graph, id: &str, y: f32| {
            graph.add_node(library, library.id(id).unwrap(), pos2(0.0, y))
        };
        let first = add(&mut graph, INPUT_PAD, 0.0);
        let second = add(&mut graph, INPUT_PAD, 100.0);
        let join = add(&mut graph, "join", 50.0);
        let out = add(&mut graph, OUTPUT_PAD, 50.0);
        graph.node_mut(first).unwrap().set_param(PAD_NAME, Value::from("left"));
        graph.node_mut(second).unwrap().set_param(PAD_NAME, Value::from("right"));
        graph.node_mut(out).unwrap().set_param(PAD_NAME, Value::from("joined"));
        graph.connect(library, (first, "out"), (join, "a")).unwrap();
        graph.connect(library, (second, "out"), (join, "b")).unwrap();
        graph.connect(library, (join, "out"), (out, "in")).unwrap();
        graph
    }

    /// A group's sockets are its pads, in the order they are drawn.
    #[test]
    fn a_groups_interface_is_read_off_its_pads() {
        let (mut library, text) = library();
        let body = inside(&library);
        let id = library
            .register_group("pair", "Pair", "Group", body)
            .unwrap();

        let template = library.expect(id);
        let names: Vec<&str> = template.inputs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["left", "right"], "top to bottom, as they are drawn");
        assert_eq!(template.outputs.len(), 1);
        assert_eq!(template.outputs[0].name, "joined");
        // And the types come from what the pads are wired to inside, not from
        // the wildcard the pads themselves carry.
        assert_eq!(template.inputs[0].ty, text);
        assert_eq!(template.outputs[0].ty, text);
    }

    /// A group node is an ordinary node, and wires to it like one.
    #[test]
    fn a_group_can_be_wired_up_like_any_other_node() {
        let (mut library, _) = library();
        let body = inside(&library);
        library.register_group("pair", "Pair", "Group", body).unwrap();

        let mut graph = Graph::new();
        let add = |graph: &mut Graph, id: &str| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, 0.0))
        };
        let one = add(&mut graph, "text");
        let two = add(&mut graph, "text");
        let pair = add(&mut graph, "pair");
        let out = add(&mut graph, "sink");
        graph.connect(&library, (one, "out"), (pair, "left")).unwrap();
        graph.connect(&library, (two, "out"), (pair, "right")).unwrap();
        graph.connect(&library, (pair, "joined"), (out, "value")).unwrap();
        assert_eq!(graph.node_count(), 4);
    }

    /// Flattening puts the inside back, and leaves nothing of the group.
    #[test]
    fn flattening_a_group_leaves_a_graph_that_knows_nothing_of_it() {
        let (mut library, _) = library();
        let body = inside(&library);
        library.register_group("pair", "Pair", "Group", body).unwrap();

        let mut graph = Graph::new();
        let add = |graph: &mut Graph, id: &str| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, 0.0))
        };
        let one = add(&mut graph, "text");
        let two = add(&mut graph, "text");
        let pair = add(&mut graph, "pair");
        let out = add(&mut graph, "sink");
        graph.node_mut(one).unwrap().set_input_value("value", "left side");
        graph.connect(&library, (one, "out"), (pair, "left")).unwrap();
        graph.connect(&library, (two, "out"), (pair, "right")).unwrap();
        graph.connect(&library, (pair, "joined"), (out, "value")).unwrap();

        let flat = graph.flatten(&library);

        // Two texts, the join that was inside, and the sink. No group, no pads.
        assert_eq!(flat.node_count(), 4, "{:?}", flat.node_count());
        for node in flat.nodes() {
            let id = &library.expect(node.template).id;
            assert!(id != "pair" && id != INPUT_PAD && id != OUTPUT_PAD, "{id} survived");
        }

        // And the wires go where they went before, through the group.
        let join = flat
            .nodes()
            .find(|n| library.expect(n.template).id == "join")
            .unwrap();
        let sink = flat
            .nodes()
            .find(|n| library.expect(n.template).id == "sink")
            .unwrap();
        assert!(flat.is_input_linked(join.id, "a"));
        assert!(flat.is_input_linked(join.id, "b"));
        assert!(flat.is_input_linked(sink.id, "value"));
        assert_eq!(flat.connection_count(), 3);

        // The values inside the outer nodes came along.
        let kept = flat
            .nodes()
            .filter_map(|n| n.input_value("value"))
            .any(|v| v.as_str() == Some("left side"));
        assert!(kept, "the text node's own value survived the flattening");
    }

    /// A group inside a group opens all the way down.
    #[test]
    fn a_group_inside_a_group_flattens_too() {
        let (mut library, _) = library();
        let body = inside(&library);
        library.register_group("pair", "Pair", "Group", body).unwrap();

        // A second group that uses the first.
        let mut outer = Graph::new();
        let add = |graph: &mut Graph, id: &str, y: f32| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, y))
        };
        let first = add(&mut outer, INPUT_PAD, 0.0);
        let second = add(&mut outer, INPUT_PAD, 100.0);
        let pair = add(&mut outer, "pair", 50.0);
        let out = add(&mut outer, OUTPUT_PAD, 50.0);
        outer.node_mut(first).unwrap().set_param(PAD_NAME, Value::from("one"));
        outer.node_mut(second).unwrap().set_param(PAD_NAME, Value::from("two"));
        outer.node_mut(out).unwrap().set_param(PAD_NAME, Value::from("both"));
        outer.connect(&library, (first, "out"), (pair, "left")).unwrap();
        outer.connect(&library, (second, "out"), (pair, "right")).unwrap();
        outer.connect(&library, (pair, "joined"), (out, "in")).unwrap();
        library.register_group("nested", "Nested", "Group", outer).unwrap();

        let mut graph = Graph::new();
        let add = |graph: &mut Graph, id: &str| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, 0.0))
        };
        let one = add(&mut graph, "text");
        let two = add(&mut graph, "text");
        let nested = add(&mut graph, "nested");
        graph.connect(&library, (one, "out"), (nested, "one")).unwrap();
        graph.connect(&library, (two, "out"), (nested, "two")).unwrap();

        let flat = graph.flatten(&library);
        assert_eq!(flat.node_count(), 3, "two texts and the join from two levels down");
        let join = flat
            .nodes()
            .find(|n| library.expect(n.template).id == "join")
            .unwrap();
        assert!(flat.is_input_linked(join.id, "a"));
        assert!(flat.is_input_linked(join.id, "b"));
    }

    /// Grouping a selection takes it out of the graph and leaves one node,
    /// and flattening puts it all back.
    #[test]
    fn a_selection_becomes_a_group_and_comes_back_out_of_one() {
        let (mut library, _) = library();
        let mut graph = Graph::new();
        let add = |graph: &mut Graph, id: &str| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, 0.0))
        };
        let one = add(&mut graph, "text");
        let two = add(&mut graph, "text");
        let join = add(&mut graph, "join");
        let out = add(&mut graph, "sink");
        graph.node_mut(one).unwrap().set_input_value("value", "kept");
        graph.connect(&library, (one, "out"), (join, "a")).unwrap();
        graph.connect(&library, (two, "out"), (join, "b")).unwrap();
        graph.connect(&library, (join, "out"), (out, "value")).unwrap();

        // Group the join alone: two wires in, one out.
        let chosen = HashSet::from([join]);
        let made = make_group(&mut graph, &mut library, &chosen, "pair", "Pair", "Group").unwrap();

        assert_eq!(graph.node_count(), 4, "the join went, one group node came");
        assert!(graph.node(join).is_none());
        let template = library.expect(graph.node(made).unwrap().template);
        assert_eq!(template.inputs.len(), 2, "one pad per socket that fed in");
        assert_eq!(template.outputs.len(), 1);
        // The wires that crossed the edge now meet the group node.
        assert_eq!(graph.connection_count(), 3);
        for socket in template.inputs.iter().map(|s| s.name.clone()) {
            assert!(graph.is_input_linked(made, &socket), "`{socket}` lost its wire");
        }

        // And flattening is the exact inverse, as far as the picture goes.
        let flat = graph.flatten(&library);
        assert_eq!(flat.node_count(), 4);
        assert_eq!(flat.connection_count(), 3);
        let join = flat
            .nodes()
            .find(|n| library.expect(n.template).id == "join")
            .unwrap();
        assert!(flat.is_input_linked(join.id, "a"));
        assert!(flat.is_input_linked(join.id, "b"));
        let kept = flat
            .nodes()
            .filter_map(|n| n.input_value("value"))
            .any(|v| v.as_str() == Some("kept"));
        assert!(kept);
    }

    /// Two wires from one socket into the selection share a single pad, so
    /// the group takes one wire where the selection took two.
    #[test]
    fn one_source_feeding_twice_makes_one_input() {
        let (mut library, _) = library();
        let mut graph = Graph::new();
        let add = |graph: &mut Graph, id: &str| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, 0.0))
        };
        let source = add(&mut graph, "text");
        let join = add(&mut graph, "join");
        graph.connect(&library, (source, "out"), (join, "a")).unwrap();
        graph.connect(&library, (source, "out"), (join, "b")).unwrap();

        let made = make_group(
            &mut graph,
            &mut library,
            &HashSet::from([join]),
            "pair",
            "Pair",
            "Group",
        )
        .unwrap();
        let template = library.expect(graph.node(made).unwrap().template);
        assert_eq!(template.inputs.len(), 1, "one socket outside, one pad");
        assert_eq!(graph.connection_count(), 1);

        // And both wires are back inside once it is flattened.
        let flat = graph.flatten(&library);
        let join = flat
            .nodes()
            .find(|n| library.expect(n.template).id == "join")
            .unwrap();
        assert!(flat.is_input_linked(join.id, "a"));
        assert!(flat.is_input_linked(join.id, "b"));
    }

    /// A group that contains itself is refused rather than left to be found
    /// by whatever tries to open it.
    #[test]
    fn a_group_cannot_contain_itself() {
        let (mut library, _) = library();
        let body = inside(&library);
        library.register_group("pair", "Pair", "Group", body).unwrap();

        let mut recursive = Graph::new();
        let add = |graph: &mut Graph, id: &str, y: f32| {
            graph.add_node(&library, library.id(id).unwrap(), pos2(0.0, y))
        };
        let pad = add(&mut recursive, INPUT_PAD, 0.0);
        let itself = add(&mut recursive, "pair", 50.0);
        recursive.node_mut(pad).unwrap().set_param(PAD_NAME, Value::from("in"));
        recursive.connect(&library, (pad, "out"), (itself, "left")).unwrap();

        let refused = library.register_group("pair", "Pair", "Group", recursive);
        assert_eq!(refused, Err(GroupError::Recursive("pair".to_owned())));
    }
}
