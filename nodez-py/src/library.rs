//! `Widget`, `Socket`, `Param` and `Library` as seen from Python.
//!
//! Data types and templates are addressed by name from Python rather than by
//! handle, which reads better and keeps error messages legible.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::convert::{parse_color, parse_shape, to_value};

/// The inline editor for a socket or a parameter.
#[pyclass(module = "nodez", frozen, from_py_object)]
#[derive(Clone)]
pub struct Widget {
    pub(crate) inner: nodez::Widget,
}

#[pymethods]
impl Widget {
    /// No inline editor; the socket can only be driven by a link.
    #[staticmethod]
    fn none() -> Self {
        Self {
            inner: nodez::Widget::None,
        }
    }

    #[staticmethod]
    fn checkbox() -> Self {
        Self {
            inner: nodez::Widget::Checkbox,
        }
    }

    #[staticmethod]
    #[pyo3(signature = (hint = "", multiline = false))]
    fn text(hint: &str, multiline: bool) -> Self {
        Self {
            inner: nodez::Widget::Text {
                multiline,
                hint: hint.to_owned(),
            },
        }
    }

    #[staticmethod]
    #[pyo3(signature = (min = i64::MIN, max = i64::MAX, speed = 1.0, suffix = ""))]
    fn int(min: i64, max: i64, speed: f64, suffix: &str) -> Self {
        Self {
            inner: nodez::Widget::Int {
                min,
                max,
                speed,
                suffix: suffix.to_owned(),
            },
        }
    }

    #[staticmethod]
    #[pyo3(signature = (min = f64::NEG_INFINITY, max = f64::INFINITY, speed = 0.01, suffix = ""))]
    fn float(min: f64, max: f64, speed: f64, suffix: &str) -> Self {
        Self {
            inner: nodez::Widget::Float {
                min,
                max,
                speed,
                suffix: suffix.to_owned(),
            },
        }
    }

    #[staticmethod]
    fn slider(min: f64, max: f64) -> Self {
        Self {
            inner: nodez::Widget::Slider { min, max },
        }
    }

    #[staticmethod]
    fn combo(options: Vec<String>) -> Self {
        Self {
            inner: nodez::Widget::Combo { options },
        }
    }

    #[staticmethod]
    #[pyo3(signature = (alpha = false))]
    fn color(alpha: bool) -> Self {
        Self {
            inner: nodez::Widget::Color { alpha },
        }
    }

    #[staticmethod]
    #[pyo3(signature = (speed = 0.01))]
    fn vec2(speed: f64) -> Self {
        Self {
            inner: nodez::Widget::Vec2 { speed },
        }
    }

    #[staticmethod]
    #[pyo3(signature = (speed = 0.01))]
    fn vec3(speed: f64) -> Self {
        Self {
            inner: nodez::Widget::Vec3 { speed },
        }
    }

    fn __repr__(&self) -> String {
        format!("Widget({:?})", self.inner)
    }
}

/// One input or output socket of a template.
#[pyclass(module = "nodez", from_py_object)]
#[derive(Clone)]
pub struct Socket {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) label: Option<String>,
    pub(crate) widget: Option<Widget>,
    pub(crate) default: Option<nodez::Value>,
    pub(crate) multi: bool,
    pub(crate) hidden: bool,
    pub(crate) description: String,
}

#[pymethods]
impl Socket {
    #[new]
    #[pyo3(signature = (
        name,
        type_name,
        *,
        label = None,
        widget = None,
        default = None,
        multi = false,
        hidden = false,
        description = "",
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        type_name: String,
        label: Option<String>,
        widget: Option<Widget>,
        default: Option<Bound<'_, PyAny>>,
        multi: bool,
        hidden: bool,
        description: &str,
    ) -> PyResult<Self> {
        let default = default
            .map(|value| to_value(&value, widget.as_ref().map(|w| &w.inner)))
            .transpose()?;
        Ok(Self {
            name,
            type_name,
            label,
            widget,
            default,
            multi,
            hidden,
            description: description.to_owned(),
        })
    }

    fn __repr__(&self) -> String {
        format!("Socket({:?}, {:?})", self.name, self.type_name)
    }
}

/// A node property that is not a socket: always drawn, never connectable.
#[pyclass(module = "nodez", from_py_object)]
#[derive(Clone)]
pub struct Param {
    pub(crate) name: String,
    pub(crate) widget: Widget,
    pub(crate) label: Option<String>,
    pub(crate) default: Option<nodez::Value>,
    pub(crate) show_label: bool,
    pub(crate) description: String,
}

#[pymethods]
impl Param {
    #[new]
    #[pyo3(signature = (
        name,
        widget,
        *,
        label = None,
        default = None,
        show_label = true,
        description = "",
    ))]
    fn new(
        name: String,
        widget: Widget,
        label: Option<String>,
        default: Option<Bound<'_, PyAny>>,
        show_label: bool,
        description: &str,
    ) -> PyResult<Self> {
        let default = default
            .map(|value| to_value(&value, Some(&widget.inner)))
            .transpose()?;
        Ok(Self {
            name,
            widget,
            label,
            default,
            show_label,
            description: description.to_owned(),
        })
    }

    fn __repr__(&self) -> String {
        format!("Param({:?})", self.name)
    }
}

/// The socket types and node templates of your domain.
#[pyclass(module = "nodez")]
pub struct Library {
    pub(crate) inner: Arc<nodez::NodeLibrary>,
}

impl Library {
    /// The library is shared with the editor through an `Arc`, so mutation has
    /// to go through `Arc::make_mut`-style unsharing. In practice a library is
    /// built once and then only read, so this stays cheap.
    fn edit<R>(&mut self, f: impl FnOnce(&mut nodez::NodeLibrary) -> R) -> PyResult<R> {
        let library = Arc::get_mut(&mut self.inner).ok_or_else(|| {
            PyValueError::new_err("the library cannot be changed while an editor is open")
        })?;
        Ok(f(library))
    }

    fn type_id(&self, name: &str) -> PyResult<nodez::DataTypeId> {
        self.inner.types.id(name).ok_or_else(|| {
            PyValueError::new_err(format!(
                "unknown data type `{name}`; known types are {:?}",
                self.type_names()
            ))
        })
    }

    pub(crate) fn template_id(&self, id: &str) -> PyResult<nodez::TemplateId> {
        self.inner.id(id).ok_or_else(|| {
            PyValueError::new_err(format!(
                "unknown node template `{id}`; known templates are {:?}",
                self.template_ids()
            ))
        })
    }

    fn type_names(&self) -> Vec<String> {
        self.inner
            .types
            .iter()
            .map(|(_, t)| t.name.clone())
            .collect()
    }

    fn template_ids(&self) -> Vec<String> {
        self.inner.iter().map(|(_, t)| t.id.clone()).collect()
    }

    fn build_socket(&self, socket: &Socket) -> PyResult<nodez::SocketSpec> {
        let mut spec = nodez::SocketSpec::new(&socket.name, self.type_id(&socket.type_name)?);
        if let Some(label) = &socket.label {
            spec = spec.label(label);
        }
        if let Some(widget) = &socket.widget {
            spec = spec.editable(widget.inner.clone());
        }
        if let Some(default) = &socket.default {
            spec = spec.default_value(default.clone());
        }
        if socket.multi {
            spec = spec.multi();
        }
        if socket.hidden {
            spec = spec.hidden();
        }
        Ok(spec.description(&socket.description))
    }

    fn build_param(&self, param: &Param) -> PyResult<nodez::ParamSpec> {
        let mut spec = nodez::ParamSpec::new(&param.name, param.widget.inner.clone());
        if let Some(label) = &param.label {
            spec = spec.label(label);
        }
        if let Some(default) = &param.default {
            spec = spec.default_value(default.clone());
        }
        Ok(spec
            .show_label(param.show_label)
            .description(&param.description))
    }
}

#[pymethods]
impl Library {
    #[new]
    fn py_new() -> Self {
        Self {
            inner: Arc::new(nodez::NodeLibrary::new()),
        }
    }

    /// Register a socket type. Returns its name, so it reads as an assignment.
    #[pyo3(signature = (name, color, *, shape = "circle", label = None, wildcard = false, description = ""))]
    fn add_type(
        &mut self,
        name: String,
        color: &Bound<'_, PyAny>,
        shape: &str,
        label: Option<String>,
        wildcard: bool,
        description: &str,
    ) -> PyResult<String> {
        let color = parse_color(color)?;
        let shape = parse_shape(shape)?;
        let mut builder = nodez::DataTypeBuilder::new(&name, color)
            .shape(shape)
            .wildcard(wildcard)
            .description(description);
        if let Some(label) = label {
            builder = builder.label(label);
        }
        self.edit(|library| library.types.register(builder))?;
        Ok(name)
    }

    /// Allow an output of type `from_type` to feed an input of type `to_type`.
    fn allow_cast(&mut self, from_type: &str, to_type: &str) -> PyResult<()> {
        let (from, to) = (self.type_id(from_type)?, self.type_id(to_type)?);
        self.edit(|library| library.types.allow_cast(from, to))
    }

    fn allow_cast_both(&mut self, a: &str, b: &str) -> PyResult<()> {
        let (a, b) = (self.type_id(a)?, self.type_id(b)?);
        self.edit(|library| library.types.allow_cast_both(a, b))
    }

    /// Whether an output of `from_type` may be wired into an input of `to_type`.
    fn compatible(&self, from_type: &str, to_type: &str) -> PyResult<bool> {
        let (from, to) = (self.type_id(from_type)?, self.type_id(to_type)?);
        Ok(self.inner.types.compatible(from, to))
    }

    fn set_category_color(&mut self, category: String, color: &Bound<'_, PyAny>) -> PyResult<()> {
        let color = parse_color(color)?;
        self.edit(|library| library.set_category_color(category, color))
    }

    /// Register a node template.
    #[pyo3(signature = (
        id,
        label,
        *,
        category = "Misc",
        inputs = None,
        outputs = None,
        params = None,
        width = 150.0,
        description = "",
        keywords = None,
        header_color = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_template(
        &mut self,
        id: String,
        label: String,
        category: &str,
        inputs: Option<Vec<Socket>>,
        outputs: Option<Vec<Socket>>,
        params: Option<Vec<Param>>,
        width: f32,
        description: &str,
        keywords: Option<Vec<String>>,
        header_color: Option<Bound<'_, PyAny>>,
    ) -> PyResult<String> {
        let mut template = nodez::NodeTemplate::new(&id, label)
            .category(category)
            .width(width)
            .description(description);
        if let Some(keywords) = keywords {
            template = template.keywords(keywords);
        }
        if let Some(color) = header_color {
            template = template.header_color(parse_color(&color)?);
        }
        for socket in inputs.unwrap_or_default() {
            template = template.input(self.build_socket(&socket)?);
        }
        for socket in outputs.unwrap_or_default() {
            template = template.output(self.build_socket(&socket)?);
        }
        for param in params.unwrap_or_default() {
            template = template.param(self.build_param(&param)?);
        }
        self.edit(|library| library.register(template))?;
        Ok(id)
    }

    #[pyo3(name = "type_names")]
    fn py_type_names(&self) -> Vec<String> {
        self.type_names()
    }

    #[pyo3(name = "template_ids")]
    fn py_template_ids(&self) -> Vec<String> {
        self.template_ids()
    }

    fn categories(&self) -> Vec<String> {
        self.inner.categories().map(str::to_owned).collect()
    }

    /// The input socket names of a template, in order.
    fn inputs_of(&self, template: &str) -> PyResult<Vec<String>> {
        let id = self.template_id(template)?;
        Ok(self
            .inner
            .expect(id)
            .inputs
            .iter()
            .map(|s| s.name.clone())
            .collect())
    }

    /// The output socket names of a template, in order.
    fn outputs_of(&self, template: &str) -> PyResult<Vec<String>> {
        let id = self.template_id(template)?;
        Ok(self
            .inner
            .expect(id)
            .outputs
            .iter()
            .map(|s| s.name.clone())
            .collect())
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "Library({} types, {} templates)",
            self.inner.types.len(),
            self.inner.len()
        )
    }
}
