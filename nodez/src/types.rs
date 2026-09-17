//! Socket data types: their colours, their shapes, and the rules deciding which
//! of them may be wired together.

use std::collections::HashMap;

use egui::Color32;

/// Handle to a data type registered in a [`TypeRegistry`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataTypeId(pub(crate) u32);

impl DataTypeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// How a socket is drawn, following Blender's visual vocabulary.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SocketShape {
    /// A single value. Blender's default.
    #[default]
    Circle,
    /// A field / per-element value.
    Diamond,
    /// A field that also carries a single fallback value.
    DiamondDot,
    /// A list or collection.
    Square,
}

/// A registered socket type.
#[derive(Clone, Debug)]
pub struct DataType {
    /// Stable identifier, also used as the display name unless `label` is set.
    pub name: String,
    pub label: String,
    /// The colour of sockets and of the wires leaving them.
    pub color: Color32,
    pub shape: SocketShape,
    /// A wildcard type connects to everything. Useful for reroute / group nodes.
    pub wildcard: bool,
    pub description: String,
    /// Types this one may be implicitly converted *to* when connecting.
    casts_to: Vec<DataTypeId>,
}

impl DataType {
    pub fn display(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }

    /// The types this one implicitly converts to, not counting itself.
    pub fn casts_to(&self) -> &[DataTypeId] {
        &self.casts_to
    }
}

/// Builder for a [`DataType`].
#[derive(Clone, Debug)]
pub struct DataTypeBuilder {
    ty: DataType,
}

impl DataTypeBuilder {
    pub fn new(name: impl Into<String>, color: Color32) -> Self {
        Self {
            ty: DataType {
                name: name.into(),
                label: String::new(),
                color,
                shape: SocketShape::Circle,
                wildcard: false,
                description: String::new(),
                casts_to: Vec::new(),
            },
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.ty.label = label.into();
        self
    }

    pub fn shape(mut self, shape: SocketShape) -> Self {
        self.ty.shape = shape;
        self
    }

    pub fn wildcard(mut self, wildcard: bool) -> Self {
        self.ty.wildcard = wildcard;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.ty.description = description.into();
        self
    }
}

/// Pick a stable colour for a socket type from its name.
///
/// The hue comes from a hash of the name, while saturation and lightness are
/// fixed, so a palette generated this way reads as one family and a type keeps
/// its colour for the life of the project. Two names can land on neighbouring
/// hues; give one of them an explicit colour if that ever matters.
pub fn auto_color(name: &str) -> Color32 {
    hsl(hash_name(name) % 360, 0.62, 0.62)
}

/// Pick a stable node-header colour from a name.
///
/// The same hue [`auto_color`] would give, but muted: a header is a large fill
/// behind light text, so it wants roughly the saturation and lightness a
/// hand-picked palette lands on rather than the vividness of a socket dot.
pub fn auto_header_color(name: &str) -> Color32 {
    hsl(hash_name(name) % 360, 0.33, 0.32)
}

/// FNV-1a: small, stable, and good enough to scatter short names.
fn hash_name(name: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn hsl(hue_degrees: u32, saturation: f32, lightness: f32) -> Color32 {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue_degrees as f32 / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = lightness - chroma / 2.0;
    let byte = |v: f32| ((v + base) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

/// Every socket type known to an editor, plus the implicit-conversion graph
/// between them.
///
/// Connections are permitted when the source type is the target type, when
/// either side is a wildcard, or when the source declares an implicit cast to
/// the target via [`TypeRegistry::allow_cast`].
#[derive(Clone, Debug, Default)]
pub struct TypeRegistry {
    types: Vec<DataType>,
    by_name: HashMap<String, DataTypeId>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a type. Registering a name twice overwrites the previous
    /// definition and keeps its id, so a library can be reloaded in place.
    pub fn register(&mut self, builder: DataTypeBuilder) -> DataTypeId {
        let ty = builder.ty;
        if let Some(&id) = self.by_name.get(&ty.name) {
            let casts = std::mem::take(&mut self.types[id.index()].casts_to);
            self.types[id.index()] = DataType { casts_to: casts, ..ty };
            return id;
        }
        let id = DataTypeId(self.types.len() as u32);
        self.by_name.insert(ty.name.clone(), id);
        self.types.push(ty);
        id
    }

    /// Shorthand for `register(DataTypeBuilder::new(name, color))`.
    pub fn add(&mut self, name: impl Into<String>, color: Color32) -> DataTypeId {
        self.register(DataTypeBuilder::new(name, color))
    }

    /// Allow `from` sockets to feed `to` sockets, the way Blender lets a Float
    /// drive a Vector input.
    pub fn allow_cast(&mut self, from: DataTypeId, to: DataTypeId) {
        let casts = &mut self.types[from.index()].casts_to;
        if !casts.contains(&to) {
            casts.push(to);
        }
    }

    /// Allow casts in both directions.
    pub fn allow_cast_both(&mut self, a: DataTypeId, b: DataTypeId) {
        self.allow_cast(a, b);
        self.allow_cast(b, a);
    }

    pub fn get(&self, id: DataTypeId) -> Option<&DataType> {
        self.types.get(id.index())
    }

    /// Panicking accessor for ids known to come from this registry.
    pub fn expect(&self, id: DataTypeId) -> &DataType {
        self.get(id).expect("DataTypeId from another registry")
    }

    pub fn id(&self, name: &str) -> Option<DataTypeId> {
        self.by_name.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (DataTypeId, &DataType)> {
        self.types
            .iter()
            .enumerate()
            .map(|(i, t)| (DataTypeId(i as u32), t))
    }

    pub fn color(&self, id: DataTypeId) -> Color32 {
        self.get(id).map_or(Color32::GRAY, |t| t.color)
    }

    pub fn shape(&self, id: DataTypeId) -> SocketShape {
        self.get(id).map_or(SocketShape::Circle, |t| t.shape)
    }

    pub fn name(&self, id: DataTypeId) -> &str {
        self.get(id).map_or("?", |t| t.display())
    }

    /// Whether an output of type `from` may be wired into an input of type `to`.
    pub fn compatible(&self, from: DataTypeId, to: DataTypeId) -> bool {
        if from == to {
            return true;
        }
        let (Some(a), Some(b)) = (self.get(from), self.get(to)) else {
            return false;
        };
        a.wildcard || b.wildcard || a.casts_to.contains(&to)
    }

    /// Whether connecting `from` to `to` needs an implicit conversion.
    pub fn needs_cast(&self, from: DataTypeId, to: DataTypeId) -> bool {
        from != to && self.compatible(from, to)
    }
}
