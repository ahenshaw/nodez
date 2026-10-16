//! The app's hooks, driven through real frames without a window.
#![cfg(feature = "app")]

use std::cell::Cell;
use std::rc::Rc;

use nodez::app::EditorApp;
use nodez::egui::{self, Pos2, RawInput, Rect, pos2, vec2};
use nodez::{NodeLibrary, NodeTemplate, SocketSpec, TemplateId, Widget};

fn library() -> (NodeLibrary, TemplateId) {
    let mut library = NodeLibrary::new();
    let text = library.types.add("Text", egui::Color32::GRAY);
    let template = library.register(
        NodeTemplate::new("text", "Text")
            .input(SocketSpec::new("value", text).editable(Widget::text())),
    );
    (library, template)
}

fn frame(ctx: &egui::Context, app: &mut EditorApp) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
}

#[test]
fn a_layout_replaces_the_default_and_draws_only_what_it_asks_for() {
    let (library, _) = library();
    let laid_out = Rc::new(Cell::new(0));
    let toolbar_drawn = Rc::new(Cell::new(false));
    let (count, drawn) = (laid_out.clone(), toolbar_drawn.clone());
    let mut app = EditorApp::new(library)
        .toolbar(move |_, _| drawn.set(true))
        .layout(move |ui, app| {
            count.set(count.get() + 1);
            egui::CentralPanel::default().show(ui, |ui| app.show_canvas(ui));
        });
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(laid_out.get(), 2);
    assert!(!toolbar_drawn.get(), "this layout has no toolbar");
}

#[test]
fn the_default_layout_draws_toolbar_additions() {
    let (library, _) = library();
    let drawn = Rc::new(Cell::new(false));
    let seen = drawn.clone();
    let mut app = EditorApp::new(library).toolbar(move |ui, _| {
        let _ = ui.button("Mine");
        seen.set(true);
    });
    frame(&egui::Context::default(), &mut app);
    assert!(drawn.get());
}

#[test]
fn edit_is_undoable_and_counts_as_unsaved() {
    let (library, template) = library();
    let mut app = EditorApp::new(library);
    let ctx = egui::Context::default();
    frame(&ctx, &mut app);
    assert!(!app.is_modified());

    let id = app.edit(|graph, library| graph.add_node(library, template, pos2(0.0, 0.0)));
    assert!(app.is_modified());
    assert!(app.graph.node(id).is_some());

    assert!(app.editor.undo(&mut app.graph));
    assert!(app.graph.node(id).is_none());
}

#[test]
fn a_graph_given_with_a_view_opens_on_it() {
    let (library, template) = library();
    let mut graph = nodez::Graph::new();
    graph.add_node(&library, template, pos2(0.0, 0.0));
    graph.set_view(Some(nodez::View { pan: vec2(40.0, -10.0), zoom: 1.5 }));
    let mut app = EditorApp::new(library).graph(graph);
    frame(&egui::Context::default(), &mut app);
    assert_eq!(app.editor.state.pan, vec2(40.0, -10.0));
    assert_eq!(app.editor.state.zoom, 1.5);
}

/// The two snippets in the README's "Making the app your own", as written.
#[test]
fn the_readme_snippets_compile() {
    use nodez::NodeData;

    let (library, _) = library();
    let _ = EditorApp::new(library)
        // A button after the built-in ones.
        .toolbar(|ui, app| {
            if ui.button("Copy JSON").clicked()
                && let Ok(json) = app.graph.to_json(&app.library)
            {
                ui.ctx().copy_text(json);
            }
        })
        // An entry in every node's right-click menu.
        .node_menu("Reset to defaults", |app, node| {
            app.edit(|graph, library| {
                let node = graph.node_mut(node).unwrap();
                node.data = NodeData::new(library.expect(node.template));
            });
        });

    let (library, _) = self::library();
    let _ = EditorApp::new(library)
        .layout(|ui, app| {
            egui::Panel::top("toolbar").show(ui, |ui| {
                ui.horizontal(|ui| app.show_toolbar(ui));
            });
            egui::Panel::left("palette").show(ui, |ui| app.show_palette(ui));
            egui::Panel::right("mine").show(ui, |ui| {
                ui.heading("Mine");
                ui.label(format!("{} nodes", app.graph.node_count()));
            });
            egui::CentralPanel::no_frame().show(ui, |ui| app.show_canvas(ui));
        });
}
