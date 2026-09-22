//! A FutureSDR flowgraph, and the Rust program that runs it.
//!
//!     cargo run -p nodez --features derive,app --example futuresdr
//!
//! The companion to [`gnuradio`](./gnuradio.rs): the same kind of editor over
//! the same kind of graph, emitting a Rust `main` against
//! [FutureSDR](https://www.futuresdr.org/) instead of a Python top block.
//! Worth having as a pair, because the two show what changes when the target
//! changes and what does not. The graph, the node kinds, the routing and the
//! walk over the graph are the same shape; what differs is one function.
//!
//! What FutureSDR asks for that GNU Radio does not:
//!
//! Blocks are Rust values with types, so each one is `let name = ...` and the
//! sample type travels in the turbofish. Which means a generator has to know
//! more than a block's name — it has to know its constructor's shape — and
//! that is why [`construct`] reads the way it does.
//!
//! Wires are a macro rather than method calls. `connect!` takes one `a > b`
//! per statement, and a block with two inputs is reached by naming the port —
//! `combine.in0`. So the sockets here are *named after FutureSDR's ports*,
//! and the generator asks the template how many stream inputs it has rather
//! than keeping a table of which blocks need naming.
//!
//! The node kinds are one-to-one with real blocks in `futuresdr::blocks` and
//! the constructor signatures are the ones in FutureSDR 0.8 — checked by
//! building and running what this writes, not by eye. There is no noise
//! source in the crate, so there is no Noise Source node; a Null Source and
//! an Apply do that job where the Python example used one.
//!
//! FutureSDR 0.8 is `#![feature(return_type_notation)]`, so a program built
//! against it needs nightly. That is the crate's requirement, not this
//! example's, and the generated `Cargo.toml` comment says so.

// The structs below are schema, not storage. `#[derive(NodeType)]` reads their
// fields to build the templates and nothing here ever constructs one, because
// the generator works from the graph rather than from folded values — so every
// field is, strictly, never read.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;

use nodez::app::{EditorApp, Preview};
use nodez::{Graph, Node, NodeId, NodeLibrary, NodeTemplate, NodeType, SocketType, Value};

// ------------------------------------------------------------- wire types

/// A stream of `f32` samples, which is what every block here speaks.
#[derive(Clone, Debug, SocketType)]
#[socket(shape = "square", description = "A stream of f32 samples.")]
struct Stream(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Waveform {
    Cos,
    Sin,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Op {
    Add,
    Subtract,
    Multiply,
}

// ------------------------------------------------------------------ input

#[derive(Debug, NodeType)]
#[node(
    id = "variable",
    label = "Variable",
    category = "Input",
    description = "A `let` the program declares once and every block can share.",
    keywords = "samp_rate, constant, binding",
    output = f64,
    output_name = "Value"
)]
struct Variable {
    #[param(default = "samp_rate", hint = "name", hide_label)]
    name: String,
    #[input(label = "", default = 320000.0)]
    value: f64,
}

// ----------------------------------------------------------------- source

#[derive(Debug, NodeType)]
#[node(
    id = "signal_source",
    label = "Signal Source",
    category = "Source",
    description = "A tone, from SignalSourceBuilder.",
    keywords = "tone, oscillator, nco",
    output = Stream,
    output_name = "output"
)]
struct SignalSource {
    #[param(default = Waveform::Cos)]
    waveform: Waveform,
    #[input(label = "Sample Rate", default = 320000.0)]
    sample_rate: f64,
    #[input(label = "Frequency", default = 1000.0)]
    frequency: f64,
    #[input(label = "Amplitude", default = 1.0)]
    amplitude: f64,
    #[input(label = "Phase", default = 0.0)]
    phase: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "null_source",
    label = "Null Source",
    category = "Source",
    description = "Zeroes, for driving a chain that makes its own samples.",
    keywords = "zero, empty",
    output = Stream,
    output_name = "output"
)]
struct NullSource {}

#[derive(Debug, NodeType)]
#[node(
    id = "file_source",
    label = "File Source",
    category = "Source",
    description = "Samples read back from disk.",
    keywords = "read, playback, capture",
    output = Stream,
    output_name = "output"
)]
struct FileSource {
    #[input(label = "File", default = "input.bin")]
    file: String,
    #[input(label = "Repeat", default = true)]
    repeat: bool,
}

// ------------------------------------------------------------------- flow

#[derive(Debug, NodeType)]
#[node(
    id = "throttle",
    label = "Throttle",
    category = "Flow",
    description = "Holds the flowgraph to real time. One per chain, never more.",
    keywords = "rate, realtime, pace",
    output = Stream,
    output_name = "output"
)]
struct Throttle {
    #[input(label = "input")]
    input: Stream,
    #[input(label = "Sample Rate", default = 320000.0)]
    sample_rate: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "head",
    label = "Head",
    category = "Flow",
    description = "Passes a fixed number of samples and then finishes the graph.",
    keywords = "limit, count, stop",
    output = Stream,
    output_name = "output"
)]
struct Head {
    #[input(label = "input")]
    input: Stream,
    #[input(label = "Items", default = 1000000, min = 1, max = 1000000000)]
    items: i64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "combine",
    label = "Combine",
    category = "Flow",
    description = "Two streams into one, sample by sample.",
    keywords = "add, multiply, mix, sum",
    output = Stream,
    output_name = "output"
)]
struct Combine {
    #[param(default = Op::Add)]
    op: Op,
    /// Named for FutureSDR's own ports, because that is what `connect!` wants
    /// to be told when a block has more than one input.
    #[input(label = "in0")]
    in0: Stream,
    #[input(label = "in1")]
    in1: Stream,
}

#[derive(Debug, NodeType)]
#[node(
    id = "apply",
    label = "Apply Gain",
    category = "Flow",
    description = "Scales every sample, as an Apply with a closure.",
    keywords = "gain, scale, attenuate, apply",
    output = Stream,
    output_name = "output"
)]
struct ApplyGain {
    #[input(label = "input")]
    input: Stream,
    #[input(label = "Gain", default = 1.0)]
    gain: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "resampler",
    label = "Resampler",
    category = "Flow",
    description = "Changes the sample rate by a ratio, with a polyphase FIR.",
    keywords = "fir, interpolate, decimate, rate",
    output = Stream,
    output_name = "output"
)]
struct Resampler {
    #[input(label = "input")]
    input: Stream,
    #[input(label = "Interpolation", default = 1, min = 1, max = 1024)]
    interpolation: i64,
    #[input(label = "Decimation", default = 1, min = 1, max = 1024)]
    decimation: i64,
}

// ------------------------------------------------------------------- sink

#[derive(Debug, NodeType)]
#[node(
    id = "file_sink",
    label = "File Sink",
    category = "Sink",
    description = "Writes raw samples to disk.",
    keywords = "write, capture, record",
    produces = String
)]
struct FileSink {
    #[input(label = "input")]
    input: Stream,
    #[input(label = "File", default = "output.bin")]
    file: String,
}

#[derive(Debug, NodeType)]
#[node(
    id = "audio_sink",
    label = "Audio Sink",
    category = "Sink",
    description = "Sends the stream to the sound card. Needs the `audio` feature.",
    keywords = "speaker, sound, listen",
    produces = String
)]
struct AudioSink {
    #[input(label = "input")]
    input: Stream,
    // A float like every other rate here, so one Variable can drive them all.
    // FutureSDR wants a `u32`, and the cast is the generator's business.
    #[input(label = "Sample Rate", default = 48000.0)]
    sample_rate: f64,
    #[input(label = "Channels", default = 1, min = 1, max = 8)]
    channels: i64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "null_sink",
    label = "Null Sink",
    category = "Sink",
    description = "Throws the stream away, so a branch can be left wired but idle.",
    keywords = "discard, terminate, drop",
    produces = String
)]
struct NullSink {
    #[input(label = "input")]
    input: Stream,
}

// --------------------------------------------------------- rust generation

/// The `futuresdr::blocks` name each kind of node is built from, and the stem
/// its `let` bindings are named after.
///
/// A Variable is not a block; it is a `let` of its own in the preamble.
fn block(template: &str) -> Option<(&'static str, &'static str)> {
    Some(match template {
        "signal_source" => ("SignalSourceBuilder", "signal_source"),
        "null_source" => ("NullSource", "null_source"),
        "file_source" => ("FileSource", "file_source"),
        "throttle" => ("Throttle", "throttle"),
        "head" => ("Head", "head"),
        "combine" => ("Combine", "combine"),
        "apply" => ("Apply", "apply"),
        "resampler" => ("FirBuilder", "resampler"),
        "file_sink" => ("FileSink", "file_sink"),
        "audio_sink" => ("AudioSink", "audio_sink"),
        "null_sink" => ("NullSink", "null_sink"),
        _ => return None,
    })
}

/// Everything the generator needs to name and read one graph.
struct Program<'a> {
    graph: &'a Graph,
    library: &'a NodeLibrary,
    /// The `let` name of each block, and of each variable.
    names: HashMap<NodeId, String>,
    problems: Vec<String>,
}

impl<'a> Program<'a> {
    fn new(graph: &'a Graph, library: &'a NodeLibrary) -> Self {
        let mut used: HashMap<&str, usize> = HashMap::new();
        let mut names = HashMap::new();
        let mut problems = Vec::new();

        for node in Self::ordered(graph, &mut problems) {
            let Some(template) = graph.template_of(library, node) else {
                continue;
            };
            let name = match block(&template.id) {
                Some((_, stem)) => {
                    let seen = used.entry(stem).or_default();
                    let name = format!("{stem}_{seen}");
                    *seen += 1;
                    name
                }
                // A variable is called whatever it was named.
                None => graph
                    .node(node)
                    .and_then(|n| n.param("name"))
                    .and_then(|v| v.as_str().map(rust_name))
                    .unwrap_or_else(|| "variable".to_owned()),
            };
            names.insert(node, name);
        }

        Self {
            graph,
            library,
            names,
            problems,
        }
    }

    /// The nodes, upstream first, so a block is made after whatever it is
    /// measured in.
    fn ordered(graph: &Graph, problems: &mut Vec<String>) -> Vec<NodeId> {
        match graph.topological_order() {
            Ok(order) => order,
            Err(cycle) => {
                problems.push(format!("{cycle}; a flowgraph cannot feed itself"));
                graph.node_ids().collect()
            }
        }
    }

    fn template(&self, node: NodeId) -> Option<&'a NodeTemplate> {
        self.graph.template_of(self.library, node)
    }

    /// One of a node's inputs as Rust: the name of the variable driving it if
    /// one is, and otherwise the value typed into it.
    ///
    /// The same trick the GNU Radio example uses, and the same reason: nothing
    /// declares that `samp_rate` is a sample rate. A wire says which blocks
    /// share it.
    fn input(&self, node: &Node, socket: &str) -> String {
        let link = self
            .graph
            .connections()
            .find(|c| c.to.node == node.id && c.to.socket == socket);
        if let Some(name) = link.and_then(|c| self.names.get(&c.from.node)) {
            return name.clone();
        }
        node.input_value(socket)
            .map(|value| literal(&value))
            .unwrap_or_else(|| "0.0".to_owned())
    }

    /// The same, as an `f32` — which is what most of FutureSDR's constructors
    /// take even where the value reads as a rate.
    fn input_f32(&self, node: &Node, socket: &str) -> String {
        let value = self.input(node, socket);
        if value.ends_with("f64") || self.is_name(&value) {
            format!("{value} as f32")
        } else {
            value.replace("f64", "")
        }
    }

    fn is_name(&self, value: &str) -> bool {
        self.names.values().any(|name| name == value)
    }

    /// The same as an integer of some width. A number typed in carries the
    /// width as a suffix; a variable driving it has to be cast, because it is
    /// an `f64` binding whatever it is called.
    fn input_int(&self, node: &Node, socket: &str, width: &str) -> String {
        let value = self.input(node, socket);
        if self.is_name(&value) {
            format!("{value} as {width}")
        } else {
            format!("{}{width}", value.trim_end_matches(".0"))
        }
    }

    fn param(&self, node: &Node, name: &str) -> String {
        node.param(name)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The Rust that makes one block.
    fn construct(&self, node: &Node, template: &NodeTemplate) -> Option<String> {
        let value = |socket| self.input(node, socket);
        let f32_of = |socket| self.input_f32(node, socket);
        Some(match template.id.as_str() {
            "signal_source" => format!(
                "SignalSourceBuilder::<f32>::{}({}, {}, {}, {})",
                self.param(node, "waveform").to_lowercase(),
                f32_of("frequency"),
                f32_of("sample_rate"),
                f32_of("amplitude"),
                f32_of("phase"),
            ),
            "null_source" => "NullSource::<f32>::new()".to_owned(),
            "file_source" => format!(
                "FileSource::<f32>::new({}, {})",
                value("file"),
                value("repeat"),
            ),
            "throttle" => format!("Throttle::<f32>::new({})", value("sample_rate")),
            "head" => format!("Head::<f32>::new({})", self.input_int(node, "items", "u64")),
            "combine" => format!(
                "Combine::new(|a: &f32, b: &f32| a {} b)",
                match self.param(node, "op").as_str() {
                    "Subtract" => "-",
                    "Multiply" => "*",
                    _ => "+",
                }
            ),
            "apply" => format!("Apply::new(|x: &f32| x * {})", f32_of("gain")),
            "resampler" => format!(
                "FirBuilder::resampling::<f32, f32>({}, {})",
                value("interpolation"),
                value("decimation"),
            ),
            "file_sink" => format!("FileSink::<f32>::new({})", value("file")),
            // The one constructor here that can fail.
            "audio_sink" => format!(
                "AudioSink::new({}, {})?",
                self.input_int(node, "sample_rate", "u32"),
                self.input_int(node, "channels", "u16"),
            ),
            "null_sink" => "NullSink::<f32>::new()".to_owned(),
            _ => return None,
        })
    }

    /// Whether a link carries samples. A sample rate travels down a link too,
    /// and `connect!` is only for the ones that carry a stream.
    fn is_stream(&self, to: NodeId, socket: &str) -> bool {
        let Some(stream) = self.library.types.id("Stream") else {
            return false;
        };
        self.template(to)
            .and_then(|t| t.input_spec(socket))
            .is_some_and(|spec| spec.ty == stream)
    }

    /// How `connect!` should name the far end of a wire.
    ///
    /// A block with one stream input is reached by its name alone; one with
    /// two has to be told which port. Counted off the template rather than
    /// looked up in a table, and the sockets are named after FutureSDR's own
    /// ports so the name is already the right one.
    ///
    /// The port goes *before* the block — `in0.combine_0`. An endpoint in
    /// that macro reads input port, then block, then output port, so the
    /// obvious `combine_0.in0` says "the block called `in0`" and fails
    /// looking for it.
    fn port(&self, to: NodeId, socket: &str) -> String {
        let name = self.names.get(&to).cloned().unwrap_or_default();
        let streams = self
            .template(to)
            .map(|t| {
                t.inputs
                    .iter()
                    .filter(|s| self.is_stream(to, &s.name))
                    .count()
            })
            .unwrap_or(0);
        if streams > 1 {
            format!("{socket}.{name}")
        } else {
            name
        }
    }
}

/// Rust for one value.
fn literal(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::Int(i) => format!("{i}"),
        Value::Float(f) => {
            let text = format!("{f:?}");
            if text.contains('.') { text } else { format!("{text}.0") }
        }
        other => format!("{:?}", other.as_str().unwrap_or_default()),
    }
}

/// A name Rust will accept.
fn rust_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    if cleaned.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

fn generate(graph: &nodez::Graph, library: &NodeLibrary) -> Preview {
    // A group is a flat flowgraph once it runs, so it is one here too.
    let graph = &graph.flatten(library);
    let program = Program::new(graph, library);
    let mut problems = program.problems.clone();
    let order = Program::ordered(graph, &mut Vec::new());

    // Which blocks are used decides the imports, so they are collected as the
    // blocks are made rather than listed by hand.
    let mut imports: Vec<&'static str> = Vec::new();
    let mut audio = false;
    let mut blocks = String::new();
    for id in &order {
        let (Some(node), Some(template), Some(name)) =
            (graph.node(*id), program.template(*id), program.names.get(id))
        else {
            continue;
        };
        let Some(made) = program.construct(node, template) else {
            continue;
        };
        if let Some((import, _)) = block(&template.id) {
            if !imports.contains(&import) {
                imports.push(import);
            }
            audio |= import == "AudioSink";
        }
        let _ = writeln!(blocks, "    let {name} = {made};");
    }
    imports.sort_unstable();

    let mut out = String::new();
    out.push_str("// Generated by nodez.\n//\n");
    // FutureSDR 0.8 is `#![feature(return_type_notation)]` itself, so this is
    // about the crate rather than about anything generated here.
    out.push_str("// Built with nightly, which FutureSDR 0.8 asks for.\n//\n");
    out.push_str("// Cargo.toml:\n");
    out.push_str("//   [dependencies]\n//   anyhow = \"1\"\n");
    if audio {
        out.push_str("//   futuresdr = { version = \"0.8\", features = [\"audio\"] }\n");
    } else {
        out.push_str("//   futuresdr = \"0.8\"\n");
    }
    out.push_str("\nuse anyhow::Result;\n");
    if audio {
        out.push_str("use futuresdr::blocks::audio::AudioSink;\n");
    }
    let listed: Vec<&str> = imports
        .iter()
        .copied()
        .filter(|import| *import != "AudioSink")
        .collect();
    if !listed.is_empty() {
        let _ = writeln!(out, "use futuresdr::blocks::{{{}}};", listed.join(", "));
    }
    out.push_str("use futuresdr::prelude::*;\n\n");
    out.push_str("fn main() -> Result<()> {\n    let mut fg = Flowgraph::new();\n\n");

    let mut variables = String::new();
    for id in &order {
        let (Some(node), Some(template), Some(name)) =
            (graph.node(*id), program.template(*id), program.names.get(id))
        else {
            continue;
        };
        if block(&template.id).is_some() {
            continue;
        }
        let _ = writeln!(
            variables,
            "    let {name}: f64 = {};",
            program.input(node, "value")
        );
    }
    if !variables.is_empty() {
        out.push_str(&variables);
        out.push('\n');
    }

    if blocks.is_empty() {
        problems.push("no blocks, so the flowgraph does nothing".to_owned());
    } else {
        out.push_str(&blocks);
        out.push('\n');
    }

    // One wire per statement. `connect!` will chain `a > b > c`, but a wire at
    // a time is what a graph actually is, and it needs no case for the block
    // that happens to have two inputs.
    let mut wires = String::new();
    for connection in graph.connections() {
        if !program.is_stream(connection.to.node, &connection.to.socket) {
            continue;
        }
        let Some(from) = program.names.get(&connection.from.node) else {
            continue;
        };
        let _ = writeln!(
            wires,
            "        {from} > {};",
            program.port(connection.to.node, &connection.to.socket)
        );
    }
    if wires.is_empty() {
        out.push_str("    // Nothing is wired up yet.\n");
    } else {
        let _ = writeln!(out, "    connect!(fg,\n{wires}    );");
    }

    out.push_str("\n    Runtime::new().run(fg)?;\n    Ok(())\n}\n");
    Preview::text(out).problems(problems)
}

// ------------------------------------------------------------------- setup

fn register<K: NodeType>(library: &mut NodeLibrary) {
    let template = K::template(&mut library.types);
    library.register(template);
}

fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    register::<Variable>(&mut library);
    register::<SignalSource>(&mut library);
    register::<NullSource>(&mut library);
    register::<FileSource>(&mut library);
    register::<Throttle>(&mut library);
    register::<Head>(&mut library);
    register::<Combine>(&mut library);
    register::<ApplyGain>(&mut library);
    register::<Resampler>(&mut library);
    register::<FileSink>(&mut library);
    register::<AudioSink>(&mut library);
    register::<NullSink>(&mut library);
    nodez::group::register_pads(&mut library);

    let graph = starting_graph(&library);

    // `--print` writes the program for the starting graph and exits, so the
    // generator can be run without a display.
    if std::env::args().any(|arg| arg == "--print") {
        let program = generate(&graph, &library);
        for problem in &program.problems {
            eprintln!("warning: {problem}");
        }
        print!("{}", program.text);
        return Ok(());
    }

    EditorApp::new(library)
        .graph(graph)
        .title("nodez \u{2014} FutureSDR flowgraph")
        .file("futuresdr-graph.json")
        .groups_dir("groups")
        .preview_extension("rs")
        .json_files()
        .preview(generate)
        .run()
}

/// A tone and a second tone multiplied together, filtered, scaled, resampled,
/// and sent to both a file and the sound card.
fn starting_graph(library: &NodeLibrary) -> Graph {
    let mut graph = Graph::new();
    let add_node = |graph: &mut Graph, id: &str| {
        let template = library
            .id(id)
            .unwrap_or_else(|| panic!("`{id}` is registered"));
        graph.add_node(library, template, egui::pos2(0.0, 0.0))
    };

    let samp_rate = add_node(&mut graph, "variable");
    let audio_rate = add_node(&mut graph, "variable");
    let tone = add_node(&mut graph, "signal_source");
    let carrier = add_node(&mut graph, "signal_source");
    let mixer = add_node(&mut graph, "combine");
    let throttle = add_node(&mut graph, "throttle");
    let gain = add_node(&mut graph, "apply");
    let resample = add_node(&mut graph, "resampler");
    let to_file = add_node(&mut graph, "file_sink");
    let to_audio = add_node(&mut graph, "audio_sink");

    graph
        .node_mut(audio_rate)
        .expect("just added")
        .set_param("name", Value::from("audio_rate"));

    for (node, socket, value) in [
        (samp_rate, "value", Value::Float(320_000.0)),
        (audio_rate, "value", Value::Float(48_000.0)),
        (tone, "frequency", Value::Float(1_200.0)),
        (tone, "amplitude", Value::Float(0.7)),
        (carrier, "frequency", Value::Float(40_000.0)),
        (gain, "gain", Value::Float(0.5)),
        (resample, "decimation", Value::Int(4)),
        (to_file, "file", Value::from("mixed.bin")),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_input_value(socket, value);
    }

    // Streams, then the sample rates the blocks share.
    for (from, to, socket) in [
        (tone, mixer, "in0"),
        (carrier, mixer, "in1"),
        (mixer, throttle, "input"),
        (throttle, gain, "input"),
        (gain, resample, "input"),
        (resample, to_file, "input"),
        (resample, to_audio, "input"),
        (samp_rate, tone, "sample_rate"),
        (samp_rate, carrier, "sample_rate"),
        (samp_rate, throttle, "sample_rate"),
        (audio_rate, to_audio, "sample_rate"),
    ] {
        let out = graph
            .template_of(library, from)
            .and_then(|template| template.outputs.first())
            .map(|socket| socket.name.clone())
            .expect("every node wired from here has an output");
        graph
            .connect(library, (from, out.as_str()), (to, socket))
            .unwrap_or_else(|e| panic!("wiring {socket}: {e}"));
    }

    let _ = nodez::layered(&mut graph, &nodez::LayoutOptions::default(), |graph, node| {
        nodez::node_size(graph, library, node, &nodez::EditorStyle::default())
    });
    graph
}
