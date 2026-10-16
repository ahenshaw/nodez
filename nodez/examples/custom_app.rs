//! Making the editor window your own: a layout of your own, a toolbar button,
//! and an entry in every node's right-click menu.
//!
//!     cargo run -p nodez --features derive,app --example custom_app
//!
//! The right-hand panel is not the app's: it is drawn here, and shows the
//! graph exactly as it would be saved — move a node and watch its position
//! change.

use nodez::app::{EditorApp, Preview};
use nodez::{eframe, egui};
use nodez::{
    Evaluate, Fold, Graph, NodeData, NodeError, NodeLibrary, NodeType, Payload, Rules,
};

#[derive(Debug, NodeType)]
#[node(category = "Input", output = String)]
struct Text {
    #[input(hint = "text…")]
    value: String,
}

#[derive(Debug, NodeType)]
#[node(category = "Convert", output = String)]
struct Uppercase {
    #[input]
    text: String,
}

#[derive(Debug, NodeType)]
#[node(category = "Convert", output = String, label = "Join Text")]
struct Join {
    #[param(default = " ")]
    separator: String,
    #[input]
    parts: Vec<String>,
}

struct Words;
impl Fold for Words {}

impl Evaluate<Words> for Text {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.value.clone())
    }
}
impl Evaluate<Words> for Uppercase {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.text.to_uppercase())
    }
}
impl Evaluate<Words> for Join {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.parts.join(&self.separator))
    }
}

fn main() -> eframe::Result {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Words>::new();
    rules.register_all::<(Text, Uppercase, Join)>(&mut library);
    let graph = starting_graph(&library);

    EditorApp::new(library)
        .graph(graph)
        .title("nodez custom app")
        .json_files()
        .preview(move |graph, library| Preview::text(shout(graph, library, &rules)))
        // Arrange the window from the app's pieces, plus a panel of our own.
        .layout(|ui, app| {
            egui::Panel::top("toolbar").show(ui, |ui| {
                ui.horizontal(|ui| app.show_toolbar(ui));
            });
            egui::Panel::bottom("status").show(ui, |ui| app.show_status(ui));
            egui::Panel::left("palette")
                .default_size(180.0)
                .show(ui, |ui| app.show_palette(ui));
            egui::Panel::right("saved")
                .default_size(360.0)
                .show(ui, |ui| saved_panel(ui, app));
            // The app's preview, moved under the canvas.
            egui::Panel::bottom("output")
                .default_size(90.0)
                .show(ui, |ui| app.show_preview(ui));
            egui::CentralPanel::no_frame().show(ui, |ui| app.show_canvas(ui));
        })
        // One more button, after the built-in ones.
        .toolbar(|ui, app| {
            if ui.button("Copy JSON").clicked() {
                match app.graph.to_json(&app.library) {
                    Ok(json) => {
                        ui.ctx().copy_text(json);
                        app.status("Copied the graph as JSON.");
                    }
                    Err(e) => app.status_error(e.to_string()),
                }
            }
        })
        // One more entry in every node's right-click menu. `edit` makes the
        // change undoable and keeps the rest of the app up to date.
        .node_menu("Reset to defaults", |app, node| {
            app.edit(|graph, library| {
                let Some(node) = graph.node_mut(node) else { return };
                if let Some(template) = library.get(node.template) {
                    node.data = NodeData::new(template);
                }
            });
            app.status("Reset the node's values.");
        })
        .run()
}

/// What every Uppercase node comes to.
fn shout(graph: &Graph, library: &NodeLibrary, rules: &Rules<Words>) -> String {
    let template = library.id("uppercase").unwrap();
    graph
        .nodes_of_template(template)
        .map(|node| {
            match graph.evaluate::<Payload, NodeError>(library, node.id, |ctx| rules.run(&ctx)) {
                Ok(payload) => *payload.downcast::<String>().unwrap_or_default(),
                Err(e) => e.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The graph as `to_json` would save it.
fn saved_panel(ui: &mut egui::Ui, app: &mut EditorApp) {
    ui.heading("As saved");
    ui.label(
        egui::RichText::new("Positions, sizes, values, wires and view: all of it.")
            .small()
            .weak(),
    );
    let json = app.graph.to_json(&app.library).unwrap_or_else(|e| e.to_string());
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(json).monospace().small())
                .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

fn starting_graph(library: &NodeLibrary) -> Graph {
    let mut graph = Graph::new();
    let mut add = |id: &str, x: f32, y: f32| {
        graph.add_node(library, library.id(id).unwrap(), egui::pos2(x, y))
    };
    let hello = add("text", 0.0, 0.0);
    let world = add("text", 0.0, 120.0);
    let join = add("join", 240.0, 40.0);
    let shout = add("uppercase", 460.0, 60.0);
    graph.node_mut(hello).unwrap().set_input_value("value", "hello");
    graph.node_mut(world).unwrap().set_input_value("value", "world");
    for (from, to, socket) in [(hello, join, "parts"), (world, join, "parts"), (join, shout, "text")] {
        graph.connect(library, (from, "out"), (to, socket)).unwrap();
    }
    graph
}
