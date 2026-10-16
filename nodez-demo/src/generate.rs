//! Turning the graph into a config file.
//!
//! The rules live on the node types in [`crate::nodes`]; this is only the entry
//! point that folds a graph and reports what is wrong with it.

use std::collections::HashSet;

use nodez::{Graph, NodeError, NodeLibrary, NodeId, Payload, Rules, TemplateId};

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
pub fn generate(shown: &Graph, library: &NodeLibrary, rules: &Rules<Config>) -> Generated {
    let mut generated = Generated::default();
    // Groups are only how the graph is drawn. What runs is every group node
    // replaced by what is inside it, which the rules below can evaluate.
    let graph = &shown.flatten(library);

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

    // Everything the output depends on, which is what is checked below.
    let mut feeding: HashSet<NodeId> = graph.ancestors(stack).collect();
    feeding.insert(stack);

    // And the same in the graph as shown, for the inspector to call live.
    // There the output is the Stack Output node, or the group node it sits
    // inside, however deep.
    let output = shown
        .nodes()
        .find(|node| node.id == stack || holds(library, node.template, stack_template))
        .map_or(stack, |node| node.id);
    generated.contributing = shown.ancestors(output).collect();
    generated.contributing.insert(output);
    let orphans = shown.node_count() - generated.contributing.len();
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
    for id in &feeding {
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

/// Whether a template is a group with a node of `wanted` somewhere inside.
fn holds(library: &NodeLibrary, template: TemplateId, wanted: TemplateId) -> bool {
    library.group(template).is_some_and(|inside| {
        inside
            .nodes()
            .any(|node| node.template == wanted || holds(library, node.template, wanted))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use nodez::{EditorStyle, NodeId};

    use super::*;
    use crate::{nodes, sample};

    fn grouped_the_same(pick: impl Fn(&Graph, &NodeLibrary) -> HashSet<NodeId>) {
        let (mut library, rules) = nodes::library();
        let mut graph = sample::build(&library, &EditorStyle::blender_dark());
        let before = generate(&graph, &library, &rules);
        assert!(before.problems.is_empty(), "{:?}", before.problems);

        let chosen = pick(&graph, &library);
        nodez::group::make_group(&mut graph, &mut library, &chosen, "grouped", "Grouped", "Group")
            .unwrap();
        let after = generate(&graph, &library, &rules);
        assert!(after.problems.is_empty(), "{:?}", after.problems);
        assert_eq!(after.text, before.text);
    }

    #[test]
    fn a_service_in_a_group_generates_the_same_config() {
        grouped_the_same(|graph, library| {
            let db = graph
                .node_ids()
                .find(|&id| {
                    graph.template_of(library, id).is_some_and(|t| t.id == "service")
                        && graph.param(library, id, "name").and_then(|v| v.as_str().map(str::to_owned))
                            == Some("db".to_owned())
                })
                .expect("the sample has a db service");
            graph.ancestors(db).chain([db]).collect()
        });
    }

    /// Everything but one Text node, so the group has a wire coming in --
    /// a group with nothing crossing its edge is not one nodez will make.
    #[test]
    fn the_output_in_a_group_generates_the_same_config() {
        grouped_the_same(|graph, library| {
            let text = library.id("text").unwrap();
            let outside = graph.nodes_of_template(text).next().unwrap().id;
            graph.node_ids().filter(|&id| id != outside).collect()
        });
    }
}
