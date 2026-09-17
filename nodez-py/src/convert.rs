//! Converting between Python objects and `nodez::Value`, and parsing the small
//! string enums the Python API uses in place of Rust enums.

use egui::Color32;
use nodez::{SocketShape, Value, Widget};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PySequence, PyString, PyTuple};

/// Turn a Python object into a `Value`.
///
/// `widget` is the editor the value will be shown in, when there is one. It
/// disambiguates cases Python cannot express on its own: a `str` destined for a
/// dropdown is a `Choice` rather than free `Text`, and a 3-tuple destined for a
/// colour picker is a `Color` rather than a plain list.
pub fn to_value(obj: &Bound<'_, PyAny>, widget: Option<&Widget>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }

    if let Some(widget) = widget {
        match widget {
            Widget::Combo { options } => {
                let text: String = obj.extract()?;
                if !options.is_empty() && !options.contains(&text) {
                    return Err(PyValueError::new_err(format!(
                        "`{text}` is not one of {options:?}"
                    )));
                }
                return Ok(Value::Choice(text));
            }
            Widget::Color { .. } => {
                let parts = number_sequence(obj)?;
                return match parts.len() {
                    3 => Ok(Value::Color([
                        parts[0] as f32,
                        parts[1] as f32,
                        parts[2] as f32,
                        1.0,
                    ])),
                    4 => Ok(Value::Color([
                        parts[0] as f32,
                        parts[1] as f32,
                        parts[2] as f32,
                        parts[3] as f32,
                    ])),
                    n => Err(PyValueError::new_err(format!(
                        "a colour needs 3 or 4 components, got {n}"
                    ))),
                };
            }
            Widget::Vec2 { .. } => {
                let parts = number_sequence(obj)?;
                if parts.len() != 2 {
                    return Err(PyValueError::new_err(format!(
                        "a vec2 needs 2 components, got {}",
                        parts.len()
                    )));
                }
                return Ok(Value::Vec2([parts[0] as f32, parts[1] as f32]));
            }
            Widget::Vec3 { .. } => {
                let parts = number_sequence(obj)?;
                if parts.len() != 3 {
                    return Err(PyValueError::new_err(format!(
                        "a vec3 needs 3 components, got {}",
                        parts.len()
                    )));
                }
                return Ok(Value::Vec3([
                    parts[0] as f32,
                    parts[1] as f32,
                    parts[2] as f32,
                ]));
            }
            Widget::Int { .. } => return Ok(Value::Int(obj.extract()?)),
            Widget::Float { .. } | Widget::Slider { .. } => {
                return Ok(Value::Float(obj.extract()?));
            }
            Widget::Checkbox => return Ok(Value::Bool(obj.extract()?)),
            Widget::Text { .. } => return Ok(Value::Text(obj.extract()?)),
            Widget::None => {}
        }
    }

    // No widget to steer us: use the natural mapping. bool is checked before
    // int, because in Python `True` is an `int`.
    if obj.is_instance_of::<PyBool>() {
        return Ok(Value::Bool(obj.extract()?));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(Value::Int(obj.extract()?));
    }
    if obj.is_instance_of::<PyFloat>() {
        return Ok(Value::Float(obj.extract()?));
    }
    if obj.is_instance_of::<PyString>() {
        return Ok(Value::Text(obj.extract()?));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (key, value) in dict {
            entries.push((key.extract::<String>()?, to_value(&value, None)?));
        }
        return Ok(Value::Map(entries));
    }
    if obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>() {
        let sequence = obj.cast::<PySequence>()?;
        let mut items = Vec::with_capacity(sequence.len()?);
        for i in 0..sequence.len()? {
            items.push(to_value(&sequence.get_item(i)?, None)?);
        }
        return Ok(Value::List(items));
    }

    Err(PyTypeError::new_err(format!(
        "cannot use {} as a node value; expected None, bool, int, float, str, list or dict",
        obj.get_type().name()?
    )))
}

/// Turn a `Value` back into the most natural Python object.
pub fn from_value<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Null => py.None().into_bound(py),
        Value::Bool(b) => b.into_pyobject(py)?.to_owned().into_any(),
        Value::Int(i) => i.into_pyobject(py)?.into_any(),
        Value::Float(f) => f.into_pyobject(py)?.into_any(),
        Value::Text(s) | Value::Choice(s) => s.into_pyobject(py)?.into_any(),
        Value::Vec2(v) => v.to_vec().into_pyobject(py)?.into_any(),
        Value::Vec3(v) => v.to_vec().into_pyobject(py)?.into_any(),
        Value::Color(c) => c.to_vec().into_pyobject(py)?.into_any(),
        Value::List(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(from_value(py, item)?)?;
            }
            list.into_any()
        }
        Value::Map(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries {
                dict.set_item(key, from_value(py, item)?)?;
            }
            dict.into_any()
        }
    })
}

fn number_sequence(obj: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    let sequence = obj.cast::<PySequence>().map_err(|_| {
        PyTypeError::new_err("expected a sequence of numbers")
    })?;
    let mut out = Vec::with_capacity(sequence.len()?);
    for i in 0..sequence.len()? {
        out.push(sequence.get_item(i)?.extract::<f64>()?);
    }
    Ok(out)
}

/// Parse `"#RRGGBB"`, `"#RRGGBBAA"` or a `(r, g, b[, a])` sequence of 0-255
/// integers or 0.0-1.0 floats.
pub fn parse_color(obj: &Bound<'_, PyAny>) -> PyResult<Color32> {
    if let Ok(text) = obj.extract::<String>() {
        let hex = text.strip_prefix('#').unwrap_or(&text);
        let byte = |i: usize| -> PyResult<u8> {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|_| PyValueError::new_err(format!("`{text}` is not a hex colour")))
        };
        return match hex.len() {
            6 => Ok(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Ok(Color32::from_rgba_unmultiplied(
                byte(0)?,
                byte(2)?,
                byte(4)?,
                byte(6)?,
            )),
            _ => Err(PyValueError::new_err(format!(
                "`{text}` is not a hex colour; expected #RRGGBB or #RRGGBBAA"
            ))),
        };
    }

    let parts = number_sequence(obj)?;
    // Floats in 0..=1 and integers in 0..=255 are both common ways to spell a
    // colour, so accept either.
    let scale = if parts.iter().all(|c| *c <= 1.0) { 255.0 } else { 1.0 };
    let byte = |i: usize| (parts[i] * scale).round().clamp(0.0, 255.0) as u8;
    match parts.len() {
        3 => Ok(Color32::from_rgb(byte(0), byte(1), byte(2))),
        4 => Ok(Color32::from_rgba_unmultiplied(
            byte(0),
            byte(1),
            byte(2),
            byte(3),
        )),
        n => Err(PyValueError::new_err(format!(
            "a colour needs 3 or 4 components, got {n}"
        ))),
    }
}

pub fn parse_shape(name: &str) -> PyResult<SocketShape> {
    match name {
        "circle" => Ok(SocketShape::Circle),
        "diamond" => Ok(SocketShape::Diamond),
        "diamond_dot" => Ok(SocketShape::DiamondDot),
        "square" => Ok(SocketShape::Square),
        other => Err(PyValueError::new_err(format!(
            "unknown socket shape `{other}`; expected circle, diamond, diamond_dot or square"
        ))),
    }
}
