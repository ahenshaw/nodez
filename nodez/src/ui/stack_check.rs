//! The graph the demo app opens with, kept here as a routing fixture.
//!
//! The generated graphs in [`super::route_check`] go for breadth; this one
//! goes for realism. It is the container-stack example the demo builds, node
//! for node and link for link, laid out by [`layered`] exactly as the app lays
//! it out — so what these tests measure is a picture somebody actually looks
//! at.
//!
//! It earns its place by what it holds that a random graph rarely does: two
//! small nodes at one end feeding the very last node at the other, past three
//! tall services and the whole bundle of wires between them. Drawn as plain
//! curves those two are long diagonals across everything, which is exactly
//! the case the crossing penalty exists for.
//!
//! It comes in two arrangements, because the same graph placed two ways is
//! two different problems for a router and the second is the one that turned
//! the crossing penalty up: [`Arrangement::Fresh`] is what the demo builds on
//! first run, and [`Arrangement::Spread`] is what the editor's Auto layout
//! button leaves, saved out of a running app and pinned here.
//!
//! The demo's own node types live in `nodez-demo` and are not visible from
//! here, so the templates below mirror them: same sockets, same params, same
//! labels, and so the same sizes out of [`node_size`].

use egui::{Color32, Pos2, Rect, pos2};

use super::geometry::{bezier_point, wire_path};
use super::style::EditorStyle;
use super::{node_size, socket_anchor};
use crate::graph::{Connection, Graph, NodeId, SocketKind};
use crate::layout::{LayoutOptions, RouteOptions, layered, route_links};
use crate::template::{NodeLibrary, NodeTemplate, ParamSpec, SocketSpec, Widget};
use crate::value::Value;

/// The demo's node library, as far as anything about layout can tell.
fn library() -> NodeLibrary {
    let mut library = NodeLibrary::new();
    let text = library
        .types
        .add("String", Color32::from_rgb(0x8C, 0xA5, 0xD6));
    let int = library
        .types
        .add("i64", Color32::from_rgb(0x8C, 0xD6, 0xA5));
    let flag = library
        .types
        .add("bool", Color32::from_rgb(0xD6, 0x8C, 0xA5));
    let image = library
        .types
        .add("ImageRef", Color32::from_rgb(0xD6, 0xA5, 0x8C));
    let env = library
        .types
        .add("EnvList", Color32::from_rgb(0xD6, 0x5C, 0xA5));
    let env_file = library
        .types
        .add("EnvFileRef", Color32::from_rgb(0xC6, 0x6C, 0x9C));
    let port = library
        .types
        .add("PortMap", Color32::from_rgb(0x6C, 0xC6, 0x9C));
    let mount = library
        .types
        .add("Mount", Color32::from_rgb(0x6C, 0xC6, 0xC6));
    let network = library
        .types
        .add("NetworkDef", Color32::from_rgb(0xC6, 0xC6, 0x6C));
    let health = library
        .types
        .add("Health", Color32::from_rgb(0x9C, 0xC6, 0x6C));
    let service = library
        .types
        .add("ServiceDef", Color32::from_rgb(0xC6, 0x6C, 0xC6));

    // Input.
    library.register(
        NodeTemplate::new("text", "Text")
            .category("Input")
            .input(
                SocketSpec::new("value", text)
                    .label("")
                    .editable(Widget::text_hint("text\u{2026}")),
            )
            .output(SocketSpec::new("out", text)),
    );
    library.register(
        NodeTemplate::new("number", "Number")
            .category("Input")
            .input(
                SocketSpec::new("value", int)
                    .label("")
                    .editable(Widget::int()),
            )
            .output(SocketSpec::new("out", int)),
    );
    library.register(
        NodeTemplate::new("secret", "Secret Reference")
            .category("Input")
            .input(
                SocketSpec::new("name", text)
                    .editable(Widget::text())
                    .default_value("DB_PASSWORD"),
            )
            .output(SocketSpec::new("out", text)),
    );

    // Convert.
    library.register(
        NodeTemplate::new("join", "Join Text")
            .category("Convert")
            .param(ParamSpec::new("separator", Widget::text_hint("separator")).show_label(false))
            .input(SocketSpec::new("parts", text).multi())
            .output(SocketSpec::new("out", text)),
    );

    // Build.
    library.register(
        NodeTemplate::new("image", "Image")
            .category("Build")
            .input(
                SocketSpec::new("repository", text)
                    .editable(Widget::text())
                    .default_value("nginx"),
            )
            .input(
                SocketSpec::new("tag", text)
                    .editable(Widget::text())
                    .default_value("latest"),
            )
            .output(SocketSpec::new("out", image)),
    );
    library.register(
        NodeTemplate::new("env_var", "Environment Variable")
            .category("Build")
            .input(
                SocketSpec::new("key", text)
                    .editable(Widget::text())
                    .default_value("KEY"),
            )
            .input(SocketSpec::new("value", text).editable(Widget::text()))
            .output(SocketSpec::new("out", env)),
    );
    library.register(
        NodeTemplate::new("env_file", "Environment File")
            .category("Build")
            .input(
                SocketSpec::new("path", text)
                    .editable(Widget::text())
                    .default_value(".env"),
            )
            .output(SocketSpec::new("out", env_file)),
    );
    library.register(
        NodeTemplate::new("port", "Port Mapping")
            .category("Build")
            .param(ParamSpec::new("protocol", Widget::combo(["tcp", "udp"])).default_value("tcp"))
            .input(
                SocketSpec::new("host", int)
                    .editable(Widget::int_range(1, 65535))
                    .default_value(8080i64),
            )
            .input(
                SocketSpec::new("container", int)
                    .editable(Widget::int_range(1, 65535))
                    .default_value(80i64),
            )
            .output(SocketSpec::new("out", port)),
    );
    library.register(
        NodeTemplate::new("volume", "Volume")
            .category("Build")
            .input(
                SocketSpec::new("source", text)
                    .editable(Widget::text())
                    .default_value("./data"),
            )
            .input(
                SocketSpec::new("target", text)
                    .editable(Widget::text())
                    .default_value("/var/lib/data"),
            )
            .input(
                SocketSpec::new("read_only", flag)
                    .label("Read Only")
                    .editable(Widget::Checkbox),
            )
            .output(SocketSpec::new("out", mount)),
    );
    library.register(
        NodeTemplate::new("network", "Network")
            .category("Build")
            .param(
                ParamSpec::new(
                    "driver",
                    Widget::combo(["bridge", "overlay", "host", "none"]),
                )
                .default_value("bridge"),
            )
            .input(
                SocketSpec::new("name", text)
                    .editable(Widget::text())
                    .default_value("frontend"),
            )
            .output(SocketSpec::new("out", network)),
    );
    library.register(
        NodeTemplate::new("healthcheck", "Health Check")
            .category("Build")
            .input(
                SocketSpec::new("command", text)
                    .editable(Widget::text())
                    .default_value("curl -f localhost/"),
            )
            .input(
                SocketSpec::new("interval", int)
                    .label("Interval (s)")
                    .editable(Widget::int_range(1, 3600))
                    .default_value(30i64),
            )
            .input(
                SocketSpec::new("retries", int)
                    .editable(Widget::int_range(1, 20))
                    .default_value(3i64),
            )
            .output(SocketSpec::new("out", health)),
    );

    // Runtime.
    library.register(
        NodeTemplate::new("service", "Service")
            .category("Runtime")
            .param(
                ParamSpec::new("name", Widget::text_hint("service name"))
                    .show_label(false)
                    .default_value("web"),
            )
            .param(
                ParamSpec::new(
                    "restart",
                    Widget::combo(["no", "always", "on-failure", "unless-stopped"]),
                )
                .default_value("unless-stopped"),
            )
            .input(SocketSpec::new("image", image))
            .input(
                SocketSpec::new("command", text)
                    .editable(Widget::text_hint("(default entrypoint)")),
            )
            .input(
                SocketSpec::new("replicas", int)
                    .editable(Widget::int_range(1, 64))
                    .default_value(1i64),
            )
            .input(SocketSpec::new("environment", env).multi())
            .input(
                SocketSpec::new("env_file", env_file)
                    .label("Env Files")
                    .multi(),
            )
            .input(SocketSpec::new("ports", port).multi())
            .input(SocketSpec::new("volumes", mount).multi())
            .input(SocketSpec::new("networks", network).multi())
            .input(
                SocketSpec::new("depends_on", service)
                    .label("Depends On")
                    .multi(),
            )
            .input(SocketSpec::new("healthcheck", health))
            .output(SocketSpec::new("out", service)),
    );

    // Output.
    library.register(
        NodeTemplate::new("stack", "Stack Output")
            .category("Output")
            .param(
                ParamSpec::new("name", Widget::text_hint("stack name"))
                    .show_label(false)
                    .default_value("my-stack"),
            )
            .param(
                ParamSpec::new("version", Widget::combo(["3.9", "3.8", "3.7"]))
                    .default_value("3.9"),
            )
            .input(SocketSpec::new("services", service).multi())
            .input(SocketSpec::new("networks", network).multi()),
    );

    library
}

/// Where the nodes sit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arrangement {
    /// Placed by [`layered`] with the spacing the demo builds the graph with.
    Fresh,
    /// Placed by the editor's own Auto layout, which spaces columns wider and
    /// so leaves longer wires between them.
    ///
    /// Read out of a graph saved from a running app rather than worked out
    /// here, because what it is worth testing against is an arrangement
    /// somebody was actually looking at. Only the positions are pinned: the
    /// nodes, their values and their wiring are the same graph either way,
    /// which is checked against the saved file when these were taken.
    Spread,
}

/// The demo's sample stack, wired and placed.
fn stack(arrangement: Arrangement) -> (NodeLibrary, Graph) {
    let library = library();
    let mut graph = Graph::new();

    let add = |graph: &mut Graph, template: &str| -> NodeId {
        let id = library.id(template).expect("registered above");
        graph.add_node(&library, id, pos2(0.0, 0.0))
    };

    let frontend = add(&mut graph, "network");
    let backend = add(&mut graph, "network");
    let web_image = add(&mut graph, "image");
    let web_port = add(&mut graph, "port");
    let web_conf = add(&mut graph, "volume");
    let web = add(&mut graph, "service");
    let api_image = add(&mut graph, "image");
    let api_port = add(&mut graph, "port");
    let api_replicas = add(&mut graph, "number");
    let db_password = add(&mut graph, "secret");
    let dsn_prefix = add(&mut graph, "text");
    let dsn_suffix = add(&mut graph, "text");
    let dsn = add(&mut graph, "join");
    let api_env = add(&mut graph, "env_var");
    let api = add(&mut graph, "service");
    let db_image = add(&mut graph, "image");
    let db_volume = add(&mut graph, "volume");
    let db_env = add(&mut graph, "env_var");
    let db_health = add(&mut graph, "healthcheck");
    let db = add(&mut graph, "service");
    let stack = add(&mut graph, "stack");

    // The values the demo fills in. They are here because a node is as wide
    // as what it has to show, and a wider node is a different picture.
    for (node, socket, value) in [
        (frontend, "name", Value::from("frontend")),
        (backend, "name", Value::from("backend")),
        (web_image, "repository", Value::from("nginx")),
        (web_image, "tag", Value::from("1.27-alpine")),
        (web_port, "host", Value::Int(8080)),
        (web_port, "container", Value::Int(80)),
        (web_conf, "source", Value::from("./nginx.conf")),
        (web_conf, "target", Value::from("/etc/nginx/nginx.conf")),
        (web_conf, "read_only", Value::Bool(true)),
        (api_image, "repository", Value::from("ghcr.io/acme/api")),
        (api_image, "tag", Value::from("2.4.0")),
        (api_port, "host", Value::Int(9000)),
        (api_port, "container", Value::Int(9000)),
        (api_replicas, "value", Value::Int(3)),
        (db_password, "name", Value::from("DB_PASSWORD")),
        (dsn_prefix, "value", Value::from("postgres://app:")),
        (dsn_suffix, "value", Value::from("@db:5432/app")),
        (api_env, "key", Value::from("DATABASE_URL")),
        (db_image, "repository", Value::from("postgres")),
        (db_image, "tag", Value::from("16-alpine")),
        (db_volume, "source", Value::from("pgdata")),
        (db_volume, "target", Value::from("/var/lib/postgresql/data")),
        (db_env, "key", Value::from("POSTGRES_PASSWORD")),
        (db_health, "command", Value::from("pg_isready -U app")),
        (db_health, "interval", Value::Int(10)),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_input_value(socket, value);
    }

    for (node, param, value) in [
        (dsn, "separator", Value::from("")),
        (web, "name", Value::from("web")),
        (api, "name", Value::from("api")),
        (api, "restart", Value::Choice("always".to_owned())),
        (db, "name", Value::from("db")),
        (stack, "name", Value::from("acme-platform")),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_param(param, value);
    }

    for (from, from_socket, to, to_socket) in [
        (web_image, "out", web, "image"),
        (web_port, "out", web, "ports"),
        (web_conf, "out", web, "volumes"),
        (frontend, "out", web, "networks"),
        (api, "out", web, "depends_on"),
        (api_image, "out", api, "image"),
        (api_port, "out", api, "ports"),
        (api_replicas, "out", api, "replicas"),
        (dsn_prefix, "out", dsn, "parts"),
        (db_password, "out", dsn, "parts"),
        (dsn_suffix, "out", dsn, "parts"),
        (dsn, "out", api_env, "value"),
        (api_env, "out", api, "environment"),
        (frontend, "out", api, "networks"),
        (backend, "out", api, "networks"),
        (db, "out", api, "depends_on"),
        (db_image, "out", db, "image"),
        (db_volume, "out", db, "volumes"),
        (db_password, "out", db_env, "value"),
        (db_env, "out", db, "environment"),
        (db_health, "out", db, "healthcheck"),
        (backend, "out", db, "networks"),
        (web, "out", stack, "services"),
        (api, "out", stack, "services"),
        (db, "out", stack, "services"),
        (frontend, "out", stack, "networks"),
        (backend, "out", stack, "networks"),
    ] {
        graph
            .connect(&library, (from, from_socket), (to, to_socket))
            .expect("the sample graph is well-typed and acyclic");
    }

    let style = EditorStyle::default();
    match arrangement {
        Arrangement::Fresh => {
            layered(
                &mut graph,
                &LayoutOptions {
                    column_gap: 70.0,
                    row_gap: 22.0,
                    origin: pos2(0.0, 0.0),
                    sweeps: 6,
                },
                |graph, node| node_size(graph, &library, node, &style),
            )
            .expect("acyclic");
        }
        Arrangement::Spread => {
            for (node, at) in [
                (frontend, pos2(967.3, 959.9)),
                (backend, pos2(387.0, 1177.3)),
                (web_image, pos2(1632.8, 596.2)),
                (web_port, pos2(1632.8, 715.2)),
                (web_conf, pos2(1632.8, 856.2)),
                (web, pos2(1904.4, 725.9)),
                (api_image, pos2(967.3, 483.9)),
                (api_port, pos2(967.3, 818.9)),
                (api_replicas, pos2(968.0, 602.9)),
                (db_password, pos2(0.4, 655.8)),
                (dsn_prefix, pos2(0.4, 548.5)),
                (dsn_suffix, pos2(0.4, 756.4)),
                (dsn, pos2(387.0, 505.3)),
                (api_env, pos2(967.3, 699.9)),
                (api, pos2(1377.1, 597.4)),
                (db_image, pos2(387.0, 657.3)),
                (db_volume, pos2(387.0, 895.3)),
                (db_env, pos2(387.0, 776.3)),
                (db_health, pos2(387.0, 1036.3)),
                (db, pos2(703.7, 616.9)),
                (stack, pos2(2202.8, 1069.4)),
            ] {
                graph.node_mut(node).expect("just added").position = at;
            }
        }
    }

    (library, graph)
}

/// The fixture, routed, with what the tests need to measure the picture.
struct Picture {
    graph: Graph,
    library: NodeLibrary,
    style: EditorStyle,
}

impl Picture {
    fn new(arrangement: Arrangement, cross: f32) -> Self {
        let (library, mut graph) = stack(arrangement);
        let style = EditorStyle::default();
        route_links(
            &mut graph,
            &RouteOptions {
                curvature: style.wire_curvature,
                min_curve: style.wire_min_curve,
                max_curve: style.wire_max_curve,
                cross,
                ..RouteOptions::default()
            },
            |graph, node| node_size(graph, &library, node, &style),
            |graph, socket, kind, slot| {
                let node = graph.node(socket.node)?;
                socket_anchor(graph, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .expect("acyclic");
        Self {
            graph,
            library,
            style,
        }
    }

    fn ends(&self, conn: &Connection) -> (Pos2, Pos2) {
        let from = self.graph.node(conn.from.node).unwrap();
        let to = self.graph.node(conn.to.node).unwrap();
        let a = socket_anchor(
            &self.graph,
            &self.library,
            from,
            &self.style,
            SocketKind::Output,
            &conn.from.socket,
            None,
        )
        .unwrap();
        let b = socket_anchor(
            &self.graph,
            &self.library,
            to,
            &self.style,
            SocketKind::Input,
            &conn.to.socket,
            Some(conn.order),
        )
        .unwrap();
        (a, b)
    }

    /// Every wire as the editor draws it, sampled.
    fn wires(&self) -> Vec<(&Connection, Vec<Pos2>)> {
        self.graph
            .connections()
            .map(|conn| {
                let (a, b) = self.ends(conn);
                let mut points = Vec::new();
                for segment in wire_path(a, b, &conn.waypoints, &self.style, 1.0) {
                    const SAMPLES: usize = 24;
                    for i in 0..=SAMPLES {
                        points.push(bezier_point(&segment, i as f32 / SAMPLES as f32));
                    }
                }
                (conn, points)
            })
            .collect()
    }

    /// How many times one wire visibly goes over another.
    ///
    /// Counted per pair of wires and per place they meet, not per pair of
    /// sampled segments: two wires that run alongside each other for a while
    /// touch at a dozen samples and cross the picture once, if at all.
    ///
    /// Two wires off the same socket leave from the same point and two into
    /// the same slot arrive at one; neither is a crossing, so both are
    /// discounted the way the router discounts them.
    fn crossings(&self) -> usize {
        const APART: f32 = 12.0;
        let options = RouteOptions::default();
        let wires = self.wires();
        let mut total = 0;
        for (i, (conn, points)) in wires.iter().enumerate() {
            let (a, b) = self.ends(conn);
            for (other, theirs) in &wires[i + 1..] {
                let (c, d) = self.ends(other);
                let mut meetings: Vec<Pos2> = Vec::new();
                for ours in points.windows(2) {
                    for them in theirs.windows(2) {
                        let Some(p) =
                            crate::layout::meeting([ours[0], ours[1]], [them[0], them[1]])
                        else {
                            continue;
                        };
                        let shared = [a, b, c, d]
                            .iter()
                            .any(|socket| p.distance(*socket) <= options.margin);
                        if !shared && !meetings.iter().any(|q| q.distance(p) <= APART) {
                            meetings.push(p);
                        }
                    }
                }
                total += meetings.len();
            }
        }
        total
    }

    fn node_boxes(&self) -> Vec<(NodeId, Rect)> {
        self.graph
            .nodes()
            .map(|n| {
                (
                    n.id,
                    Rect::from_min_size(
                        n.position,
                        node_size(&self.graph, &self.library, n, &self.style),
                    ),
                )
            })
            .collect()
    }
}

/// Charging for crossings has to leave both pictures with fewer of them.
///
/// A comparison rather than a bound: how many crossings a graph this size
/// needs is not something anybody knows, and a number written down here would
/// only be a record of the last time it was run. What is worth holding the
/// router to is that paying for them beats not paying.
#[test]
fn paying_for_crossings_untangles_the_sample_stack() {
    for arrangement in [Arrangement::Fresh, Arrangement::Spread] {
        let free = Picture::new(arrangement, 0.0).crossings();
        let priced = Picture::new(arrangement, RouteOptions::default().cross).crossings();
        assert!(
            priced < free,
            "laid out {arrangement:?}, the sample stack is drawn with {priced} crossings \
             when they cost something and {free} when they are free"
        );
    }
}

/// Neither arrangement may have a wire running under a node.
///
/// The generated sweeps say this of graphs nobody has looked at; this says it
/// of two somebody has.
#[test]
fn no_wire_in_the_sample_stack_runs_under_a_node() {
    for arrangement in [Arrangement::Fresh, Arrangement::Spread] {
        let picture = Picture::new(arrangement, RouteOptions::default().cross);
        let boxes = picture.node_boxes();
        for (conn, points) in picture.wires() {
            for p in &points {
                for (id, rect) in &boxes {
                    if *id != conn.from.node && *id != conn.to.node {
                        assert!(
                            !rect.contains(*p),
                            "laid out {arrangement:?}, {:?} runs under {id:?}",
                            conn.id
                        );
                    }
                }
            }
        }
    }
}

/// The wire the whole thing came from: the `frontend` network feeds both the
/// services in the middle and the stack node at the far right, and the far
/// one is a long drop across everything between. Laid out fresh it has to be
/// routed — left to its own curve it is the diagonal that started this.
///
/// Only laid out fresh. Spread out, the same two wires keep their curves,
/// because going round out there crosses as much as cutting across does and
/// the router is comparing rather than ruling. Asserting they bend in both
/// arrangements would be asserting a rule the router deliberately does not
/// have.
#[test]
fn the_long_drop_to_the_stack_node_is_routed() {
    let picture = Picture::new(Arrangement::Fresh, RouteOptions::default().cross);

    let mut checked = 0;
    for conn in picture.graph.connections() {
        let from = picture.graph.node(conn.from.node).unwrap();
        let to = picture.graph.node(conn.to.node).unwrap();
        let long = picture.library.expect(from.template).id == "network"
            && picture.library.expect(to.template).id == "stack";
        if !long {
            continue;
        }
        checked += 1;
        assert!(
            !conn.waypoints.is_empty(),
            "{:?} runs straight from the network to the stack node",
            conn.id
        );
    }
    assert_eq!(checked, 2, "both networks feed the stack node");
}
