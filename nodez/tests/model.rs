//! Tests for the graph model: typing rules, cycle refusal and traversal.

use egui::{Color32, pos2};
use nodez::{
    ConnectError, Graph, LayoutOptions, NodeLibrary, NodeTemplate, SocketSpec, TemplateId, Value,
    Widget,
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
                "text" => ctx.literal_str("value").unwrap_or_default().to_owned(),
                "number" => ctx.literal_f64("value").unwrap_or_default().to_string(),
                "join" => {
                    let separator = ctx.param_str("separator").unwrap_or_else(|| " ".to_owned());
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
        graph.node(node).unwrap().param("separator"),
        Some(Value::Text(" ".to_owned()))
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

#[test]
fn layered_layout_sorts_into_columns() {
    let f = fixture();
    let mut graph = Graph::new();
    let a = graph.add_node(&f.library, f.text, pos2(500.0, 500.0));
    let b = graph.add_node(&f.library, f.join, pos2(0.0, 0.0));
    let c = graph.add_node(&f.library, f.sink, pos2(-300.0, 900.0));
    graph.connect(&f.library, (a, "out"), (b, "parts")).unwrap();
    graph.connect(&f.library, (b, "out"), (c, "value")).unwrap();

    nodez::layered(&mut graph, &LayoutOptions::default(), |_graph, _node| 80.0).unwrap();

    let x = |id| graph.node(id).unwrap().position.x;
    assert!(x(a) < x(b));
    assert!(x(b) < x(c));
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
        restored.node(a).unwrap().input_value("value"),
        Some(Value::Text("round trip".to_owned()))
    );

    // Ids must not be handed out again after a load.
    let c = restored.add_node(&f.library, f.text, pos2(0.0, 0.0));
    assert_ne!(c, a);
    assert_ne!(c, b);
}
