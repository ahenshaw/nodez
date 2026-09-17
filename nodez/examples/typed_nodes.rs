//! **Prototype.** `Image`, `Port` and `Service` described as Rust types.
//!
//! Run with `cargo run -p nodez --features derive --example typed_nodes`.
//!
//! Compare against `nodez-demo/src/domain.rs` (the schema) and
//! `nodez-demo/src/generate.rs` (the rules), which describe the same three
//! nodes by hand. There, every socket name is written twice and nothing checks
//! the two spellings agree; here a field *is* the socket.

use std::collections::BTreeMap;

use nodez::{
    DynNode, Evaluate, Fold, Graph, Multi, NodeData, NodeError, NodeId, NodeLibrary, NodeTemplate,
    NodeType, Payload, Rules, SocketType, Value,
};

// ---------------------------------------------------------------------------
// Wire types. The color and shape the editor draws live on the Rust type, so
// the socket type system and Rust's are the same system.
//
// Color is derived from the type name unless you say otherwise, so a socket
// type needs no attribute at all. Shape is semantic, so it stays explicit.
//
// A type that names a `widget` can also be typed in by hand when the socket is
// unconnected; one that does not is link-only. No annotation says which.
// ---------------------------------------------------------------------------

// `String`, `i64`, `f64` and `bool` are socket types already, so a field that
// is just a string or a number needs no wrapper at all. Only types that carry
// real domain meaning are declared below — and those you would write anyway.

#[derive(Clone, Debug, SocketType)]
#[socket(description = "A container image reference.")]
struct ImageRef(String);

#[derive(Clone, Debug, SocketType)]
#[socket(shape = "diamond", description = "A published port.")]
struct PortMap(String);

/// The case that breaks "`Vec<T>` means multi": one link carrying a list.
#[derive(Clone, Debug, SocketType)]
#[socket(shape = "diamond", description = "Environment entries.")]
struct EnvList(Vec<String>);

#[derive(Clone, Debug, SocketType)]
#[socket(description = "A container health probe.")]
struct Health(BTreeMap<String, String>);

#[derive(Clone, Debug, SocketType)]
#[socket(shape = "square", description = "A described service.")]
struct ServiceDef {
    name: String,
    body: Vec<(String, Value)>,
}

/// A fieldless enum becomes a dropdown. Variants are kebab-cased.
#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Protocol {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Restart {
    No,
    Always,
    OnFailure,
    UnlessStopped,
}

// ---------------------------------------------------------------------------
// Nodes. A field carrying `#[input]` is a socket; a bare field is a plain
// parameter. The field *type* gives the arity: `T` is one required link,
// `Option<T>` is optional, `Multi<T>` is a fan-in.
// ---------------------------------------------------------------------------

/// Everything on `#[node]` has a default, so a node kind needs only to say what
/// it produces. The id is the struct name snake_cased (`image`) and the label is
/// it title-cased (`Image`), which is usually what you wanted anyway.
#[derive(Debug, NodeType)]
#[node(output = ImageRef)]
struct Image {
    #[input(default = "nginx")]
    repository: String,
    #[input(default = "latest")]
    tag: String,
}

/// Here the derived label would be "Port", so it is worth overriding. `id` still
/// comes out as `port`.
#[derive(Debug, NodeType)]
#[node(label = "Port Mapping", output = PortMap)]
struct Port {
    #[input(default = 8080)]
    host: i64,
    #[input(default = 80)]
    container: i64,
    /// No `#[input]`, so this is a parameter: drawn in the body, not connectable.
    #[param(default = Protocol::Tcp)]
    protocol: Protocol,
}

/// One entry. Wired into `Multi<EnvList>` alongside the node below, this is a
/// fan-in; on its own it is a link carrying a one-element list.
#[derive(Debug, NodeType)]
#[node(label = "Environment Variable", output = EnvList)]
struct EnvVar {
    #[input(default = "KEY")]
    key: String,
    #[input(default = "")]
    value: String,
}

/// Several entries from one link — the case `Vec<T>` could not tell apart from
/// a fan-in, and the reason arity lives in `Multi` instead.
#[derive(Debug, NodeType)]
#[node(label = "Environment File", output = EnvList)]
struct EnvFile {
    #[input(default = ".env")]
    path: String,
}

/// The other end of the range: everything spelled out. `category` groups the
/// add-node menu and tints the header, which is worth doing once a library has
/// enough kinds to need sorting — `nodez-demo` has fourteen and five categories.
/// Its header color is derived from "Runtime"; the others, having no category,
/// are derived from their own ids. `header_color` overrides that when needed.
#[derive(Debug, NodeType)]
#[node(
    category = "Runtime",
    description = "One service in the stack.",
    output = ServiceDef
)]
struct Service {
    #[param(default = "web", hide_label)]
    name: String,
    #[param(default = Restart::UnlessStopped)]
    restart: Restart,

    /// `ImageRef` offers no widget, so this socket is link-only and required.
    #[input(description = "Required.")]
    image: ImageRef,
    /// `Count` offers a widget, so this one can also be typed in.
    #[input(default = 1)]
    replicas: i64,
    /// Many links, each carrying a *list* — the case `Vec<T>` could not express.
    #[input]
    environment: Multi<EnvList>,
    #[input]
    ports: Multi<PortMap>,
    /// Optional: `None` when nothing is wired in.
    #[input]
    healthcheck: Option<Health>,
}

// ---------------------------------------------------------------------------
// One fold: the config document. `Fold` names the traversal, not the output
// type, so each node still returns its own type.
// ---------------------------------------------------------------------------

struct Config;
impl Fold for Config {}

impl Evaluate<Config> for Image {
    fn evaluate(&self) -> Result<ImageRef, NodeError> {
        if self.repository.is_empty() {
            return Err(NodeError::custom("Image needs a repository."));
        }
        Ok(ImageRef(if self.tag.is_empty() {
            self.repository.clone()
        } else {
            format!("{}:{}", self.repository, self.tag)
        }))
    }
}

impl Evaluate<Config> for Port {
    fn evaluate(&self) -> Result<PortMap, NodeError> {
        let (host, container) = (self.host, self.container);
        Ok(PortMap(match self.protocol {
            Protocol::Tcp => format!("{host}:{container}"),
            Protocol::Udp => format!("{host}:{container}/udp"),
        }))
    }
}

impl Evaluate<Config> for EnvVar {
    fn evaluate(&self) -> Result<EnvList, NodeError> {
        if self.key.is_empty() {
            return Err(NodeError::custom("Environment Variable needs a key."));
        }
        Ok(EnvList(vec![format!("{}={}", self.key, self.value)]))
    }
}

impl Evaluate<Config> for EnvFile {
    fn evaluate(&self) -> Result<EnvList, NodeError> {
        // Stand-in for parsing the file: one link, several entries.
        Ok(EnvList(vec![
            format!("# from {}", self.path),
            "LOG_LEVEL=info".to_owned(),
            "TZ=UTC".to_owned(),
        ]))
    }
}

impl Evaluate<Config> for Service {
    fn evaluate(&self) -> Result<ServiceDef, NodeError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(NodeError::custom("Service needs a name."));
        }

        let body = Value::map()
            .set("image", self.image.0.clone())
            .set_if(self.restart != Restart::No, "restart", self.restart.to_value())
            .set_if(
                self.replicas > 1,
                "deploy",
                Value::map().set("replicas", self.replicas),
            )
            .set_list("ports", self.ports.iter().map(|p| p.0.clone()))
            // `Multi<EnvList>` is a fan-in of lists: flatten across links.
            .set_list(
                "environment",
                self.environment.iter().flat_map(|list| list.0.iter().cloned()),
            )
            .set_some(
                "healthcheck",
                self.healthcheck.as_ref().map(|health| {
                    health
                        .0
                        .iter()
                        .map(|(k, v)| (k.clone(), Value::Text(v.clone())))
                        .collect::<nodez::MapBuilder>()
                }),
            );

        Ok(ServiceDef {
            name: name.to_owned(),
            body: body.entries(),
        })
    }
}

// ---------------------------------------------------------------------------
// Typed storage: `Graph<StackNode>` keeps one of these per node instead of the
// default maps of `Value`.
//
// It is deliberately *not* the same shape as the node structs above. Those are
// the resolved view an evaluation sees, where `image: ImageRef` has already
// been pulled off a wire. Storage holds only what a user can type in, so every
// link-only field disappears — `Service` keeps three fields here and has nine
// sockets up there.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum StackNode {
    Image { repository: String, tag: String },
    Port { host: i64, container: i64, protocol: Protocol },
    EnvVar { key: String, value: String },
    EnvFile { path: String },
    Service { name: String, restart: Restart, replicas: i64 },
}

/// Read a template's declared default for one socket or parameter.
fn default_of<T: SocketType>(template: &NodeTemplate, name: &str) -> T {
    template
        .input_spec(name)
        .map(|s| &s.default)
        .or_else(|| template.param_spec(name).map(|p| &p.default))
        .and_then(T::from_value)
        .unwrap_or_else(|| {
            T::from_value(&T::widget().default_value()).expect("widget default fits its type")
        })
}

impl NodeData for StackNode {
    fn new(template: &NodeTemplate) -> Self {
        match template.id.as_str() {
            "port" => Self::Port {
                host: default_of(template, "host"),
                container: default_of(template, "container"),
                protocol: default_of(template, "protocol"),
            },
            "env_var" => Self::EnvVar {
                key: default_of(template, "key"),
                value: default_of(template, "value"),
            },
            "env_file" => Self::EnvFile {
                path: default_of(template, "path"),
            },
            "service" => Self::Service {
                name: default_of(template, "name"),
                restart: default_of(template, "restart"),
                replicas: default_of(template, "replicas"),
            },
            // `image` and anything unrecognised.
            _ => Self::Image {
                repository: default_of(template, "repository"),
                tag: default_of(template, "tag"),
            },
        }
    }

    fn input_value(&self, socket: &str) -> Option<std::borrow::Cow<'_, Value>> {
        Some(std::borrow::Cow::Owned(match (self, socket) {
            (Self::Image { repository, .. }, "repository") => repository.to_value(),
            (Self::Image { tag, .. }, "tag") => tag.to_value(),
            (Self::Port { host, .. }, "host") => host.to_value(),
            (Self::Port { container, .. }, "container") => container.to_value(),
            (Self::EnvVar { key, .. }, "key") => key.to_value(),
            (Self::EnvVar { value, .. }, "value") => value.to_value(),
            (Self::EnvFile { path }, "path") => path.to_value(),
            (Self::Service { replicas, .. }, "replicas") => replicas.to_value(),
            _ => return None,
        }))
    }

    fn set_input_value(&mut self, socket: &str, value: Value) {
        match (self, socket) {
            (Self::Image { repository, .. }, "repository") => set(repository, &value),
            (Self::Image { tag, .. }, "tag") => set(tag, &value),
            (Self::Port { host, .. }, "host") => set(host, &value),
            (Self::Port { container, .. }, "container") => set(container, &value),
            (Self::EnvVar { key, .. }, "key") => set(key, &value),
            (Self::EnvVar { value: v, .. }, "value") => set(v, &value),
            (Self::EnvFile { path }, "path") => set(path, &value),
            (Self::Service { replicas, .. }, "replicas") => set(replicas, &value),
            _ => {}
        }
    }

    fn param(&self, name: &str) -> Option<std::borrow::Cow<'_, Value>> {
        Some(std::borrow::Cow::Owned(match (self, name) {
            (Self::Port { protocol, .. }, "protocol") => protocol.to_value(),
            (Self::Service { name: n, .. }, "name") => n.to_value(),
            (Self::Service { restart, .. }, "restart") => restart.to_value(),
            _ => return None,
        }))
    }

    fn set_param(&mut self, name: &str, value: Value) {
        match (self, name) {
            (Self::Port { protocol, .. }, "protocol") => set(protocol, &value),
            (Self::Service { name: n, .. }, "name") => set(n, &value),
            (Self::Service { restart, .. }, "restart") => set(restart, &value),
            _ => {}
        }
    }
}

fn set<T: SocketType>(slot: &mut T, value: &Value) {
    if let Some(parsed) = T::from_value(value) {
        *slot = parsed;
    }
}

/// The same wiring, built over whichever payload the caller asks for.
fn build_graph<N: NodeData>(library: &NodeLibrary) -> (Graph<N>, NodeId) {
    let mut graph = Graph::<N>::default();
    let id = |name: &str| library.id(name).expect("registered above");

    let image = graph.add_node(library, id("image"), egui::pos2(0.0, 0.0));
    let port_a = graph.add_node(library, id("port"), egui::pos2(0.0, 120.0));
    let port_b = graph.add_node(library, id("port"), egui::pos2(0.0, 240.0));
    let env_var = graph.add_node(library, id("env_var"), egui::pos2(0.0, 360.0));
    let env_file = graph.add_node(library, id("env_file"), egui::pos2(0.0, 480.0));
    let service = graph.add_node(library, id("service"), egui::pos2(300.0, 0.0));

    for (node, socket, value) in [
        (image, "repository", Value::from("ghcr.io/acme/api")),
        (image, "tag", Value::from("2.4.0")),
        (port_a, "host", Value::Int(9000)),
        (port_a, "container", Value::Int(9000)),
        (port_b, "host", Value::Int(9443)),
        (port_b, "container", Value::Int(443)),
        (env_var, "key", Value::from("DATABASE_URL")),
        (env_var, "value", Value::from("postgres://app@db/app")),
        (env_file, "path", Value::from("./api.env")),
        (service, "replicas", Value::Int(3)),
    ] {
        graph.node_mut(node).expect("just added").set_input_value(socket, value);
    }
    for (node, param, value) in [
        (port_b, "protocol", Value::Choice("udp".to_owned())),
        (service, "name", Value::from("api")),
        (service, "restart", Value::Choice("always".to_owned())),
    ] {
        graph.node_mut(node).expect("just added").set_param(param, value);
    }

    for (from, socket, to, target) in [
        (image, "out", service, "image"),
        (port_a, "out", service, "ports"),
        (port_b, "out", service, "ports"),
        (env_var, "out", service, "environment"),
        (env_file, "out", service, "environment"),
    ] {
        graph
            .connect(library, (from, socket), (to, target))
            .expect("well-typed");
    }
    (graph, service)
}

fn describe(def: &ServiceDef) -> String {
    let mut out = format!("service `{}`:\n", def.name);
    for (key, value) in &def.body {
        out.push_str(&format!("  {key}: {}\n", value.to_literal()));
    }
    out
}

/// Evaluate a graph of any payload type through the rules for that payload.
fn run<N: NodeData>(
    graph: &Graph<N>,
    library: &NodeLibrary,
    rules: &Rules<Config, N>,
    target: NodeId,
) -> Result<ServiceDef, String> {
    let payload = graph
        .evaluate::<Payload, NodeError>(library, target, |ctx| rules.run(&ctx))
        .map_err(|e| e.to_string())?;
    payload
        .downcast::<ServiceDef>()
        .map(|def| *def)
        .map_err(|_| "target was not a Service".to_owned())
}

fn main() {
    let mut library = NodeLibrary::new();

    // Schema and rules, registered together. Forgetting a node is a missing
    // `register` call, not a match arm that silently never fires.
    type Nodes = (Image, Port, EnvVar, EnvFile, Service);

    let mut rules = Rules::<Config>::new();
    rules.register_all::<Nodes>(&mut library);

    // The same node kinds, evaluated against typed storage. The templates are
    // already registered, so this only needs the rules.
    let mut typed_rules = Rules::<Config, StackNode>::new();
    typed_rules.add_all::<Nodes>();

    println!("library: {} types, {} templates", library.types.len(), library.len());
    for (_, ty) in library.types.iter() {
        let c = ty.color;
        println!("  {:10} #{:02X}{:02X}{:02X}", ty.name, c.r(), c.g(), c.b());
    }
    for (_, template) in library.iter() {
        let inputs: Vec<String> = template
            .inputs
            .iter()
            .map(|s| {
                let arity = if s.multi { "multi" } else { "one" };
                let editable = if s.widget == nodez::Widget::None {
                    "link-only"
                } else {
                    "editable"
                };
                format!(
                    "{}: {} ({arity}, {editable})",
                    s.name,
                    library.types.name(s.ty)
                )
            })
            .collect();
        println!("  {:8} inputs [{}]", template.id, inputs.join(", "));
        let params: Vec<&str> = template.params.iter().map(|p| p.name.as_str()).collect();
        if !params.is_empty() {
            println!("           params [{}]", params.join(", "));
        }
    }

    // Two graphs, same wiring, different storage.
    let (dynamic, dyn_target) = build_graph::<DynNode>(&library);
    let (typed, typed_target) = build_graph::<StackNode>(&library);

    let from_dynamic = run(&dynamic, &library, &rules, dyn_target).expect("dynamic");
    let from_typed = run(&typed, &library, &typed_rules, typed_target).expect("typed");

    println!("\n--- Graph<DynNode> ---");
    print!("{}", describe(&from_dynamic));
    println!("--- Graph<StackNode> ---");
    print!("{}", describe(&from_typed));
    println!(
        "\nsame result from both storage types: {}",
        describe(&from_dynamic) == describe(&from_typed)
    );

    // The link the editor would refuse, refused on typed storage too.
    let mut refusable = typed;
    let image = refusable.node_ids().next().expect("non-empty");
    let refused = refusable.connect(&library, (image, "out"), (image, "repository"));
    println!("\nwiring a node into itself: {}", refused.unwrap_err());

    // A required link that is missing is reported against its socket.
    let mut broken = Graph::<StackNode>::default();
    let lonely = broken.add_node(&library, library.id("service").unwrap(), egui::pos2(0.0, 0.0));
    let error = run(&broken, &library, &typed_rules, lonely).unwrap_err();
    println!("a service with nothing wired in: {error}");

    // The editor works on either payload; this one is driving typed storage.
    if std::env::args().any(|arg| arg == "--edit") {
        let (graph, _) = build_graph::<StackNode>(&library);
        show_editor(library, graph);
    }
}

fn show_editor<N: NodeData + 'static>(library: NodeLibrary, graph: Graph<N>) {
    struct App<N> {
        library: NodeLibrary,
        graph: Graph<N>,
        editor: nodez::NodeEditor,
        framed: bool,
    }

    impl<N: NodeData + 'static> eframe::App for App<N> {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            egui::CentralPanel::no_frame().show(ui, |ui| {
                if !self.framed {
                    self.framed = true;
                    let rect = ui.available_rect_before_wrap();
                    self.editor.fit_to_graph(rect, &self.graph, &self.library);
                }
                self.editor.show(ui, &self.library, &mut self.graph);
            });
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_title("nodez \u{2014} derived nodes"),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "typed_nodes",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App {
                library,
                graph,
                editor: nodez::NodeEditor::new(),
                framed: false,
            }))
        }),
    );
}
