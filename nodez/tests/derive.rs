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

/// A list is a socket type, so a node can emit one without a newtype.
#[derive(Debug, NodeType)]
#[node(output = Vec<String>)]
struct Split {
    #[input]
    text: String,
}

/// `one` makes the outer `Vec` the payload: one link carrying a list.
#[derive(Debug, NodeType)]
#[node(produces = String)]
struct Last {
    #[input(one)]
    words: Vec<String>,
}

/// The outer `Vec` is still the fan-in, so each link carries a whole list.
#[derive(Debug, NodeType)]
#[node(produces = Vec<usize>)]
struct Lengths {
    #[input]
    lists: Vec<Vec<String>>,
}

impl Evaluate<Text> for Split {
    fn evaluate(&self) -> Result<Vec<String>, NodeError> {
        Ok(self.text.split(' ').map(str::to_owned).collect())
    }
}
impl Evaluate<Text> for Last {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.words.last().cloned().unwrap_or_default())
    }
}
impl Evaluate<Text> for Lengths {
    fn evaluate(&self) -> Result<Vec<usize>, NodeError> {
        Ok(self.lists.iter().map(Vec::len).collect())
    }
}

#[test]
fn a_list_travels_along_one_wire() {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Text>::new();
    rules.register_all::<(Word, Join, Split, Last, Lengths)>(&mut library);

    let mut graph = Graph::new();
    let word = library.id("word").unwrap();
    let split = library.id("split").unwrap();
    let short = graph.add_node(&library, word, pos2(0.0, 0.0));
    let long = graph.add_node(&library, word, pos2(0.0, 80.0));
    graph.node_mut(short).unwrap().set_input_value("value", "a b");
    graph.node_mut(long).unwrap().set_input_value("value", "c d e");
    let split_short = graph.add_node(&library, split, pos2(200.0, 0.0));
    let split_long = graph.add_node(&library, split, pos2(200.0, 80.0));
    graph.connect(&library, (short, "out"), (split_short, "text")).unwrap();
    graph.connect(&library, (long, "out"), (split_long, "text")).unwrap();

    let last = graph.add_node(&library, library.id("last").unwrap(), pos2(400.0, 0.0));
    graph.connect(&library, (split_long, "out"), (last, "words")).unwrap();
    // `one` is a single-link socket, not a fan-in.
    let spec = &library.get(library.id("last").unwrap()).unwrap().inputs[0];
    assert!(!spec.multi);
    let value = graph
        .evaluate::<Payload, NodeError>(&library, last, |ctx| rules.run(&ctx))
        .unwrap();
    assert_eq!(*value.downcast::<String>().unwrap(), "e");

    let lengths = graph.add_node(&library, library.id("lengths").unwrap(), pos2(400.0, 80.0));
    graph.connect(&library, (split_short, "out"), (lengths, "lists")).unwrap();
    graph.connect(&library, (split_long, "out"), (lengths, "lists")).unwrap();
    let value = graph
        .evaluate::<Payload, NodeError>(&library, lengths, |ctx| rules.run(&ctx))
        .unwrap();
    assert_eq!(*value.downcast::<Vec<usize>>().unwrap(), vec![2, 3]);

    // A list of Text is not Text, in either direction.
    let join = graph.add_node(&library, library.id("join").unwrap(), pos2(400.0, 160.0));
    assert!(graph.connect(&library, (split_short, "out"), (join, "parts")).is_err());
    assert!(graph.connect(&library, (short, "out"), (last, "words")).is_err());
}

/// Every numeric width is a socket type of its own, with an inline editor.
#[derive(Debug, NodeType)]
#[node(output = f32)]
struct Half {
    #[input]
    value: f32,
}

#[derive(Debug, NodeType)]
#[node(output = u8)]
struct Byte {
    #[input]
    value: u8,
}

#[derive(Debug, NodeType)]
#[node(produces = f64)]
struct Wide {
    #[input]
    value: f64,
}

struct Numbers;
impl Fold for Numbers {}

impl Evaluate<Numbers> for Half {
    fn evaluate(&self) -> Result<f32, NodeError> {
        Ok(self.value / 2.0)
    }
}
impl Evaluate<Numbers> for Byte {
    fn evaluate(&self) -> Result<u8, NodeError> {
        Ok(self.value)
    }
}
impl Evaluate<Numbers> for Wide {
    fn evaluate(&self) -> Result<f64, NodeError> {
        Ok(self.value)
    }
}

#[test]
fn every_numeric_width_travels_along_a_wire() {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Numbers>::new();
    rules.register_all::<(Half, Byte, Wide)>(&mut library);

    let mut graph = Graph::new();
    let half = library.id("half").unwrap();
    let first = graph.add_node(&library, half, pos2(0.0, 0.0));
    let second = graph.add_node(&library, half, pos2(200.0, 0.0));
    graph.node_mut(first).unwrap().set_input_value("value", 3.0_f32);
    graph.connect(&library, (first, "out"), (second, "value")).unwrap();

    let halved = graph
        .evaluate::<Payload, NodeError>(&library, second, |ctx| rules.run(&ctx))
        .unwrap();
    assert_eq!(*halved.downcast::<f32>().unwrap(), 0.75);

    // A width is its own type, so an f32 does not feed an f64.
    let wide = graph.add_node(&library, library.id("wide").unwrap(), pos2(400.0, 0.0));
    assert!(graph.connect(&library, (first, "out"), (wide, "value")).is_err());

    // An inline value outside the type's range is not read as that type.
    let byte = graph.add_node(&library, library.id("byte").unwrap(), pos2(0.0, 80.0));
    graph.node_mut(byte).unwrap().set_input_value("value", 200_i64);
    let read = graph
        .evaluate::<Payload, NodeError>(&library, byte, |ctx| rules.run(&ctx))
        .unwrap();
    assert_eq!(*read.downcast::<u8>().unwrap(), 200);
    graph.node_mut(byte).unwrap().set_input_value("value", 300_i64);
    assert!(graph
        .evaluate::<Payload, NodeError>(&library, byte, |ctx| rules.run(&ctx))
        .is_err());
}
