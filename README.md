# nodez
> [!NOTE]
> This code (and documentation) are created with a great deal of
> AI assistance.  However, this is a library that I needed and I didn't
> like the look-and-feel, nor the usage model, of the existing node-editing
> libraries. This library will be maintained, as it is needed for another
> (non-AI) project.


A Blender-style node editor for [egui](https://github.com/emilk/egui), with a
typed graph you can walk.

Sockets are color-coded and typed: the editor refuses a drag between
incompatible sockets, and so does the API, so a graph on disk is always
well-typed. You describe your nodes as Rust structs and the rest is generated.

![The demo app: a node graph on the left, the config it generates on the right](https://raw.githubusercontent.com/ahenshaw/nodez/main/docs/screenshot.png)

## Quickstart

```toml
[dependencies]
nodez = { version = "0.2", features = ["derive", "app"] }
```

Four kinds of node that build URLs. This is a whole program:

```rust
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
```

```
cargo run -p nodez --features derive,app --example quickstart
```

![The quickstart: five nodes building a URL](https://raw.githubusercontent.com/ahenshaw/nodez/main/docs/quickstart.png)

## The derive, at a glance

Everything is optional; the second column is what you get if you say nothing.

**`#[derive(SocketType)]` — a value that travels along a wire**

| `#[socket(…)]` | default | |
|---|---|---|
| `color = "#RRGGBB"` | hashed from the name | the socket and the wires leaving it |
| `shape = "…"` | `circle` | `circle`, `diamond`, `diamond_dot`, `square` |
| `widget = …` | none | `text`, `int`, `float`, `checkbox`, `choice` |
| `name = "…"` | the type's own name | what it registers and displays as |
| `description = "…"` | — | socket tooltip |
| `wildcard` | off | connects to every other type |
| `rename = "…"` | variant name, kebab-cased | on an enum variant: its spelling in the dropdown |

A type with a `widget` can be typed into when nothing is wired to it; one
without is link-only. `choice` wants a fieldless enum, the rest a one-field
tuple struct over `String`, `i64`, `f64` or `bool`. Those four are wire types
already, so a field that is just a string or a number needs no wrapper.

**`#[derive(NodeType)]` — a kind of node**

| `#[node(…)]` | default | |
|---|---|---|
| `id = "…"` | struct name, snake_cased | what saved files refer to |
| `label = "…"` | struct name, title-cased | shown in the header and the menu |
| `category = "…"` | `Misc` | menu grouping and header tint |
| `description = "…"` | — | tooltip in the add menu |
| `keywords = "a, b"` | — | extra terms the add-menu search matches |
| `width = …` | `150.0` | starting body width; rarely worth setting |
| `output = T` | none | an output socket carrying `T`, **and** `Output = T` |
| `produces = T` | `()` | `Output = T` with no socket, for a sink |
| `output_name = "…"` | `out` | renames the output socket |
| `header_color = "#RRGGBB"` | from the category | overrides the tint for this kind alone |

`output` and `produces` both fix the type your `evaluate` returns; only `output`
also puts a socket on the node. A node with nothing downstream — the root of a
document, say — uses `produces`. Giving both is a compile error, and so is
returning anything else from `evaluate`.

**Fields**

| you write | you get |
|---|---|
| `#[input] x: T` | a socket that must be wired |
| `#[input] x: Option<T>` | a link-only socket that may be wired |
| `#[input] x: Multi<T>` | a socket accepting any number of links, in order |
| `x: T` (no attribute) | a parameter, drawn in the body, never wired |

| `#[input(…)]`, `#[param(…)]` | |
|---|---|
| `default = …` | what a new node starts with |
| `label = "…"` | the row's label; `""` hides it |
| `hint = "…"` | placeholder shown in an empty text box |
| `description = "…"` | socket tooltip |
| `min = …`, `max = …` | clamp a numeric widget, per socket rather than per type |
| `hide_label` | parameters only: let the widget fill the row |

A `Multi` socket draws one attachment point per link plus an empty one below
them, so where you drop a wire decides where it lands in the order, and dragging
a link up or down reorders it. The order is stored per link, not inferred from
when it was made.

`Option` only says something on a link-only socket. An editable one always has a
value, whether or not anything is wired to it, so an `Option<String>` would
never be `None`; a debug assertion says so when a template is built.

## What a fold is

`Fold` names one way of walking the graph. It is a marker type holding nothing:

```rust
struct Urls;
impl Fold for Urls {}
```

It exists so a node kind can be evaluated more than one way without having to
pick one — the real thing and a redacted preview, say. `Rules<F>` holds the
rules for one fold, and you can build several over the same library:

```rust
impl Evaluate<Urls>    for Address { .. }
impl Evaluate<Preview> for Address { .. }
```

A fold varies *how* each node computes, not what it produces: every node still
yields whatever its output socket declares, because that is what the nodes
downstream read back. If you only ever walk the graph one way — and most
projects do — one marker type is all you will write, and it costs two lines.

## What a category is

`category` is a string you invent. It does three things, all cosmetic:

- groups nodes under a heading in the `Shift+A` menu and in the palette
- gives every node in it the same header tint
- is matched by the add-menu search, so typing `build` finds the whole group

Nothing in the library knows the names. `Input`, `Build` and `Output` in the
quickstart are only what that example chose, and they are unrelated to the fold
marker beside them. Categories appear in the menu in the order you register
them, and exist as soon as a node names one.

A node naming no category lands in `Misc` and takes a header color derived from
its own id, so uncategorised nodes stay distinguishable from each other.

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
| `Ctrl`+`Z` | undo the last move |
| `A` / `Alt`+`A` | select all / none |
| `Home` / `.` | frame everything / the selection |
| Double-click a header | rename |
| `RMB` on a node | context menu, including align and spacing |

## Reading a graph

`Graph` is a DAG, and everything below is on it.

| | |
|---|---|
| `nodes()`, `connections()`, `nodes_of_template()` | contents |
| `incoming()`, `outgoing()`, `links_into()`, `links_from()` | wires at a node or socket |
| `predecessors()`, `successors()` | immediate neighbors |
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

Graphs serialize with serde, and `Graph::validate` repairs one loaded against a
library that has since changed.

## Arranging nodes

`nodez::layered` arranges a whole graph into dependency columns — good for one
built in code, or for tidying up after a load. `align` and `distribute` work on
a handful of nodes instead, and are what the editor's `RMB` → Align menu calls.

```rust
nodez::align(&mut graph, editor.state.selection(), nodez::Align::Left, size_of);
nodez::distribute(&mut graph, ids, nodez::Axis::Y, nodez::Spacing::Even, size_of);
```

`Spacing::Even` equalizes the gaps and leaves the outermost two nodes where
they are; `Spacing::Fixed(gap)` stacks everything a set distance apart. Gaps go
between bounding boxes, so nodes of different heights come out evenly spaced
rather than evenly staggered. Both return the nodes that actually moved.

All three take a `size_of` closure so they can measure what the editor draws:

```rust
|graph, node| nodez::node_size(graph, library, node, style)
```

## Keeping wires out from under nodes

Columns alone do not stop a wire from crossing whatever stands between its
ends, so `route_links` steers those around. Run it after `layered` — the
demo's Auto layout button does both:

```rust
nodez::layered(&mut graph, &LayoutOptions::default(), size_of)?;
nodez::route_links(&mut graph, &RouteOptions::default(), size_of, anchor_of)?;
```

`anchor_of` says where a wire attaches, which `socket_anchor` answers:

```rust
|graph, socket, kind| {
    let node = graph.node(socket.node)?;
    nodez::socket_anchor(graph, library, node, style, kind, &socket.socket)
}
```

A wire whose own curve already clears everything in its way is left alone, so
simple graphs keep their plain noodles. Anything else crosses every column in
its way at a single height, picked clear of all of them and, where it can be,
level with one end — so a routed wire bends twice at most, and often once.

It writes `Connection::waypoints`, points the wire bends through. Those are
only how the wire is drawn: traversal, evaluation and the config you generate
see exactly what they would have seen unrouted. An unrouted graph serializes
without the field at all.

`RouteOptions` carries a copy of the wire shape (`curvature`, `min_curve`,
`max_curve`) so the router can judge where a wire really goes. If you change
those on `EditorStyle`, change them here too.

Waypoints stay where they are put, so moving a node afterwards does not
re-route its wires. Run `route_links` again to redo them, or clear
`waypoints` on a connection to straighten one back out.

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
and selection. `EditorStyle` holds every color and metric, defaulting to
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
