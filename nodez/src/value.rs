//! The dynamic value type carried by node parameters and unconnected input sockets.

use std::fmt;

/// A discriminant for [`Value`], useful for validating that a stored value still
/// matches the shape a template expects.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ValueKind {
    Null,
    Bool,
    Int,
    Float,
    Text,
    Vec2,
    Vec3,
    Color,
    Choice,
    List,
    Map,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Text => "text",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Color => "color",
            Self::Choice => "choice",
            Self::List => "list",
            Self::Map => "map",
        };
        f.write_str(s)
    }
}

/// A concrete value stored on a node.
///
/// `Value` is deliberately small and closed: it covers what a node editor needs
/// to *edit* inline. Domain data that flows along the wires is whatever type
/// your evaluator produces — see [`crate::Graph::evaluate`].
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", content = "value"))]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    /// Linear RGBA in `0..=1`.
    Color([f32; 4]),
    /// One selected variant of a [`crate::Widget::Combo`] socket or parameter.
    Choice(String),
    List(Vec<Value>),
    /// An insertion-ordered map. Ordering is preserved so generated config files
    /// come out in a stable, author-controlled order.
    Map(Vec<(String, Value)>),
}

impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Null => ValueKind::Null,
            Self::Bool(_) => ValueKind::Bool,
            Self::Int(_) => ValueKind::Int,
            Self::Float(_) => ValueKind::Float,
            Self::Text(_) => ValueKind::Text,
            Self::Vec2(_) => ValueKind::Vec2,
            Self::Vec3(_) => ValueKind::Vec3,
            Self::Color(_) => ValueKind::Color,
            Self::Choice(_) => ValueKind::Choice,
            Self::List(_) => ValueKind::List,
            Self::Map(_) => ValueKind::Map,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Int(i) => Some(*i != 0),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Float(f) => Some(*f as i64),
            Self::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(i) => Some(*i as f64),
            Self::Bool(b) => Some(f64::from(*b)),
            _ => None,
        }
    }

    /// Borrow the string payload of a [`Value::Text`] or [`Value::Choice`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) | Self::Choice(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_vec2(&self) -> Option<[f32; 2]> {
        match self {
            Self::Vec2(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        match self {
            Self::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<[f32; 4]> {
        match self {
            Self::Color(c) => Some(*c),
            Self::Vec3([r, g, b]) => Some([*r, *g, *b, 1.0]),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Map(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Look up a key in a [`Value::Map`].
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_map()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Render the value the way most config languages would spell it.
    ///
    /// This is a convenience for simple emitters; anything with real quoting or
    /// indentation rules should match on the value itself.
    pub fn to_literal(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            }
            Self::Text(s) | Self::Choice(s) => s.clone(),
            Self::Vec2([x, y]) => format!("[{x}, {y}]"),
            Self::Vec3([x, y, z]) => format!("[{x}, {y}, {z}]"),
            Self::Color([r, g, b, a]) => format!("#{:02X}{:02X}{:02X}{:02X}",
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
                (a * 255.0).round() as u8,
            ),
            Self::List(items) => {
                let inner: Vec<_> = items.iter().map(Self::to_literal).collect();
                format!("[{}]", inner.join(", "))
            }
            Self::Map(entries) => {
                let inner: Vec<_> = entries
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.to_literal()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }
}

impl Value {
    /// Start building an ordered map.
    ///
    /// Config files care about key order, so entries come out in the order they
    /// were set. The `set_*` variants drop an entry that would be empty, which
    /// is most of what assembling a document by hand spends its lines on:
    ///
    /// ```
    /// # use nodez::Value;
    /// # let (restart, replicas, ports) = ("always", 3_i64, vec!["8080:80"]);
    /// let service = Value::map()
    ///     .set("image", "nginx:latest")
    ///     .set_if(restart != "no", "restart", restart)
    ///     .set_if(replicas > 1, "deploy", Value::map().set("replicas", replicas))
    ///     .set_list("ports", ports)
    ///     .set_some::<String>("command", None);
    /// # let _ = Value::from(service);
    /// ```
    pub fn map() -> MapBuilder {
        MapBuilder::new()
    }
}

/// Builds an ordered [`Value::Map`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapBuilder(Vec<(String, Value)>);

impl MapBuilder {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Set a key, whatever its value.
    #[must_use]
    pub fn set(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.0.push((key.into(), value.into()));
        self
    }

    /// Set a key only when `keep` holds.
    #[must_use]
    pub fn set_if(self, keep: bool, key: impl Into<String>, value: impl Into<Value>) -> Self {
        if keep { self.set(key, value) } else { self }
    }

    /// Set a key only when there is a value for it.
    #[must_use]
    pub fn set_some<T: Into<Value>>(self, key: impl Into<String>, value: Option<T>) -> Self {
        match value {
            Some(value) => self.set(key, value),
            None => self,
        }
    }

    /// Set a key to a list, dropping the key when the list is empty.
    #[must_use]
    pub fn set_list<T: Into<Value>>(
        self,
        key: impl Into<String>,
        items: impl IntoIterator<Item = T>,
    ) -> Self {
        let items: Vec<Value> = items.into_iter().map(Into::into).collect();
        if items.is_empty() {
            self
        } else {
            self.set(key, Value::List(items))
        }
    }

    /// Set a key to a nested map, dropping the key when that map is empty.
    #[must_use]
    pub fn set_map(self, key: impl Into<String>, map: MapBuilder) -> Self {
        if map.is_empty() { self } else { self.set(key, map) }
    }

    /// The entries, in the order they were set.
    pub fn entries(self) -> Vec<(String, Value)> {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<MapBuilder> for Value {
    fn from(builder: MapBuilder) -> Self {
        Self::Map(builder.0)
    }
}

impl FromIterator<(String, Value)> for MapBuilder {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_literal())
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Self::Float(f64::from(v))
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Text(v.to_owned())
    }
}
