//! Folding a graph into Python values.
//!
//! `nodez::EvalContext` borrows the graph, the library and the results map, so
//! it cannot cross into Python as-is. Instead each node's context is flattened
//! into an owned [`EvalNode`] just before the callback runs — the upstream
//! results are already Python objects, so this costs a few dictionary
//! insertions per node rather than any real work.

use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::convert::from_value;

/// The node being evaluated, as handed to an `evaluate` callback.
#[pyclass(module = "nodez")]
pub struct EvalNode {
    #[pyo3(get)]
    id: u64,
    /// The template id, e.g. `"service"`.
    #[pyo3(get, name = "type")]
    template: String,
    #[pyo3(get)]
    title: String,
    #[pyo3(get)]
    muted: bool,
    params: HashMap<String, Py<PyAny>>,
    literals: HashMap<String, Py<PyAny>>,
    linked: HashMap<String, bool>,
    inputs: HashMap<String, Vec<Py<PyAny>>>,
}

#[pymethods]
impl EvalNode {
    /// A node parameter, falling back to the template default.
    #[pyo3(signature = (name, default = None))]
    fn param(&self, py: Python<'_>, name: &str, default: Option<Py<PyAny>>) -> Py<PyAny> {
        self.params
            .get(name)
            .map(|v| v.clone_ref(py))
            .unwrap_or_else(|| default.unwrap_or_else(|| py.None()))
    }

    /// The inline value typed into an input socket, whether or not it is wired.
    #[pyo3(signature = (name, default = None))]
    fn literal(&self, py: Python<'_>, name: &str, default: Option<Py<PyAny>>) -> Py<PyAny> {
        self.literals
            .get(name)
            .map(|v| v.clone_ref(py))
            .unwrap_or_else(|| default.unwrap_or_else(|| py.None()))
    }

    /// Whether anything is wired into an input socket.
    fn is_linked(&self, name: &str) -> bool {
        self.linked.get(name).copied().unwrap_or(false)
    }

    /// The first upstream result arriving at an input socket, or `None`.
    fn input(&self, py: Python<'_>, name: &str) -> Py<PyAny> {
        self.inputs
            .get(name)
            .and_then(|v| v.first())
            .map(|v| v.clone_ref(py))
            .unwrap_or_else(|| py.None())
    }

    /// Every upstream result arriving at an input socket, in connection order.
    fn inputs<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for value in self.inputs.get(name).into_iter().flatten() {
            list.append(value.bind(py))?;
        }
        Ok(list)
    }

    /// Whatever is wired in, else the inline value. The common case.
    #[pyo3(signature = (name, default = None))]
    fn resolve(&self, py: Python<'_>, name: &str, default: Option<Py<PyAny>>) -> Py<PyAny> {
        if let Some(value) = self.inputs.get(name).and_then(|v| v.first()) {
            return value.clone_ref(py);
        }
        self.literal(py, name, default)
    }

    /// All parameters as a dict.
    #[getter]
    fn params<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.params {
            dict.set_item(key, value.bind(py))?;
        }
        Ok(dict)
    }

    /// All inline socket values as a dict.
    #[getter]
    fn literals<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.literals {
            dict.set_item(key, value.bind(py))?;
        }
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        format!("EvalNode(id={}, type={:?})", self.id, self.template)
    }
}

/// Flatten one `EvalContext` into an owned, Python-visible node.
pub fn snapshot(
    py: Python<'_>,
    ctx: &nodez::EvalContext<'_, Py<PyAny>>,
) -> PyResult<EvalNode> {
    let template = ctx.template();
    let mut params = HashMap::with_capacity(template.params.len());
    for spec in &template.params {
        if let Some(value) = ctx.param(&spec.name) {
            params.insert(spec.name.clone(), from_value(py, value)?.unbind());
        }
    }

    let mut literals = HashMap::new();
    let mut linked = HashMap::with_capacity(template.inputs.len());
    let mut inputs = HashMap::with_capacity(template.inputs.len());
    for spec in &template.inputs {
        if let Some(value) = ctx.literal(&spec.name) {
            literals.insert(spec.name.clone(), from_value(py, value)?.unbind());
        }
        linked.insert(spec.name.clone(), ctx.is_linked(&spec.name));
        let upstream: Vec<Py<PyAny>> = ctx
            .inputs(&spec.name)
            .iter()
            .map(|link| link.value.clone_ref(py))
            .collect();
        if !upstream.is_empty() {
            inputs.insert(spec.name.clone(), upstream);
        }
    }

    Ok(EvalNode {
        id: ctx.id().0,
        template: template.id.clone(),
        title: ctx.title().to_owned(),
        muted: ctx.is_muted(),
        params,
        literals,
        linked,
        inputs,
    })
}
