//! Undo and redo through the editor itself: real frames, real key presses.

use nodez::egui::{self, Event, Id, Key, Modifiers, Pos2, RawInput, Rect, pos2, vec2};
use nodez::{
    Graph, NodeEditor, NodeId, NodeLibrary, NodeTemplate, SocketKind, SocketSpec, Widget,
    socket_anchor,
};

struct Rig {
    ctx: egui::Context,
    editor: NodeEditor,
    library: NodeLibrary,
    graph: Graph,
}

impl Rig {
    fn new() -> (Self, NodeId) {
        let mut library = NodeLibrary::new();
        let text = library.types.add("Text", egui::Color32::GRAY);
        let template = library.register(
            NodeTemplate::new("text", "Text")
                .input(SocketSpec::new("value", text).editable(Widget::text())),
        );
        let mut graph = Graph::new();
        let id = graph.add_node(&library, template, pos2(0.0, 0.0));
        let rig = Self {
            ctx: egui::Context::default(),
            editor: NodeEditor::new(),
            library,
            graph,
        };
        (rig, id)
    }

    /// Run one frame with the pointer resting over the canvas, away from the
    /// node, and these keys pressed.
    fn frame(&mut self, keys: &[(Key, Modifiers)]) -> bool {
        let mut events = vec![Event::PointerMoved(pos2(700.0, 500.0))];
        events.extend(keys.iter().map(|&(key, modifiers)| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }));
        self.run(events)
    }

    /// Press and release the primary button at `at`, over two frames.
    fn click(&mut self, at: Pos2) {
        for pressed in [true, false] {
            self.run(vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
    }

    fn run(&mut self, events: Vec<Event>) -> bool {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut changed = false;
        let (editor, library, graph) = (&mut self.editor, &self.library, &mut self.graph);
        let mut output = self.ctx.run_ui(input, |ui| {
            changed = editor.show(ui, library, graph).changed;
        });
        // Nothing renders here, so the font atlas has nowhere to go.
        output.textures_delta.clear();
        changed
    }
}

const CTRL: Modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };

#[test]
fn ctrl_z_undoes_a_delete_and_ctrl_shift_z_redoes_it() {
    let (mut rig, id) = Rig::new();
    rig.frame(&[]);
    rig.editor.state.select_only(id);

    assert!(rig.frame(&[(Key::X, Modifiers::NONE)]));
    assert!(rig.graph.node(id).is_none());
    rig.frame(&[]);

    assert!(rig.frame(&[(Key::Z, CTRL)]));
    assert!(rig.graph.node(id).is_some());
    rig.frame(&[]);

    assert!(rig.frame(&[(Key::Z, CTRL_SHIFT)]));
    assert!(rig.graph.node(id).is_none());
    rig.frame(&[]);

    assert!(rig.frame(&[(Key::Z, CTRL)]));
    assert!(rig.frame(&[(Key::Y, CTRL)]));
    assert!(rig.graph.node(id).is_none());
}

#[test]
fn ctrl_z_works_while_a_value_widget_has_focus() {
    let (mut rig, id) = Rig::new();
    rig.frame(&[]);
    rig.editor.state.select_only(id);
    rig.frame(&[(Key::X, Modifiers::NONE)]);
    rig.frame(&[]);

    // A checkbox or a drag box keeps focus after it is clicked.
    rig.ctx.memory_mut(|m| m.request_focus(Id::new("some value widget")));
    assert!(rig.frame(&[(Key::Z, CTRL)]));
    assert!(rig.graph.node(id).is_some());
}

#[test]
fn an_outside_change_is_undoable_after_a_checkpoint() {
    let (mut rig, id) = Rig::new();
    rig.frame(&[]);
    rig.editor.checkpoint(&rig.graph);
    rig.graph.node_mut(id).unwrap().set_input_value("value", "changed");
    rig.frame(&[]);

    assert!(rig.frame(&[(Key::Z, CTRL)]));
    let value = rig.graph.node(id).unwrap().input_value("value").unwrap();
    assert_eq!(value.as_str(), Some(""));
}

#[test]
fn typing_into_a_value_is_one_step_that_ctrl_z_takes_back() {
    let (mut rig, id) = Rig::new();
    rig.frame(&[]);
    let value = |rig: &Rig| {
        let node = rig.graph.node(id).unwrap();
        node.input_value("value").unwrap().as_str().unwrap().to_owned()
    };

    // With no pan and no zoom, graph space is screen space, and the field for
    // an inline value sits just right of its socket.
    let node = rig.graph.node(id).unwrap();
    let socket = socket_anchor(
        &rig.graph,
        &rig.library,
        node,
        &rig.editor.style,
        SocketKind::Input,
        "value",
        None,
    )
    .unwrap();
    rig.click(socket + vec2(30.0, 0.0));
    assert!(rig.ctx.text_edit_focused(), "the click should land in the field");

    for letter in ["a", "b", "c"] {
        rig.run(vec![Event::Text(letter.into())]);
    }
    assert_eq!(value(&rig), "abc");

    // Clicking the canvas takes focus from the field and closes the step.
    rig.click(pos2(700.0, 500.0));
    assert!(!rig.ctx.text_edit_focused());

    assert!(rig.frame(&[(Key::Z, CTRL)]));
    assert_eq!(value(&rig), "");
    rig.frame(&[]);
    assert!(rig.frame(&[(Key::Z, CTRL_SHIFT)]));
    assert_eq!(value(&rig), "abc");
}
