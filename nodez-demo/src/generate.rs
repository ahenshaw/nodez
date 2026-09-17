//! Turning the graph into a config file.
//!
//! The rules live on the node types in [`crate::nodes`]; this is only the entry
//! point that folds a graph and reports what is wrong with it.

use std::collections::HashSet;

use nodez::{Graph, NodeError, NodeLibrary, NodeId, Payload, Rules};

use crate::nodes::{Config, StackDoc};
use crate::yaml;

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
pub fn generate(graph: &Graph, library: &NodeLibrary, rules: &Rules<Config>) -> Generated {
    let mut generated = Generated::default();

    let Some(stack_template) = library.id("stack") else {
        return generated;
    };
    let stacks: Vec<NodeId> = graph
        .nodes_of_template(stack_template)
        .map(|node| node.id)
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

    match graph.evaluate::<Payload, NodeError>(library, stack, |ctx| rules.run(&ctx)) {
        Ok(payload) => match payload.downcast::<StackDoc>() {
            Ok(document) => generated.text = yaml::to_yaml(&document.0),
            Err(_) => generated.text = "# the stack node produced nothing\n".to_owned(),
        },
        Err(e) => {
            generated.problems.push(e.to_string());
            generated.text = format!("# generation failed\n# {e}\n");
        }
    }

    let mut names = HashSet::new();
    for id in &generated.contributing {
        if graph.template_of(library, *id).is_some_and(|t| t.id == "service")
            && let Some(name) = graph.param(library, *id, "name").and_then(|v| v.as_str().map(str::to_owned))
            && !names.insert(name.clone())
        {
            generated
                .problems
                .push(format!("Duplicate service name `{name}`."));
        }
    }

    generated
}
