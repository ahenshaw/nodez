//! Turning the graph into a config file.
//!
//! Every node folds to a [`Fragment`]; the stack node assembles the fragments
//! reachable from it into an ordered document, which `yaml` then spells out.
//! Nothing here touches the editor — the same code runs headlessly.

use std::collections::HashSet;

use nodez::{EvalContext, Graph, NodeId, NodeLibrary, Value};

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
pub fn generate(graph: &Graph, library: &NodeLibrary) -> Generated {
    let mut generated = Generated::default();

    let Some(stack_template) = library.id("stack") else {
        return generated;
    };
    let stacks: Vec<NodeId> = graph
        .nodes_of_template(stack_template)
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
    let result = graph.evaluate::<Fragment, String>(library, stack, |ctx| {
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
            let entry = Value::map()
                .set("type", "bind")
                .set("source", source)
                .set("target", target)
                .set_if(resolve_flag(ctx, "read_only"), "read_only", true);
            Ok(Fragment::Mount(entry.into()))
        }
        "network" => {
            let name = resolve_text(ctx, "name");
            if name.is_empty() {
                return Err("Network needs a name.".to_owned());
            }
            let driver = ctx.param_str("driver").unwrap_or_else(|| "bridge".to_owned());
            Ok(Fragment::Network {
                name,
                body: Value::map().set("driver", driver).into(),
            })
        }
        "healthcheck" => {
            let command = resolve_text(ctx, "command");
            if command.is_empty() {
                return Err("Health Check needs a command.".to_owned());
            }
            let probe = Value::map()
                .set_list("test", ["CMD-SHELL".to_owned(), command])
                .set(
                    "interval",
                    format!("{}s", resolve_number(ctx, "interval", 30.0) as i64),
                )
                .set("retries", resolve_number(ctx, "retries", 3.0) as i64);
            Ok(Fragment::Health(probe.into()))
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

    // Env is the one socket type carrying two shapes: inline entries from a
    // variable, or a path from a file.
    let mut environment = Vec::new();
    let mut env_files = Vec::new();
    for link in ctx.inputs("environment") {
        match link.value {
            Fragment::Env(entries) => environment.extend(entries.iter().cloned()),
            Fragment::EnvFile(path) if !path.is_empty() => env_files.push(path.clone()),
            _ => {}
        }
    }

    let replicas = resolve_number(ctx, "replicas", 1.0) as i64;
    let command = resolve_text(ctx, "command");

    let body = Value::map()
        .set("image", image)
        .set_if(!command.is_empty(), "command", command)
        .set_some(
            "restart",
            ctx.param_str("restart").filter(|restart| restart != "no"),
        )
        .set_if(
            replicas > 1,
            "deploy",
            Value::map().set("replicas", replicas),
        )
        .set_list("ports", collect(ctx, "ports", |fragment| match fragment {
            Fragment::Port(mapping) => Some(Value::Text(mapping.clone())),
            _ => None,
        }))
        .set_list("environment", environment)
        .set_list("env_file", env_files)
        .set_list("volumes", collect(ctx, "volumes", |fragment| match fragment {
            Fragment::Mount(entry) => Some(entry.clone()),
            _ => None,
        }))
        .set_list("networks", collect(ctx, "networks", |fragment| match fragment {
            Fragment::Network { name, .. } => Some(Value::Text(name.clone())),
            _ => None,
        }))
        .set_list("depends_on", collect(ctx, "depends_on", |fragment| match fragment {
            Fragment::Service { name, .. } => Some(Value::Text(name.clone())),
            _ => None,
        }))
        .set_some(
            "healthcheck",
            match ctx.input("healthcheck").map(|link| link.value) {
                Some(Fragment::Health(probe)) => Some(probe.clone()),
                _ => None,
            },
        );

    Ok(Fragment::Service {
        name,
        body: body.into(),
    })
}

/// Every fragment arriving at a multi-input that matches a shape.
fn collect(
    ctx: &EvalContext<'_, Fragment>,
    socket: &str,
    mut pick: impl FnMut(&Fragment) -> Option<Value>,
) -> Vec<Value> {
    ctx.inputs(socket)
        .iter()
        .filter_map(|link| pick(link.value))
        .collect()
}

fn build_stack(ctx: &EvalContext<'_, Fragment>) -> Result<Fragment, String> {
    let mut services = nodez::MapBuilder::new();
    for link in ctx.inputs("services") {
        if let Fragment::Service { name, body } = link.value {
            services = services.set(name.clone(), body.clone());
        }
    }

    let mut networks: Vec<(String, Value)> = Vec::new();
    for link in ctx.inputs("networks") {
        if let Fragment::Network { name, body } = link.value {
            networks.push((name.clone(), body.clone()));
        }
    }

    // A service that names a network implies the stack declares it, even if the
    // network node is not wired to the stack directly.
    let declared: Vec<(String, Value)> = services.clone().entries();
    for (_, body) in &declared {
        let Some(used) = body.get("networks").and_then(Value::as_list) else {
            continue;
        };
        for entry in used {
            let Some(name) = entry.as_str() else { continue };
            if !networks.iter().any(|(declared, _)| declared == name) {
                networks.push((
                    name.to_owned(),
                    Value::map().set("driver", "bridge").into(),
                ));
            }
        }
    }

    let name = ctx.param_str("name").unwrap_or_default().trim().to_owned();
    let document = Value::map()
        .set(
            "version",
            ctx.param_str("version").unwrap_or_else(|| "3.9".to_owned()),
        )
        .set_if(!name.is_empty(), "name", name)
        .set("services", services)
        .set_map("networks", networks.into_iter().collect());

    Ok(Fragment::Document(document.into()))
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
