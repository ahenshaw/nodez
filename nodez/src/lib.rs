//! # nodez
//!
//! A Blender-style node editor for [`egui`], plus the typed graph model behind
//! it.
//!
//! The crate has two halves that can be used independently:
//!
//! * **The model** — [`NodeLibrary`] describes the node types your domain has,
//!   [`Graph`] holds an instance of them, and the traversal API
//!   ([`Graph::topological_order`], [`Graph::ancestors`], [`Graph::evaluate`], …)
//!   turns that graph into whatever you need. None of it depends on the editor
//!   running, so the same graph can be loaded and rendered headlessly.
//! * **The editor** — [`NodeEditor`] draws the graph the way Blender's node
//!   editor does: color-coded sockets, bezier noodles that fade between the two
//!   socket colors, box select, `Shift+A` to add, `G` to grab, `X` to delete.
//!
//! Sockets are typed. A [`TypeRegistry`] assigns each type a color and a
//! shape, and [`TypeRegistry::allow_cast`] declares which types may implicitly
//! feed which others. The editor refuses incompatible drags, and
//! [`Graph::connect`] refuses them in code too, so a graph on disk is always
//! well-typed.
//!
//! ## Nodes as Rust types
//!
//! The usual way in, with the `derive` feature: each kind of node is a struct,
//! a field with `#[input]` is a socket, and the field's type picks its color
//! and widget. A [`Fold`] names one way of walking the graph, and
//! [`Evaluate`] says what each kind contributes to it.
//!
//! ```
//! # #[cfg(feature = "derive")] fn main() {
//! use nodez::{Evaluate, Fold, Graph, NodeError, NodeLibrary, NodeType, Payload, Rules};
//!
//! #[derive(Debug, NodeType)]
//! #[node(category = "Input", output = String)]
//! struct Text {
//!     #[input]
//!     value: String,
//! }
//!
//! #[derive(Debug, NodeType)]
//! #[node(category = "Convert", output = String)]
//! struct Uppercase {
//!     #[input]
//!     text: String,
//! }
//!
//! struct Strings;
//! impl Fold for Strings {}
//!
//! impl Evaluate<Strings> for Text {
//!     fn evaluate(&self) -> Result<String, NodeError> { Ok(self.value.clone()) }
//! }
//! impl Evaluate<Strings> for Uppercase {
//!     fn evaluate(&self) -> Result<String, NodeError> { Ok(self.text.to_uppercase()) }
//! }
//!
//! let mut library = NodeLibrary::new();
//! let mut rules = Rules::<Strings>::new();
//! rules.register_all::<(Text, Uppercase)>(&mut library);
//!
//! let mut graph = Graph::new();
//! let a = graph.add_node(&library, library.id("text").unwrap(), egui::pos2(0.0, 0.0));
//! let b = graph.add_node(&library, library.id("uppercase").unwrap(), egui::pos2(220.0, 0.0));
//! graph.node_mut(a).unwrap().set_input_value("value", "hello");
//! graph.connect(&library, (a, "out"), (b, "text")).unwrap();
//!
//! let out = graph
//!     .evaluate::<Payload, NodeError>(&library, b, |ctx| rules.run(&ctx))
//!     .unwrap();
//! assert_eq!(*out.downcast::<String>().unwrap(), "HELLO");
//! # }
//! # #[cfg(not(feature = "derive"))] fn main() {}
//! ```
//!
//! ## Showing the editor
//!
//! ```no_run
//! # use nodez::{Graph, NodeEditor, NodeLibrary};
//! # struct App { editor: NodeEditor, library: NodeLibrary, graph: Graph }
//! # impl App {
//! fn ui(&mut self, ui: &mut egui::Ui) {
//!     let response = self.editor.show(ui, &self.library, &mut self.graph);
//!     if response.changed {
//!         // regenerate your config file
//!     }
//! }
//! # }
//! ```
//!
//! ## A minimal library, by hand
//!
//! The same two nodes without the derive. This is the dynamic model the
//! derive expands to, and what to reach for when node kinds are only known at
//! run time.
//!
//! ```
//! use nodez::{Graph, NodeLibrary, NodeTemplate, SocketSpec, Widget};
//! use egui::Color32;
//!
//! let mut library = NodeLibrary::new();
//! let text = library.types.add("Text", Color32::from_rgb(0xA1, 0xA1, 0xA1));
//! let number = library.types.add("Number", Color32::from_rgb(0x63, 0x63, 0x63));
//! // A number may be dropped into a text socket; the reverse is not allowed.
//! library.types.allow_cast(number, text);
//!
//! let literal = library.register(
//!     NodeTemplate::new("text", "Text")
//!         .category("Input")
//!         .input(SocketSpec::new("value", text).editable(Widget::text()))
//!         .output(SocketSpec::new("out", text)),
//! );
//! let upper = library.register(
//!     NodeTemplate::new("upper", "Uppercase")
//!         .category("Convert")
//!         .input(SocketSpec::new("text", text))
//!         .output(SocketSpec::new("out", text)),
//! );
//!
//! let mut graph = Graph::new();
//! let a = graph.add_node(&library, literal, egui::pos2(0.0, 0.0));
//! let b = graph.add_node(&library, upper, egui::pos2(220.0, 0.0));
//! graph.node_mut(a).unwrap().set_input_value("value", "hello");
//! graph.connect(&library, (a, "out"), (b, "text")).unwrap();
//!
//! // Fold the graph into strings, in dependency order.
//! let out = graph
//!     .evaluate::<String, std::convert::Infallible>(&library, b, |ctx| {
//!         Ok(match ctx.template().id.as_str() {
//!             "text" => ctx.literal_str("value").unwrap_or_default().into_owned(),
//!             "upper" => ctx.input("text").map(|l| l.value.to_uppercase()).unwrap_or_default(),
//!             _ => String::new(),
//!         })
//!     })
//!     .unwrap();
//! assert_eq!(out, "HELLO");
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

// The egui (and eframe) this crate was built against, so an app can use them
// through `nodez::egui` rather than adding its own copy and risking a second,
// incompatible version when egui moves on.
pub use egui;
#[cfg(feature = "app")]
pub use eframe;

#[cfg(feature = "app")]
pub mod app;
pub mod graph;
pub mod group;
pub mod layout;
pub mod template;
pub mod traversal;
#[cfg(feature = "derive")]
pub mod typed;
pub mod types;
pub mod ui;
pub mod value;

/// Items the derive macros expand to. Not part of the hand-written API.
#[doc(hidden)]
#[cfg(feature = "derive")]
pub mod __macro_support {
    /// `Color32::from_rgb` as a plain function, so the macro need not import it.
    pub const fn color(r: u8, g: u8, b: u8) -> egui::Color32 {
        egui::Color32::from_rgb(r, g, b)
    }
}

// The derive macro and the trait it implements deliberately share a name, the
// way `serde::Serialize` does: one `use nodez::NodeType` brings in both.
#[cfg(feature = "derive")]
pub use nodez_derive::{NodeType, SocketType};
#[cfg(feature = "derive")]
pub use typed::{
    Evaluate, Fold, Multi, NodeError, NodeSet, NodeType, Payload, Rules, SocketType,
};

pub use graph::{
    Connection, ConnectError, ConnectionId, CycleError, DynNode, Graph, Node, NodeData, NodeId,
    Repairs, SocketKind, SocketRef, View,
};
pub use layout::{
    Align, Axis, LayoutOptions, RouteOptions, Spacing, align, distribute, layered, route_links,
};
pub use template::{
    DEFAULT_CATEGORY, NodeLibrary, NodeTemplate, ParamSpec, SocketSpec, TemplateId, Widget,
};
pub use traversal::{
    Direction, EvalContext, EvalError, InputSource, Linked, Topological, Walk,
};
pub use types::{
    DataType, DataTypeBuilder, DataTypeId, SocketShape, TypeRegistry, auto_color,
    auto_header_color,
};
pub use ui::{
    EditorAction, EditorResponse, EditorState, EditorStyle, NodeEditor, ScrollMode, node_size,
    socket_anchor,
};
pub use value::{MapBuilder, Value, ValueKind};
