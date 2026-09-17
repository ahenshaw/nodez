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
