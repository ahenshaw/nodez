//! A visual generator for container-stack config files, built on `nodez`.
//!
//! The left panel adds nodes and inspects the graph, the middle is the editor,
//! and the right shows the config the graph currently describes. Editing
//! anything regenerates the document immediately.

#![forbid(unsafe_code)]

mod domain;
mod generate;
mod sample;
mod yaml;

use std::path::PathBuf;

use egui::{Color32, RichText};
use nodez::{
    EditorAction, EditorStyle, Graph, LayoutOptions, NodeEditor, NodeId, ScrollMode, node_size,
};

use domain::Domain;
use generate::Generated;

const WINDOW_TITLE: &str = "nodez \u{2014} stack config editor";

fn main() -> eframe::Result {
    // `--print` generates the sample stack's config and exits, so the emitter
    // can be exercised without a display.
    if std::env::args().any(|arg| arg == "--print") {
        let domain = Domain::new();
        let style = EditorStyle::blender_dark();
        let graph = sample::build(&domain, &style);
        let generated = generate::generate(&graph, &domain);
        for problem in &generated.problems {
            eprintln!("warning: {problem}");
        }
        print!("{}", generated.text);
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title(WINDOW_TITLE),
        ..Default::default()
    };
    eframe::run_native(
        "nodez-demo",
        options,
        Box::new(|cc| Ok(Box::new(DemoApp::new(cc)))),
    )
}

struct DemoApp {
    domain: Domain,
    graph: Graph,
    editor: NodeEditor,
    generated: Generated,
    path: String,
    status: Status,
    show_inspector: bool,
    /// Frame the graph once the editor's rect is known.
    frame_next: bool,
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

impl DemoApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.all_styles_mut(|style| {
            style.visuals.panel_fill = Color32::from_rgb(0x2B, 0x2B, 0x2B);
            style.visuals.window_fill = Color32::from_rgb(0x2B, 0x2B, 0x2B);
        });

        let domain = Domain::new();
        let editor = NodeEditor::with_style(EditorStyle::blender_dark());
        let graph = sample::build(&domain, &editor.style);
        let generated = generate::generate(&graph, &domain);

        Self {
            domain,
            graph,
            editor,
            generated,
            path: default_path().to_string_lossy().into_owned(),
            status: Status::default(),
            show_inspector: true,
            frame_next: true,
        }
    }

    fn regenerate(&mut self) {
        self.generated = generate::generate(&self.graph, &self.domain);
    }

    fn auto_layout(&mut self) {
        let library = &self.domain.library;
        let style = &self.editor.style;
        let result = nodez::layered(
            &mut self.graph,
            &LayoutOptions {
                column_gap: 70.0,
                row_gap: 22.0,
                ..LayoutOptions::default()
            },
            |graph, node| node_size(graph, library, node, style).y,
        );
        match result {
            Ok(()) => {
                self.frame_next = true;
                self.status.info("Laid out by dependency depth.");
            }
            Err(e) => self.status.error(e.to_string()),
        }
    }

    fn save(&mut self) {
        match serde_json::to_string_pretty(&self.graph)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&self.path, json).map_err(|e| e.to_string()))
        {
            Ok(()) => self.status.info(format!("Saved {}", self.path)),
            Err(e) => self.status.error(format!("Save failed: {e}")),
        }
    }

    fn load(&mut self) {
        let loaded = std::fs::read_to_string(&self.path)
            .map_err(|e| e.to_string())
            .and_then(|json| serde_json::from_str::<Graph>(&json).map_err(|e| e.to_string()));
        match loaded {
            Ok(mut graph) => {
                // The file may predate a change to the library, so repair it
                // against the templates we actually have.
                let repairs = graph.validate(&self.domain.library);
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

    fn write_config(&mut self) {
        let path = PathBuf::from(&self.path).with_extension("yaml");
        match std::fs::write(&path, &self.generated.text) {
            Ok(()) => self.status.info(format!("Wrote {}", path.display())),
            Err(e) => self.status.error(format!("Write failed: {e}")),
        }
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.top_bar(ui);
        self.status_bar(ui);
        self.side_panel(ui);
        self.config_panel(ui);

        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.frame_next {
                self.frame_next = false;
                let rect = ui.available_rect_before_wrap();
                self.editor
                    .fit_to_graph(rect, &self.graph, &self.domain.library);
            }

            let response = self.editor.show(ui, &self.domain.library, &mut self.graph);

            if response.changed {
                self.regenerate();
            }
            for action in &response.actions {
                if let EditorAction::ConnectionRejected(e) = action {
                    self.status.error(e.to_string());
                }
            }
        });
    }
}

impl DemoApp {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if ui.button("New").clicked() {
                    self.graph = Graph::new();
                    self.graph.add_node(
                        &self.domain.library,
                        self.domain.templates.stack,
                        egui::pos2(0.0, 0.0),
                    );
                    self.editor.state.clear_selection();
                    self.regenerate();
                    self.frame_next = true;
                    self.status.info("New graph.");
                }
                if ui.button("Sample").clicked() {
                    self.graph = sample::build(&self.domain, &self.editor.style);
                    self.editor.state.clear_selection();
                    self.regenerate();
                    self.frame_next = true;
                    self.status.info("Loaded the sample stack.");
                }
                ui.separator();

                if ui.button("Load").clicked() {
                    self.load();
                }
                if ui.button("Save").clicked() {
                    self.save();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.path)
                        .desired_width(260.0)
                        .hint_text("graph.json"),
                );
                if ui
                    .button("Write config")
                    .on_hover_text("Write the generated YAML next to the graph file")
                    .clicked()
                {
                    self.write_config();
                }
                ui.separator();

                if ui.button("Auto layout").clicked() {
                    self.auto_layout();
                }
                if ui.button("Frame all").clicked() {
                    self.frame_next = true;
                }
                ui.checkbox(&mut self.editor.style.show_grid, "Grid");
                ui.checkbox(&mut self.show_inspector, "Inspector");

                // Whether a bare scroll pans or zooms cannot always be decided
                // from the event alone, so let the user say.
                egui::ComboBox::from_id_salt("scroll-mode")
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
        egui::Panel::left("library")
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

                egui::ScrollArea::vertical()
                    .id_salt("library-scroll")
                    .max_height(ui.available_height() * 0.5)
                    .show(ui, |ui| {
                        self.library_list(ui);
                    });

                if self.show_inspector {
                    ui.separator();
                    self.inspector(ui);
                }
            });
    }

    fn library_list(&mut self, ui: &mut egui::Ui) {
        let categories: Vec<String> = self
            .domain
            .library
            .categories()
            .map(str::to_owned)
            .collect();
        let mut to_add = None;

        for category in categories {
            let color = self
                .domain
                .library
                .category_color(&category)
                .unwrap_or(Color32::GRAY);
            egui::CollapsingHeader::new(RichText::new(&category).color(lighten(color)))
                .default_open(true)
                .show(ui, |ui| {
                    for (id, template) in self.domain.library.in_category(&category) {
                        let button = ui.add(
                            egui::Button::new(&template.label)
                                .fill(Color32::from_rgb(0x38, 0x38, 0x38))
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        );
                        let button = if template.description.is_empty() {
                            button
                        } else {
                            button.on_hover_text(&template.description)
                        };
                        if button.clicked() {
                            to_add = Some(id);
                        }
                    }
                });
        }

        if let Some(template) = to_add {
            // Drop new nodes in the middle of the current view.
            let center = egui::pos2(
                -self.editor.state.pan.x + 200.0,
                -self.editor.state.pan.y + 150.0,
            );
            let id = self
                .graph
                .add_node(&self.domain.library, template, center);
            self.editor.state.select_only(id);
            self.regenerate();
            self.status.info("Added a node.");
        }
    }

    /// Shows off the traversal API: order, dependencies and dependents.
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.heading("Graph");
        ui.label(format!(
            "{} nodes, {} links, {} component(s)",
            self.graph.node_count(),
            self.graph.connection_count(),
            self.graph.components().len(),
        ));

        let contributing = self.generated.contributing.len();
        ui.label(
            RichText::new(format!(
                "{contributing} of {} nodes feed the output",
                self.graph.node_count()
            ))
            .small()
            .weak(),
        );

        ui.add_space(6.0);
        let Some(active) = self.editor.state.active.filter(|id| self.graph.contains_node(*id))
        else {
            ui.label(RichText::new("Select a node to inspect it.").weak());
            return;
        };

        let title = self
            .graph
            .node(active)
            .map(|n| n.title.clone())
            .unwrap_or_default();
        ui.label(RichText::new(title).strong());

        egui::ScrollArea::vertical()
            .id_salt("inspector-scroll")
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
                ui.collapsing("Evaluation order", |ui| {
                    match self.graph.topological_order() {
                        Ok(order) => {
                            for (i, id) in order.iter().enumerate() {
                                let Some(node) = self.graph.node(*id) else {
                                    continue;
                                };
                                let text = format!("{:>2}. {}", i + 1, node.title);
                                let reachable = self.generated.contributing.contains(id);
                                ui.label(if reachable {
                                    RichText::new(text).monospace().small()
                                } else {
                                    RichText::new(text).monospace().small().weak()
                                });
                            }
                        }
                        Err(e) => {
                            ui.colored_label(Color32::LIGHT_RED, e.to_string());
                        }
                    }
                });
            });
    }

    fn node_list(&self, ui: &mut egui::Ui, heading: &str, ids: &[NodeId]) {
        ui.add_space(4.0);
        ui.label(RichText::new(format!("{heading}: {}", ids.len())).small().weak());
        for id in ids {
            if let Some(node) = self.graph.node(*id) {
                ui.label(RichText::new(format!("  \u{2022} {}", node.title)).small());
            }
        }
    }

    fn config_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("config")
            .default_size(400.0)
            .min_size(280.0)
            .max_size(720.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.heading("Generated config");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Copy").clicked() {
                            ui.ctx().copy_text(self.generated.text.clone());
                            self.status.info("Copied to the clipboard.");
                        }
                    });
                });

                for problem in &self.generated.problems {
                    ui.colored_label(Color32::from_rgb(0xE0, 0xA0, 0x40), format!("\u{26A0} {problem}"));
                }

                ui.add_space(4.0);
                egui::ScrollArea::both()
                    .id_salt("config-scroll")
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
        egui::Panel::bottom("status").show(ui, |ui| {
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
}

fn lighten(color: Color32) -> Color32 {
    Color32::from_rgb(
        color.r().saturating_add(0x50),
        color.g().saturating_add(0x50),
        color.b().saturating_add(0x50),
    )
}

fn default_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("stack-graph.json")
}
