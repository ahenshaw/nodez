//! The graph, its editing operations and its traversal API, as seen from Python.

use egui::pos2;
use pyo3::exceptions::{PyKeyError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::convert::{from_value, to_value};
use crate::eval;
use crate::library::Library;

/// A directed acyclic graph of typed nodes.
#[pyclass(module = "nodez")]
pub struct Graph {
    pub(crate) inner: nodez::Graph,
}

impl Graph {
    fn get(&self, id: u64) -> PyResult<&nodez::Node> {
        self.inner
            .node(nodez::NodeId(id))
            .ok_or_else(|| PyKeyError::new_err(format!("no node with id {id}")))
    }

    fn get_mut(&mut self, id: u64) -> PyResult<&mut nodez::Node> {
        self.inner
            .node_mut(nodez::NodeId(id))
            .ok_or_else(|| PyKeyError::new_err(format!("no node with id {id}")))
    }

    /// The widget a socket or parameter is edited with, so a Python value can
    /// be coerced to the right `Value` variant.
    fn input_widget(&self, library: &Library, id: u64, socket: &str) -> Option<nodez::Widget> {
        let node = self.inner.node(nodez::NodeId(id))?;
        let template = library.inner.get(node.template)?;
        template.input_spec(socket).map(|s| s.widget.clone())
    }

    fn param_widget(&self, library: &Library, id: u64, name: &str) -> Option<nodez::Widget> {
        let node = self.inner.node(nodez::NodeId(id))?;
        let template = library.inner.get(node.template)?;
        template.param_spec(name).map(|p| p.widget.clone())
    }
}

/// Accept `(node_id, "socket")` as a socket address.
fn socket_ref(obj: &Bound<'_, PyAny>) -> PyResult<nodez::SocketRef> {
    let tuple = obj.cast::<PyTuple>().map_err(|_| {
        PyValueError::new_err("expected a socket as a (node_id, socket_name) tuple")
    })?;
    if tuple.len() != 2 {
        return Err(PyValueError::new_err(
            "expected a socket as a (node_id, socket_name) tuple",
        ));
    }
    Ok(nodez::SocketRef::new(
        nodez::NodeId(tuple.get_item(0)?.extract::<u64>()?),
        tuple.get_item(1)?.extract::<String>()?,
    ))
}

fn ids(values: impl IntoIterator<Item = nodez::NodeId>) -> Vec<u64> {
    values.into_iter().map(|id| id.0).collect()
}

#[pymethods]
impl Graph {
    #[new]
    fn py_new() -> Self {
        Self {
            inner: nodez::Graph::new(),
        }
    }

    // ---------------------------------------------------------------- nodes

    /// Add a node of the given template. Returns its id.
    #[pyo3(signature = (library, template, position = (0.0, 0.0)))]
    fn add_node(
        &mut self,
        library: &Library,
        template: &str,
        position: (f32, f32),
    ) -> PyResult<u64> {
        let template = library.template_id(template)?;
        let id = self
            .inner
            .add_node(&library.inner, template, pos2(position.0, position.1));
        Ok(id.0)
    }

    fn remove_node(&mut self, id: u64) -> bool {
        self.inner.remove_node(nodez::NodeId(id)).is_some()
    }

    fn contains_node(&self, id: u64) -> bool {
        self.inner.contains_node(nodez::NodeId(id))
    }

    fn node_ids(&self) -> Vec<u64> {
        ids(self.inner.node_ids())
    }

    /// A node as a dict: id, type, title, position, width, collapsed, muted,
    /// params and inputs.
    fn node<'py>(&self, py: Python<'py>, library: &Library, id: u64) -> PyResult<Bound<'py, PyDict>> {
        let node = self.get(id)?;
        let dict = PyDict::new(py);
        dict.set_item("id", node.id.0)?;
        dict.set_item(
            "type",
            library.inner.get(node.template).map(|t| t.id.as_str()),
        )?;
        dict.set_item("title", &node.title)?;
        dict.set_item("position", (node.position.x, node.position.y))?;
        dict.set_item("width", node.width)?;
        dict.set_item("collapsed", node.collapsed)?;
        dict.set_item("muted", node.muted)?;

        let params = PyDict::new(py);
        for (key, value) in &node.params {
            params.set_item(key, from_value(py, value)?)?;
        }
        dict.set_item("params", params)?;

        let inputs = PyDict::new(py);
        for (key, value) in &node.input_values {
            inputs.set_item(key, from_value(py, value)?)?;
        }
        dict.set_item("inputs", inputs)?;
        Ok(dict)
    }

    fn nodes<'py>(&self, py: Python<'py>, library: &Library) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty(py);
        for id in self.inner.node_ids() {
            list.append(self.node(py, library, id.0)?)?;
        }
        Ok(list)
    }

    /// The template id of a node.
    fn type_of(&self, library: &Library, id: u64) -> PyResult<String> {
        let node = self.get(id)?;
        library
            .inner
            .get(node.template)
            .map(|t| t.id.clone())
            .ok_or_else(|| PyRuntimeError::new_err("node has no template in this library"))
    }

    fn title(&self, id: u64) -> PyResult<String> {
        Ok(self.get(id)?.title.clone())
    }

    fn set_title(&mut self, id: u64, title: String) -> PyResult<()> {
        self.get_mut(id)?.title = title;
        Ok(())
    }

    fn position(&self, id: u64) -> PyResult<(f32, f32)> {
        let p = self.get(id)?.position;
        Ok((p.x, p.y))
    }

    fn set_position(&mut self, id: u64, position: (f32, f32)) -> PyResult<()> {
        self.get_mut(id)?.position = pos2(position.0, position.1);
        Ok(())
    }

    fn set_collapsed(&mut self, id: u64, collapsed: bool) -> PyResult<()> {
        self.get_mut(id)?.collapsed = collapsed;
        Ok(())
    }

    fn set_muted(&mut self, id: u64, muted: bool) -> PyResult<()> {
        self.get_mut(id)?.muted = muted;
        Ok(())
    }

    // --------------------------------------------------------------- values

    /// Set the inline value of an input socket.
    fn set_input(
        &mut self,
        library: &Library,
        id: u64,
        socket: &str,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let widget = self.input_widget(library, id, socket);
        if widget.is_none() {
            return Err(PyKeyError::new_err(format!(
                "node {id} has no input socket named `{socket}`"
            )));
        }
        let value = to_value(value, widget.as_ref())?;
        self.get_mut(id)?.set_input_value(socket, value);
        Ok(())
    }

    fn input_value<'py>(
        &self,
        py: Python<'py>,
        id: u64,
        socket: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        match self.get(id)?.input_value(socket) {
            Some(value) => from_value(py, value),
            None => Ok(py.None().into_bound(py)),
        }
    }

    /// Set a node parameter.
    fn set_param(
        &mut self,
        library: &Library,
        id: u64,
        name: &str,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let widget = self.param_widget(library, id, name);
        if widget.is_none() {
            return Err(PyKeyError::new_err(format!(
                "node {id} has no parameter named `{name}`"
            )));
        }
        let value = to_value(value, widget.as_ref())?;
        self.get_mut(id)?.set_param(name, value);
        Ok(())
    }

    fn param<'py>(&self, py: Python<'py>, id: u64, name: &str) -> PyResult<Bound<'py, PyAny>> {
        match self.get(id)?.param(name) {
            Some(value) => from_value(py, value),
            None => Ok(py.None().into_bound(py)),
        }
    }

    // ---------------------------------------------------------- connections

    /// Wire an output socket to an input socket. Raises `ValueError` if the
    /// types are incompatible or the link would create a cycle.
    fn connect(
        &mut self,
        library: &Library,
        from: &Bound<'_, PyAny>,
        to: &Bound<'_, PyAny>,
    ) -> PyResult<u64> {
        let (from, to) = (socket_ref(from)?, socket_ref(to)?);
        self.inner
            .connect(&library.inner, from, to)
            .map(|id| id.0)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Whether a link would be accepted. Returns `None` if it would, else the
    /// reason it would not.
    fn why_not_connect(
        &self,
        library: &Library,
        from: &Bound<'_, PyAny>,
        to: &Bound<'_, PyAny>,
    ) -> PyResult<Option<String>> {
        let (from, to) = (socket_ref(from)?, socket_ref(to)?);
        Ok(self
            .inner
            .can_connect(&library.inner, &from, &to)
            .err()
            .map(|e| e.to_string()))
    }

    fn disconnect(&mut self, connection_id: u64) -> bool {
        self.inner
            .disconnect(nodez::ConnectionId(connection_id))
            .is_some()
    }

    fn disconnect_socket(&mut self, socket: &Bound<'_, PyAny>) -> PyResult<usize> {
        let socket = socket_ref(socket)?;
        Ok(self.inner.disconnect_socket(&socket).len())
    }

    /// Every wire, as `(from_node, from_socket, to_node, to_socket)`.
    fn connections(&self) -> Vec<(u64, String, u64, String)> {
        self.inner
            .connections()
            .map(|c| {
                (
                    c.from.node.0,
                    c.from.socket.clone(),
                    c.to.node.0,
                    c.to.socket.clone(),
                )
            })
            .collect()
    }

    fn connection_ids(&self) -> Vec<u64> {
        self.inner.connections().map(|c| c.id.0).collect()
    }

    /// The output socket feeding an input socket, or `None`.
    fn source_of(&self, id: u64, socket: &str) -> Option<(u64, String)> {
        self.inner
            .source_of(nodez::NodeId(id), socket)
            .map(|s| (s.node.0, s.socket.clone()))
    }

    fn links_into(&self, id: u64, socket: &str) -> Vec<(u64, String)> {
        self.inner
            .links_into(nodez::NodeId(id), socket)
            .map(|c| (c.from.node.0, c.from.socket.clone()))
            .collect()
    }

    fn links_from(&self, id: u64, socket: &str) -> Vec<(u64, String)> {
        self.inner
            .links_from(nodez::NodeId(id), socket)
            .map(|c| (c.to.node.0, c.to.socket.clone()))
            .collect()
    }

    fn is_input_linked(&self, id: u64, socket: &str) -> bool {
        self.inner.is_input_linked(nodez::NodeId(id), socket)
    }

    // ------------------------------------------------------------ traversal

    fn predecessors(&self, id: u64) -> Vec<u64> {
        ids(self.inner.predecessors(nodez::NodeId(id)))
    }

    fn successors(&self, id: u64) -> Vec<u64> {
        ids(self.inner.successors(nodez::NodeId(id)))
    }

    /// Every node this one transitively depends on.
    fn ancestors(&self, id: u64) -> Vec<u64> {
        ids(self.inner.ancestors(nodez::NodeId(id)))
    }

    /// Every node that transitively depends on this one.
    fn descendants(&self, id: u64) -> Vec<u64> {
        ids(self.inner.descendants(nodez::NodeId(id)))
    }

    fn depends_on(&self, node: u64, target: u64) -> bool {
        self.inner
            .depends_on(nodez::NodeId(node), nodez::NodeId(target))
    }

    /// Nodes with no incoming wires.
    fn roots(&self) -> Vec<u64> {
        ids(self.inner.roots())
    }

    /// Nodes with no outgoing wires.
    fn sinks(&self) -> Vec<u64> {
        ids(self.inner.sinks())
    }

    fn isolated(&self) -> Vec<u64> {
        ids(self.inner.isolated())
    }

    /// Every node, ordered so each comes after everything it depends on.
    fn topological_order(&self) -> PyResult<Vec<u64>> {
        self.inner
            .topological_order()
            .map(ids)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// `node` and everything it transitively depends on, in evaluation order.
    fn dependency_order(&self, id: u64) -> PyResult<Vec<u64>> {
        self.inner
            .dependency_order(nodez::NodeId(id))
            .map(ids)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn components(&self) -> Vec<Vec<u64>> {
        self.inner.components().into_iter().map(ids).collect()
    }

    /// Longest path from any root, per node.
    fn depths(&self) -> PyResult<std::collections::HashMap<u64, usize>> {
        self.inner
            .depths()
            .map(|map| map.into_iter().map(|(k, v)| (k.0, v)).collect())
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn find_cycle(&self) -> Option<Vec<u64>> {
        self.inner.find_cycle().map(ids)
    }

    fn is_acyclic(&self) -> bool {
        self.inner.is_acyclic()
    }

    // ----------------------------------------------------------- evaluation

    /// Fold the graph into a value per node, in dependency order.
    ///
    /// `rule` is called once per node with an `EvalNode` and returns whatever
    /// that node evaluates to. Only what `target` depends on is visited.
    fn evaluate(
        &self,
        py: Python<'_>,
        library: &Library,
        target: u64,
        rule: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        self.inner
            .evaluate::<Py<PyAny>, PyErr>(&library.inner, nodez::NodeId(target), |ctx| {
                let node = eval::snapshot(py, &ctx)?;
                rule.call1((node,)).map(Bound::unbind)
            })
            .map_err(eval_error)
    }

    /// Like `evaluate`, but visits every node and returns `{node_id: value}`.
    fn evaluate_all(
        &self,
        py: Python<'_>,
        library: &Library,
        rule: &Bound<'_, PyAny>,
    ) -> PyResult<std::collections::HashMap<u64, Py<PyAny>>> {
        self.inner
            .evaluate_all::<Py<PyAny>, PyErr>(&library.inner, |ctx| {
                let node = eval::snapshot(py, &ctx)?;
                rule.call1((node,)).map(Bound::unbind)
            })
            .map(|map| map.into_iter().map(|(k, v)| (k.0, v)).collect())
            .map_err(eval_error)
    }

    // ------------------------------------------------------------ utilities

    /// Arrange the graph in columns by dependency depth.
    #[pyo3(signature = (library, column_gap = 70.0, row_gap = 22.0))]
    fn layout(&mut self, library: &Library, column_gap: f32, row_gap: f32) -> PyResult<()> {
        let style = nodez::EditorStyle::default();
        nodez::layered(
            &mut self.inner,
            &nodez::LayoutOptions {
                column_gap,
                row_gap,
                ..nodez::LayoutOptions::default()
            },
            |graph, node| nodez::node_size(graph, &library.inner, node, &style).y,
        )
        .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Drop nodes and wires that no longer fit the library, and fill in any
    /// values that templates have gained. Returns `(nodes, wires)` removed.
    fn validate(&mut self, library: &Library) -> (usize, usize) {
        let repairs = self.inner.validate(&library.inner);
        (repairs.removed_nodes, repairs.removed_connections)
    }

    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[staticmethod]
    #[pyo3(signature = (text, library = None))]
    fn from_json(text: &str, library: Option<&Library>) -> PyResult<Self> {
        let mut inner: nodez::Graph =
            serde_json::from_str(text).map_err(|e| PyValueError::new_err(e.to_string()))?;
        if let Some(library) = library {
            inner.validate(&library.inner);
        }
        Ok(Self { inner })
    }

    #[getter]
    fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    #[getter]
    fn connection_count(&self) -> usize {
        self.inner.connection_count()
    }

    fn clear(&mut self) {
        self.inner.clear();
    }

    fn __len__(&self) -> usize {
        self.inner.node_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "Graph({} nodes, {} links)",
            self.inner.node_count(),
            self.inner.connection_count()
        )
    }
}

/// Unwrap a node error back to the Python exception that caused it.
fn eval_error(error: nodez::EvalError<PyErr>) -> PyErr {
    match error {
        nodez::EvalError::Node { source, .. } => source,
        other => PyValueError::new_err(other.to_string()),
    }
}
