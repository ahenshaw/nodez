//! A GNU Radio flowgraph, and the Python that runs it.
//!
//!     cargo run -p nodez --features derive,app --example gnuradio
//!
//! The node kinds are a slice of GNU Radio's blocks — enough to build and
//! filter a signal — and the preview pane is the kind of top-block script GNU
//! Radio Companion writes out of a `.grc` file: declare the variables, make
//! the blocks, connect the ports.
//!
//! Like [`blender_nodes`](./blender_nodes.rs) and unlike
//! [`quickstart`](./quickstart.rs), this walks the graph rather than folding
//! it. A flowgraph is a description of a machine, not a sum, so what the
//! generator wants is every block and every connection.
//!
//! Two things here are worth stealing for a generator of your own:
//!
//! A Variable block has an ordinary `f64` output, and every block that needs a
//! sample rate has an ordinary `f64` input. Wire one to the other and the
//! script says `samp_rate`; leave the input alone and it says the number. GNU
//! Radio Companion's variables work exactly this way, and the graph already
//! knows which it is, so nothing has to be declared twice.
//!
//! Not every wire is a stream. A sample rate travels down a link too, and the
//! Connections section has to leave those out — so it asks each link what it
//! carries rather than assuming.
//!
//! It also carries a hier block — GNU Radio's word for a flowgraph installed
//! into the block tree and used as one block. `Audio Chain` is a group: it
//! appears in the Flow category beside the blocks that came with the example,
//! its sockets are read off the pads inside it, and the generator does not
//! know it exists. One `flatten` puts the interior back and the script comes
//! out exactly as it did before there was a hier block at all.
//!
//! Left out on purpose: the `qtgui` sinks. They are most of what a GRC script
//! looks like and almost none of what it does, and a generated Qt application
//! would say more about Qt than about nodez.

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

#[derive(Clone, Debug, SocketType)]
#[socket(shape = "square", description = "A stream of float samples.")]
struct Stream(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Waveform {
    #[socket(rename = "GR_COS_WAVE")]
    Cosine,
    #[socket(rename = "GR_SIN_WAVE")]
    Sine,
    #[socket(rename = "GR_SQR_WAVE")]
    Square,
    #[socket(rename = "GR_SAW_WAVE")]
    Sawtooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Noise {
    #[socket(rename = "GR_GAUSSIAN")]
    Gaussian,
    #[socket(rename = "GR_UNIFORM")]
    Uniform,
    #[socket(rename = "GR_LAPLACIAN")]
    Laplacian,
}

// ------------------------------------------------------------------ input

#[derive(Debug, NodeType)]
#[node(
    id = "variable",
    label = "Variable",
    category = "Input",
    description = "A value the script declares once and every block can share.",
    keywords = "samp_rate, constant, parameter",
    output = f64,
    output_name = "Value"
)]
struct Variable {
    #[param(default = "samp_rate", hint = "name", hide_label)]
    name: String,
    #[input(label = "", default = 320000.0)]
    value: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "sig_source",
    label = "Signal Source",
    category = "Source",
    description = "A tone.",
    keywords = "tone, oscillator, carrier",
    output = Stream,
    output_name = "out"
)]
struct SignalSource {
    #[param(default = Waveform::Cosine)]
    waveform: Waveform,
    #[input(label = "Sample Rate", default = 320000.0)]
    sample_rate: f64,
    #[input(label = "Frequency", default = 1000.0)]
    frequency: f64,
    #[input(label = "Amplitude", default = 1.0)]
    amplitude: f64,
    #[input(label = "Offset", default = 0.0)]
    offset: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "noise_source",
    label = "Noise Source",
    category = "Source",
    description = "Random samples, for testing a chain under noise.",
    keywords = "random, gaussian, awgn",
    output = Stream,
    output_name = "out"
)]
struct NoiseSource {
    #[param(default = Noise::Gaussian)]
    noise_type: Noise,
    #[input(label = "Amplitude", default = 0.1)]
    amplitude: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "file_source",
    label = "File Source",
    category = "Source",
    description = "Samples read back from disk.",
    keywords = "read, playback, capture",
    output = Stream,
    output_name = "out"
)]
struct FileSource {
    #[input(label = "File", default = "input.dat")]
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
    description = "Holds the graph to real time. One per chain, never more.",
    keywords = "rate, realtime, pace",
    output = Stream,
    output_name = "out"
)]
struct Throttle {
    #[input(label = "in")]
    input: Stream,
    #[input(label = "Sample Rate", default = 320000.0)]
    sample_rate: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "add",
    label = "Add",
    category = "Flow",
    description = "Sums every stream wired into it.",
    keywords = "sum, mix, combine",
    output = Stream,
    output_name = "out"
)]
struct Add {
    #[input(label = "in")]
    inputs: nodez::Multi<Stream>,
}

#[derive(Debug, NodeType)]
#[node(
    id = "multiply",
    label = "Multiply",
    category = "Flow",
    description = "Multiplies every stream wired into it. Mixing, in the radio sense.",
    keywords = "mix, modulate, product",
    output = Stream,
    output_name = "out"
)]
struct Multiply {
    #[input(label = "in")]
    inputs: nodez::Multi<Stream>,
}

#[derive(Debug, NodeType)]
#[node(
    id = "multiply_const",
    label = "Multiply Const",
    category = "Flow",
    description = "Scales a stream by a fixed amount.",
    keywords = "gain, scale, attenuate",
    output = Stream,
    output_name = "out"
)]
struct MultiplyConst {
    #[input(label = "in")]
    input: Stream,
    #[input(label = "Constant", default = 1.0)]
    constant: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "low_pass",
    label = "Low Pass Filter",
    category = "Flow",
    description = "Keeps what is below the cutoff and decimates by the given factor.",
    keywords = "fir, filter, firdes",
    output = Stream,
    output_name = "out"
)]
struct LowPass {
    #[input(label = "in")]
    input: Stream,
    #[input(label = "Sample Rate", default = 320000.0)]
    sample_rate: f64,
    #[input(label = "Cutoff", default = 20000.0)]
    cutoff: f64,
    #[input(label = "Transition", default = 5000.0)]
    transition: f64,
    #[input(label = "Decimation", default = 1, min = 1, max = 1024)]
    decimation: i64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "resampler",
    label = "Rational Resampler",
    category = "Flow",
    description = "Changes the sample rate by a ratio of whole numbers.",
    keywords = "interpolate, decimate, rate",
    output = Stream,
    output_name = "out"
)]
struct Resampler {
    #[input(label = "in")]
    input: Stream,
    #[input(label = "Interpolation", default = 1, min = 1, max = 1024)]
    interpolation: i64,
    #[input(label = "Decimation", default = 1, min = 1, max = 1024)]
    decimation: i64,
}

// ----------------------------------------------------------------- output

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
    #[input(label = "in")]
    input: Stream,
    #[input(label = "File", default = "output.dat")]
    file: String,
}

#[derive(Debug, NodeType)]
#[node(
    id = "audio_sink",
    label = "Audio Sink",
    category = "Sink",
    description = "Sends the stream to the sound card.",
    keywords = "speaker, sound, listen",
    produces = String
)]
struct AudioSink {
    #[input(label = "in")]
    input: Stream,
    #[input(label = "Sample Rate", default = 48000.0)]
    sample_rate: f64,
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
    #[input(label = "in")]
    input: Stream,
}

// ------------------------------------------------------- python generation

/// The Python module each kind of block comes from, and the stem GNU Radio
/// Companion names its instances after.
///
/// GRC calls the first `analog.sig_source_x` in a flowgraph
/// `analog_sig_source_x_0`, and the scripts people have read for years look
/// like that, so these do too.
fn block(template: &str) -> Option<(&'static str, &'static str)> {
    Some(match template {
        "sig_source" => ("analog", "analog_sig_source_x"),
        "noise_source" => ("analog", "analog_noise_source_x"),
        "file_source" => ("blocks", "blocks_file_source"),
        "throttle" => ("blocks", "blocks_throttle"),
        "add" => ("blocks", "blocks_add_xx"),
        "multiply" => ("blocks", "blocks_multiply_xx"),
        "multiply_const" => ("blocks", "blocks_multiply_const_xx"),
        "low_pass" => ("filter", "low_pass_filter"),
        "resampler" => ("filter", "rational_resampler_xxx"),
        "file_sink" => ("blocks", "blocks_file_sink"),
        "audio_sink" => ("audio", "audio_sink"),
        "null_sink" => ("blocks", "blocks_null_sink"),
        // A Variable is not a block; it is a line in the Variables section.
        _ => return None,
    })
}

/// Everything the generator needs to name and read one graph.
struct Script<'a> {
    graph: &'a Graph,
    library: &'a NodeLibrary,
    /// The Python name of each block, and of each variable.
    names: HashMap<NodeId, String>,
    problems: Vec<String>,
}

impl<'a> Script<'a> {
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
                    .and_then(|v| v.as_str().map(python_name))
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

    /// The nodes, upstream first, so the script reads the way the samples
    /// flow. A block cannot be made before the variable it is measured in.
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

    /// One of a node's inputs as Python: the name of the variable driving it
    /// if one is, and otherwise the value typed into it.
    ///
    /// This is the whole of what a GRC variable is. Nothing declares that
    /// `samp_rate` is a sample rate; a link says which blocks share it.
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
            .unwrap_or_else(|| "0".to_owned())
    }

    fn param(&self, node: &Node, name: &str) -> String {
        node.param(name)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The Python that makes one block.
    fn construct(&self, node: &Node, template: &NodeTemplate) -> Option<String> {
        let rate = |socket| self.input(node, socket);
        let value = |socket| self.input(node, socket);
        Some(match template.id.as_str() {
            "sig_source" => format!(
                "analog.sig_source_f({}, analog.{}, {}, {}, {}, 0)",
                rate("sample_rate"),
                self.param(node, "waveform"),
                value("frequency"),
                value("amplitude"),
                value("offset"),
            ),
            "noise_source" => format!(
                "analog.noise_source_f(analog.{}, {}, 0)",
                self.param(node, "noise_type"),
                value("amplitude"),
            ),
            "file_source" => format!(
                "blocks.file_source(gr.sizeof_float * 1, {}, {}, 0, 0)",
                value("file"),
                value("repeat"),
            ),
            "throttle" => format!(
                "blocks.throttle(gr.sizeof_float * 1, {}, True)",
                rate("sample_rate"),
            ),
            "add" => "blocks.add_vff(1)".to_owned(),
            "multiply" => "blocks.multiply_vff(1)".to_owned(),
            "multiply_const" => {
                format!("blocks.multiply_const_ff({})", value("constant"))
            }
            "low_pass" => format!(
                "filter.fir_filter_fff({}, firdes.low_pass(\n            \
                 1, {}, {}, {}, window.WIN_HAMMING, 6.76))",
                value("decimation"),
                rate("sample_rate"),
                value("cutoff"),
                value("transition"),
            ),
            "resampler" => format!(
                "filter.rational_resampler_fff(\n            \
                 interpolation={}, decimation={}, taps=[], fractional_bw=0.0)",
                value("interpolation"),
                value("decimation"),
            ),
            "file_sink" => format!(
                "blocks.file_sink(gr.sizeof_float * 1, {}, False)",
                value("file"),
            ),
            "audio_sink" => format!("audio.sink(int({}), \"\", True)", rate("sample_rate")),
            "null_sink" => "blocks.null_sink(gr.sizeof_float * 1)".to_owned(),
            _ => return None,
        })
    }

    /// Whether a link carries samples. A sample rate travels down a link too,
    /// and `connect` is only for the ones that carry a stream.
    fn is_stream(&self, to: NodeId, socket: &str) -> bool {
        let Some(stream) = self.library.types.id("Stream") else {
            return false;
        };
        self.template(to)
            .and_then(|t| t.input_spec(socket))
            .is_some_and(|spec| spec.ty == stream)
    }
}

/// Python for one value.
fn literal(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
        Value::Int(i) => format!("{i}"),
        Value::Float(f) => format!("{f:?}"),
        other => format!("{:?}", other.as_str().unwrap_or_default()),
    }
}

/// A name Python will accept.
fn python_name(name: &str) -> String {
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

fn generate(graph: &Graph, library: &NodeLibrary) -> Preview {
    // A hier block is a flat flowgraph once it runs, so it is one here too.
    // This is the whole of what the generator has to know about groups: put
    // the interiors back and carry on as before.
    let graph = &graph.flatten(library);
    let script = Script::new(graph, library);
    let mut problems = script.problems.clone();
    let order = Script::ordered(graph, &mut Vec::new());

    let mut out = String::new();
    out.push_str("#!/usr/bin/env python3\n# Generated by nodez.\n\n");
    out.push_str("from gnuradio import analog\n");
    out.push_str("from gnuradio import audio\n");
    out.push_str("from gnuradio import blocks\n");
    out.push_str("from gnuradio import filter\n");
    out.push_str("from gnuradio import gr\n");
    out.push_str("from gnuradio.fft import window\n");
    out.push_str("from gnuradio.filter import firdes\n\n\n");
    out.push_str("class flowgraph(gr.top_block):\n");
    out.push_str("    def __init__(self):\n");
    out.push_str("        gr.top_block.__init__(self, \"Flowgraph\")\n\n");

    out.push_str("        # Variables\n");
    let mut variables = 0;
    for id in &order {
        let (Some(node), Some(template), Some(name)) =
            (graph.node(*id), script.template(*id), script.names.get(id))
        else {
            continue;
        };
        if block(&template.id).is_some() {
            continue;
        }
        let _ = writeln!(
            out,
            "        self.{name} = {name} = {}",
            script.input(node, "value")
        );
        variables += 1;
    }
    if variables == 0 {
        out.push_str("        pass\n");
    }

    out.push_str("\n        # Blocks\n");
    let mut blocks = 0;
    for id in &order {
        let (Some(node), Some(template), Some(name)) =
            (graph.node(*id), script.template(*id), script.names.get(id))
        else {
            continue;
        };
        let Some(made) = script.construct(node, template) else {
            continue;
        };
        let _ = writeln!(out, "        self.{name} = {made}");
        blocks += 1;
    }
    if blocks == 0 {
        out.push_str("        pass\n");
        problems.push("no blocks, so the flowgraph does nothing".to_owned());
    }

    out.push_str("\n        # Connections\n");
    let mut wired = 0;
    for connection in graph.connections() {
        if !script.is_stream(connection.to.node, &connection.to.socket) {
            continue;
        }
        let (Some(from), Some(to)) = (
            script.names.get(&connection.from.node),
            script.names.get(&connection.to.node),
        ) else {
            continue;
        };
        // A fan-in socket's links land on numbered ports, in the order the
        // editor shows them; everything else is port zero.
        let _ = writeln!(
            out,
            "        self.connect((self.{from}, 0), (self.{to}, {}))",
            connection.order
        );
        wired += 1;
    }
    if wired == 0 {
        out.push_str("        pass\n");
    }

    out.push_str("\n\ndef main(top_block_cls=flowgraph):\n");
    out.push_str("    tb = top_block_cls()\n");
    out.push_str("    tb.start()\n");
    out.push_str("    try:\n");
    out.push_str("        input(\"Press Enter to quit: \")\n");
    out.push_str("    except EOFError:\n");
    out.push_str("        pass\n");
    out.push_str("    tb.stop()\n");
    out.push_str("    tb.wait()\n\n\n");
    out.push_str("if __name__ == \"__main__\":\n    main()\n");

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
    register::<NoiseSource>(&mut library);
    register::<FileSource>(&mut library);
    register::<Throttle>(&mut library);
    register::<Add>(&mut library);
    register::<Multiply>(&mut library);
    register::<MultiplyConst>(&mut library);
    register::<LowPass>(&mut library);
    register::<Resampler>(&mut library);
    register::<FileSink>(&mut library);
    register::<AudioSink>(&mut library);
    register::<NullSink>(&mut library);

    // A hier block, which is GNU Radio's word for a flowgraph installed into
    // the block tree and used as one block. Built here so the example stands
    // alone; in a real setup this is a `.grc` file read off disk, which is
    // the whole reason a group lives in the library rather than in the
    // document — one definition, every flowgraph that loads the library.
    nodez::group::register_pads(&mut library);
    let inside = audio_chain(&library);
    library
        .register_group("audio_chain", "Audio Chain", "Flow", inside)
        .expect("the hier block is well formed");

    let graph = starting_graph(&library);

    // `--print` writes the script for the starting graph and exits, so the
    // generator can be run without a display.
    if std::env::args().any(|arg| arg == "--print") {
        let script = generate(&graph, &library);
        for problem in &script.problems {
            eprintln!("warning: {problem}");
        }
        print!("{}", script.text);
        return Ok(());
    }

    EditorApp::new(library)
        .graph(graph)
        .title("nodez \u{2014} GNU Radio flowgraph")
        .file("flowgraph.json")
        .preview_extension("py")
        .json_files()
        .preview(generate)
        .run()
}

/// The inside of the Audio Chain hier block: take the sample rate and a
/// stream, filter it, scale it and resample it for the sound card.
///
/// The pads are what become its sockets. Their order down the canvas is the
/// order the sockets come out in, and their type is whatever they are wired
/// to in here — so nothing about the interface is written down twice.
fn audio_chain(library: &NodeLibrary) -> Graph {
    let mut graph = Graph::new();
    let add = |graph: &mut Graph, id: &str, y: f32| {
        let template = library
            .id(id)
            .unwrap_or_else(|| panic!("`{id}` is registered"));
        graph.add_node(library, template, egui::pos2(0.0, y))
    };
    use nodez::group::{INPUT_PAD, OUTPUT_PAD, PAD_NAME};

    let stream_in = add(&mut graph, INPUT_PAD, 0.0);
    let rate_in = add(&mut graph, INPUT_PAD, 120.0);
    let lowpass = add(&mut graph, "low_pass", 0.0);
    let gain = add(&mut graph, "multiply_const", 0.0);
    let resample = add(&mut graph, "resampler", 0.0);
    let stream_out = add(&mut graph, OUTPUT_PAD, 0.0);

    for (node, name) in [
        (stream_in, "in"),
        (rate_in, "Sample Rate"),
        (stream_out, "out"),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_param(PAD_NAME, Value::from(name));
    }
    for (node, socket, value) in [
        (lowpass, "cutoff", Value::Float(15_000.0)),
        (lowpass, "transition", Value::Float(4_000.0)),
        (gain, "constant", Value::Float(0.5)),
        (resample, "decimation", Value::Int(4)),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_input_value(socket, value);
    }

    for (from, from_socket, to, to_socket) in [
        (stream_in, "out", lowpass, "input"),
        (rate_in, "out", lowpass, "sample_rate"),
        (lowpass, "out", gain, "input"),
        (gain, "out", resample, "input"),
        (resample, "out", stream_out, "in"),
    ] {
        graph
            .connect(library, (from, from_socket), (to, to_socket))
            .unwrap_or_else(|e| panic!("wiring {to_socket}: {e}"));
    }

    let _ = nodez::layered(&mut graph, &nodez::LayoutOptions::default(), |graph, node| {
        nodez::node_size(graph, library, node, &nodez::EditorStyle::default())
    });
    graph
}

/// A tone and some noise, mixed up to a carrier, filtered back down, and sent
/// to both a file and the sound card.
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
    let tone = add_node(&mut graph, "sig_source");
    let hiss = add_node(&mut graph, "noise_source");
    let sum = add_node(&mut graph, "add");
    let throttle = add_node(&mut graph, "throttle");
    let carrier = add_node(&mut graph, "sig_source");
    let mixer = add_node(&mut graph, "multiply");
    // The hier block, standing in for the filter, the gain and the resampler.
    let chain = add_node(&mut graph, "audio_chain");
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
        (hiss, "amplitude", Value::Float(0.05)),
        (carrier, "frequency", Value::Float(40_000.0)),
        (to_file, "file", Value::from("mixed.dat")),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_input_value(socket, value);
    }

    // Streams, then the sample rates the blocks share.
    for (from, to, socket) in [
        (tone, sum, "inputs"),
        (hiss, sum, "inputs"),
        (sum, throttle, "input"),
        (throttle, mixer, "inputs"),
        (carrier, mixer, "inputs"),
        (mixer, chain, "in"),
        (chain, to_file, "input"),
        (chain, to_audio, "input"),
        (samp_rate, tone, "sample_rate"),
        (samp_rate, carrier, "sample_rate"),
        (samp_rate, throttle, "sample_rate"),
        (samp_rate, chain, "Sample Rate"),
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
