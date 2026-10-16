//! A ready-made editor window.
//!
//! [`NodeEditor`] is just the canvas. Every app built on it then needs the same
//! surroundings: a toolbar to make and save graphs, a palette to add nodes
//! from, somewhere to inspect the graph, somewhere to show what the graph
//! generates, and a reminder of the keybindings. None of that is domain work,
//! so it lives here.
//!
//! ```no_run
//! # fn library() -> nodez::NodeLibrary { todo!() }
//! # fn generate(g: &nodez::Graph, l: &nodez::NodeLibrary) -> String { todo!() }
//! nodez::app::EditorApp::new(library())
//!     .title("my editor")
//!     .preview(|graph, library| nodez::app::Preview::text(generate(graph, library)))
//!     .json_files()
//!     .run()
//! # .unwrap();
//! ```
//!
//! ## Making it your own
//!
//! * [`EditorApp::toolbar`] adds buttons to the toolbar, and
//!   [`EditorApp::node_menu`] adds entries to a node's right-click menu.
//! * [`EditorApp::layout`] arranges the window yourself, from the app's
//!   pieces — [`show_canvas`](EditorApp::show_canvas),
//!   [`show_toolbar`](EditorApp::show_toolbar),
//!   [`show_palette`](EditorApp::show_palette),
//!   [`show_inspector`](EditorApp::show_inspector),
//!   [`show_preview`](EditorApp::show_preview) and
//!   [`show_status`](EditorApp::show_status) — and panels of your own.
//! * [`EditorApp::edit`] changes the graph from any of these, undoably.
//! * [`EditorApp::ui`] draws the whole thing inside a `Ui` you already have,
//!   for embedding it in a larger application.
//!
//! What is saved is the whole graph, positions and view included; see
//! [`Graph::to_json`].

mod files;

use std::collections::HashSet;

use egui::{Color32, RichText};

use crate::graph::{DynNode, Graph, NodeData, NodeId, View};
use crate::template::{NodeLibrary, TemplateId};
use crate::ui::{EditorAction, EditorStyle, NodeEditor, ScrollMode, free_position, node_size};

/// What a preview callback produces: the generated text, and anything wrong
/// with the graph that produced it.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub text: String,
    /// Shown above the text, in warning color.
    pub problems: Vec<String>,
    /// Nodes that feed the output. Anything else is dimmed in the inspector.
    pub contributing: HashSet<NodeId>,
}

impl Preview {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn problems(mut self, problems: Vec<String>) -> Self {
        self.problems = problems;
        self
    }

    pub fn contributing(mut self, nodes: HashSet<NodeId>) -> Self {
        self.contributing = nodes;
        self
    }
}

type PreviewFn<N> = Box<dyn Fn(&Graph<N>, &NodeLibrary) -> Preview>;
/// Draws into a `Ui` with the whole app to hand.
type UiFn<N> = Box<dyn FnMut(&mut egui::Ui, &mut EditorApp<N>)>;
/// Runs when an entry added with [`EditorApp::node_menu`] is picked.
type NodeMenuFn<N> = Box<dyn FnMut(&mut EditorApp<N>, NodeId)>;
type SaveFn<N> = Box<dyn Fn(&Graph<N>) -> Result<String, String>>;
type LoadFn<N> = Box<dyn Fn(&str) -> Result<Graph<N>, String>>;

/// Reading and writing graphs, when the app should offer it.
struct Persistence<N> {
    save: SaveFn<N>,
    load: LoadFn<N>,
}

#[derive(Default)]
struct Status {
    message: String,
    error: bool,
}

impl Status {
    fn info(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.error = false;
    }

    fn error(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.error = true;
    }
}

/// An editor window: toolbar, palette, inspector, preview and status bar around
/// a [`NodeEditor`].
pub struct EditorApp<N: NodeData = DynNode> {
    pub library: NodeLibrary,
    pub graph: Graph<N>,
    pub editor: NodeEditor,
    title: String,
    path: String,
    preview: Option<PreviewFn<N>>,
    persistence: Option<Persistence<N>>,
    generated: Preview,
    status: Status,
    inspector_open: bool,
    /// Whether wires are steered around the nodes in their way.
    route_wires: bool,
    /// Frame the graph once the editor's rect is known.
    frame_next: bool,
    /// Route a graph that arrived whole, before it is first drawn.
    route_next: bool,
    preview_extension: String,
    /// The groups being edited, outermost first. Empty means the graph the
    /// app was given.
    inside: Vec<Opened<N>>,
    /// Where reusable groups are kept, if anywhere.
    groups_dir: Option<String>,
    /// Whether the graph has changed since it was last saved, loaded or
    /// started afresh.
    modified: bool,
    /// Something that would throw away unsaved changes, waiting on an answer.
    confirm: Option<Discard>,
    /// The window may close: its changes were saved or let go.
    may_close: bool,
    /// The arrangement of the pieces, if not the default.
    layout: Option<UiFn<N>>,
    /// Set while that arrangement is drawing, when `layout` is out of its
    /// slot.
    laying_out: bool,
    /// More for the toolbar, after the built-in buttons.
    toolbar: Option<UiFn<N>>,
    /// What each entry added to the node menu does, by label.
    node_menu: Vec<(String, NodeMenuFn<N>)>,
}

/// What would throw away unsaved changes.
#[derive(Clone, Copy)]
enum Discard {
    New,
    Load,
    Close,
}

/// A group opened for editing: which template it is, and the graph it was
/// opened from.
///
/// The interior is taken out of the library while it is being edited and put
/// back on the way out, which is what keeps the editor from having to borrow
/// the library and one of its groups at the same time.
struct Opened<N: NodeData> {
    template: TemplateId,
    outer: Graph<N>,
    /// Where the editor was looking at `outer`, to come back to.
    view: View,
}

impl EditorApp<DynNode> {
    /// An editor for graphs with the default payload.
    ///
    /// Pinned to [`DynNode`] for the same reason [`Graph::new`] is: a default
    /// type parameter takes no part in inference, and the closures handed to
    /// [`layout`](Self::layout) and friends would otherwise have nothing to
    /// infer it from. With a payload of your own, use
    /// [`with_library`](Self::with_library).
    pub fn new(library: NodeLibrary) -> Self {
        Self::with_library(library)
    }
}

impl<N: NodeData> EditorApp<N> {
    /// An editor for graphs of any payload: `EditorApp::<MyNode>::with_library(..)`.
    pub fn with_library(library: NodeLibrary) -> Self {
        Self {
            library,
            graph: Graph::default(),
            editor: NodeEditor::new(),
            title: "nodez".to_owned(),
            path: "graph.json".to_owned(),
            preview: None,
            persistence: None,
            generated: Preview::default(),
            status: Status::default(),
            inspector_open: true,
            route_wires: true,
            frame_next: true,
            route_next: true,
            inside: Vec::new(),
            groups_dir: None,
            preview_extension: "out".to_owned(),
            modified: false,
            confirm: None,
            may_close: false,
            layout: None,
            laying_out: false,
            toolbar: None,
            node_menu: Vec::new(),
        }
    }

    /// Whether to steer wires around the nodes in their way, rather than let
    /// them run straight and pass underneath. On by default.
    ///
    /// The editor's Route box toggles the same thing.
    pub fn routing(mut self, route: bool) -> Self {
        self.route_wires = route;
        self
    }

    /// Start from an existing graph rather than an empty one.
    pub fn graph(mut self, graph: Graph<N>) -> Self {
        self.graph = graph;
        self.open_view();
        self
    }

    /// Look where the graph was last looked at, if it remembers, and frame
    /// all of it if not.
    fn open_view(&mut self) {
        match self.graph.view() {
            Some(view) => {
                self.editor.state.pan = view.pan;
                self.editor.state.zoom = view.zoom;
                self.frame_next = false;
            }
            None => self.frame_next = true,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The file the Load and Save buttons use.
    pub fn file(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Extension for the file the preview panel's Write button produces.
    /// Where to keep reusable groups, the way GNU Radio keeps hier blocks in
    /// a directory of their own.
    ///
    /// Every `.json` group in it is read into the library before the window
    /// opens, in whatever order they turn out to need, and `Ctrl+G` writes
    /// what it makes back into it. The file is named after the group's id;
    /// the id is what saved graphs refer to, so renaming the file is safe and
    /// renaming the group is what makes a new one.
    pub fn groups_dir(mut self, path: impl Into<String>) -> Self {
        self.groups_dir = Some(path.into());
        self
    }

    pub fn preview_extension(mut self, extension: impl Into<String>) -> Self {
        self.preview_extension = extension.into();
        self
    }

    pub fn style(mut self, style: EditorStyle) -> Self {
        self.editor.style = style;
        self
    }

    pub fn scroll_mode(mut self, mode: ScrollMode) -> Self {
        self.editor.scroll_mode = mode;
        self
    }

    /// Show what the graph generates in a panel, regenerated on every edit.
    pub fn preview(
        mut self,
        f: impl Fn(&Graph<N>, &NodeLibrary) -> Preview + 'static,
    ) -> Self {
        self.preview = Some(Box::new(f));
        self
    }

    /// Supply your own reader and writer, if JSON is not what you want.
    pub fn persistence(
        mut self,
        save: impl Fn(&Graph<N>) -> Result<String, String> + 'static,
        load: impl Fn(&str) -> Result<Graph<N>, String> + 'static,
    ) -> Self {
        self.persistence = Some(Persistence {
            save: Box::new(save),
            load: Box::new(load),
        });
        self
    }

    /// Arrange the window yourself.
    ///
    /// `f` is called every frame in place of [`EditorApp::default_layout`],
    /// and builds the window from the app's pieces — `show_canvas`,
    /// `show_toolbar`, `show_palette`, `show_inspector`, `show_preview`,
    /// `show_status` — and any panels of your own, in whatever arrangement
    /// you like. Leave a piece out and it is not shown.
    ///
    /// ```no_run
    /// # fn library() -> nodez::NodeLibrary { todo!() }
    /// use nodez::egui;
    ///
    /// nodez::app::EditorApp::new(library())
    ///     .layout(|ui, app| {
    ///         egui::Panel::left("palette").show(ui, |ui| app.show_palette(ui));
    ///         egui::Panel::right("mine").show(ui, |ui| {
    ///             ui.label(format!("{} nodes", app.graph.node_count()));
    ///         });
    ///         egui::CentralPanel::default().show(ui, |ui| app.show_canvas(ui));
    ///     })
    ///     .run()
    /// # .unwrap();
    /// ```
    pub fn layout(mut self, f: impl FnMut(&mut egui::Ui, &mut EditorApp<N>) + 'static) -> Self {
        self.layout = Some(Box::new(f));
        self
    }

    /// Add to the toolbar. `f` draws after the built-in buttons, with the
    /// whole app to hand; change the graph through [`EditorApp::edit`] so the
    /// change is undoable and the preview keeps up.
    pub fn toolbar(mut self, f: impl FnMut(&mut egui::Ui, &mut EditorApp<N>) + 'static) -> Self {
        self.toolbar = Some(Box::new(f));
        self
    }

    /// Add an entry to every node's right-click menu. `f` runs with the
    /// node that was clicked when the entry is picked.
    pub fn node_menu(
        mut self,
        label: impl Into<String>,
        f: impl FnMut(&mut EditorApp<N>, NodeId) + 'static,
    ) -> Self {
        let label = label.into();
        self.editor.node_menu.push(label.clone());
        self.node_menu.push((label, Box::new(f)));
        self
    }

    /// Change the graph from outside the editor: undoably, and with the
    /// preview regenerated and the wires rerouted afterwards.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Graph<N>, &NodeLibrary) -> R) -> R {
        self.editor.checkpoint(&self.graph);
        let result = f(&mut self.graph, &self.library);
        self.after_edit();
        result
    }

    /// Show a message in the status bar.
    pub fn status(&mut self, message: impl Into<String>) {
        self.status.info(message);
    }

    /// Show an error in the status bar.
    pub fn status_error(&mut self, message: impl Into<String>) {
        self.status.error(message);
    }

    /// What the preview callback last produced.
    pub fn generated(&self) -> &Preview {
        &self.generated
    }

    /// Whether the graph has changed since it was last saved, loaded or
    /// started afresh.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    fn run_node_menu(&mut self, item: &str, node: NodeId) {
        let mut entries = std::mem::take(&mut self.node_menu);
        if let Some((_, f)) = entries.iter_mut().find(|(label, _)| label == item) {
            f(self, node);
        }
        // Anything added while the entries were out stays, after them.
        entries.append(&mut self.node_menu);
        self.node_menu = entries;
    }

    fn regenerate(&mut self) {
        if let Some(preview) = &self.preview {
            self.generated = preview(&self.graph, &self.library);
        }
    }
}

#[cfg(feature = "serde")]
impl<N> EditorApp<N>
where
    N: NodeData + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    /// Read and write the graph as JSON. Loaded graphs are repaired against the
    /// current library with [`Graph::validate`].
    pub fn json_files(self) -> Self {
        self.persistence(
            |graph| serde_json::to_string_pretty(graph).map_err(|e| e.to_string()),
            |text| serde_json::from_str::<Graph<N>>(text).map_err(|e| e.to_string()),
        )
    }
}

#[cfg(feature = "app")]
impl<N: NodeData + 'static> EditorApp<N> {
    /// Open the window and block until it closes.
    ///
    /// In a browser there is no window to open and nothing to block: the app
    /// is started on the page's `<canvas id="nodez">`, or on a canvas made to
    /// fill the page if there is none, and this returns straight away.
    pub fn run(mut self) -> eframe::Result {
        self.load_groups();
        self.regenerate();
        self.launch()
    }

    /// Hand the finished app to eframe, pinning its egui theme to the style.
    fn creator(self) -> eframe::AppCreator<'static> {
        let (theme, panel) = chrome(&self.editor.style);
        Box::new(move |cc| {
            // The canvas is painted from EditorStyle, not from egui's
            // theme, so pin egui to whichever the style is. Left to follow
            // the system, a light-mode desktop paints this chrome with
            // near-black text and the sidebar becomes unreadable.
            cc.egui_ctx.set_theme(theme);
            cc.egui_ctx.style_mut_of(theme, |style| {
                style.visuals.panel_fill = panel;
                style.visuals.window_fill = panel;
            });
            Ok(Box::new(self))
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn launch(self) -> eframe::Result {
        let title = self.title.clone();
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1480.0, 920.0])
                .with_min_inner_size([900.0, 560.0])
                .with_title(&title),
            ..Default::default()
        };
        eframe::run_native(&title, options, self.creator())
    }

    #[cfg(target_arch = "wasm32")]
    fn launch(self) -> eframe::Result {
        use wasm_bindgen_futures::wasm_bindgen::JsCast;

        let document = web_sys::window()
            .and_then(|window| window.document())
            .expect("a page to run in");
        if let Some(body) = document.body() {
            let _ = body.style().set_property("margin", "0");
        }
        document.set_title(&self.title);
        let canvas = match document.get_element_by_id("nodez") {
            Some(canvas) => canvas,
            None => {
                let canvas = document.create_element("canvas").expect("a canvas");
                canvas.set_id("nodez");
                if let Some(body) = document.body() {
                    let _ = body.append_child(&canvas);
                }
                canvas
            }
        };
        let canvas: web_sys::HtmlCanvasElement = canvas
            .dyn_into()
            .expect("#nodez to be a <canvas>");
        if canvas.style().get_property_value("width").is_ok_and(|w| w.is_empty()) {
            let style = canvas.style();
            let _ = style.set_property("display", "block");
            let _ = style.set_property("width", "100vw");
            let _ = style.set_property("height", "100vh");
        }
        let creator = self.creator();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = eframe::WebRunner::new()
                .start(canvas, eframe::WebOptions::default(), creator)
                .await
            {
                web_sys::console::error_1(&e);
            }
        });
        Ok(())
    }
}

#[cfg(feature = "app")]
impl<N: NodeData + 'static> eframe::App for EditorApp<N> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        EditorApp::ui(self, ui);
    }
}

impl<N: NodeData> EditorApp<N> {
    /// Draw the whole thing inside a `Ui`: the arrangement set with
    /// [`EditorApp::layout`], or [`EditorApp::default_layout`].
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        match self.layout.take() {
            Some(mut layout) => {
                self.laying_out = true;
                layout(ui, self);
                self.laying_out = false;
                self.layout = Some(layout);
            }
            None => self.default_layout(ui),
        }
    }

    /// The standard arrangement: toolbar along the top, status along the
    /// bottom, palette and inspector on the left, the preview on the right if
    /// there is one, and the canvas in what is left.
    ///
    /// A layout of your own is built from the same pieces —
    /// [`show_toolbar`](Self::show_toolbar), [`show_palette`](Self::show_palette),
    /// [`show_inspector`](Self::show_inspector), [`show_preview`](Self::show_preview),
    /// [`show_status`](Self::show_status) and [`show_canvas`](Self::show_canvas) —
    /// each of which draws into whatever `Ui` it is given.
    pub fn default_layout(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("nodez-toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| self.show_toolbar(ui));
            ui.add_space(3.0);
        });
        egui::Panel::bottom("nodez-status").show(ui, |ui| self.show_status(ui));
        egui::Panel::left("nodez-library")
            .default_size(260.0)
            .min_size(200.0)
            .max_size(400.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading("Nodes");
                ui.label(
                    RichText::new("Click to add, or press Shift+A in the canvas.")
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);

                let half = ui.available_height() * 0.5;
                egui::ScrollArea::vertical()
                    .id_salt("nodez-palette")
                    .max_height(if self.inspector_open { half } else { f32::INFINITY })
                    .show(ui, |ui| self.show_palette(ui));

                if self.inspector_open {
                    ui.separator();
                    self.show_inspector(ui);
                }
            });
        if self.preview.is_some() {
            egui::Panel::right("nodez-preview")
                .default_size(400.0)
                .min_size(280.0)
                .max_size(720.0)
                .show(ui, |ui| self.show_preview(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.show_canvas(ui));
    }

    /// The node editor itself, filling `ui`, and everything that follows from
    /// an edit in it: the preview regenerated, wires rerouted, groups made and
    /// opened, and the question asked before unsaved changes are lost.
    ///
    /// Every layout needs this one.
    pub fn show_canvas(&mut self, ui: &mut egui::Ui) {
        // A graph that arrives whole -- the one the app was built with, or one
        // just loaded -- has never been past the router. Route it before it is
        // drawn, rather than leaving it straight until the first edit.
        if self.route_next {
            self.route_next = false;
            self.apply_routing();
        }
        if self.frame_next {
            self.frame_next = false;
            let rect = ui.available_rect_before_wrap();
            self.editor.fit_to_graph(rect, &self.graph, &self.library);
        }
        let response = self.editor.show(ui, &self.library, &mut self.graph);
        if response.changed
            && let Some(preview) = &self.preview
        {
            self.generated = preview(&self.graph, &self.library);
        }
        // Keep the wires routed as the graph changes. Both of these land
        // once an edit or a drag is finished, not while one is in flight,
        // so this costs nothing per frame.
        let moved = response
            .actions
            .iter()
            .any(|a| matches!(a, EditorAction::NodesMoved(_)));
        self.modified |= response.changed || moved;
        if self.route_wires && (response.changed || moved) {
            self.apply_routing();
        }
        for action in response.actions {
            match action {
                EditorAction::ConnectionRejected(e) => self.status.error(e.to_string()),
                EditorAction::GroupSelection(nodes) => self.group_selection(&nodes),
                EditorAction::EnterGroup(id) => self.enter_group(id),
                EditorAction::LeaveGroup => self.leave_group(),
                EditorAction::NodeMenu { node, item } => self.run_node_menu(&item, node),
                _ => {}
            }
        }
        self.confirm_discard(ui);
    }

    /// The toolbar's buttons, laid out in whatever direction `ui` is — the
    /// default layout puts them in a row. Anything added with
    /// [`EditorApp::toolbar`] comes last.
    pub fn show_toolbar(&mut self, ui: &mut egui::Ui) {
        if ui.button("New").clicked() {
            self.unless_unsaved(Discard::New);
        }
        ui.separator();

        let undo = ui.add_enabled(self.editor.state.can_undo(), egui::Button::new("Undo"));
        if undo.on_hover_text("Ctrl+Z").clicked() && self.editor.undo(&mut self.graph) {
            self.after_edit();
        }
        let redo = ui.add_enabled(self.editor.state.can_redo(), egui::Button::new("Redo"));
        if redo.on_hover_text("Ctrl+Shift+Z").clicked() && self.editor.redo(&mut self.graph) {
            self.after_edit();
        }
        ui.separator();

        if self.persistence.is_some() {
            if ui.button("Load").clicked() {
                self.unless_unsaved(Discard::Load);
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .desired_width(240.0)
                    .hint_text("graph.json"),
            );
            ui.separator();
        }

        if ui.button("Auto layout").clicked() {
            self.auto_layout();
        }
        if ui.button("Frame all").clicked() {
            self.frame_next = true;
        }
        ui.checkbox(&mut self.editor.style.show_grid, "Grid");
        if ui
            .checkbox(&mut self.route_wires, "Route")
            .on_hover_text("Steer wires around the nodes in their way")
            .changed()
        {
            self.apply_routing();
        }
        // Only the default layout has an inspector to hide; a layout of your
        // own shows it or not.
        if self.layout.is_none() && !self.laying_out {
            ui.checkbox(&mut self.inspector_open, "Inspector");
        }
        // Where you are, when you are inside a group. Clicking a step goes
        // back out to it.
        if !self.inside.is_empty() {
            ui.separator();
            let mut back_to = None;
            if ui.link(&self.path).clicked() {
                back_to = Some(0);
            }
            let names: Vec<String> = self
                .inside
                .iter()
                .skip(1)
                .map(|open| self.library.expect(open.template).label.clone())
                .chain(std::iter::once(
                    self.inside
                        .last()
                        .map(|open| self.library.expect(open.template).label.clone())
                        .unwrap_or_default(),
                ))
                .collect();
            for (depth, name) in names.iter().enumerate() {
                ui.label(RichText::new("\u{203A}").weak());
                if depth + 1 == names.len() {
                    ui.label(RichText::new(name).strong());
                } else if ui.link(name).clicked() {
                    back_to = Some(depth + 1);
                }
            }
            if let Some(depth) = back_to {
                while self.inside.len() > depth {
                    self.leave_group();
                }
            }
        }
        ui.checkbox(&mut self.editor.style.show_missing_inputs, "Unfilled")
            .on_hover_text("Mark inputs that have to be wired and are not");

        egui::ComboBox::from_id_salt("nodez-scroll-mode")
            .width(96.0)
            .selected_text(match self.editor.scroll_mode {
                ScrollMode::Auto => "Scroll: auto",
                ScrollMode::Pan => "Scroll: pan",
                ScrollMode::Zoom => "Scroll: zoom",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.editor.scroll_mode,
                    ScrollMode::Auto,
                    "Auto (pan on trackpad)",
                );
                ui.selectable_value(&mut self.editor.scroll_mode, ScrollMode::Pan, "Pan");
                ui.selectable_value(&mut self.editor.scroll_mode, ScrollMode::Zoom, "Zoom");
            })
            .response
            .on_hover_text("What a bare scroll does. Ctrl+scroll and pinch always zoom.");

        if let Some(mut extra) = self.toolbar.take() {
            ui.separator();
            extra(ui, self);
            self.toolbar = Some(extra);
        }
    }

    /// The last message, the zoom, and a reminder of the keys.
    pub fn show_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let color = if self.status.error {
                Color32::from_rgb(0xE8, 0x7C, 0x6A)
            } else {
                Color32::from_gray(0xA0)
            };
            ui.label(RichText::new(&self.status.message).color(color).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", self.editor.state.zoom * 100.0))
                        .monospace()
                        .small()
                        .weak(),
                );
                ui.separator();
                ui.label(
                    RichText::new(
                        "LMB select / box  \u{2022}  MMB or 2-finger pan  \
                         \u{2022}  wheel / ctrl+scroll zoom  \u{2022}  Shift+A add  \
                         \u{2022}  G grab  \u{2022}  X delete  \u{2022}  H collapse  \
                         \u{2022}  M mute  \u{2022}  Ctrl+drag cut  \
                         \u{2022}  Ctrl+G group  \u{2022}  double-click to open",
                    )
                    .small()
                    .weak(),
                );
            });
        });
    }

    /// What the preview callback generated, with Copy and Write buttons.
    /// Draws nothing useful without [`EditorApp::preview`].
    pub fn show_preview(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Generated");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(self.generated.text.clone());
                    self.status.info("Copied to the clipboard.");
                }
                if self.persistence.is_some() && ui.button("Write").clicked() {
                    self.write_preview();
                }
            });
        });

        for problem in &self.generated.problems {
            ui.colored_label(
                Color32::from_rgb(0xE0, 0xA0, 0x40),
                format!("\u{26A0} {problem}"),
            );
        }

        ui.add_space(4.0);
        egui::ScrollArea::both()
            .id_salt("nodez-preview-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&self.generated.text)
                            .monospace()
                            .color(Color32::from_rgb(0xCE, 0xD6, 0xC8)),
                    )
                    .selectable(true)
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
    }

    /// A button for every node kind in the library, by category. Clicking
    /// one adds that node in the first free space near the middle of the view.
    pub fn show_palette(&mut self, ui: &mut egui::Ui) {
        let dark = self.editor.style.background.r() < 128;
        let mut chosen = None;
        let categories: Vec<String> = self.library.categories().map(str::to_owned).collect();
        for category in categories {
            let tint = self
                .library
                .category_color(&category)
                .unwrap_or(Color32::GRAY);
            egui::CollapsingHeader::new(RichText::new(&category).color(readable(tint, dark)))
                .default_open(true)
                .show(ui, |ui| {
                    for (id, template) in self.library.in_category(&category) {
                        let button = ui.add(
                            egui::Button::new(&template.label)
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        );
                        let button = if template.description.is_empty() {
                            button
                        } else {
                            button.on_hover_text(&template.description)
                        };
                        if button.clicked() {
                            chosen = Some(id);
                        }
                    }
                });
        }
        if let Some(template) = chosen {
            self.add_from_palette(template);
        }
    }

    /// Add a node where there is room for it. There is no cursor to place it
    /// under, so it goes in the first open space near the middle of the view
    /// rather than always the same spot.
    fn add_from_palette(&mut self, template: TemplateId) {
        self.editor.checkpoint(&self.graph);
        let id = self
            .graph
            .add_node(&self.library, template, self.editor.view_center());
        if let Some((preferred, size)) = self.graph.node(id).map(|node| {
            (
                node.position,
                node_size(&self.graph, &self.library, node, &self.editor.style),
            )
        }) {
            let free = free_position(
                &self.graph,
                &self.library,
                &self.editor.style,
                size,
                preferred - size / 2.0,
                Some(id),
            );
            if let Some(node) = self.graph.node_mut(id) {
                node.position = free;
            }
        }
        self.editor.state.select_only(id);
        self.regenerate();
        self.modified = true;
        self.status.info("Added a node.");
    }

    /// The traversal API, shown live for whichever node is active.
    pub fn show_inspector(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.heading("Graph");
        ui.label(format!(
            "{} nodes, {} links, {} component(s)",
            self.graph.node_count(),
            self.graph.connection_count(),
            self.graph.components().len(),
        ));
        if !self.generated.contributing.is_empty() {
            ui.label(
                RichText::new(format!(
                    "{} of {} nodes feed the output",
                    self.generated.contributing.len(),
                    self.graph.node_count()
                ))
                .small()
                .weak(),
            );
        }
        let missing = self.graph.missing_inputs(&self.library);
        if !missing.is_empty() {
            let nodes: HashSet<NodeId> = missing.iter().map(|socket| socket.node).collect();
            ui.label(
                RichText::new(format!(
                    "{} input(s) still to wire, on {} node(s)",
                    missing.len(),
                    nodes.len()
                ))
                .small()
                .color(self.editor.style.missing_input),
            );
        }

        ui.add_space(6.0);
        let Some(active) = self
            .editor
            .state
            .active
            .filter(|id| self.graph.contains_node(*id))
        else {
            ui.label(RichText::new("Select a node to inspect it.").weak());
            return;
        };

        let title = self.graph.node(active).map(|n| n.title.clone()).unwrap_or_default();
        ui.label(RichText::new(title).strong());

        // What is wrong with this node, in words. The canvas says which node
        // and which row; this is what says what to do about it.
        let unwired: Vec<String> = self
            .graph
            .template_of(&self.library, active)
            .into_iter()
            .flat_map(|template| template.inputs.iter())
            .filter(|socket| self.graph.is_input_missing(&self.library, active, &socket.name))
            .map(|socket| socket.display().to_owned())
            .collect();
        if !unwired.is_empty() {
            ui.label(
                RichText::new(format!("Needs a wire into: {}", unwired.join(", ")))
                    .small()
                    .color(self.editor.style.missing_input),
            );
        }

        egui::ScrollArea::vertical()
            .id_salt("nodez-inspector")
            .show(ui, |ui| {
                self.node_list(ui, "Depends on (direct)", &self.graph.predecessors(active));
                self.node_list(
                    ui,
                    "Depends on (all)",
                    &self.graph.ancestors(active).collect::<Vec<_>>(),
                );
                self.node_list(ui, "Used by (direct)", &self.graph.successors(active));
                self.node_list(
                    ui,
                    "Used by (all)",
                    &self.graph.descendants(active).collect::<Vec<_>>(),
                );

                ui.add_space(6.0);
                ui.collapsing("Evaluation order", |ui| match self.graph.topological_order() {
                    Ok(order) => {
                        let dim = !self.generated.contributing.is_empty();
                        for (i, id) in order.iter().enumerate() {
                            let Some(node) = self.graph.node(*id) else {
                                continue;
                            };
                            let text = format!("{:>2}. {}", i + 1, node.title);
                            let reaches =
                                !dim || self.generated.contributing.contains(id);
                            ui.label(if reaches {
                                RichText::new(text).monospace().small()
                            } else {
                                RichText::new(text).monospace().small().weak()
                            });
                        }
                    }
                    Err(e) => {
                        ui.colored_label(Color32::LIGHT_RED, e.to_string());
                    }
                });
            });
    }

    fn node_list(&self, ui: &mut egui::Ui, heading: &str, ids: &[NodeId]) {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("{heading}: {}", ids.len()))
                .small()
                .weak(),
        );
        for id in ids {
            if let Some(node) = self.graph.node(*id) {
                ui.label(RichText::new(format!("  \u{2022} {}", node.title)).small());
            }
        }
    }

    // ------------------------------------------------------------- groups

    /// Read every group kept beside the app into the library.
    fn load_groups(&mut self) {
        let Some(dir) = self.groups_dir.clone() else {
            return;
        };
        let mut problems = Vec::new();
        let mut found = Vec::new();
        for (name, text) in files::read_json_dir(&dir) {
            match text {
                Ok(text) => found.push((name, text)),
                Err(e) => problems.push((name, crate::group::GroupError::Unreadable(e))),
            }
        }
        problems.extend(self.library.read_groups(found));
        if !problems.is_empty() {
            self.status.error(format!(
                "{} group(s) in {dir} could not be read: {}",
                problems.len(),
                problems
                    .iter()
                    .map(|(name, why)| format!("{name}: {why}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    /// Write a group out, so it is there the next time the app starts.
    fn keep_group(&mut self, template: crate::template::TemplateId) {
        let Some(dir) = self.groups_dir.clone() else {
            return;
        };
        let id = self.library.expect(template).id.clone();
        let written = self
            .library
            .write_group(template)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                let path = format!("{}/{id}.json", dir.trim_end_matches('/'));
                files::write(&path, &text)
            });
        match written {
            Ok(path) => self.status.info(format!("Wrote {path}")),
            Err(e) => self.status.error(format!("Could not write {id}: {e}")),
        }
    }

    /// Make a group of the selection, and leave one node in its place.
    ///
    /// The group's id has to be unique in the library and there is nobody to
    /// ask for one, so it is numbered. Renaming the node renames what you see;
    /// the id is only ever how the file refers to it.
    fn group_selection(&mut self, nodes: &[NodeId]) {
        let chosen: std::collections::HashSet<NodeId> = nodes.iter().copied().collect();
        let mut n = 1;
        let id = loop {
            let id = format!("group_{n}");
            if self.library.id(&id).is_none() {
                break id;
            }
            n += 1;
        };
        let label = format!("Group {n}");

        // The editor's graph carries whatever payload the app was built with;
        // a group's interior is always the dynamic one, so the selection goes
        // across by name and comes back the same way.
        let mut flat = self.graph.convert::<crate::graph::DynNode>(&self.library);
        match crate::group::make_group(&mut flat, &mut self.library, &chosen, &id, &label, "Group")
        {
            Ok(made) => {
                self.editor.checkpoint(&self.graph);
                self.graph = flat.convert::<N>(&self.library);
                self.editor.state.clear_selection();
                self.editor.state.selection.insert(made);
                self.editor.state.active = Some(made);
                self.status.info(format!("Grouped {} nodes as {label}", chosen.len()));
                if let Some(template) = self.graph.node(made).map(|node| node.template) {
                    self.keep_group(template);
                }
                self.after_edit();
            }
            Err(e) => self.status.error(e.to_string()),
        }
    }

    /// Back out of every open group, putting each one's edits into the
    /// library on the way.
    fn leave_all_groups(&mut self) {
        while !self.inside.is_empty() {
            self.leave_group();
        }
    }

    /// Open what is inside a group node.
    fn enter_group(&mut self, at: NodeId) {
        let Some(template) = self.graph.node(at).map(|node| node.template) else {
            return;
        };
        // Most nodes are not groups; double-clicking one of those is not
        // worth saying anything about.
        let Some(inside) = self.library.take_group(template) else {
            return;
        };
        let name = self.library.expect(template).label.clone();
        let opened = inside.convert::<N>(&self.library);
        let outer = std::mem::replace(&mut self.graph, opened);
        let view = View {
            pan: self.editor.state.pan,
            zoom: self.editor.state.zoom,
        };
        self.inside.push(Opened {
            template,
            outer,
            view,
        });
        // Undo walks back through one graph's edits, and this is another graph.
        self.editor.clear_history();
        self.editor.state.clear_selection();
        self.frame_next = true;
        self.route_next = true;
        self.status.info(format!("Editing {name}"));
    }

    /// Put the group being edited back, and return to what it was opened from.
    fn leave_group(&mut self) {
        let Some(Opened {
            template,
            outer,
            view,
        }) = self.inside.pop()
        else {
            return;
        };
        let (id, label, category) = {
            let was = self.library.expect(template);
            (was.id.clone(), was.label.clone(), was.category.clone())
        };
        let edited = std::mem::replace(&mut self.graph, outer)
            .convert::<crate::graph::DynNode>(&self.library);

        // Registering it again is what re-reads the interface off the pads.
        // A socket that went takes its wires with it, which is what `validate`
        // is for -- and what every other instance of the group needs too.
        match self
            .library
            .register_group(&id, &label, &category, edited)
        {
            // And written back, or the edits last only as long as the window.
            Ok(template) => self.keep_group(template),
            Err(e) => self.status.error(format!("{label}: {e}")),
        }
        let repairs = self.graph.validate(&self.library);
        if !repairs.is_clean() {
            self.status.info(format!(
                "{} wire(s) no longer had a socket to meet",
                repairs.removed_connections
            ));
        }
        self.editor.clear_history();
        self.editor.state.clear_selection();
        self.editor.state.pan = view.pan;
        self.editor.state.zoom = view.zoom;
        self.after_edit();
    }

    /// What has to happen after the graph is rearranged from outside the
    /// editor: the preview is stale and the wires are bent around nodes that
    /// have gone.
    fn after_edit(&mut self) {
        self.modified = true;
        if let Some(preview) = &self.preview {
            self.generated = preview(&self.graph, &self.library);
        }
        self.apply_routing();
    }

    /// Route the wires, or straighten them, to match the Route box.
    fn apply_routing(&mut self) {
        if !self.route_wires {
            self.graph.clear_routing();
            return;
        }
        let library = &self.library;
        let style = &self.editor.style;
        let _ = crate::layout::route_links(
            &mut self.graph,
            &crate::layout::RouteOptions {
                curvature: style.wire_curvature,
                min_curve: style.wire_min_curve,
                max_curve: style.wire_max_curve,
                ..crate::layout::RouteOptions::default()
            },
            |graph, node| node_size(graph, library, node, style),
            |graph, socket, kind, slot| {
                let node = graph.node(socket.node)?;
                crate::socket_anchor(graph, library, node, style, kind, &socket.socket, slot)
            },
        );
    }

    fn auto_layout(&mut self) {
        self.editor.checkpoint(&self.graph);
        let library = &self.library;
        let style = &self.editor.style;
        let result = crate::layout::layered(
            &mut self.graph,
            &crate::layout::LayoutOptions {
                column_gap: 70.0,
                row_gap: 22.0,
                ..crate::layout::LayoutOptions::default()
            },
            |graph, node| node_size(graph, library, node, style),
        );
        if result.is_ok() {
            // Columns alone still let a long wire pass under everything
            // between its ends.
            self.apply_routing();
        }
        match result {
            Ok(()) => {
                self.modified = true;
                self.frame_next = true;
                self.status.info("Laid out by dependency depth.");
            }
            Err(e) => self.status.error(e.to_string()),
        }
    }

    fn save(&mut self) {
        if self.persistence.is_none() {
            return;
        }
        // The file is the whole graph, not whichever group is open in it.
        self.leave_all_groups();
        self.graph.set_view(Some(View {
            pan: self.editor.state.pan,
            zoom: self.editor.state.zoom,
        }));
        // Names for the template ids on the way out, so the file can be read
        // back by a library with a different set of templates in it.
        self.graph.name_templates(&self.library);
        let Some(persistence) = &self.persistence else {
            return;
        };
        let written = (persistence.save)(&self.graph)
            .and_then(|text| files::write(&self.path, &text));
        match written {
            Ok(path) => {
                self.modified = false;
                self.status.info(format!("Saved {path}"));
            }
            Err(e) => self.status.error(format!("Save failed: {e}")),
        }
    }

    /// Start a new, empty graph.
    fn new_graph(&mut self) {
        self.editor.checkpoint(&self.graph);
        self.graph.clear();
        self.editor.state.clear_selection();
        self.regenerate();
        self.modified = false;
        self.frame_next = true;
        self.status.info("New graph.");
    }

    /// Do `what` now if nothing unsaved would be lost, or ask first.
    fn unless_unsaved(&mut self, what: Discard) {
        if self.modified {
            self.confirm = Some(what);
        } else {
            self.go_ahead(what);
        }
    }

    fn go_ahead(&mut self, what: Discard) {
        // A new or loaded graph replaces the document, not the group open in
        // it -- which would otherwise become that group's insides on the way
        // back out.
        if !matches!(what, Discard::Close) {
            self.leave_all_groups();
        }
        match what {
            Discard::New => self.new_graph(),
            Discard::Load => self.load(),
            Discard::Close => self.may_close = true,
        }
    }

    /// Ask before throwing away unsaved changes, and catch the window being
    /// closed with some.
    fn confirm_discard(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.may_close && self.modified {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Discard::Close);
        }
        let Some(what) = self.confirm else {
            return;
        };
        let doing = match what {
            Discard::New => "Starting a new graph".to_owned(),
            Discard::Load => format!("Loading {}", self.path),
            Discard::Close => "Closing the window".to_owned(),
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::new("nodez-discard")).show(&ctx, |ui| {
            ui.set_width(340.0);
            ui.heading("Unsaved changes");
            ui.add_space(4.0);
            ui.label(format!("{doing} will lose the changes made since the graph was last saved."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if self.persistence.is_some() && ui.button("Save first").clicked() {
                    answer = Some(true);
                }
                if ui.button("Discard changes").clicked() {
                    answer = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm = None;
                }
            });
        });
        if modal.should_close() {
            self.confirm = None;
        }
        let Some(save_first) = answer else {
            return;
        };
        self.confirm = None;
        if save_first {
            self.save();
            if self.modified {
                return; // the save failed, and says so in the status bar
            }
        }
        self.go_ahead(what);
        if matches!(what, Discard::Close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn load(&mut self) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let loaded = files::read(&self.path).and_then(|text| (persistence.load)(&text));
        match loaded {
            Ok(mut graph) => {
                let repairs = graph.validate(&self.library);
                self.graph = graph;
                self.editor.clear_history();
                self.editor.state.clear_selection();
                self.modified = false;
                self.regenerate();
                self.open_view();
                self.route_next = true;
                if repairs.is_clean() {
                    self.status.info(format!("Loaded {}", self.path));
                } else {
                    self.status.error(format!(
                        "Loaded with repairs: dropped {} node(s) and {} link(s).",
                        repairs.removed_nodes, repairs.removed_connections
                    ));
                }
            }
            Err(e) => self.status.error(format!("Load failed: {e}")),
        }
    }

    /// Write the generated text beside the graph file.
    fn write_preview(&mut self) {
        let path = std::path::PathBuf::from(&self.path).with_extension(&self.preview_extension);
        match files::export(&path.to_string_lossy(), &self.generated.text) {
            Ok(path) => self.status.info(format!("Wrote {path}")),
            Err(e) => self.status.error(format!("Write failed: {e}")),
        }
    }
}

impl<N: NodeData> std::fmt::Debug for EditorApp<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorApp")
            .field("title", &self.title)
            .field("path", &self.path)
            .field("nodes", &self.graph.node_count())
            .field("has_preview", &self.preview.is_some())
            .field("has_persistence", &self.persistence.is_some())
            .finish()
    }
}

/// The egui theme and panel color that go with an editor style.
///
/// The panel sits one step off the canvas, the way Blender's editors do, so
/// both come from the one background color rather than being picked twice.
fn chrome(style: &EditorStyle) -> (egui::Theme, Color32) {
    let background = style.background;
    if background.r() < 128 {
        (egui::Theme::Dark, shift(background, 0x0E))
    } else {
        (egui::Theme::Light, shift(background, -0x0E))
    }
}

fn shift(color: Color32, by: i32) -> Color32 {
    let channel = |c: u8| (i32::from(c) + by).clamp(0, 255) as u8;
    Color32::from_rgb(channel(color.r()), channel(color.g()), channel(color.b()))
}

/// Nudge a category tint away from the panel behind it, so one tint reads on
/// either theme.
fn readable(tint: Color32, dark: bool) -> Color32 {
    shift(tint, if dark { 0x50 } else { -0x50 })
}
