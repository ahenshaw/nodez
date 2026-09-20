//! A Blender shader tree, and the Python that rebuilds it.
//!
//!     cargo run -p nodez --features derive,app --example blender_nodes
//!
//! The node kinds are a slice of Blender's shader nodes — enough to build a
//! procedural material — and the preview pane is a `bpy` script of the kind
//! Blender's own Node-to-Python addons produce: make the nodes, set the values
//! that are not driven by a link, wire them up.
//!
//! Where [`quickstart`](../quickstart.rs) folds a graph into a value, this
//! walks the graph itself. That is the difference between a graph that
//! *computes* something and one that *describes* something: a shader tree is
//! the artifact, so what the generator wants is every node and every link, not
//! what they add up to. Nothing here implements `Evaluate`, and the templates
//! are registered straight from [`NodeType::template`].
//!
//! Node positions are carried across, so the arrangement made here is the
//! arrangement Blender opens with. Blender's y axis points up, so it is
//! negated on the way out.

// The structs below are schema, not storage. `#[derive(NodeType)]` reads their
// fields to build the templates and nothing here ever constructs one, because
// the generator works from the graph rather than from folded values — so every
// field is, strictly, never read.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;

use nodez::app::{EditorApp, Preview};
use nodez::{Graph, Node, NodeLibrary, NodeTemplate, NodeType, SocketType, Value};

// ------------------------------------------------------------- wire types
//
// Blender's own: a shader, a color, a vector. Floats travel as `f64`, which is
// a wire type already, so a Fac output plugs into a Roughness input without
// anything being declared for it.

#[derive(Clone, Debug, SocketType)]
#[socket(description = "A surface shader.")]
struct Shader(String);

#[derive(Clone, Debug, SocketType)]
#[socket(shape = "diamond", description = "An RGB color.")]
struct Color(String);

#[derive(Clone, Debug, SocketType)]
#[socket(shape = "diamond", description = "A coordinate, direction or normal.")]
struct Vector(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Coordinate {
    Generated,
    Normal,
    #[socket(rename = "UV")]
    Uv,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Interpolation {
    #[socket(rename = "LINEAR")]
    Linear,
    #[socket(rename = "EASE")]
    Ease,
    #[socket(rename = "CONSTANT")]
    Constant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(widget = choice)]
enum Blend {
    #[socket(rename = "MIX")]
    Mix,
    #[socket(rename = "MULTIPLY")]
    Multiply,
    #[socket(rename = "SCREEN")]
    Screen,
    #[socket(rename = "OVERLAY")]
    Overlay,
}

// ------------------------------------------------------------------ input

#[derive(Debug, NodeType)]
#[node(
    id = "tex_coord",
    label = "Texture Coordinate",
    category = "Input",
    description = "Where on the surface a texture is read.",
    keywords = "uv, generated, object",
    output = Vector,
    output_name = "Vector"
)]
struct TexCoord {
    /// Which of the node's outputs the wire is taken from. Blender draws them
    /// all at once; here the choice is a parameter and the generator reads it.
    #[param(default = Coordinate::Uv, hide_label)]
    source: Coordinate,
}

#[derive(Debug, NodeType)]
#[node(
    id = "value",
    label = "Value",
    category = "Input",
    description = "A single number, for driving several inputs at once.",
    keywords = "float, constant",
    output = f64,
    output_name = "Value"
)]
struct FloatValue {
    #[input(label = "", default = 0.5)]
    value: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "rgb",
    label = "RGB",
    category = "Input",
    description = "A fixed color.",
    keywords = "color, constant",
    output = Color,
    output_name = "Color"
)]
struct Rgb {
    #[input(default = 0.8, min = 0.0, max = 1.0)]
    r: f64,
    #[input(default = 0.5, min = 0.0, max = 1.0)]
    g: f64,
    #[input(default = 0.3, min = 0.0, max = 1.0)]
    b: f64,
}

// ---------------------------------------------------------------- texture

#[derive(Debug, NodeType)]
#[node(
    category = "Texture",
    description = "Moves, turns and scales the coordinates a texture is read at.",
    keywords = "transform, scale, uv",
    output = Vector,
    output_name = "Vector"
)]
struct Mapping {
    #[input]
    vector: Vector,
    #[input(default = 1.0, min = 0.0, max = 1000.0)]
    scale: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "noise",
    label = "Noise Texture",
    category = "Texture",
    description = "Fractal noise. The workhorse of a procedural material.",
    keywords = "perlin, fractal, fbm",
    output = f64,
    output_name = "Fac"
)]
struct Noise {
    #[input]
    vector: Vector,
    #[input(default = 5.0, min = 0.0, max = 1000.0)]
    scale: f64,
    #[input(default = 2.0, min = 0.0, max = 15.0)]
    detail: f64,
    #[input(default = 0.5, min = 0.0, max = 1.0)]
    roughness: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "wave",
    label = "Wave Texture",
    category = "Texture",
    description = "Bands or rings, distorted by noise. Wood and marble.",
    keywords = "bands, rings, wood",
    output = f64,
    output_name = "Fac"
)]
struct Wave {
    #[input]
    vector: Vector,
    #[input(default = 5.0, min = 0.0, max = 1000.0)]
    scale: f64,
    #[input(default = 2.0, min = 0.0, max = 100.0)]
    distortion: f64,
}

// ---------------------------------------------------------------- convert

#[derive(Debug, NodeType)]
#[node(
    id = "ramp",
    label = "Color Ramp",
    category = "Convert",
    description = "Turns a single value into a color, through a gradient.",
    keywords = "gradient, remap, valtorgb",
    output = Color,
    output_name = "Color"
)]
struct Ramp {
    #[param(default = Interpolation::Linear)]
    interpolation: Interpolation,
    #[input(label = "Fac", default = 0.5, min = 0.0, max = 1.0)]
    fac: f64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "mix",
    label = "Mix Color",
    category = "Convert",
    description = "Blends two colors.",
    keywords = "blend, multiply, overlay",
    output = Color,
    output_name = "Color"
)]
struct Mix {
    #[param(default = Blend::Mix)]
    blend_type: Blend,
    #[input(label = "Fac", default = 0.5, min = 0.0, max = 1.0)]
    fac: f64,
    #[input(label = "Color1")]
    color1: Color,
    #[input(label = "Color2")]
    color2: Color,
}

#[derive(Debug, NodeType)]
#[node(
    category = "Convert",
    description = "Fakes surface detail from a height value.",
    keywords = "normal, height, detail",
    output = Vector,
    output_name = "Normal"
)]
struct Bump {
    #[input(default = 1.0, min = 0.0, max = 10.0)]
    strength: f64,
    #[input]
    height: f64,
}

// ----------------------------------------------------------------- shader

#[derive(Debug, NodeType)]
#[node(
    id = "principled",
    label = "Principled BSDF",
    category = "Shader",
    description = "Blender's general-purpose surface shader.",
    keywords = "bsdf, surface, pbr",
    output = Shader,
    output_name = "BSDF"
)]
struct Principled {
    #[input(label = "Base Color")]
    base_color: Color,
    #[input(default = 0.0, min = 0.0, max = 1.0)]
    metallic: f64,
    #[input(default = 0.5, min = 0.0, max = 1.0)]
    roughness: f64,
    #[input]
    normal: Option<Vector>,
}

#[derive(Debug, NodeType)]
#[node(
    id = "output",
    label = "Material Output",
    category = "Output",
    description = "What the material actually renders.",
    keywords = "surface, material, root",
    produces = String
)]
struct MaterialOutput {
    #[param(default = "Procedural Rock", hint = "material name", hide_label)]
    name: String,
    #[input]
    surface: Shader,
}

// ------------------------------------------------------- python generation

/// What each kind of node is called in Blender.
///
/// The one thing about these nodes that cannot be read off the graph: the
/// sockets carry their own Blender names already, because the fields are
/// spelled the way Blender spells them.
fn bpy_type(template: &str) -> &'static str {
    match template {
        "tex_coord" => "ShaderNodeTexCoord",
        "value" => "ShaderNodeValue",
        "rgb" => "ShaderNodeRGB",
        "mapping" => "ShaderNodeMapping",
        "noise" => "ShaderNodeTexNoise",
        "wave" => "ShaderNodeTexWave",
        "ramp" => "ShaderNodeValToRGB",
        "mix" => "ShaderNodeMixRGB",
        "bump" => "ShaderNodeBump",
        "principled" => "ShaderNodeBsdfPrincipled",
        "output" => "ShaderNodeOutputMaterial",
        _ => "ShaderNodeValue",
    }
}

/// The Blender property one of this editor's parameters sets, where it sets
/// one at all.
///
/// Some parameters here steer the generator rather than the node: which of a
/// Texture Coordinate's outputs to wire from, what to call the material.
/// Those describe the graph, not the shader, and writing them out would be
/// writing Python that sets attributes Blender does not have.
fn property(template: &str, param: &str) -> Option<&'static str> {
    match (template, param) {
        // The ramp itself is a sub-object of the node.
        ("ramp", "interpolation") => Some("color_ramp.interpolation"),
        ("mix", "blend_type") => Some("blend_type"),
        _ => None,
    }
}

/// Python for one value.
fn literal(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
        Value::Float(f) => format!("{f:?}"),
        Value::Int(i) => format!("{i}.0"),
        other => format!("{:?}", other.as_str().unwrap_or_default()),
    }
}

/// The script that rebuilds this graph in Blender.
fn generate(graph: &Graph, library: &NodeLibrary) -> Preview {
    let mut problems = Vec::new();

    // A Python name per node, kept unique the way Blender keeps its own: the
    // kind, then a number once there is more than one of it.
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut names: HashMap<nodez::NodeId, String> = HashMap::new();
    let ordered: Vec<&Node> = match graph.topological_order() {
        // Upstream first, so the script reads the way the signal flows.
        Ok(order) => order.iter().filter_map(|id| graph.node(*id)).collect(),
        Err(cycle) => {
            problems.push(cycle.to_string());
            graph.nodes().collect()
        }
    };
    for node in &ordered {
        let Some(template) = graph.template_of(library, node.id) else {
            continue;
        };
        let seen = used.entry(template.id.clone()).or_default();
        let name = if *seen == 0 {
            template.id.clone()
        } else {
            format!("{}_{}", template.id, seen)
        };
        *seen += 1;
        names.insert(node.id, name);
    }

    let material = ordered
        .iter()
        .find(|node| {
            graph
                .template_of(library, node.id)
                .is_some_and(|t| t.id == "output")
        })
        .and_then(|node| node.param("name"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            problems.push("no Material Output node, so the material has no name".to_owned());
            "Material".to_owned()
        });

    let mut out = String::new();
    out.push_str("# Generated by nodez.\nimport bpy\n\n\n");
    let _ = writeln!(out, "def build(name={material:?}):");
    out.push_str("    material = bpy.data.materials.new(name)\n");
    out.push_str("    material.use_nodes = True\n");
    out.push_str("    tree = material.node_tree\n");
    out.push_str("    tree.nodes.clear()\n\n");

    for node in &ordered {
        let (Some(template), Some(name)) = (
            graph.template_of(library, node.id),
            names.get(&node.id),
        ) else {
            continue;
        };
        let _ = writeln!(
            out,
            "    {name} = tree.nodes.new({:?})",
            bpy_type(&template.id)
        );
        // Blender's y axis points up and the editor's points down. Written
        // as `0.0 - y` rather than `-y` so a node at the origin does not come
        // out at minus zero.
        let _ = writeln!(
            out,
            "    {name}.location = ({:?}, {:?})",
            node.position.x,
            0.0 - node.position.y
        );
        for line in &settings(graph, node, template) {
            let _ = writeln!(out, "    {name}.{line}");
        }
        out.push('\n');
    }

    out.push_str("    link = tree.links.new\n");
    for connection in graph.connections() {
        let (Some(from), Some(to)) = (
            names.get(&connection.from.node),
            names.get(&connection.to.node),
        ) else {
            continue;
        };
        let output = graph
            .node(connection.from.node)
            .and_then(|node| output_socket(graph, library, node))
            .unwrap_or_else(|| connection.from.socket.clone());
        let input = graph
            .template_of(library, connection.to.node)
            .and_then(|t| t.input_spec(&connection.to.socket))
            .map_or(connection.to.socket.clone(), |s| s.display().to_owned());
        let _ = writeln!(
            out,
            "    link({from}.outputs[{output:?}], {to}.inputs[{input:?}])"
        );
    }

    out.push_str("    return material\n\n\n");
    out.push_str("if __name__ == \"__main__\":\n    build()\n");

    Preview::text(out).problems(problems)
}

/// Which output socket a node's wires leave from.
///
/// Every node here has one, except Texture Coordinate, which has a parameter
/// saying which of Blender's it means.
fn output_socket(graph: &Graph, library: &NodeLibrary, node: &Node) -> Option<String> {
    let template = graph.template_of(library, node.id)?;
    if template.id == "tex_coord" {
        return node
            .param("source")
            .and_then(|v| v.as_str().map(str::to_owned));
    }
    // The socket's name, not its display: the derive labels an output with
    // its wire type, which is what the editor draws beside it, and names it
    // with what `output_name` said — which is what Blender calls it.
    template
        .outputs
        .first()
        .map(|socket| socket.name.clone())
}

/// The lines that set up one node beyond making it.
///
/// An input with a link takes its value from the link, so only the ones left
/// alone are written out. A handful of nodes keep their value somewhere other
/// than an input socket, and those are spelled out here — which is the same
/// handful any Node-to-Python script has to special-case.
fn settings(graph: &Graph, node: &Node, template: &NodeTemplate) -> Vec<String> {
    let number = |socket: &str| {
        node.input_value(socket)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    match template.id.as_str() {
        // No input sockets at all: the value lives on the output.
        "value" => {
            return vec![format!(
                "outputs[\"Value\"].default_value = {:?}",
                number("value")
            )];
        }
        "rgb" => {
            return vec![format!(
                "outputs[\"Color\"].default_value = ({:?}, {:?}, {:?}, 1.0)",
                number("r"),
                number("g"),
                number("b")
            )];
        }
        _ => {}
    }

    let mut lines = Vec::new();
    for param in &template.params {
        let (Some(property), Some(value)) = (
            property(&template.id, &param.name),
            node.param(&param.name),
        ) else {
            continue;
        };
        lines.push(format!("{property} = {}", literal(&value)));
    }
    for socket in &template.inputs {
        if graph.is_input_linked(node.id, &socket.name) {
            continue;
        }
        let Some(value) = node.input_value(&socket.name) else {
            continue;
        };
        // Blender's Mapping node scales on three axes; the editor offers one
        // number for all three, which is what anybody types into it anyway.
        let literal = if template.id == "mapping" && socket.name == "scale" {
            let scale = value.as_f64().unwrap_or(1.0);
            format!("({scale:?}, {scale:?}, {scale:?})")
        } else {
            literal(&value)
        };
        lines.push(format!(
            "inputs[{:?}].default_value = {literal}",
            socket.display()
        ));
    }
    lines
}

// ------------------------------------------------------------------- setup

fn register<K: NodeType>(library: &mut NodeLibrary) {
    let template = K::template(&mut library.types);
    library.register(template);
}

fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    register::<TexCoord>(&mut library);
    register::<FloatValue>(&mut library);
    register::<Rgb>(&mut library);
    register::<Mapping>(&mut library);
    register::<Noise>(&mut library);
    register::<Wave>(&mut library);
    register::<Ramp>(&mut library);
    register::<Mix>(&mut library);
    register::<Bump>(&mut library);
    register::<Principled>(&mut library);
    register::<MaterialOutput>(&mut library);

    // Blender lets a single number stand in for a color, and so does this.
    if let (Some(number), Some(color)) = (library.types.id("Float"), library.types.id("Color")) {
        library.types.allow_cast(number, color);
    }

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
        .title("nodez \u{2014} Blender shader nodes")
        .file("shader-graph.json")
        .preview_extension("py")
        .json_files()
        .preview(generate)
        .run()
}

/// A procedural material to open with: noise for the color, the same noise
/// for the bumps, and a wave texture mixed over the top.
fn starting_graph(library: &NodeLibrary) -> Graph {
    let mut graph = Graph::new();
    let add = |graph: &mut Graph, id: &str| {
        let template = library.id(id).unwrap_or_else(|| panic!("`{id}` is registered"));
        graph.add_node(library, template, egui::pos2(0.0, 0.0))
    };

    let coords = add(&mut graph, "tex_coord");
    let mapping = add(&mut graph, "mapping");
    let noise = add(&mut graph, "noise");
    let wave = add(&mut graph, "wave");
    let ramp = add(&mut graph, "ramp");
    let tint = add(&mut graph, "rgb");
    let mix = add(&mut graph, "mix");
    let bump = add(&mut graph, "bump");
    let roughness = add(&mut graph, "value");
    let surface = add(&mut graph, "principled");
    let output = add(&mut graph, "output");

    for (node, socket, value) in [
        (mapping, "scale", Value::Float(4.0)),
        (noise, "scale", Value::Float(12.0)),
        (noise, "detail", Value::Float(6.0)),
        (noise, "roughness", Value::Float(0.7)),
        (wave, "scale", Value::Float(3.0)),
        (wave, "distortion", Value::Float(8.0)),
        (bump, "strength", Value::Float(0.4)),
        (roughness, "value", Value::Float(0.65)),
        (tint, "r", Value::Float(0.42)),
        (tint, "g", Value::Float(0.38)),
        (tint, "b", Value::Float(0.35)),
    ] {
        graph
            .node_mut(node)
            .expect("just added")
            .set_input_value(socket, value);
    }
    graph
        .node_mut(mix)
        .expect("just added")
        .set_param("blend_type", Value::Choice("OVERLAY".to_owned()));

    // Output sockets carry their Blender names — "Fac", "BSDF", "Color" —
    // so a wire is made by asking the template what its output is called
    // rather than by assuming.
    let output_of = |graph: &Graph, id| {
        graph
            .template_of(library, id)
            .and_then(|template| template.outputs.first())
            .map(|socket| socket.name.clone())
            .expect("every node wired from here has an output")
    };

    for (from, to, socket) in [
        (coords, mapping, "vector"),
        (mapping, noise, "vector"),
        (mapping, wave, "vector"),
        (noise, ramp, "fac"),
        (ramp, mix, "color1"),
        (tint, mix, "color2"),
        (wave, mix, "fac"),
        (mix, surface, "base_color"),
        (roughness, surface, "roughness"),
        (noise, bump, "height"),
        (bump, surface, "normal"),
        (surface, output, "surface"),
    ] {
        let out = output_of(&graph, from);
        graph
            .connect(library, (from, out.as_str()), (to, socket))
            .unwrap_or_else(|e| panic!("wiring {socket}: {e}"));
    }

    let _ = nodez::layered(&mut graph, &nodez::LayoutOptions::default(), |graph, node| {
        nodez::node_size(graph, library, node, &nodez::EditorStyle::default())
    });
    graph
}
