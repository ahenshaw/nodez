//! Opening the editor window on a graph built in Python.

use std::sync::{Arc, Mutex};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use crate::graph::Graph;
use crate::library::Library;

/// Open the node editor on `graph` and block until the window is closed.
///
/// The graph is edited in place, so after the call returns `graph` holds
/// whatever the user built.
///
/// `on_change` is optional. When given it is called with the graph each time
/// the user changes it; if it returns a string, that string is shown in a panel
/// beside the canvas — which is how you get a live preview of the config your
/// graph generates.
#[pyfunction]
#[pyo3(signature = (
    library,
    graph,
    *,
    title = "nodez",
    theme = "dark",
    scroll_mode = "auto",
    on_change = None,
    size = (1280.0, 820.0),
))]
#[allow(clippy::too_many_arguments)]
pub fn edit(
    py: Python<'_>,
    library: &Library,
    graph: &Bound<'_, Graph>,
    title: &str,
    theme: &str,
    scroll_mode: &str,
    on_change: Option<Py<PyAny>>,
    size: (f32, f32),
) -> PyResult<()> {
    let style = match theme {
        "dark" => nodez::EditorStyle::blender_dark(),
        "light" => nodez::EditorStyle::light(),
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown theme `{other}`; expected dark or light"
            )));
        }
    };
    let scroll_mode = match scroll_mode {
        "auto" => nodez::ScrollMode::Auto,
        "pan" => nodez::ScrollMode::Pan,
        "zoom" => nodez::ScrollMode::Zoom,
        other => {
            return Err(PyValueError::new_err(format!(
                "unknown scroll mode `{other}`; expected auto, pan or zoom"
            )));
        }
    };

    // The graph moves into a shared slot for the duration of the window, so the
    // Python object is not borrowed while the GIL is released.
    let shared = Arc::new(Mutex::new(std::mem::take(&mut graph.borrow_mut().inner)));
    let library = Arc::clone(&library.inner);

    let app = EditorApp {
        library: Arc::clone(&library),
        graph: Arc::clone(&shared),
        editor: {
            let mut editor = nodez::NodeEditor::with_style(style);
            editor.scroll_mode = scroll_mode;
            editor
        },
        on_change,
        panel: None,
        failed: None,
        needs_refresh: true,
        framed: false,
    };

    // `NativeOptions` and `eframe::Error` both hold non-Send pieces, so they
    // are built and consumed entirely inside the closure; only plain data
    // crosses the boundary.
    let title = title.to_owned();
    let outcome: Result<(), String> = py.detach(move || {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([size.0, size.1])
                .with_title(title),
            ..Default::default()
        };
        eframe::run_native(
            "nodez",
            options,
            Box::new(|cc| {
                cc.egui_ctx.set_visuals(egui::Visuals::dark());
                Ok(Box::new(app))
            }),
        )
        .map_err(|e| e.to_string())
    });

    // Hand the graph back whether or not the window came up cleanly.
    let restored = Arc::try_unwrap(shared)
        .map_err(|_| PyRuntimeError::new_err("the editor outlived its graph"))?
        .into_inner()
        .map_err(|_| PyRuntimeError::new_err("the editor panicked"))?;
    graph.borrow_mut().inner = restored;

    outcome.map_err(|e| PyRuntimeError::new_err(format!("could not open the editor: {e}")))
}

struct EditorApp {
    library: Arc<nodez::NodeLibrary>,
    graph: Arc<Mutex<nodez::Graph>>,
    editor: nodez::NodeEditor,
    on_change: Option<Py<PyAny>>,
    /// Whatever `on_change` last returned.
    panel: Option<String>,
    /// The traceback from `on_change`, if it raised.
    failed: Option<String>,
    needs_refresh: bool,
    framed: bool,
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.on_change.is_some() && (self.panel.is_some() || self.failed.is_some()) {
            self.show_panel(ui);
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
            let mut graph = match self.graph.lock() {
                Ok(graph) => graph,
                Err(poisoned) => poisoned.into_inner(),
            };

            if !self.framed {
                self.framed = true;
                let rect = ui.available_rect_before_wrap();
                self.editor.fit_to_graph(rect, &graph, &self.library);
            }

            let response = self.editor.show(ui, &self.library, &mut graph);
            if response.changed || !response.actions.is_empty() {
                self.needs_refresh = true;
            }
        });

        if self.needs_refresh {
            self.needs_refresh = false;
            self.refresh();
        }
    }
}

impl EditorApp {
    /// Call back into Python with the current graph.
    fn refresh(&mut self) {
        let Some(callback) = &self.on_change else {
            return;
        };
        let json = {
            let graph = match self.graph.lock() {
                Ok(graph) => graph,
                Err(poisoned) => poisoned.into_inner(),
            };
            serde_json::to_string(&*graph)
        };
        let json = match json {
            Ok(json) => json,
            Err(e) => {
                self.failed = Some(e.to_string());
                return;
            }
        };

        // Re-acquire the GIL we released around the event loop.
        Python::attach(|py| {
            let graph = Graph {
                inner: serde_json::from_str(&json).unwrap_or_default(),
            };
            match callback.bind(py).call1((graph,)) {
                Ok(value) if value.is_none() => {
                    self.panel = None;
                    self.failed = None;
                }
                Ok(value) => match value.extract::<String>() {
                    Ok(text) => {
                        self.panel = Some(text);
                        self.failed = None;
                    }
                    Err(e) => self.failed = Some(e.to_string()),
                },
                Err(e) => {
                    let traceback = e
                        .traceback(py)
                        .and_then(|tb| tb.format().ok())
                        .unwrap_or_default();
                    self.failed = Some(format!("{traceback}{e}"));
                }
            }
        });
    }

    fn show_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("nodez-py-panel")
            .default_size(420.0)
            .min_size(260.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                if let Some(error) = &self.failed {
                    ui.colored_label(egui::Color32::from_rgb(0xE8, 0x7C, 0x6A), "on_change raised");
                    ui.add_space(4.0);
                    egui::ScrollArea::both()
                        .id_salt("nodez-py-error")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(error).monospace().small())
                                    .selectable(true)
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        });
                    return;
                }
                let Some(text) = &self.panel else { return };
                egui::ScrollArea::both()
                    .id_salt("nodez-py-panel-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .monospace()
                                    .color(egui::Color32::from_rgb(0xCE, 0xD6, 0xC8)),
                            )
                            .selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    });
            });
    }
}
