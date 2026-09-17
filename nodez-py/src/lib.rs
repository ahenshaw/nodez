//! Python bindings for [`nodez`].
//!
//! The module is built as a cdylib and imported as `nodez`. See
//! `nodez-py/python/demo.py` for a worked example.

#![forbid(unsafe_code)]

mod convert;
mod editor;
mod eval;
mod graph;
mod library;

use pyo3::prelude::*;

#[pymodule]
fn nodez(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__doc__", "A Blender-style node editor and typed graph model.")?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<library::Library>()?;
    m.add_class::<library::Widget>()?;
    m.add_class::<library::Socket>()?;
    m.add_class::<library::Param>()?;
    m.add_class::<graph::Graph>()?;
    m.add_class::<eval::EvalNode>()?;
    m.add_function(wrap_pyfunction!(editor::edit, m)?)?;
    Ok(())
}
