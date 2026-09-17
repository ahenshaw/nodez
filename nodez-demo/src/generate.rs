//! Turning the graph into a config file.
//!
//! Every node folds to a [`Fragment`]; the stack node assembles the fragments
//! reachable from it into an ordered document, which `yaml` then spells out.
//! Nothing here touches the editor — the same code runs headlessly.

use std::collections::HashSet;

use nodez::{EvalContext, Graph, NodeId, Value};

use crate::domain::Domain;
use crate::yaml;

/// What one node evaluates to.
#[derive(Clone, Debug, Default)]
pub enum Fragment {
    /// A muted node, or one whose required inputs are missing.
    #[default]
    Nothing,
    /// A plain value: text, number, flag or an image reference.
    Scalar(Value),
    /// Inline `KEY=value` entries.
    Env(Vec<String>),
    /// A path listed under `env_file`.
    EnvFile(String),
    Port(String),
    Mount(Value),
    Network { name: String, body: Value },
    Health(Value),
    Service { name: String, body: Value },
    /// The finished document.
    Document(Value),
}

impl Fragment {
    fn text(&self) -> Option<String> {
        match self {
            Self::Scalar(Value::Text(s) | Value::Choice(s)) => Some(s.clone()),
            Self::Scalar(Value::Bool(b)) => Some(b.to_string()),
            Self::Scalar(other) => Some(other.to_literal()),
            _ => None,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Scalar(v) => v.as_f64(),
            _ => None,
        }
    }
}

/// The generator's output, ready to show in a panel or write to disk.
#[derive(Clone, Debug, Default)]
pub struct Generated {
    pub text: String,
    /// Things that are wrong or suspicious, in the order they were found.
    pub problems: Vec<String>,
    /// Nodes that feed the output.
    pub contributing: HashSet<NodeId>,
}

/// Build the config for the graph's stack node.
pub fn generate(graph: &Graph, domain: &Domain) -> Generated {
    let mut generated = Generated::default();

    let stacks: Vec<NodeId> = graph
        .nodes_of_template(domain.templates.stack)
        .map(|n| n.id)
        .collect();
    let Some(&stack) = stacks.first() else {
        generated
            .problems
            .push("No Stack Output node — add one to produce a config.".to_owned());
        generated.text = "# add a Stack Output node\n".to_owned();
        return generated;
    };
    if stacks.len() > 1 {
        generated.problems.push(format!(
            "{} Stack Output nodes; only the first is emitted.",
            stacks.len()
        ));
    }

    // Everything the output depends on. Used both for evaluation and to tell
    // the user which nodes are currently dead weight.
    generated.contributing = graph.ancestors(stack).collect();
    generated.contributing.insert(stack);
    let orphans = graph.node_count() - generated.contributing.len();
    if orphans > 0 {
        generated.problems.push(format!(
            "{orphans} node(s) are not connected to the output and were skipped."
        ));
    }

    let mut names: Vec<String> = Vec::new();
    let result = graph.evaluate::<Fragment, String>(&domain.library, stack, |ctx| {
        let fragment = evaluate_node(&ctx)?;
        if let Fragment::Service { name, .. } = &fragment {
            names.push(name.clone());
        }
        Ok(fragment)
    });

    match result {
        Ok(Fragment::Document(document)) => generated.text = yaml::to_yaml(&document),
        Ok(_) => generated.text = "# the stack node produced nothing\n".to_owned(),
        Err(e) => {
            generated.problems.push(e.to_string());
            generated.text = format!("# generation failed\n# {e}\n");
        }
    }

    let mut seen = HashSet::new();
    for name in &names {
        if !seen.insert(name.clone()) {
            generated
                .problems
                .push(format!("Duplicate service name `{name}`."));
        }
    }

    generated
}

fn evaluate_node(ctx: &EvalContext<'_, Fragment>) -> Result<Fragment, String> {
    if ctx.is_muted() {
        return Ok(Fragment::Nothing);
    }

    match ctx.template().id.as_str() {
        "text" => Ok(Fragment::Scalar(Value::Text(resolve_text(ctx, "value")))),
        "number" => Ok(Fragment::Scalar(Value::Float(resolve_number(
            ctx, "value", 0.0,
        )))),
        "flag" => Ok(Fragment::Scalar(Value::Bool(
            ctx.unlinked_literal("value")
                .as_ref()
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ))),
        "secret" => {
            let name = resolve_text(ctx, "name");
            Ok(Fragment::Scalar(Value::Text(format!("${{{name}}}"))))
        }
        "join" => {
            let separator = ctx.param_str("separator").unwrap_or_default();
            let parts: Vec<String> = ctx
                .inputs("parts")
                .iter()
                .filter_map(|link| link.value.text())
                .collect();
            Ok(Fragment::Scalar(Value::Text(parts.join(&separator))))
        }

        "image" => {
            let repository = resolve_text(ctx, "repository");
            let tag = resolve_text(ctx, "tag");
            if repository.is_empty() {
                return Err("Image needs a repository.".to_owned());
            }
            Ok(Fragment::Scalar(Value::Text(if tag.is_empty() {
                repository
            } else {
                format!("{repository}:{tag}")
            })))
        }
        "env_var" => {
            let key = resolve_text(ctx, "key");
            if key.is_empty() {
                return Err("Environment Variable needs a key.".to_owned());
            }
            Ok(Fragment::Env(vec![format!(
                "{key}={}",
                resolve_text(ctx, "value")
            )]))
        }
        "env_file" => Ok(Fragment::EnvFile(resolve_text(ctx, "path"))),
        "port" => {
            let host = resolve_number(ctx, "host", 0.0) as i64;
            let container = resolve_number(ctx, "container", 0.0) as i64;
            let protocol = ctx.param_str("protocol").unwrap_or_else(|| "tcp".to_owned());
            let mapping = if protocol == "tcp" {
                format!("{host}:{container}")
            } else {
                format!("{host}:{container}/{protocol}")
            };
            Ok(Fragment::Port(mapping))
        }
        "volume" => {
            let source = resolve_text(ctx, "source");
            let target = resolve_text(ctx, "target");
            if target.is_empty() {
                return Err("Volume needs a target path.".to_owned());
            }
            let read_only = resolve_flag(ctx, "read_only");
            let mut entry = vec![
                ("type".to_owned(), Value::Text("bind".to_owned())),
                ("source".to_owned(), Value::Text(source)),
                ("target".to_owned(), Value::Text(target)),
            ];
            if read_only {
                entry.push(("read_only".to_owned(), Value::Bool(true)));
            }
            Ok(Fragment::Mount(Value::Map(entry)))
        }
        "network" => {
            let name = resolve_text(ctx, "name");
            if name.is_empty() {
                return Err("Network needs a name.".to_owned());
            }
            let driver = ctx.param_str("driver").unwrap_or_else(|| "bridge".to_owned());
            Ok(Fragment::Network {
                name,
                body: Value::Map(vec![("driver".to_owned(), Value::Text(driver))]),
            })
        }
        "healthcheck" => {
            let command = resolve_text(ctx, "command");
            if command.is_empty() {
                return Err("Health Check needs a command.".to_owned());
            }
            Ok(Fragment::Health(Value::Map(vec![
                (
                    "test".to_owned(),
                    Value::List(vec![
                        Value::Text("CMD-SHELL".to_owned()),
                        Value::Text(command),
                    ]),
                ),
                (
                    "interval".to_owned(),
                    Value::Text(format!("{}s", resolve_number(ctx, "interval", 30.0) as i64)),
                ),
                (
                    "retries".to_owned(),
                    Value::Int(resolve_number(ctx, "retries", 3.0) as i64),
                ),
            ])))
        }

        "service" => build_service(ctx),
        "stack" => build_stack(ctx),

        other => Err(format!("no rule for node type `{other}`")),
    }
}

fn build_service(ctx: &EvalContext<'_, Fragment>) -> Result<Fragment, String> {
    let name = ctx.param_str("name").unwrap_or_default().trim().to_owned();
    if name.is_empty() {
        return Err("Service needs a name.".to_owned());
    }

    let Some(image) = ctx.input("image").and_then(|link| link.value.text()) else {
        return Err(format!("Service `{name}` has no image connected."));
    };

    let mut body = vec![("image".to_owned(), Value::Text(image))];

    let command = resolve_text(ctx, "command");
    if !command.is_empty() {
        body.push(("command".to_owned(), Value::Text(command)));
    }

    if let Some(restart) = ctx.param_str("restart").filter(|r| r != "no") {
        body.push(("restart".to_owned(), Value::Text(restart)));
    }

    let replicas = resolve_number(ctx, "replicas", 1.0) as i64;
    if replicas > 1 {
        body.push((
            "deploy".to_owned(),
            Value::Map(vec![("replicas".to_owned(), Value::Int(replicas))]),
        ));
    }

    let ports: Vec<Value> = ctx
        .inputs("ports")
        .iter()
        .filter_map(|link| match link.value {
            Fragment::Port(mapping) => Some(Value::Text(mapping.clone())),
            _ => None,
        })
        .collect();
    if !ports.is_empty() {
        body.push(("ports".to_owned(), Value::List(ports)));
    }

    let mut environment = Vec::new();
    let mut env_files = Vec::new();
    for link in ctx.inputs("environment") {
        match link.value {
            Fragment::Env(entries) => {
                environment.extend(entries.iter().cloned().map(Value::Text));
            }
            Fragment::EnvFile(path) if !path.is_empty() => {
                env_files.push(Value::Text(path.clone()));
            }
            _ => {}
        }
    }
    if !environment.is_empty() {
        body.push(("environment".to_owned(), Value::List(environment)));
    }
    if !env_files.is_empty() {
        body.push(("env_file".to_owned(), Value::List(env_files)));
    }

    let volumes: Vec<Value> = ctx
        .inputs("volumes")
        .iter()
        .filter_map(|link| match link.value {
            Fragment::Mount(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect();
    if !volumes.is_empty() {
        body.push(("volumes".to_owned(), Value::List(volumes)));
    }

    let networks: Vec<Value> = ctx
        .inputs("networks")
        .iter()
        .filter_map(|link| match link.value {
            Fragment::Network { name, .. } => Some(Value::Text(name.clone())),
            _ => None,
        })
        .collect();
    if !networks.is_empty() {
        body.push(("networks".to_owned(), Value::List(networks)));
    }

    let depends: Vec<Value> = ctx
        .inputs("depends_on")
        .iter()
        .filter_map(|link| match link.value {
            Fragment::Service { name, .. } => Some(Value::Text(name.clone())),
            _ => None,
        })
        .collect();
    if !depends.is_empty() {
        body.push(("depends_on".to_owned(), Value::List(depends)));
    }

    if let Some(Fragment::Health(probe)) = ctx.input("healthcheck").map(|link| link.value) {
        body.push(("healthcheck".to_owned(), probe.clone()));
    }

    Ok(Fragment::Service {
        name,
        body: Value::Map(body),
    })
}

fn build_stack(ctx: &EvalContext<'_, Fragment>) -> Result<Fragment, String> {
    let version = ctx.param_str("version").unwrap_or_else(|| "3.9".to_owned());
    let name = ctx.param_str("name").unwrap_or_default().trim().to_owned();

    let mut services = Vec::new();
    for link in ctx.inputs("services") {
        if let Fragment::Service { name, body } = link.value {
            services.push((name.clone(), body.clone()));
        }
    }

    // A service that names a network implies the stack declares it, even if the
    // network node is not wired to the stack directly.
    let mut networks: Vec<(String, Value)> = Vec::new();
    for link in ctx.inputs("networks") {
        if let Fragment::Network { name, body } = link.value {
            networks.push((name.clone(), body.clone()));
        }
    }
    for (_, body) in &services {
        let Some(used) = body.get("networks").and_then(Value::as_list) else {
            continue;
        };
        for entry in used {
            let Some(used_name) = entry.as_str() else {
                continue;
            };
            if !networks.iter().any(|(n, _)| n == used_name) {
                networks.push((
                    used_name.to_owned(),
                    Value::Map(vec![(
                        "driver".to_owned(),
                        Value::Text("bridge".to_owned()),
                    )]),
                ));
            }
        }
    }

    let mut document = vec![("version".to_owned(), Value::Text(version))];
    if !name.is_empty() {
        document.push(("name".to_owned(), Value::Text(name)));
    }
    document.push(("services".to_owned(), Value::Map(services)));
    if !networks.is_empty() {
        document.push(("networks".to_owned(), Value::Map(networks)));
    }

    Ok(Fragment::Document(Value::Map(document)))
}

/// An input socket's value: whatever is wired in, else the inline literal.
fn resolve_text(ctx: &EvalContext<'_, Fragment>, socket: &str) -> String {
    if let Some(link) = ctx.input(socket)
        && let Some(text) = link.value.text()
    {
        return text;
    }
    ctx.unlinked_literal(socket)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn resolve_number(ctx: &EvalContext<'_, Fragment>, socket: &str, fallback: f64) -> f64 {
    if let Some(link) = ctx.input(socket)
        && let Some(number) = link.value.number()
    {
        return number;
    }
    ctx.unlinked_literal(socket)
        .as_ref()
        .and_then(Value::as_f64)
        .unwrap_or(fallback)
}

fn resolve_flag(ctx: &EvalContext<'_, Fragment>, socket: &str) -> bool {
    if let Some(link) = ctx.input(socket) {
        if let Fragment::Scalar(Value::Bool(b)) = link.value {
            return *b;
        }
        if let Some(text) = link.value.text() {
            return text == "true";
        }
    }
    ctx.unlinked_literal(socket)
        .as_ref()
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
