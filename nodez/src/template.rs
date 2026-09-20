//! Node *templates*: the schema describing what a kind of node looks like and
//! which sockets it exposes. A [`NodeLibrary`] is the set of templates plus the
//! [`TypeRegistry`] their sockets refer to.

use std::collections::HashMap;

use egui::Color32;

use crate::types::{DataTypeId, TypeRegistry};
use crate::value::Value;

/// The category a template lands in when it names none.
pub const DEFAULT_CATEGORY: &str = "Misc";

/// Handle to a template registered in a [`NodeLibrary`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TemplateId(pub(crate) u32);

impl TemplateId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The inline editor drawn for an unconnected input socket or a node parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum Widget {
    /// No inline editor; the socket is link-only.
    None,
    Checkbox,
    Int {
        min: i64,
        max: i64,
        speed: f64,
        suffix: String,
    },
    Float {
        min: f64,
        max: f64,
        speed: f64,
        suffix: String,
    },
    /// A float rendered as Blender's 0..1 slider.
    Slider {
        min: f64,
        max: f64,
    },
    Text {
        multiline: bool,
        hint: String,
    },
    Combo {
        options: Vec<String>,
    },
    Color {
        alpha: bool,
    },
    Vec2 {
        speed: f64,
    },
    Vec3 {
        speed: f64,
    },
}

impl Widget {
    pub fn int() -> Self {
        Self::Int {
            min: i64::MIN,
            max: i64::MAX,
            speed: 1.0,
            suffix: String::new(),
        }
    }

    pub fn int_range(min: i64, max: i64) -> Self {
        Self::Int {
            min,
            max,
            speed: 1.0,
            suffix: String::new(),
        }
    }

    pub fn float() -> Self {
        Self::Float {
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            speed: 0.01,
            suffix: String::new(),
        }
    }

    pub fn float_range(min: f64, max: f64) -> Self {
        Self::Float {
            min,
            max,
            speed: (max - min) / 200.0,
            suffix: String::new(),
        }
    }

    pub fn text() -> Self {
        Self::Text {
            multiline: false,
            hint: String::new(),
        }
    }

    pub fn text_hint(hint: impl Into<String>) -> Self {
        Self::Text {
            multiline: false,
            hint: hint.into(),
        }
    }

    pub fn combo<I, S>(options: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Combo {
            options: options.into_iter().map(Into::into).collect(),
        }
    }

    /// A value that is valid for this widget, used when a template gains a new
    /// socket that existing saved nodes do not have.
    pub fn default_value(&self) -> Value {
        match self {
            Self::None => Value::Null,
            Self::Checkbox => Value::Bool(false),
            Self::Int { min, .. } => Value::Int(if *min > 0 { *min } else { 0 }),
            Self::Float { min, .. } | Self::Slider { min, .. } => {
                Value::Float(if *min > 0.0 { *min } else { 0.0 })
            }
            Self::Text { .. } => Value::Text(String::new()),
            Self::Combo { options } => Value::Choice(options.first().cloned().unwrap_or_default()),
            Self::Color { .. } => Value::Color([1.0, 1.0, 1.0, 1.0]),
            Self::Vec2 { .. } => Value::Vec2([0.0; 2]),
            Self::Vec3 { .. } => Value::Vec3([0.0; 3]),
        }
    }

    /// Height hint in unzoomed points, used for node layout.
    pub(crate) fn rows(&self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Vec2 { .. } => 2.0,
            Self::Vec3 { .. } => 3.0,
            Self::Text { multiline: true, .. } => 3.0,
            _ => 1.0,
        }
    }
}

/// One input or output socket of a template.
#[derive(Clone, Debug)]
pub struct SocketSpec {
    /// Stable identifier, used to address the socket from code and config.
    pub name: String,
    /// Display label; falls back to `name` when empty.
    pub label: String,
    pub ty: DataTypeId,
    /// Value used when the socket is an unconnected input.
    pub default: Value,
    /// Inline editor shown when the socket is an unconnected input.
    pub widget: Widget,
    /// Inputs are single-link by default, like Blender. Set this to accept a fan-in.
    pub multi: bool,
    /// Whether the node still means something with this input left unwired.
    ///
    /// Only says anything about a link-only input: one with an inline editor
    /// always has a value, and a fan-in is allowed to be empty.
    pub optional: bool,
    /// Hidden sockets still exist in the model but are not drawn.
    pub hidden: bool,
    pub description: String,
}

impl SocketSpec {
    pub fn new(name: impl Into<String>, ty: DataTypeId) -> Self {
        let name = name.into();
        Self {
            label: title_case(&name),
            name,
            ty,
            default: Value::Null,
            widget: Widget::None,
            multi: false,
            optional: false,
            hidden: false,
            description: String::new(),
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Attach an inline editor and the value it starts at.
    pub fn widget(mut self, widget: Widget, default: impl Into<Value>) -> Self {
        self.widget = widget;
        self.default = default.into();
        self
    }

    /// Attach an inline editor, starting from the widget's own default value.
    pub fn editable(mut self, widget: Widget) -> Self {
        self.default = widget.default_value();
        self.widget = widget;
        self
    }

    pub fn default_value(mut self, value: impl Into<Value>) -> Self {
        self.default = value.into();
        self
    }

    /// Accept more than one incoming link, collected in connection order.
    pub fn multi(mut self) -> Self {
        self.multi = true;
        self
    }

    /// Say the node works with this input left unwired.
    ///
    /// `#[derive(NodeType)]` sets this for an `Option<T>` field. It is what
    /// tells a required input apart from one that is merely empty, which is
    /// the difference between a node that is unfinished and one that is done.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn display(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }

    /// Whether this input has to be wired for the node to mean anything.
    ///
    /// Derived rather than declared, from three things the schema already
    /// says: an input with an inline editor always has a value to fall back
    /// on, a fan-in is allowed to be empty, and an optional one says outright
    /// that it can be left alone. What is left is an input with nowhere else
    /// to get its value from.
    pub fn required(&self) -> bool {
        !self.optional && !self.multi && !self.hidden && self.widget == Widget::None
    }
}

/// A node property that is not a socket: always drawn in the node body, never
/// connectable. Blender's enum dropdowns and checkboxes are these.
#[derive(Clone, Debug)]
pub struct ParamSpec {
    pub name: String,
    pub label: String,
    pub widget: Widget,
    pub default: Value,
    /// Draw the label next to the widget rather than letting the widget fill the row.
    pub show_label: bool,
    pub description: String,
}

impl ParamSpec {
    pub fn new(name: impl Into<String>, widget: Widget) -> Self {
        let name = name.into();
        let default = widget.default_value();
        Self {
            label: title_case(&name),
            name,
            widget,
            default,
            show_label: true,
            description: String::new(),
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn default_value(mut self, value: impl Into<Value>) -> Self {
        self.default = value.into();
        self
    }

    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn display(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }
}

/// The schema for one kind of node.
#[derive(Clone, Debug)]
pub struct NodeTemplate {
    /// Stable identifier used by saved graphs.
    pub id: String,
    pub label: String,
    /// Groups the template in the add-node menu and picks a default header color.
    pub category: String,
    /// Overrides the category color for this node's header.
    pub header_color: Option<Color32>,
    pub inputs: Vec<SocketSpec>,
    pub outputs: Vec<SocketSpec>,
    pub params: Vec<ParamSpec>,
    /// Default body width in unzoomed points.
    pub width: f32,
    pub description: String,
    /// Extra terms matched by the add-node search box.
    pub keywords: Vec<String>,
}

impl NodeTemplate {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: DEFAULT_CATEGORY.to_owned(),
            header_color: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            params: Vec::new(),
            width: 150.0,
            description: String::new(),
            keywords: Vec::new(),
        }
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    pub fn header_color(mut self, color: Color32) -> Self {
        self.header_color = Some(color);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    pub fn input(mut self, socket: SocketSpec) -> Self {
        self.inputs.push(socket);
        self
    }

    pub fn output(mut self, socket: SocketSpec) -> Self {
        self.outputs.push(socket);
        self
    }

    pub fn param(mut self, param: ParamSpec) -> Self {
        self.params.push(param);
        self
    }

    pub fn input_index(&self, name: &str) -> Option<usize> {
        self.inputs.iter().position(|s| s.name == name)
    }

    pub fn output_index(&self, name: &str) -> Option<usize> {
        self.outputs.iter().position(|s| s.name == name)
    }

    pub fn input_spec(&self, name: &str) -> Option<&SocketSpec> {
        self.inputs.iter().find(|s| s.name == name)
    }

    pub fn output_spec(&self, name: &str) -> Option<&SocketSpec> {
        self.outputs.iter().find(|s| s.name == name)
    }

    pub fn param_spec(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// What a derived header color is keyed on: the category, so that a
    /// category reads as one family, or the template's own id when it has no
    /// category to belong to.
    pub fn color_key(&self) -> &str {
        if self.category == DEFAULT_CATEGORY {
            &self.id
        } else {
            &self.category
        }
    }
}

/// The catalog of node templates and the types their sockets speak.
#[derive(Debug, Default)]
pub struct NodeLibrary {
    pub types: TypeRegistry,
    templates: Vec<NodeTemplate>,
    by_id: HashMap<String, TemplateId>,
    category_colors: HashMap<String, Color32>,
    category_order: Vec<String>,
}

impl NodeLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a library from an existing type registry.
    pub fn with_types(types: TypeRegistry) -> Self {
        Self {
            types,
            ..Self::default()
        }
    }

    /// Register a template. Re-registering an id replaces the template and keeps
    /// its [`TemplateId`], so existing graphs stay valid.
    pub fn register(&mut self, template: NodeTemplate) -> TemplateId {
        if !self.category_order.contains(&template.category) {
            self.category_order.push(template.category.clone());
        }
        if let Some(&id) = self.by_id.get(&template.id) {
            self.templates[id.index()] = template;
            return id;
        }
        let id = TemplateId(self.templates.len() as u32);
        self.by_id.insert(template.id.clone(), id);
        self.templates.push(template);
        id
    }

    /// Give a category its own header color in the editor.
    pub fn set_category_color(&mut self, category: impl Into<String>, color: Color32) {
        let category = category.into();
        if !self.category_order.contains(&category) {
            self.category_order.push(category.clone());
        }
        self.category_colors.insert(category, color);
    }

    pub fn category_color(&self, category: &str) -> Option<Color32> {
        self.category_colors.get(category).copied()
    }

    /// The color a template's header is drawn in.
    ///
    /// Its own color if it names one, else its category's if that names one,
    /// else one derived from [`NodeTemplate::color_key`] — so a library needs
    /// no color choices at all to come out looking deliberate.
    pub fn header_color(&self, template: &NodeTemplate) -> Color32 {
        template
            .header_color
            .or_else(|| self.category_color(&template.category))
            .unwrap_or_else(|| crate::types::auto_header_color(template.color_key()))
    }

    /// Categories in registration order.
    pub fn categories(&self) -> impl Iterator<Item = &str> {
        self.category_order.iter().map(String::as_str)
    }

    pub fn get(&self, id: TemplateId) -> Option<&NodeTemplate> {
        self.templates.get(id.index())
    }

    pub fn expect(&self, id: TemplateId) -> &NodeTemplate {
        self.get(id).expect("TemplateId from another library")
    }

    pub fn id(&self, template_id: &str) -> Option<TemplateId> {
        self.by_id.get(template_id).copied()
    }

    pub fn by_name(&self, template_id: &str) -> Option<(TemplateId, &NodeTemplate)> {
        let id = self.id(template_id)?;
        Some((id, self.expect(id)))
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (TemplateId, &NodeTemplate)> {
        self.templates
            .iter()
            .enumerate()
            .map(|(i, t)| (TemplateId(i as u32), t))
    }

    /// Templates in a category, in registration order.
    pub fn in_category<'a>(
        &'a self,
        category: &'a str,
    ) -> impl Iterator<Item = (TemplateId, &'a NodeTemplate)> {
        self.iter().filter(move |(_, t)| t.category == category)
    }

    /// Templates with an input socket that could accept `ty`.
    pub fn accepting(&self, ty: DataTypeId) -> impl Iterator<Item = (TemplateId, &NodeTemplate)> {
        self.iter().filter(move |(_, t)| {
            t.inputs
                .iter()
                .any(|s| !s.hidden && self.types.compatible(ty, s.ty))
        })
    }

    /// Templates with an output socket that could feed `ty`.
    pub fn producing(&self, ty: DataTypeId) -> impl Iterator<Item = (TemplateId, &NodeTemplate)> {
        self.iter().filter(move |(_, t)| {
            t.outputs
                .iter()
                .any(|s| !s.hidden && self.types.compatible(s.ty, ty))
        })
    }

    /// Case-insensitive fuzzy-ish search over label, id, category and keywords.
    pub fn search<'a>(&'a self, query: &str) -> Vec<(TemplateId, &'a NodeTemplate)> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.iter().collect();
        }
        let mut scored: Vec<_> = self
            .iter()
            .filter_map(|(id, t)| match_score(t, &q).map(|s| (s, id, t)))
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.label.cmp(&b.2.label)));
        scored.into_iter().map(|(_, id, t)| (id, t)).collect()
    }
}

/// Lower is a better match; `None` means no match.
fn match_score(template: &NodeTemplate, query: &str) -> Option<u32> {
    let label = template.label.to_lowercase();
    if label == query {
        return Some(0);
    }
    if label.starts_with(query) {
        return Some(1);
    }
    if label.contains(query) {
        return Some(2);
    }
    if template.id.to_lowercase().contains(query) {
        return Some(3);
    }
    if template.category.to_lowercase().contains(query) {
        return Some(4);
    }
    if template
        .keywords
        .iter()
        .any(|k| k.to_lowercase().contains(query))
    {
        return Some(5);
    }
    // Last resort: all query characters appear in order in the label.
    let mut chars = query.chars();
    let mut needle = chars.next();
    for c in label.chars() {
        if Some(c) == needle {
            needle = chars.next();
            if needle.is_none() {
                return Some(6);
            }
        }
    }
    None
}

/// `"read_only"` -> `"Read Only"`.
fn title_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut capitalize = true;
    for c in name.chars() {
        if c == '_' || c == '-' {
            out.push(' ');
            capitalize = true;
        } else if capitalize {
            out.extend(c.to_uppercase());
            capitalize = false;
        } else {
            out.push(c);
        }
    }
    out
}
