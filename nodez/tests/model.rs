//! Tests for the graph model: typing rules, cycle refusal and traversal.

use egui::{Color32, pos2};
use nodez::{
    ConnectError, Graph, LayoutOptions, NodeId, NodeLibrary, NodeTemplate, SocketSpec, TemplateId,
    Value, Widget,
};

struct Fixture {
    library: NodeLibrary,
    number: TemplateId,
    text: TemplateId,
    join: TemplateId,
    sink: TemplateId,
}

fn fixture() -> Fixture {
    let mut library = NodeLibrary::new();
    let number_ty = library.types.add("Number", Color32::from_rgb(0x63, 0x63, 0x63));
    let text_ty = library.types.add("Text", Color32::from_rgb(0xA1, 0xA1, 0xA1));
    // A number may be spelled as text, but not the other way round.
    library.types.allow_cast(number_ty, text_ty);

    let number = library.register(
        NodeTemplate::new("number", "Number")
            .category("Input")
            .input(SocketSpec::new("value", number_ty).editable(Widget::float()))
            .output(SocketSpec::new("out", number_ty)),
    );
    let text = library.register(
        NodeTemplate::new("text", "Text")
            .category("Input")
            .input(SocketSpec::new("value", text_ty).editable(Widget::text()))
            .output(SocketSpec::new("out", text_ty)),
    );
    let join = library.register(
        NodeTemplate::new("join", "Join")
            .category("Convert")
            .input(SocketSpec::new("parts", text_ty).multi())
            .param(nodez::ParamSpec::new("separator", Widget::text()).default_value(" "))
            .output(SocketSpec::new("out", text_ty)),
    );
    let sink = library.register(
        NodeTemplate::new("sink", "Output")
            .category("Output")
            .input(SocketSpec::new("value", text_ty)),
    );

    Fixture {
        library,
        number,
        text,
        join,
        sink,
    }
}

#[test]
fn compatible_types_connect() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.sink, pos2(200.0, 0.0));
    assert!(graph.connect(&f.library, (a, "out"), (b, "value")).is_ok());
    assert_eq!(graph.connection_count(), 1);
}

#[test]
fn implicit_cast_is_directional() {
    let f = fixture();
    let mut graph = Graph::new();
    let number = graph.add_node(&f.library, f.number, pos2(0.0, 0.0));
    let text = graph.add_node(&f.library, f.text, pos2(0.0, 100.0));
    let sink = graph.add_node(&f.library, f.sink, pos2(200.0, 0.0));

    // Number -> Text is declared as an implicit cast.
    assert!(
        graph
            .connect(&f.library, (number, "out"), (sink, "value"))
            .is_ok()
    );

    // Text -> Number is not.
    let number_input = graph.add_node(&f.library, f.number, pos2(400.0, 0.0));
    let err = graph
        .connect(&f.library, (text, "out"), (number_input, "value"))
        .unwrap_err();
    assert!(matches!(err, ConnectError::TypeMismatch { .. }), "{err:?}");
}

#[test]
fn single_link_input_replaces_its_wire() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.text, pos2(0.0, 80.0));
    let sink = graph.add_node(&f.library, f.sink, pos2(200.0, 0.0));

    graph.connect(&f.library, (a, "out"), (sink, "value")).unwrap();
    graph.connect(&f.library, (b, "out"), (sink, "value")).unwrap();

    assert_eq!(graph.connection_count(), 1);
    assert_eq!(graph.source_of(sink, "value").unwrap().node, b);
}

#[test]
fn multi_input_accumulates_wires() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));
    for i in 0..3 {
        let a = graph.add_node(&f.library, f.text, pos2(0.0, 80.0 * f64::from(i) as f32));
        graph.connect(&f.library, (a, "out"), (join, "parts")).unwrap();
    }
    assert_eq!(graph.links_into(join, "parts").count(), 3);
}

#[test]
fn cycles_are_refused() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));
    graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();

    let err = graph
        .connect(&f.library, (b, "out"), (a, "parts"))
        .unwrap_err();
    assert_eq!(err, ConnectError::WouldCycle);

    let err = graph
        .connect(&f.library, (a, "out"), (a, "parts"))
        .unwrap_err();
    assert_eq!(err, ConnectError::SelfLink);

    assert!(graph.is_acyclic());
}

#[test]
fn topological_order_respects_dependencies() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));
    let c = graph.add_node(&f.library, f.sink, pos2(400.0, 0.0));
    graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();
    graph.connect(&f.library, (b, "out"), (c, "value")).unwrap();

    let order = graph.topological_order().unwrap();
    let index = |id| order.iter().position(|&n| n == id).unwrap();
    assert!(index(a) < index(b));
    assert!(index(b) < index(c));

    assert_eq!(graph.dependency_order(c).unwrap(), vec![a, b, c]);
    assert_eq!(graph.roots().collect::<Vec<_>>(), vec![a]);
    assert_eq!(graph.sinks().collect::<Vec<_>>(), vec![c]);
    assert_eq!(graph.ancestors(c).collect::<Vec<_>>(), vec![b, a]);
    assert_eq!(graph.descendants(a).collect::<Vec<_>>(), vec![b, c]);
    assert!(graph.depends_on(a, c));
    assert!(!graph.depends_on(c, a));
    assert_eq!(graph.depths().unwrap()[&c], 2);
    assert_eq!(graph.components().len(), 1);
}

#[test]
fn evaluation_folds_in_dependency_order() {
    let f = fixture();
    let mut graph = Graph::new();
    let hello = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let world = graph.add_node(&f.library, f.text, pos2(0.0, 80.0));
    let count = graph.add_node(&f.library, f.number, pos2(0.0, 160.0));
    let join = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));

    graph.node_mut(hello).unwrap().set_input_value("value", "hello");
    graph.node_mut(world).unwrap().set_input_value("value", "world");
    graph.node_mut(count).unwrap().set_input_value("value", 3.0);
    graph.node_mut(join).unwrap().set_param("separator", ", ");

    for source in [hello, world, count] {
        graph.connect(&f.library, (source, "out"), (join, "parts")).unwrap();
    }

    let rendered = graph
        .evaluate::<String, std::convert::Infallible>(&f.library, join, |ctx| {
            Ok(match ctx.template().id.as_str() {
                "text" => ctx.literal_str("value").unwrap_or_default().into_owned(),
                "number" => ctx.literal_f64("value").unwrap_or_default().to_string(),
                "join" => {
                    let separator = ctx.param_str("separator").unwrap_or(" ".into());
                    let parts: Vec<&str> =
                        ctx.inputs("parts").iter().map(|l| l.value.as_str()).collect();
                    parts.join(&separator)
                }
                other => panic!("unexpected template {other}"),
            })
        })
        .unwrap();

    assert_eq!(rendered, "hello, world, 3");
}

#[test]
fn evaluating_a_target_skips_unrelated_nodes() {
    let f = fixture();
    let mut graph = Graph::new();
    let used = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let unused = graph.add_node(&f.library, f.text, pos2(0.0, 80.0));
    let sink = graph.add_node(&f.library, f.sink, pos2(200.0, 0.0));
    graph.connect(&f.library, (used, "out"), (sink, "value")).unwrap();

    let mut visited = Vec::new();
    graph
        .evaluate::<(), std::convert::Infallible>(&f.library, sink, |ctx| {
            visited.push(ctx.id());
            Ok(())
        })
        .unwrap();

    assert!(visited.contains(&used));
    assert!(!visited.contains(&unused));
}

#[test]
fn removing_a_node_removes_its_wires() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.sink, pos2(200.0, 0.0));
    graph.connect(&f.library, (a, "out"), (b, "value")).unwrap();

    graph.remove_node(a);
    assert_eq!(graph.connection_count(), 0);
    assert_eq!(graph.node_count(), 1);
}

#[test]
fn duplicating_a_subgraph_keeps_internal_wires() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));
    let outside = graph.add_node(&f.library, f.sink, pos2(400.0, 0.0));
    graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();
    graph.connect(&f.library, (b, "out"), (outside, "value")).unwrap();

    let selection = [a, b].into_iter().collect();
    let mapping = graph.duplicate_subgraph(&selection, egui::vec2(20.0, 20.0));

    assert_eq!(mapping.len(), 2);
    assert_eq!(graph.node_count(), 5);
    // The internal a -> b wire is copied; the b -> outside wire is not.
    assert_eq!(graph.connection_count(), 3);
    assert_eq!(graph.links_into(mapping[&b], "parts").count(), 1);
    assert_eq!(graph.outgoing(mapping[&b]).count(), 0);
}

#[test]
fn input_values_default_from_the_template() {
    let f = fixture();
    let mut graph = Graph::new();
    let node = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    assert_eq!(
        graph.node(node).unwrap().param("separator").as_deref(),
        Some(&Value::Text(" ".to_owned()))
    );
}

#[test]
fn validate_repairs_a_graph_against_a_changed_library() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.sink, pos2(200.0, 0.0));
    graph.connect(&f.library, (a, "out"), (b, "value")).unwrap();

    // Re-register the sink with a differently named input socket.
    let mut library = fixture().library;
    let text_ty = library.types.id("Text").unwrap();
    library.register(
        NodeTemplate::new("sink", "Output")
            .category("Output")
            .input(SocketSpec::new("renamed", text_ty)),
    );

    let repairs = graph.validate(&library);
    assert_eq!(repairs.removed_connections, 1);
    assert_eq!(repairs.removed_nodes, 0);
    assert_eq!(graph.node_count(), 2);
}

/// Three nodes of deliberately different sizes, placed by hand.
fn scattered() -> (Fixture, Graph, Vec<nodez::NodeId>, Vec<egui::Vec2>) {
    let f = fixture();
    let mut graph = Graph::new();
    let ids: Vec<_> = [pos2(0.0, 0.0), pos2(40.0, 200.0), pos2(90.0, 500.0)]
        .into_iter()
        .map(|p| graph.add_node(&f.library, f.text, p))
        .collect();
    // Widths and heights differ, so edge alignment and center alignment
    // cannot accidentally agree.
    let sizes = vec![
        egui::vec2(100.0, 40.0),
        egui::vec2(200.0, 60.0),
        egui::vec2(60.0, 100.0),
    ];
    (f, graph, ids, sizes)
}

#[test]
fn align_lines_a_selection_up() {
    let (_f, graph, ids, sizes) = scattered();
    let size_of = {
        let ids = ids.clone();
        let sizes = sizes.clone();
        move |_g: &Graph, node: &nodez::Node| {
            sizes[ids.iter().position(|id| *id == node.id).unwrap()]
        }
    };

    let mut left = graph.clone();
    nodez::align(&mut left, ids.clone(), nodez::Align::Left, &size_of);
    assert!(ids.iter().all(|id| left.node(*id).unwrap().position.x == 0.0));

    // Right aligns trailing edges, so each node's x depends on its width.
    let mut right = graph.clone();
    nodez::align(&mut right, ids.clone(), nodez::Align::Right, &size_of);
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(right.node(*id).unwrap().position.x + sizes[i].x, 240.0);
    }

    // Centering lines up middles, not edges.
    let mut center = graph.clone();
    nodez::align(&mut center, ids.clone(), nodez::Align::CenterX, &size_of);
    for (i, id) in ids.iter().enumerate() {
        let node = center.node(*id).unwrap();
        assert_eq!(node.position.x + sizes[i].x * 0.5, 120.0);
    }

    // Aligning on x leaves y alone.
    assert_eq!(left.node(ids[1]).unwrap().position.y, 200.0);
}

#[test]
fn align_reports_only_what_moved() {
    let (_f, mut graph, ids, _sizes) = scattered();
    // ids[0] is already the leftmost, so a left-align must not claim it moved.
    let moved = nodez::align(&mut graph, ids.clone(), nodez::Align::Left, |_g, _n| {
        egui::vec2(100.0, 40.0)
    });
    assert_eq!(moved, vec![ids[1], ids[2]]);

    // Running it again is a no-op.
    let again = nodez::align(&mut graph, ids, nodez::Align::Left, |_g, _n| {
        egui::vec2(100.0, 40.0)
    });
    assert!(again.is_empty());
}

#[test]
fn distribute_evens_the_gaps_between_boxes() {
    let (_f, mut graph, ids, sizes) = scattered();
    let size_of = {
        let ids = ids.clone();
        let sizes = sizes.clone();
        move |_g: &Graph, node: &nodez::Node| {
            sizes[ids.iter().position(|id| *id == node.id).unwrap()]
        }
    };
    nodez::distribute(
        &mut graph,
        ids.clone(),
        nodez::Axis::Y,
        nodez::Spacing::Even,
        &size_of,
    );

    // The outermost two stay put; the gaps between boxes come out equal.
    assert_eq!(graph.node(ids[0]).unwrap().position.y, 0.0);
    assert_eq!(graph.node(ids[2]).unwrap().position.y + 100.0, 600.0);
    let gaps: Vec<f32> = ids
        .windows(2)
        .map(|w| {
            let above = graph.node(w[0]).unwrap();
            let below = graph.node(w[1]).unwrap();
            let height = sizes[ids.iter().position(|id| *id == w[0]).unwrap()].y;
            below.position.y - (above.position.y + height)
        })
        .collect();
    assert!(
        (gaps[0] - gaps[1]).abs() < 0.01,
        "gaps should match: {gaps:?}"
    );
}

#[test]
fn distribute_with_a_fixed_gap_stacks_from_the_first() {
    let (_f, mut graph, ids, _sizes) = scattered();
    nodez::distribute(
        &mut graph,
        ids.clone(),
        nodez::Axis::Y,
        nodez::Spacing::Fixed(10.0),
        |_g, _n| egui::vec2(100.0, 40.0),
    );
    let y = |id| graph.node(id).unwrap().position.y;
    assert_eq!(y(ids[0]), 0.0);
    assert_eq!(y(ids[1]), 50.0);
    assert_eq!(y(ids[2]), 100.0);
}

#[test]
fn aligning_fewer_than_two_nodes_does_nothing() {
    let (_f, mut graph, ids, _sizes) = scattered();
    let before = graph.node(ids[0]).unwrap().position;
    let moved = nodez::align(&mut graph, [ids[0]], nodez::Align::Right, |_g, _n| {
        egui::vec2(100.0, 40.0)
    });
    assert!(moved.is_empty());
    assert_eq!(graph.node(ids[0]).unwrap().position, before);
}

/// A chain a -> b -> c -> d, plus a wire from a straight to d that has to get
/// past the two columns in between.
fn spanning() -> (Fixture, Graph, Vec<nodez::NodeId>) {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let c = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let d = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    for (from, to) in [(a, b), (b, c), (c, d)] {
        graph.connect(&f.library, (from, "out"), (to, "parts")).unwrap();
    }
    graph.connect(&f.library, (a, "out"), (d, "parts")).unwrap();
    let size = |_: &Graph, _: &nodez::Node| egui::vec2(120.0, 60.0);
    nodez::layered(&mut graph, &LayoutOptions::default(), size).unwrap();
    (f, graph, vec![a, b, c, d])
}

#[test]
fn routing_bends_only_the_wires_that_span_columns() {
    let (_f, mut graph, ids) = spanning();
    let size = |_: &Graph, _: &nodez::Node| egui::vec2(120.0, 60.0);
    let anchors = |_: &Graph, _: &nodez::SocketRef, _: nodez::SocketKind, _: Option<u32>| None;
    nodez::route_links(&mut graph, &nodez::RouteOptions::default(), size, anchors).unwrap();

    // Neighbor-to-neighbor wires have a clear channel already.
    for (from, to) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[3])] {
        let link = graph
            .connections()
            .find(|c| c.from.node == from && c.to.node == to)
            .unwrap();
        assert!(link.waypoints.is_empty(), "{from:?}->{to:?} should stay straight");
    }

    // The long one has to be steered, and whatever route it is given runs
    // one way: out of the source, across, and into the target.
    let long = graph
        .connections()
        .find(|c| c.from.node == ids[0] && c.to.node == ids[3])
        .unwrap();
    assert!(!long.waypoints.is_empty());
    let xs: Vec<f32> = long.waypoints.iter().map(|p| p.x).collect();
    assert!(
        xs.windows(2).all(|w| w[0] <= w[1] + 0.01),
        "waypoints double back: {xs:?}"
    );
}

#[test]
fn a_routed_wire_clears_the_nodes_it_passes() {
    let (_f, mut graph, ids) = spanning();
    let size = |_: &Graph, _: &nodez::Node| egui::vec2(120.0, 60.0);
    let anchors = |_: &Graph, _: &nodez::SocketRef, _: nodez::SocketKind, _: Option<u32>| None;
    nodez::route_links(&mut graph, &nodez::RouteOptions::default(), size, anchors).unwrap();

    let blockers: Vec<egui::Rect> = [ids[1], ids[2]]
        .iter()
        .map(|id| egui::Rect::from_min_size(graph.node(*id).unwrap().position, egui::vec2(120.0, 60.0)))
        .collect();
    let long = graph
        .connections()
        .find(|c| c.from.node == ids[0] && c.to.node == ids[3])
        .unwrap();
    for point in &long.waypoints {
        for rect in &blockers {
            assert!(!rect.contains(*point), "waypoint {point:?} sits inside {rect:?}");
        }
    }
}

#[test]
fn routing_can_be_undone() {
    let (_f, mut graph, _ids) = spanning();
    let size = |_: &Graph, _: &nodez::Node| egui::vec2(120.0, 60.0);
    let anchors = |_: &Graph, _: &nodez::SocketRef, _: nodez::SocketKind, _: Option<u32>| None;
    nodez::route_links(&mut graph, &nodez::RouteOptions::default(), size, anchors).unwrap();
    assert!(graph.connections().any(|c| !c.waypoints.is_empty()));

    let cleared = graph.clear_routing();
    assert_eq!(cleared, 1, "only the long wire needed routing");
    assert!(graph.connections().all(|c| c.waypoints.is_empty()));
    // And a graph with nothing to undo says so.
    assert_eq!(graph.clear_routing(), 0);
}

#[test]
fn routing_is_idempotent() {
    let (_f, mut graph, _ids) = spanning();
    let size = |_: &Graph, _: &nodez::Node| egui::vec2(120.0, 60.0);
    let anchors = |_: &Graph, _: &nodez::SocketRef, _: nodez::SocketKind, _: Option<u32>| None;
    nodez::route_links(&mut graph, &nodez::RouteOptions::default(), size, anchors).unwrap();
    let once: Vec<_> = graph.connections().map(|c| c.waypoints.clone()).collect();
    nodez::route_links(&mut graph, &nodez::RouteOptions::default(), size, anchors).unwrap();
    let twice: Vec<_> = graph.connections().map(|c| c.waypoints.clone()).collect();
    assert_eq!(once, twice, "re-routing should replace, not accumulate");
}

#[test]
fn routing_leaves_evaluation_alone() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    for part in ["a", "b", "c"] {
        let n = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
        graph.node_mut(n).unwrap().set_input_value("value", part);
        graph.connect(&f.library, (n, "out"), (join, "parts")).unwrap();
    }
    let order = |g: &Graph| -> Vec<String> {
        g.links_into(join, "parts")
            .filter_map(|c| g.node(c.from.node)?.input_value("value"))
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let before = order(&graph);

    // Bend every wire by hand, then check nothing about the graph moved.
    for id in graph.connections().map(|c| c.id).collect::<Vec<_>>() {
        graph.connection_mut(id).unwrap().waypoints = vec![pos2(5.0, 5.0), pos2(9.0, 9.0)];
    }
    assert_eq!(order(&graph), before);
    assert_eq!(graph.predecessors(join).len(), 3);
    assert_eq!(graph.topological_order().unwrap().len(), 4);
}

#[test]
fn waypoints_survive_a_save_and_load() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let link = graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();
    graph.connection_mut(link).unwrap().waypoints = vec![pos2(12.0, 34.0)];

    let restored: Graph = serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
    assert_eq!(
        restored.connection(link).unwrap().waypoints,
        vec![pos2(12.0, 34.0)]
    );
}

#[test]
fn a_straight_wire_costs_nothing_to_store() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();
    // An unrouted graph should serialize exactly as it did before waypoints.
    assert!(!serde_json::to_string(&graph).unwrap().contains("waypoints"));
}

/// An input that has to be wired and is not, which is what the editor marks.
///
/// Which inputs those are is read off the schema: an inline editor is a value
/// to fall back on, a fan-in may be empty, and `Option<T>` says outright that
/// the node works without it. What is left has nowhere else to get a value.
#[test]
fn missing_inputs_are_the_ones_with_nowhere_else_to_look() {
    let mut library = NodeLibrary::new();
    let text_ty = library.types.add("Text", Color32::from_rgb(0xA1, 0xA1, 0xA1));
    let source = library.register(
        NodeTemplate::new("source", "Source").output(SocketSpec::new("out", text_ty)),
    );
    let sink = library.register(
        NodeTemplate::new("sink", "Sink")
            // Nowhere else to look: this one has to be wired.
            .input(SocketSpec::new("needed", text_ty))
            .input(SocketSpec::new("spare", text_ty).optional())
            .input(SocketSpec::new("many", text_ty).multi())
            .input(SocketSpec::new("typed", text_ty).editable(Widget::text())),
    );

    let mut graph = Graph::new();
    let target = graph.add_node(&library, sink, pos2(0.0, 0.0));

    let missing = graph.missing_inputs(&library);
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!(missing[0].socket, "needed");
    assert!(graph.is_input_missing(&library, target, "needed"));
    for socket in ["spare", "many", "typed"] {
        assert!(
            !graph.is_input_missing(&library, target, socket),
            "`{socket}` has somewhere else to get its value"
        );
    }

    // Wiring it is what fills it.
    let from = graph.add_node(&library, source, pos2(0.0, 0.0));
    graph.connect(&library, (from, "out"), (target, "needed")).unwrap();
    assert!(graph.missing_inputs(&library).is_empty());
}

/// A muted node is deliberately switched off, not unfinished.
#[test]
fn a_muted_node_has_no_missing_inputs() {
    let mut library = NodeLibrary::new();
    let text_ty = library.types.add("Text", Color32::from_rgb(0xA1, 0xA1, 0xA1));
    let sink = library.register(
        NodeTemplate::new("sink", "Sink").input(SocketSpec::new("needed", text_ty)),
    );

    let mut graph = Graph::new();
    let target = graph.add_node(&library, sink, pos2(0.0, 0.0));
    assert_eq!(graph.missing_inputs(&library).len(), 1);

    graph.node_mut(target).unwrap().muted = true;
    assert!(graph.missing_inputs(&library).is_empty());
    assert!(!graph.is_input_missing(&library, target, "needed"));
}

#[test]
fn layered_layout_sorts_into_columns() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(500.0, 500.0));
    let b = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let c = graph.add_node(&f.library, f.sink, pos2(-300.0, 900.0));
    graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();
    graph.connect(&f.library, (b, "out"), (c, "value")).unwrap();

    nodez::layered(&mut graph, &LayoutOptions::default(), |_graph, _node| {
        egui::vec2(160.0, 80.0)
    })
    .unwrap();

    let x = |id| graph.node(id).unwrap().position.x;
    assert!(x(a) < x(b));
    assert!(x(b) < x(c));
}

/// A node sits as near as it can to what it is wired to, not as early as
/// dependency lets it.
///
/// `text` feeds only the last `join` of a chain. Placed by depth alone it
/// would sit in the first column with the head of that chain, a whole graph
/// away from the one node it feeds, with a wire across everything to show
/// for it.
#[test]
fn a_node_sits_beside_what_it_feeds() {
    let f = fixture();
    let mut graph = Graph::new();
    let head = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let one = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let two = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let three = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let out = graph.add_node(&f.library, f.sink, pos2(0.0, 0.0));
    // The one that has no business being in the first column.
    let late = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));

    for (from, to) in [(head, one), (one, two), (two, three)] {
        graph.connect(&f.library, (from, "out"), (to, "parts")).unwrap();
    }
    graph.connect(&f.library, (three, "out"), (out, "value")).unwrap();
    graph.connect(&f.library, (late, "out"), (three, "parts")).unwrap();

    nodez::layered(&mut graph, &LayoutOptions::default(), |_graph, _node| {
        egui::vec2(160.0, 80.0)
    })
    .unwrap();

    let x = |id| graph.node(id).unwrap().position.x;
    assert_eq!(
        x(late),
        x(two),
        "the late input is in the column before the node it feeds"
    );
    assert!(
        x(late) > x(head),
        "and well clear of the first column it used to be stranded in"
    );
}

/// And it lands level with the average of what it is wired to, so the wires
/// between two columns are close to straight.
#[test]
fn a_node_lands_level_with_what_feeds_it() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let b = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
    let both = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    graph.connect(&f.library, (a, "out"), (both, "parts")).unwrap();
    graph.connect(&f.library, (b, "out"), (both, "parts")).unwrap();

    nodez::layered(&mut graph, &LayoutOptions::default(), |_graph, _node| {
        egui::vec2(160.0, 80.0)
    })
    .unwrap();

    let middle = |id| graph.node(id).unwrap().position.y + 40.0;
    assert!(
        (middle(both) - (middle(a) + middle(b)) / 2.0).abs() < 0.5,
        "{} is not halfway between {} and {}",
        middle(both),
        middle(a),
        middle(b)
    );
}

#[cfg(feature = "serde")]
#[test]
fn graphs_round_trip_through_json() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(10.0, 20.0));
    let b = graph.add_node(&f.library, f.sink, pos2(300.0, 20.0));
    graph.node_mut(a).unwrap().set_input_value("value", "round trip");
    graph.connect(&f.library, (a, "out"), (b, "value")).unwrap();

    let json = serde_json::to_string(&graph).unwrap();
    let mut restored: Graph = serde_json::from_str(&json).unwrap();
    assert!(restored.validate(&f.library).is_clean());

    assert_eq!(restored.node_count(), 2);
    assert_eq!(restored.connection_count(), 1);
    assert_eq!(
        restored.node(a).unwrap().input_value("value").as_deref(),
        Some(&Value::Text("round trip".to_owned()))
    );

    // Ids must not be handed out again after a load.
    let c = restored.add_node(&f.library, f.text, pos2(0.0, 0.0));
    assert_ne!(c, a);
    assert_ne!(c, b);
}

#[test]
fn multi_input_order_survives_a_rewire() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));

    let mut links = Vec::new();
    for part in ["a", "b", "c"] {
        let n = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
        graph.node_mut(n).unwrap().set_input_value("value", part);
        links.push(graph.connect(&f.library, (n, "out"), (join, "parts")).unwrap());
    }
    let order = |g: &Graph| -> Vec<String> {
        g.links_into(join, "parts")
            .filter_map(|c| g.node(c.from.node)?.input_value("value"))
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    assert_eq!(order(&graph), ["a", "b", "c"]);

    // Unplug the middle link and plug it straight back in.
    let middle = graph.connection(links[1]).unwrap().clone();
    graph.disconnect(links[1]);
    assert_eq!(order(&graph), ["a", "c"]);
    graph
        .connect_at(&f.library, middle.from.clone(), middle.to.clone(), 1)
        .unwrap();
    assert_eq!(order(&graph), ["a", "b", "c"]);
}

#[test]
fn a_link_can_be_inserted_ahead_of_the_first() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));

    let text = |graph: &mut Graph, part: &str| {
        let n = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
        graph.node_mut(n).unwrap().set_input_value("value", part);
        n
    };
    for part in ["b", "c"] {
        let n = text(&mut graph, part);
        graph.connect(&f.library, (n, "out"), (join, "parts")).unwrap();
    }
    // Slot 0 is the head of the queue, not a replacement for whoever is there.
    let first = text(&mut graph, "a");
    graph
        .connect_at(&f.library, (first, "out"), (join, "parts"), 0)
        .unwrap();

    let order: Vec<String> = graph
        .links_into(join, "parts")
        .filter_map(|c| graph.node(c.from.node)?.input_value("value"))
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert_eq!(order, ["a", "b", "c"]);
}

#[test]
fn links_can_be_reordered() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));

    let mut links = Vec::new();
    for part in ["a", "b", "c", "d"] {
        let n = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
        graph.node_mut(n).unwrap().set_input_value("value", part);
        links.push(graph.connect(&f.library, (n, "out"), (join, "parts")).unwrap());
    }
    let order = |g: &Graph| -> Vec<String> {
        g.links_into(join, "parts")
            .filter_map(|c| g.node(c.from.node)?.input_value("value"))
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };

    assert!(graph.reorder_link(links[3], 0));
    assert_eq!(order(&graph), ["d", "a", "b", "c"]);
    assert!(graph.reorder_link(links[3], 2));
    assert_eq!(order(&graph), ["a", "b", "d", "c"]);
    // Past the end clamps rather than leaving a gap.
    assert!(graph.reorder_link(links[0], 99));
    assert_eq!(order(&graph), ["b", "d", "c", "a"]);
    assert!(!graph.reorder_link(nodez::ConnectionId(999), 0));
}

#[test]
fn removing_a_link_closes_the_gap() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(200.0, 0.0));
    let mut links = Vec::new();
    for _ in 0..3 {
        let n = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
        links.push(graph.connect(&f.library, (n, "out"), (join, "parts")).unwrap());
    }
    graph.disconnect(links[0]);
    let orders: Vec<u32> = graph.links_into(join, "parts").map(|c| c.order).collect();
    assert_eq!(orders, [0, 1]);
}

#[cfg(feature = "serde")]
#[test]
fn order_survives_a_save_and_load() {
    let f = fixture();
    let mut graph = Graph::new();
    let join = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let mut sources = Vec::new();
    for part in ["a", "b", "c"] {
        let n = graph.add_node(&f.library, f.text, pos2(0.0, 0.0));
        graph.node_mut(n).unwrap().set_input_value("value", part);
        graph.connect(&f.library, (n, "out"), (join, "parts")).unwrap();
        sources.push(n);
    }
    let last = graph.links_into(join, "parts").last().unwrap().id;
    graph.reorder_link(last, 0);

    let restored: Graph = serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
    let order: Vec<NodeId> = restored
        .links_into(join, "parts")
        .map(|c| c.from.node)
        .collect();
    assert_eq!(order, vec![sources[2], sources[0], sources[1]]);
}
