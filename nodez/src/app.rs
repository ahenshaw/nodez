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
//! [`EditorApp::ui`] draws the same thing inside a `Ui` you already have, for
//! embedding it in a larger application.

use std::collections::HashSet;

use egui::{Color32, RichText};

use crate::graph::{DynNode, Graph, NodeData, NodeId};
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
    show_inspector: bool,
    /// Whether wires are steered around the nodes in their way.
    route_wires: bool,
    /// Frame the graph once the editor's rect is known.
    frame_next: bool,
    /// A template the palette asked to add, applied after the panel closes.
    pending_add: Option<TemplateId>,
    preview_extension: String,
}

impl<N: NodeData> EditorApp<N> {
    pub fn new(library: NodeLibrary) -> Self {
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
            show_inspector: true,
            route_wires: true,
            frame_next: true,
            pending_add: None,
            preview_extension: "out".to_owned(),
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
        self
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
    pub fn run(mut self) -> eframe::Result {
        self.regenerate();
        let title = self.title.clone();
        let (theme, panel) = chrome(&self.editor.style);
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1480.0, 920.0])
                .with_min_inner_size([900.0, 560.0])
                .with_title(&title),
            ..Default::default()
        };
        eframe::run_native(
            &title,
            options,
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
            }),
        )
    }
}

#[cfg(feature = "app")]
impl<N: NodeData + 'static> eframe::App for EditorApp<N> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        EditorApp::ui(self, ui);
    }
}

impl<N: NodeData> EditorApp<N> {
    /// Draw the whole thing inside a `Ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.top_bar(ui);
        self.status_bar(ui);
        self.side_panel(ui);
        if self.preview.is_some() {
            self.preview_panel(ui);
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
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
            if self.route_wires && (response.changed || moved) {
                self.apply_routing();
            }
            for action in &response.actions {
                if let EditorAction::ConnectionRejected(e) = action {
                    self.status.error(e.to_string());
                }
            }
        });

        // Adding from the palette needs the graph, which the panel borrowed.
        // There is no cursor to place it under, so it goes in the first open
        // space near the middle of the view rather than always the same spot.
        if let Some(template) = self.pending_add.take() {
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
            self.status.info("Added a node.");
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("nodez-toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if ui.button("New").clicked() {
                    self.graph.clear();
                    self.editor.state.clear_selection();
                    self.regenerate();
                    self.frame_next = true;
                    self.status.info("New graph.");
                }
                ui.separator();

                if self.persistence.is_some() {
                    if ui.button("Load").clicked() {
                        self.load();
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
                ui.checkbox(&mut self.show_inspector, "Inspector");

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

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{:.0}%", self.editor.state.zoom * 100.0))
                            .monospace()
                            .weak(),
                    );
                    ui.label(RichText::new("zoom").weak());
                });
            });
            ui.add_space(3.0);
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
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
                    .max_height(if self.show_inspector { half } else { f32::INFINITY })
                    .show(ui, |ui| self.palette(ui));

                if self.show_inspector {
                    ui.separator();
                    self.inspector(ui);
                }
            });
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        let dark = self.editor.style.background.r() < 128;
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
                            self.pending_add = Some(id);
                        }
                    }
                });
        }
    }

    /// The traversal API, shown live for whichever node is active.
    fn inspector(&mut self, ui: &mut egui::Ui) {
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

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("nodez-preview")
            .default_size(400.0)
            .min_size(280.0)
            .max_size(720.0)
            .show(ui, |ui| {
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
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("nodez-status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let color = if self.status.error {
                    Color32::from_rgb(0xE8, 0x7C, 0x6A)
                } else {
                    Color32::from_gray(0xA0)
                };
                ui.label(RichText::new(&self.status.message).color(color).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(
                            "LMB select / box  \u{2022}  MMB or 2-finger pan  \
                             \u{2022}  wheel / ctrl+scroll zoom  \u{2022}  Shift+A add  \
                             \u{2022}  G grab  \u{2022}  X delete  \u{2022}  H collapse  \
                             \u{2022}  M mute  \u{2022}  Ctrl+drag cut",
                        )
                        .small()
                        .weak(),
                    );
                });
            });
        });
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
            |graph, socket, kind| {
                let node = graph.node(socket.node)?;
                crate::socket_anchor(graph, library, node, style, kind, &socket.socket)
            },
        );
    }

    fn auto_layout(&mut self) {
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
                self.frame_next = true;
                self.status.info("Laid out by dependency depth.");
            }
            Err(e) => self.status.error(e.to_string()),
        }
    }

    fn save(&mut self) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let written = (persistence.save)(&self.graph)
            .and_then(|text| std::fs::write(&self.path, text).map_err(|e| e.to_string()));
        match written {
            Ok(()) => self.status.info(format!("Saved {}", self.path)),
            Err(e) => self.status.error(format!("Save failed: {e}")),
        }
    }

    fn load(&mut self) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let loaded = std::fs::read_to_string(&self.path)
            .map_err(|e| e.to_string())
            .and_then(|text| (persistence.load)(&text));
        match loaded {
            Ok(mut graph) => {
                let repairs = graph.validate(&self.library);
                self.graph = graph;
                self.editor.state.clear_selection();
                self.regenerate();
                self.frame_next = true;
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
        match std::fs::write(&path, &self.generated.text) {
            Ok(()) => self.status.info(format!("Wrote {}", path.display())),
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
