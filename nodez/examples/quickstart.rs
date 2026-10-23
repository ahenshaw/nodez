//! A complete nodez app: four kinds of node that build URLs.
//!
//!     cargo run -p nodez --features derive,app --example quickstart

use nodez::app::{EditorApp, Preview};
use nodez::{eframe, egui};
use nodez::{
    Evaluate, Fold, Graph, NodeError, NodeLibrary, NodeType, Payload, Rules, SocketType,
};

// 1. The kinds of node. A field with `#[input]` is a socket; a bare field is a
//    parameter drawn in the node body. The field's type decides the rest:
//    `String`, the numbers and `bool` get an inline editor, and `Vec<T>`
//    accepts many links.
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
    parts: Vec<String>,
}

// A struct of plain data can be its own node and its own wire type. With
// `output = Self` its inputs fill its fields and it is what comes out, so it
// needs no `evaluate`. The wire color comes from the name. It has no inline
// editor, so a socket taking an `Address` can only be wired.
#[derive(Clone, Debug, SocketType, NodeType)]
#[socket(description = "Where a page lives.")]
#[node(category = "Build", output = Self)]
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
    addresses: Vec<Address>,
}

impl Address {
    fn url(&self) -> String {
        let scheme = if self.port == 443 { "https" } else { "http" };
        format!("{scheme}://{}:{}/{}", self.host, self.port, self.path)
    }
}

// 2. What the other kinds do. `Fold` names one way of walking the graph — named
//    after what it produces, like a category is named after what it groups.
//    The two are unrelated; you can add more folds without touching the nodes.
struct Urls;
impl Fold for Urls {}

impl Evaluate<Urls> for Text {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.value.clone())
    }
}

impl Evaluate<Urls> for Join {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.parts.join(&self.separator))
    }
}

impl Evaluate<Urls> for Collect {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.addresses.iter().map(Address::url).collect::<Vec<_>>().join("\n"))
    }
}

fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Urls>::new();
    rules.register_all::<(Text, Join, Address, Collect)>(&mut library);

    let graph = starting_graph(&library);
    EditorApp::new(library)
        .graph(graph)
        .title("nodez quickstart")
        .preview(move |graph, library| Preview::text(run(graph, library, &rules)))
        .run()
}

/// Fold the graph and show whatever the Collect node came to.
fn run(graph: &Graph, library: &NodeLibrary, rules: &Rules<Urls>) -> String {
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
        (address, collect, "addresses"),
    ] {
        graph.connect(library, (from, "out"), (to, socket)).unwrap();
    }
    let _ = nodez::layered(&mut graph, &nodez::LayoutOptions::default(), |graph, node| {
        nodez::node_size(graph, library, node, &nodez::EditorStyle::default())
    });
    graph
}
