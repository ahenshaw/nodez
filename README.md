# nodez

A Blender-style node editor for [egui](https://github.com/emilk/egui), with a
typed graph you can walk.

Sockets are colour-coded and typed: the editor refuses a drag between
incompatible sockets, and so does the API, so a graph on disk is always
well-typed. You describe your nodes as Rust structs and the rest is generated.

![The demo app: a node graph on the left, the config it generates on the right](docs/screenshot.png)

## Quickstart

```toml
[dependencies]
nodez = { version = "0.1", features = ["derive", "app"] }
```

Four kinds of node that build URLs. This is a whole program:

```rust
use nodez::app::{EditorApp, Preview};
use nodez::{Evaluate, Fold, Graph, Multi, NodeError, NodeLibrary, NodeType, Payload, Rules,
            SocketType};

// 1. Values that travel along wires. String, i64, f64 and bool already are
//    wire types; anything else you declare. The colour comes from the name.
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

// 3. What each kind does.
struct Build;
impl Fold for Build {}

impl Evaluate<Build> for Text {
    fn evaluate(&self) -> Result<String, NodeError> { Ok(self.value.clone()) }
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

// 4. A window.
fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Build>::new();
    rules.register_all::<(Text, Join, Address, Collect)>(&mut library);

    EditorApp::new(library)
        .title("nodez quickstart")
        .preview(move |graph, library| Preview::text(run(graph, library, &rules)))
        .run()
}

fn run(graph: &Graph, library: &NodeLibrary, rules: &Rules<Build>) -> String {
    let Some(target) = graph.nodes_of_template(library.id("collect").unwrap()).next()
    else { return "add a Collect node".to_owned() };
    match graph.evaluate::<Payload, NodeError>(library, target.id, |ctx| rules.run(&ctx)) {
        Ok(payload) => *payload.downcast::<String>().unwrap_or_default(),
        Err(e) => e.to_string(),
    }
}
```

```
cargo run -p nodez --features derive,app --example quickstart
```

![The quickstart: five nodes building a URL](docs/quickstart.png)

## What a field means

| you write | you get |
|---|---|
| `#[input] x: T` | a socket that must be wired |
| `#[input] x: Option<T>` | a socket that may be wired |
| `#[input] x: Multi<T>` | a socket accepting any number of links |
| `x: T` (no attribute) | a parameter, drawn in the body, never wired |

A wire type that offers an inline editor — `String`, `i64`, `f64`, `bool`, or
anything with `#[socket(widget = …)]` — makes a socket you can also type into.
One that doesn't is link-only.

`#[input]` takes `default`, `label`, `hint`, `description`, `min` and `max`.
`#[node]` takes `id`, `label`, `category`, `description`, `keywords`, `width`,
`output`, `produces` and `header_color`, and all of them have defaults — `id`
and `label` come from the struct name.

## Controls

| | |
|---|---|
| `LMB` | select; drag on empty canvas to box-select |
| `Shift`+`LMB` | add to the selection |
| `MMB` drag, trackpad scroll | pan |
| Wheel, `Ctrl`+scroll, pinch | zoom about the cursor |
| `Shift`+`A` | add-node search |
| Drag a wire into empty space | add-node search, filtered to what can take it |
| Drag off a wired input | pick the wire up and move it |
| `Ctrl`+drag, double-click a wire | cut wires |
| `G` | grab: the selection follows the pointer, `Esc` cancels |
| `Shift`+`D` | duplicate, keeping the wires between the copies |
| `X` / `Del` | delete the selection |
| `H` / `M` | collapse / mute |
| `A` / `Alt`+`A` | select all / none |
| `Home` / `.` | frame everything / the selection |
| Double-click a header | rename |
| `RMB` on a node | context menu |

## Reading a graph

`Graph` is a DAG, and everything below is on it.

| | |
|---|---|
| `nodes()`, `connections()`, `nodes_of_template()` | contents |
| `incoming()`, `outgoing()`, `links_into()`, `links_from()` | wires at a node or socket |
| `predecessors()`, `successors()` | immediate neighbours |
| `ancestors()`, `descendants()`, `walk()` | transitive walks, as iterators |
| `roots()`, `sinks()`, `isolated()` | ends of the graph |
| `topological_order()`, `dependency_order()` | evaluation order |
| `components()`, `depths()`, `find_cycle()` | shape |
| `evaluate()`, `evaluate_all()` | folds |

`evaluate` visits only what the target depends on; `evaluate_all` visits
everything. Both hand each node the results of everything upstream.

```rust
graph.connect(&library, (a, "out"), (b, "parts"))?;      // Ok
graph.connect(&library, (b, "out"), (a, "parts"))        // Err(WouldCycle)
graph.can_connect(&library, &from, &to)                   // Err: "Text cannot drive Int"
```

Graphs serialise with serde, and `Graph::validate` repairs one loaded against a
library that has since changed. `nodez::layered` arranges a graph built in code.

## Just the widget

`EditorApp` is a window; `NodeEditor` is the canvas alone, for dropping into an
app you already have.

```rust
let response = self.editor.show(ui, &self.library, &mut self.graph);
if response.changed {
    self.regenerate();
}
```

`response.actions` reports what happened — `NodeAdded`, `Connected`,
`InputChanged`, `ConnectionRejected(..)` — and `editor.state` holds pan, zoom
and selection. `EditorStyle` holds every colour and metric, defaulting to
Blender's dark theme; `EditorStyle::light()` is the other preset.

## The demo

[`nodez-demo`](nodez-demo) is the app in the first screenshot: fourteen node
kinds that generate a container-stack config file.

```
cargo run                  # the editor
cargo run -- --print       # generate the sample config headlessly
```

[`nodes.rs`](nodez-demo/src/nodes.rs) is the whole domain — wire types, node
kinds, and what each one emits.

## Crates

| | |
|---|---|
| [`nodez/`](nodez) | the library: graph, traversal, editor widget, `app` window |
| [`nodez-derive/`](nodez-derive) | the derive macros |
| [`nodez-demo/`](nodez-demo) | the config-generator app |

## License

MIT OR Apache-2.0
