//! A starting graph, so the app has something to show on first run.
//!
//! Also a worked example of building a graph in code: add nodes by template id,
//! set their inline values, wire them up, then let `nodez::layered` place them.

use egui::pos2;
use nodez::{EditorStyle, Graph, LayoutOptions, NodeId, NodeLibrary, Value, node_size};

pub fn build(library: &NodeLibrary, style: &EditorStyle) -> Graph {
    let mut graph = Graph::new();

    let add = |graph: &mut Graph, template: &str| -> NodeId {
        let id = library
            .id(template)
            .unwrap_or_else(|| panic!("`{template}` is registered"));
        graph.add_node(library, id, pos2(0.0, 0.0))
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
        // DATABASE_URL is assembled from literals and a secret reference,
        // which is what the Join Text node is for.
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
            .connect(library, (from, from_socket), (to, to_socket))
            .expect("the sample graph is well-typed and acyclic");
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
