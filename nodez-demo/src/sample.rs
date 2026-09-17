//! A starting graph, so the app has something to show on first run.
//!
//! It is also a worked example of building a graph entirely in code: register
//! nodes, set their inline values, wire them up, then let `nodez::layered`
//! place them.

use egui::pos2;
use nodez::{EditorStyle, Graph, LayoutOptions, NodeId, TemplateId, node_size};

use crate::domain::Domain;

pub fn build(domain: &Domain, style: &EditorStyle) -> Graph {
    let library = &domain.library;
    let t = &domain.templates;
    let mut graph = Graph::new();

    // A helper so the wiring below reads like the config it produces.
    let add = |graph: &mut Graph, template: TemplateId| -> NodeId {
        graph.add_node(library, template, pos2(0.0, 0.0))
    };

    let frontend = add(&mut graph, t.network);
    set_input(&mut graph, frontend, "name", "frontend");
    let backend = add(&mut graph, t.network);
    set_input(&mut graph, backend, "name", "backend");

    // ------------------------------------------------------------- web
    let web_image = add(&mut graph, t.image);
    set_input(&mut graph, web_image, "repository", "nginx");
    set_input(&mut graph, web_image, "tag", "1.27-alpine");

    let web_port = add(&mut graph, t.port);
    set_input(&mut graph, web_port, "host", 8080_i64);
    set_input(&mut graph, web_port, "container", 80_i64);

    let web_conf = add(&mut graph, t.volume);
    set_input(&mut graph, web_conf, "source", "./nginx.conf");
    set_input(&mut graph, web_conf, "target", "/etc/nginx/nginx.conf");
    set_input(&mut graph, web_conf, "read_only", true);

    let web = add(&mut graph, t.service);
    set_param(&mut graph, web, "name", "web");

    // ------------------------------------------------------------- api
    let api_image = add(&mut graph, t.image);
    set_input(&mut graph, api_image, "repository", "ghcr.io/acme/api");
    set_input(&mut graph, api_image, "tag", "2.4.0");

    let api_port = add(&mut graph, t.port);
    set_input(&mut graph, api_port, "host", 9000_i64);
    set_input(&mut graph, api_port, "container", 9000_i64);

    // DATABASE_URL is assembled from literals and a secret reference, which is
    // what the Join Text node is for.
    let db_password = add(&mut graph, t.secret);
    set_input(&mut graph, db_password, "name", "DB_PASSWORD");

    let dsn_prefix = add(&mut graph, t.text);
    set_input(&mut graph, dsn_prefix, "value", "postgres://app:");
    let dsn_suffix = add(&mut graph, t.text);
    set_input(&mut graph, dsn_suffix, "value", "@db:5432/app");

    let dsn = add(&mut graph, t.join);
    set_param(&mut graph, dsn, "separator", "");

    let api_env = add(&mut graph, t.env_var);
    set_input(&mut graph, api_env, "key", "DATABASE_URL");

    let api_replicas = add(&mut graph, t.number);
    set_input(&mut graph, api_replicas, "value", 3.0);

    let api = add(&mut graph, t.service);
    set_param(&mut graph, api, "name", "api");
    set_param(&mut graph, api, "restart", nodez::Value::Choice("always".to_owned()));

    // -------------------------------------------------------------- db
    let db_image = add(&mut graph, t.image);
    set_input(&mut graph, db_image, "repository", "postgres");
    set_input(&mut graph, db_image, "tag", "16-alpine");

    let db_volume = add(&mut graph, t.volume);
    set_input(&mut graph, db_volume, "source", "pgdata");
    set_input(&mut graph, db_volume, "target", "/var/lib/postgresql/data");

    let db_env = add(&mut graph, t.env_var);
    set_input(&mut graph, db_env, "key", "POSTGRES_PASSWORD");

    let db_health = add(&mut graph, t.healthcheck);
    set_input(&mut graph, db_health, "command", "pg_isready -U app");
    set_input(&mut graph, db_health, "interval", 10_i64);

    let db = add(&mut graph, t.service);
    set_param(&mut graph, db, "name", "db");

    let stack = add(&mut graph, t.stack);
    set_param(&mut graph, stack, "name", "acme-platform");

    // ------------------------------------------------------------ wires
    let links: &[(NodeId, &str, NodeId, &str)] = &[
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
    ];
    for &(from, from_socket, to, to_socket) in links {
        graph
            .connect(library, (from, from_socket), (to, to_socket))
            .expect("sample graph is well-typed and acyclic");
    }

    let _ = nodez::layered(
        &mut graph,
        &LayoutOptions {
            column_gap: 70.0,
            row_gap: 22.0,
            origin: pos2(0.0, 0.0),
            sweeps: 6,
        },
        |graph, node| node_size(graph, library, node, style).y,
    );

    graph
}

fn set_input(graph: &mut Graph, node: NodeId, socket: &str, value: impl Into<nodez::Value>) {
    if let Some(node) = graph.node_mut(node) {
        node.set_input_value(socket, value);
    }
}

fn set_param(graph: &mut Graph, node: NodeId, param: &str, value: impl Into<nodez::Value>) {
    if let Some(node) = graph.node_mut(node) {
        node.set_param(param, value);
    }
}
