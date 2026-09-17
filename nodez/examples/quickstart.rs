//! A complete nodez app: four kinds of node that build URLs.
//!
//!     cargo run -p nodez --features derive,app --example quickstart

use nodez::app::{EditorApp, Preview};
use nodez::{
    Evaluate, Fold, Graph, Multi, NodeError, NodeLibrary, NodeType, Payload, Rules, SocketType,
};

// 1. The values that travel along wires. `String`, `i64`, `f64` and `bool` are
//    already wire types; anything else you declare. The colour comes from the
//    name, so there is nothing to choose.
#[derive(Clone, Debug, SocketType)]
#[socket(description = "A complete URL.")]
struct Url(String);

// 2. The kinds of node. A field with `#[input]` is a socket; a bare field is a
//    parameter drawn in the node body. The field's type decides the rest:
//    `Multi<T>` accepts many links, and a type with no inline editor — like
//    `Url` — makes a socket that can only be wired.
#[derive(Debug, NodeType)]
#[node(category = "Input", output = String)]
struct Text {
    #[input(hint = "text\u{2026}")]
    value: String,
}

#[derive(Debug, NodeType)]
#[node(category = "Build", output = String, label = "Join Path")]
struct Join {
    #[param(default = "/")]
    separator: String,
    #[input]
    parts: Multi<String>,
}

#[derive(Debug, NodeType)]
#[node(category = "Build", output = Url)]
struct Address {
    #[input(default = "example.com")]
    host: String,
    #[input(default = 443, min = 1, max = 65535)]
    port: i64,
    #[input]
    path: String,
}

#[derive(Debug, NodeType)]
#[node(category = "Output", produces = String)]
struct Collect {
    #[input]
    urls: Multi<Url>,
}

// 3. What each kind does. `Fold` names one way of walking the graph; you can
//    add others later without touching the nodes above.
struct Build;
impl Fold for Build {}

impl Evaluate<Build> for Text {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.value.clone())
    }
}

impl Evaluate<Build> for Join {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.parts.iter().cloned().collect::<Vec<_>>().join(&self.separator))
    }
}

impl Evaluate<Build> for Address {
    fn evaluate(&self) -> Result<Url, NodeError> {
        let scheme = if self.port == 443 { "https" } else { "http" };
        Ok(Url(format!("{scheme}://{}:{}/{}", self.host, self.port, self.path)))
    }
}

impl Evaluate<Build> for Collect {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.urls.iter().map(|u| u.0.as_str()).collect::<Vec<_>>().join("\n"))
    }
}

fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Build>::new();
    rules.register_all::<(Text, Join, Address, Collect)>(&mut library);

    let graph = starting_graph(&library);
    EditorApp::new(library)
        .graph(graph)
        .title("nodez quickstart")
        .preview(move |graph, library| Preview::text(run(graph, library, &rules)))
        .run()
}

/// Fold the graph and show whatever the Collect node came to.
fn run(graph: &Graph, library: &NodeLibrary, rules: &Rules<Build>) -> String {
    let Some(target) = graph.nodes_of_template(library.id("collect").unwrap()).next()
    else { return "add a Collect node".to_owned() };
    match graph.evaluate::<Payload, NodeError>(library, target.id, |ctx| rules.run(&ctx)) {
        Ok(payload) => *payload.downcast::<String>().unwrap_or_default(),
        Err(e) => e.to_string(),
    }
}

/// Something to look at on the first run. Everything here is also reachable
/// from the editor: Shift+A adds a node, and dragging between sockets wires it.
fn starting_graph(library: &NodeLibrary) -> Graph {
    let mut graph = Graph::new();
    let add = |graph: &mut Graph, id: &str| {
        graph.add_node(library, library.id(id).unwrap(), egui::pos2(0.0, 0.0))
    };

    let docs = add(&mut graph, "text");
    let page = add(&mut graph, "text");
    let path = add(&mut graph, "join");
    let address = add(&mut graph, "address");
    let collect = add(&mut graph, "collect");

    graph.node_mut(docs).unwrap().set_input_value("value", "docs");
    graph.node_mut(page).unwrap().set_input_value("value", "quickstart");

    for (from, to, socket) in [
        (docs, path, "parts"),
        (page, path, "parts"),
        (path, address, "path"),
        (address, collect, "urls"),
    ] {
        graph.connect(library, (from, "out"), (to, socket)).unwrap();
    }
    let _ = nodez::layered(&mut graph, &nodez::LayoutOptions::default(), |graph, node| {
        nodez::node_size(graph, library, node, &nodez::EditorStyle::default()).y
    });
    graph
}
