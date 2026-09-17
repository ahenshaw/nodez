# nodez

A Blender-style node editor for [egui](https://github.com/emilk/egui), plus the
typed graph model behind it.

Sockets are colour-coded and typed: the editor refuses a drag between
incompatible sockets, and so does `Graph::connect` in code, so a graph on disk
is always well-typed. The traversal API turns that graph into whatever your
domain needs — the bundled demo turns it into a container-stack config file.

![The demo app: a node graph on the left, the config it generates on the right](docs/screenshot.png)

## Layout

| Crate | What it is |
|---|---|
| [`nodez/`](nodez) | The library: graph model, traversal, and the egui editor widget |
| [`nodez-demo/`](nodez-demo) | A visual generator for container-stack config files |
| [`nodez-derive/`](nodez-derive) | Derive macros describing nodes as Rust types |

```
cargo run                  # the demo app
cargo run -- --print       # generate the sample config headlessly
cargo test                 # model, emitter and doc tests

cargo run -p nodez --features derive --example typed_nodes           # the prototype
cargo run -p nodez --features derive --example typed_nodes -- --edit # …in the editor
```

## The two halves

The crate splits cleanly in two, and the model half never needs the editor
running — the same graph can be loaded and rendered from a build script or a
CLI.

### The model

`NodeLibrary` is the schema for your domain: the socket types, their colours,
and the node templates. `Graph` holds an instance of it.

```rust
use nodez::{Graph, NodeLibrary, NodeTemplate, SocketSpec, Widget};
use egui::Color32;

let mut library = NodeLibrary::new();
let text   = library.types.add("Text",   Color32::from_rgb(0x70, 0xB2, 0xFF));
let number = library.types.add("Number", Color32::from_rgb(0xA1, 0xA1, 0xA1));

// A Number may be dropped into a Text socket. The reverse is refused.
library.types.allow_cast(number, text);

let join = library.register(
    NodeTemplate::new("join", "Join Text")
        .category("Convert")
        .input(SocketSpec::new("parts", text).multi())   // accepts a fan-in
        .param(nodez::ParamSpec::new("separator", Widget::text()))
        .output(SocketSpec::new("out", text)),
);
```

`Graph` enforces its own invariants on every edit — types must be compatible,
single-link inputs hold at most one wire, and no edit may introduce a cycle — so
traversal code can rely on the graph being a DAG without re-checking.

```rust
graph.connect(&library, (a, "out"), (b, "parts"))?;      // Ok
graph.connect(&library, (b, "out"), (a, "parts"))        // Err(WouldCycle)
```

### Traversal

Iterators and folds for walking the graph, all on `Graph`:

| | |
|---|---|
| `nodes()`, `connections()`, `nodes_of_template()` | contents |
| `incoming()`, `outgoing()`, `links_into()`, `links_from()` | wires at a node or socket |
| `predecessors()`, `successors()`, `neighbors()` | immediate neighbours |
| `ancestors()`, `descendants()`, `walk()` | transitive walks, as `Iterator`s |
| `roots()`, `sinks()`, `isolated()` | ends of the graph |
| `topological_order()`, `iter_topological()`, `dependency_order()` | evaluation order |
| `components()`, `component_of()`, `depths()`, `find_cycle()` | shape |
| `input_source()`, `source_of()`, `param()` | resolving one input |
| `evaluate()`, `evaluate_all()`, `for_each_topological()` | folds |

`Value::map()` builds the ordered maps most config formats want, skipping
entries that would be empty — which is most of what assembling a document by
hand costs:

```rust
let body = Value::map()
    .set("image", image)
    .set_if(replicas > 1, "deploy", Value::map().set("replicas", replicas))
    .set_list("ports", ports)          // key dropped when the list is empty
    .set_some("healthcheck", probe);   // key dropped when there is no probe
```

`evaluate` is the one that turns a graph into a config file. It calls your
closure once per node, in dependency order, handing it the results of everything
upstream:

```rust
let yaml = graph.evaluate::<Fragment, String>(&library, stack_node, |ctx| {
    match ctx.template().id.as_str() {
        "text" => Ok(Fragment::Text(ctx.literal_str("value").unwrap_or_default().into())),
        "join" => {
            let separator = ctx.param_str("separator").unwrap_or(" ");
            let parts: Vec<_> = ctx.inputs("parts").iter().filter_map(|l| l.value.text()).collect();
            Ok(Fragment::Text(parts.join(separator)))
        }
        other => Err(format!("no rule for `{other}`")),
    }
})?;
```

`ctx.inputs(socket)` gives the upstream results in connection order;
`ctx.literal(socket)` and `ctx.param(name)` give the values the user typed into
the node. `evaluate` visits only what the target depends on;
`evaluate_all` visits everything.

Graphs built in code can be arranged with `nodez::layered`, which places nodes in
columns by dependency depth and sweeps to reduce crossings.

### The editor

```rust
let response = self.editor.show(ui, &self.library, &mut self.graph);
if response.changed {
    self.regenerate();
}
```

`EditorResponse::actions` reports what happened — `NodeAdded`, `Connected`,
`InputChanged`, `ConnectionRejected(..)` and so on — so the host app can react
without diffing the graph. `NodeEditor::state` holds pan, zoom and selection,
and is public so the app can drive the view.

Everything visual lives in `EditorStyle`, which defaults to Blender's dark
theme. `EditorStyle::light()` is the other preset.

## Controls

| | |
|---|---|
| `LMB` | select; drag on empty canvas to box-select |
| `Shift`+`LMB` | add to the selection |
| `MMB` drag | pan |
| Trackpad two-finger scroll | pan |
| Wheel | zoom about the cursor |
| `NodeEditor::scroll_mode` | force a bare scroll to always pan or always zoom |
| `Ctrl`/`Cmd`+scroll, pinch | zoom about the cursor |
| `Shift`/`Alt`+wheel | pan horizontally / vertically |
| `Shift`+`A` | add-node search |
| Drag a wire into empty space | add-node search, filtered to what can take that wire |
| Drag off a wired input | pick the wire up and move it |
| `Ctrl`+drag | cut every wire the stroke crosses |
| Double-click a wire | cut it |
| `G` | grab: the selection follows the pointer until a click confirms, `Esc` cancels |
| `Shift`+`D` | duplicate, keeping the wires between the copies |
| `X` / `Del` | delete the selection |
| `H` | collapse / expand |
| `M` | mute (kept in the graph, excluded from the config) |
| `A` / `Alt`+`A` | select all / none |
| `Home` / `.` | frame everything / the selection |
| Double-click a header | rename |
| Drag a node's right edge | resize |
| `RMB` on a node | context menu |

## The demo

`nodez-demo` describes a container stack as a graph and emits a compose-style
YAML document from it. It shows the pieces working together:

- **[`domain.rs`](nodez-demo/src/domain.rs)** — the socket types, their colours
  and casts, and the fourteen node templates. This is the file you would replace
  for your own format.
- **[`generate.rs`](nodez-demo/src/generate.rs)** — `evaluate` folding each node
  into a `Fragment` and the stack node assembling the document. It also uses
  `ancestors()` to report which nodes are not wired to the output.
- **[`yaml.rs`](nodez-demo/src/yaml.rs)** — an emitter over `nodez::Value`, which
  is an ordered document tree, so generated keys keep the order the templates
  declare.
- **[`sample.rs`](nodez-demo/src/sample.rs)** — the starting graph, built entirely
  in code and placed by `nodez::layered`.

The left panel's inspector shows the traversal API live: the active node's direct
and transitive dependencies and dependents, and the whole graph's evaluation
order with unreachable nodes dimmed.

Save / Load round-trip the graph as JSON (serde, on by default). A graph saved
against an older library is repaired on load by `Graph::validate`, which drops
nodes whose template is gone and wires that no longer typecheck, and fills in
values for sockets that have since been added.

## The editor window

`NodeEditor` is just the canvas. The `app` feature adds the surroundings every
app built on it would otherwise rewrite — a toolbar, a node palette grouped by
category, a graph inspector, a preview panel and a status bar listing the
keybindings:

```rust
nodez::app::EditorApp::new(library)
    .graph(graph)
    .title("stack config editor")
    .file("stack-graph.json")
    .json_files()
    .preview(|graph, library| Preview::text(generate(graph, library)))
    .run()
```

That is what [`nodez-demo`](nodez-demo/src/main.rs) does; its `main.rs` is 49
lines, of which the editor window is seven. `EditorApp::ui` draws the same thing
inside a `Ui` you already have, for embedding it in something larger.

## Nodes as Rust types

Behind the `derive` feature, the schema and the evaluation rules come from Rust
types instead of runtime builders. This is how
[`nodez-demo`](nodez-demo/src/nodes.rs) describes its fourteen node kinds. A field *is* a socket, its type gives the socket type, and
its outer wrapper gives the arity:

```rust
#[derive(NodeType)]
#[node(output = ServiceDef)]
struct Service {
    name: String,                        // no #[input]: a parameter
    #[input] image: ImageRef,            // no widget on ImageRef, so link-only
    #[input] replicas: i64,              // i64 is editable, so it gets a drag box
    #[input] env: Multi<EnvList>,        // Multi: a fan-in
    #[input] health: Option<Health>,     // Option: nothing wired is None
}

impl Evaluate<Config> for Service { .. }   // a fold, so a graph can have several
```

`Evaluate` does not restate the output type: it is pinned to the output socket
the schema declares, since that is what downstream nodes downcast to.

`Rules` registers schemas and rules together, so there is no match on template
ids and no socket name written twice. A whole library goes in at once, naming
the kinds as a tuple:

```rust
type Nodes = (Image, Port, EnvVar, EnvFile, Service);

let mut rules = Rules::<Config>::new();
rules.register_all::<Nodes>(&mut library);
``` Primitive
types are socket types already, so only types carrying real domain meaning need
declaring.

Colours are derived and need never be chosen: a socket type's from its name, a
node header's from its category — or from the node's own id when it has no
category, so nodes stay distinguishable either way. Both are stable for the life
of the project, and either can be overridden.

`Graph<N>` also prototypes typed storage: `Graph<DynNode>` (the default) keeps
maps of `Value`, while a domain can store its own enum instead. The example
builds the same graph both ways and gets identical output.

[`nodez-demo/src/nodes.rs`](nodez-demo/src/nodes.rs) is the worked example;
[`nodez/examples/typed_nodes.rs`](nodez/examples/typed_nodes.rs) is a smaller
one that also demonstrates typed storage. The dynamic API is unchanged and still
what `Graph` uses by default.

## License

MIT OR Apache-2.0
