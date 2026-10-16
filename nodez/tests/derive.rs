//! The derive, read back through a real graph.

use nodez::egui::pos2;
use nodez::{Evaluate, Fold, Graph, Multi, NodeError, NodeLibrary, NodeType, Payload, Rules};

#[derive(Debug, NodeType)]
#[node(output = String)]
struct Word {
    #[input]
    value: String,
}

/// A plain `Vec` on an input is a fan-in, read in link order.
#[derive(Debug, NodeType)]
#[node(produces = String)]
struct Join {
    #[input]
    parts: Vec<String>,
}

/// `Multi` is the same socket, with the source of each value kept.
#[derive(Debug, NodeType)]
#[node(produces = usize)]
struct Count {
    #[input]
    parts: Multi<String>,
}

struct Text;
impl Fold for Text {}

impl Evaluate<Text> for Word {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.value.clone())
    }
}
impl Evaluate<Text> for Join {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.parts.join(" "))
    }
}
impl Evaluate<Text> for Count {
    fn evaluate(&self) -> Result<usize, NodeError> {
        Ok(self.parts.links().count())
    }
}

#[test]
fn vec_input_is_a_fan_in() {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Text>::new();
    rules.register_all::<(Word, Join, Count)>(&mut library);

    let mut graph = Graph::new();
    let word = library.id("word").unwrap();
    let hello = graph.add_node(&library, word, pos2(0.0, 0.0));
    let world = graph.add_node(&library, word, pos2(0.0, 80.0));
    graph.node_mut(hello).unwrap().set_input_value("value", "hello");
    graph.node_mut(world).unwrap().set_input_value("value", "world");

    let join = graph.add_node(&library, library.id("join").unwrap(), pos2(200.0, 0.0));
    let count = graph.add_node(&library, library.id("count").unwrap(), pos2(200.0, 80.0));
    for target in [join, count] {
        graph.connect(&library, (hello, "out"), (target, "parts")).unwrap();
        graph.connect(&library, (world, "out"), (target, "parts")).unwrap();
    }

    let joined = graph
        .evaluate::<Payload, NodeError>(&library, join, |ctx| rules.run(&ctx))
        .unwrap();
    assert_eq!(*joined.downcast::<String>().unwrap(), "hello world");

    let counted = graph
        .evaluate::<Payload, NodeError>(&library, count, |ctx| rules.run(&ctx))
        .unwrap();
    assert_eq!(*counted.downcast::<usize>().unwrap(), 2);
}
