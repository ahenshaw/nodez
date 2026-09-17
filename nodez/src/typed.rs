//! **Prototype.** Describing nodes as Rust types instead of runtime schemas.
//!
//! The dynamic model in the rest of the crate is the storage format: a
//! [`Graph`](crate::Graph) of [`Value`](crate::Value) maps that the editor draws
//! and serde saves. This module sits on top of it and lets the *schema* and the
//! *evaluation rules* come from Rust types, so socket names stop being strings
//! you have to keep in sync by hand.
//!
//! ```ignore
//! #[derive(Clone, SocketType)]
//! #[socket(color = "#70B2FF", widget = text)]
//! struct Text(String);
//!
//! #[derive(NodeType)]
//! #[node(id = "image", label = "Image", category = "Build", output = ImageRef)]
//! struct Image {
//!     #[input(default = "nginx")] repository: Text,
//!     #[input(default = "latest")] tag: Text,
//! }
//!
//! impl Evaluate<ImageRef> for Image {
//!     fn evaluate(&self) -> Result<ImageRef, NodeError> {
//!         Ok(ImageRef(format!("{}:{}", self.repository.0, self.tag.0)))
//!     }
//! }
//! ```

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;

use crate::graph::{DynNode, NodeData, NodeId};
use crate::template::{NodeLibrary, NodeTemplate, Widget};
use crate::traversal::EvalContext;
use crate::types::{DataTypeBuilder, TypeRegistry};
use crate::value::Value;

/// What travels along a wire while a graph is being evaluated.
///
/// Erasing to `Any` rather than to a generated enum works because the editor
/// and [`Graph::connect`](crate::Graph::connect) already refused every
/// incompatible link: by the time a value is read back, its Rust type is
/// whatever the socket type said it would be.
pub type Payload = Box<dyn Any>;

/// A Rust type that can travel along a wire.
///
/// The `widget`/`to_value`/`from_value` defaults make a type link-only. A type
/// that overrides them can also be typed in by hand, and the editor draws the
/// widget on the socket when nothing is connected.
pub trait SocketType: Clone + 'static {
    /// The name the type is registered under. Must be unique in a library.
    const NAME: &'static str;

    /// Colour, shape and description, as the editor should draw it.
    fn data_type() -> DataTypeBuilder;

    /// The inline editor for an unconnected input. `None` means link-only.
    fn widget() -> Widget {
        Widget::None
    }

    /// Only meaningful when [`SocketType::widget`] is overridden.
    fn to_value(&self) -> Value {
        Value::Null
    }

    /// Only meaningful when [`SocketType::widget`] is overridden.
    fn from_value(_value: &Value) -> Option<Self> {
        None
    }
}

/// The primitive types are socket types out of the box, so a field that is
/// just a string or a number needs no wrapper. Their colours follow Blender's
/// convention rather than the name hash, since these are the types a reader
/// sees most often.
///
/// Declare a newtype when a type carries real domain meaning (`ImageRef`), or
/// when two text-ish sockets must not connect to each other.
macro_rules! primitive_socket_type {
    ($ty:ty, $name:literal, $r:literal, $g:literal, $b:literal, $widget:expr, $to:expr, $from:expr) => {
        impl SocketType for $ty {
            const NAME: &'static str = $name;

            fn data_type() -> DataTypeBuilder {
                DataTypeBuilder::new($name, egui::Color32::from_rgb($r, $g, $b))
            }

            fn widget() -> Widget {
                $widget
            }

            #[allow(clippy::redundant_closure_call)]
            fn to_value(&self) -> Value {
                ($to)(self)
            }

            #[allow(clippy::redundant_closure_call)]
            fn from_value(value: &Value) -> Option<Self> {
                ($from)(value)
            }
        }
    };
}

primitive_socket_type!(
    String, "Text", 0x70, 0xB2, 0xFF,
    Widget::text(),
    |s: &String| Value::Text(s.clone()),
    |v: &Value| v.as_str().map(str::to_owned)
);
primitive_socket_type!(
    i64, "Int", 0x59, 0x8C, 0x5C,
    Widget::int(),
    |n: &i64| Value::Int(*n),
    Value::as_i64
);
primitive_socket_type!(
    f64, "Float", 0xA1, 0xA1, 0xA1,
    Widget::float(),
    |n: &f64| Value::Float(*n),
    Value::as_f64
);
primitive_socket_type!(
    bool, "Bool", 0xCC, 0xA6, 0xD6,
    Widget::Checkbox,
    |b: &bool| Value::Bool(*b),
    Value::as_bool
);

/// An input socket that accepts any number of links.
///
/// This is the *arity* of the socket, kept separate from the payload type, so
/// `Multi<EnvList>` reads as "many links, each carrying a list" and
/// `Vec<T>` inside a payload keeps its ordinary meaning.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Multi<T> {
    values: Vec<T>,
    sources: Vec<NodeId>,
}

impl<T> Multi<T> {
    pub fn new(values: Vec<T>, sources: Vec<NodeId>) -> Self {
        Self { values, sources }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.values.iter()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Each value with the node it came from, for reporting an error against
    /// the upstream node rather than this one.
    pub fn links(&self) -> impl Iterator<Item = (NodeId, &T)> {
        self.sources.iter().copied().zip(self.values.iter())
    }

    pub fn into_vec(self) -> Vec<T> {
        self.values
    }
}

impl<T> Deref for Multi<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.values
    }
}

impl<'a, T> IntoIterator for &'a Multi<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<T> IntoIterator for Multi<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

/// A kind of node, described by a Rust type.
///
/// This is the compile-time form of [`NodeTemplate`]: an instance in a graph is
/// a [`Node`](crate::Node), and the kind it is an instance *of* is a `NodeType`.
///
/// `#[derive(NodeType)]` writes both methods: the schema from the field types
/// and their attributes, and the reader that resolves each field from its link
/// or its inline value.
pub trait NodeType: Sized + 'static {
    /// The template id saved in files. Must be unique in a library.
    const ID: &'static str;

    /// Build the template, registering any socket types it mentions.
    fn template(types: &mut TypeRegistry) -> NodeTemplate;

    /// Read one node's resolved fields out of the graph.
    ///
    /// Generic over the graph's payload type, so a node kind can be evaluated
    /// against either dynamic or typed storage.
    fn from_context<N: NodeData>(
        ctx: &EvalContext<'_, Payload, N>,
    ) -> Result<Self, NodeError>;
}

/// A marker naming one way of folding a graph.
///
/// It is the *fold* that is named, not the output type, because different nodes
/// in the same fold produce different types — an Image yields an image
/// reference, a Service yields a service definition.
///
/// ```ignore
/// struct Config;
/// impl Fold for Config {}
/// ```
pub trait Fold: 'static {}

/// How a node contributes to one fold.
///
/// Kept separate from [`NodeType`] so a graph can be folded more than one way —
/// emit the config, emit a dependency report, compute a preview — without the
/// node type having to pick one.
pub trait Evaluate<F: Fold>: NodeType {
    /// What this node produces in this fold.
    type Output: 'static;

    fn evaluate(&self) -> Result<Self::Output, NodeError>;
}

/// Something went wrong evaluating one node.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum NodeError {
    #[error("`{socket}` needs a connection")]
    MissingInput { socket: &'static str },
    #[error("`{socket}` has no value")]
    MissingValue { socket: &'static str },
    /// Should be unreachable: connections are type-checked before they exist.
    #[error("`{socket}` carried a {found} where a {expected} was expected")]
    WrongType {
        socket: &'static str,
        expected: &'static str,
        found: &'static str,
    },
    #[error("{0}")]
    Custom(String),
}

impl NodeError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom(message.into())
    }
}

impl From<String> for NodeError {
    fn from(message: String) -> Self {
        Self::Custom(message)
    }
}

impl From<&str> for NodeError {
    fn from(message: &str) -> Self {
        Self::Custom(message.to_owned())
    }
}

/// The evaluation rules for one fold over a set of node types.
///
/// Registering a node stores a closure that reads it, evaluates it and erases
/// the result, so folding a graph needs no match on template ids — and adding a
/// node type is one `register` call rather than a schema entry plus a match arm
/// that nothing checks against it.
pub struct Rules<F: Fold, N = DynNode> {
    #[allow(clippy::type_complexity)]
    rules: HashMap<
        &'static str,
        Box<dyn Fn(&EvalContext<'_, Payload, N>) -> Result<Payload, NodeError>>,
    >,
    _fold: std::marker::PhantomData<fn() -> (F, N)>,
}

impl<F: Fold, N: NodeData> Default for Rules<F, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: Fold, N: NodeData> Rules<F, N> {
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            _fold: std::marker::PhantomData,
        }
    }

    /// Register a node type's schema and its rule in one step.
    pub fn register<K>(&mut self, library: &mut NodeLibrary)
    where
        K: Evaluate<F>,
    {
        let template = K::template(&mut library.types);
        library.register(template);
        self.add::<K>();
    }

    /// Register only the rule, for a template registered elsewhere.
    pub fn add<K>(&mut self)
    where
        K: Evaluate<F>,
    {
        self.rules.insert(
            K::ID,
            Box::new(|ctx| {
                let node = K::from_context(ctx)?;
                let out = node.evaluate()?;
                Ok(Box::new(out) as Payload)
            }),
        );
    }

    /// Run the rule for whatever node the context is on.
    pub fn run(&self, ctx: &EvalContext<'_, Payload, N>) -> Result<Payload, NodeError> {
        let id = ctx.template().id.as_str();
        match self.rules.get(id) {
            Some(rule) => rule(ctx),
            None => Err(NodeError::custom(format!("no rule registered for `{id}`"))),
        }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

impl<F: Fold, N> fmt::Debug for Rules<F, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ids: Vec<_> = self.rules.keys().copied().collect();
        ids.sort_unstable();
        f.debug_struct("Rules").field("nodes", &ids).finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers the derive macro calls. Public so generated code can reach them, but
// not part of the API you write by hand.
// ---------------------------------------------------------------------------

#[doc(hidden)]
pub mod __private {
    use super::*;

    /// Register a socket type and return its id, reusing it if already present.
    pub fn register_type<T: SocketType>(types: &mut TypeRegistry) -> crate::DataTypeId {
        types.register(T::data_type())
    }

    /// Pull a required input: the upstream value if wired, else the inline one.
    pub fn required<T: SocketType, N: NodeData>(
        ctx: &EvalContext<'_, Payload, N>,
        socket: &'static str,
    ) -> Result<T, NodeError> {
        if let Some(link) = ctx.input(socket) {
            return downcast::<T>(link.value, socket);
        }
        // A link-only type has no inline value to fall back on, so the useful
        // complaint is that nothing is connected.
        if T::widget() == Widget::None {
            return Err(NodeError::MissingInput { socket });
        }
        literal::<T, N>(ctx, socket).ok_or(NodeError::MissingValue { socket })
    }

    /// Pull an optional input. Unwired and unset both give `None`.
    pub fn optional<T: SocketType, N: NodeData>(
        ctx: &EvalContext<'_, Payload, N>,
        socket: &'static str,
    ) -> Result<Option<T>, NodeError> {
        if let Some(link) = ctx.input(socket) {
            return downcast::<T>(link.value, socket).map(Some);
        }
        Ok(literal::<T, N>(ctx, socket))
    }

    /// Pull every link arriving at a fan-in socket, in connection order.
    pub fn multi<T: SocketType, N: NodeData>(
        ctx: &EvalContext<'_, Payload, N>,
        socket: &'static str,
    ) -> Result<Multi<T>, NodeError> {
        let mut values = Vec::new();
        let mut sources = Vec::new();
        for link in ctx.inputs(socket) {
            values.push(downcast::<T>(link.value, socket)?);
            sources.push(link.node);
        }
        Ok(Multi::new(values, sources))
    }

    /// Give a text widget a placeholder when it has none. The socket type
    /// chooses the kind of editor; the field it sits on names it.
    pub fn with_hint(widget: Widget, hint: &str) -> Widget {
        match widget {
            Widget::Text { multiline, hint: existing } if existing.is_empty() => Widget::Text {
                multiline,
                hint: hint.to_owned(),
            },
            other => other,
        }
    }

    /// Read a node parameter, falling back to the template default.
    pub fn param<T: SocketType, N: NodeData>(
        ctx: &EvalContext<'_, Payload, N>,
        name: &'static str,
    ) -> Result<T, NodeError> {
        ctx.param(name)
            .and_then(|value| T::from_value(&value))
            .ok_or(NodeError::MissingValue { socket: name })
    }

    fn literal<T: SocketType, N: NodeData>(
        ctx: &EvalContext<'_, Payload, N>,
        socket: &str,
    ) -> Option<T> {
        ctx.literal(socket).and_then(|value| T::from_value(&value))
    }

    fn downcast<T: SocketType>(value: &Payload, socket: &'static str) -> Result<T, NodeError> {
        value
            .downcast_ref::<T>()
            .cloned()
            .ok_or(NodeError::WrongType {
                socket,
                expected: T::NAME,
                found: "another type",
            })
    }
}
