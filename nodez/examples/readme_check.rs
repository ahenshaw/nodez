//! Compiled copy of the README's quickstart block. If this fails, the
//! README is wrong.

use nodez::app::{EditorApp, Preview};
use nodez::{Evaluate, Fold, Graph, Multi, NodeError, NodeLibrary, NodeType, Payload, Rules,
            SocketType};

// 1. Values that travel along wires. String, i64, f64 and bool already are
//    wire types; anything else you declare. The color comes from the name.
#[derive(Clone, Debug, SocketType)]
struct Url(String);

// 2. Kinds of node. A field with #[input] is a socket, a bare field is a
//    parameter drawn in the body, and the field's type does the rest.
#[derive(Debug, NodeType)]
#[node(category = "Input", output = String)]
struct Text {
    #[input(hint = "text…")]
    value: String,
}

#[derive(Debug, NodeType)]
#[node(category = "Build", output = String, label = "Join Path")]
struct Join {
    #[param(default = "/")]
    separator: String,
    #[input]
    parts: Multi<String>,          // Multi: accepts any number of links
}

#[derive(Debug, NodeType)]
#[node(category = "Build", output = Url)]
struct Address {
    #[input(default = "example.com")]
    host: String,
    #[input(default = 443, min = 1, max = 65535)]
    port: i64,                     // i64 is editable, so it gets a drag box
    #[input]
    path: String,
}

#[derive(Debug, NodeType)]
#[node(category = "Output", produces = String)]
struct Collect {
    #[input]
    urls: Multi<Url>,              // Url has no editor, so this is link-only
}

// 3. What each kind does. A `Fold` names one way of walking the graph. It is
//    unrelated to a category, which only groups nodes in the menu.
struct Urls;
impl Fold for Urls {}

impl Evaluate<Urls> for Text {
    fn evaluate(&self) -> Result<String, NodeError> { Ok(self.value.clone()) }
}
impl Evaluate<Urls> for Join {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.parts.iter().cloned().collect::<Vec<_>>().join(&self.separator))
    }
}
impl Evaluate<Urls> for Address {
    fn evaluate(&self) -> Result<Url, NodeError> {
        let scheme = if self.port == 443 { "https" } else { "http" };
        Ok(Url(format!("{scheme}://{}:{}/{}", self.host, self.port, self.path)))
    }
}
impl Evaluate<Urls> for Collect {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.urls.iter().map(|u| u.0.as_str()).collect::<Vec<_>>().join("\n"))
    }
}

// 4. A window.
fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Urls>::new();
    rules.register_all::<(Text, Join, Address, Collect)>(&mut library);

    EditorApp::new(library)
        .title("nodez quickstart")
        .preview(move |graph, library| Preview::text(run(graph, library, &rules)))
        .run()
}

fn run(graph: &Graph, library: &NodeLibrary, rules: &Rules<Urls>) -> String {
    let Some(target) = graph.nodes_of_template(library.id("collect").unwrap()).next()
    else { return "add a Collect node".to_owned() };
    match graph.evaluate::<Payload, NodeError>(library, target.id, |ctx| rules.run(&ctx)) {
        Ok(payload) => *payload.downcast::<String>().unwrap_or_default(),
        Err(e) => e.to_string(),
    }
}
