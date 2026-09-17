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
//!   editor does: colour-coded sockets, bezier noodles that fade between the two
//!   socket colours, box select, `Shift+A` to add, `G` to grab, `X` to delete.
//!
//! Sockets are typed. A [`TypeRegistry`] assigns each type a colour and a
//! shape, and [`TypeRegistry::allow_cast`] declares which types may implicitly
//! feed which others. The editor refuses incompatible drags, and
//! [`Graph::connect`] refuses them in code too, so a graph on disk is always
//! well-typed.
//!
//! ## A minimal library
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
//!             "text" => ctx.literal_str("value").unwrap_or_default().to_owned(),
//!             "upper" => ctx.input("text").map(|l| l.value.to_uppercase()).unwrap_or_default(),
//!             _ => String::new(),
//!         })
//!     })
//!     .unwrap();
//! assert_eq!(out, "HELLO");
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

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod graph;
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
pub use typed::{Evaluate, Fold, Multi, NodeError, NodeType, Payload, Rules, SocketType};

pub use graph::{
    Connection, ConnectError, ConnectionId, CycleError, DynNode, Graph, Node, NodeData, NodeId,
    Repairs, SocketKind, SocketRef,
};
pub use layout::{LayoutOptions, layered};
pub use template::{NodeLibrary, NodeTemplate, ParamSpec, SocketSpec, TemplateId, Widget};
pub use traversal::{
    Direction, EvalContext, EvalError, InputSource, Linked, Topological, Walk,
};
pub use types::{
    DataType, DataTypeBuilder, DataTypeId, SocketShape, TypeRegistry, auto_color,
};
pub use ui::{
    EditorAction, EditorResponse, EditorState, EditorStyle, NodeEditor, ScrollMode, node_size,
};
pub use value::{Value, ValueKind};
