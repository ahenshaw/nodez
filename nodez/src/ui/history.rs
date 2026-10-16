//! Undo and redo, as snapshots of the whole graph.
//!
//! A snapshot is a clone of the graph from just before an edit, so every kind
//! of edit is covered by the same mechanism: a value typed into a socket, a
//! parameter, a wire, a node added or deleted, a move, a resize. Nothing has to
//! know how to reverse itself.
//!
//! Edits are grouped the way a person thinks of them. A drag is one step
//! however many frames it moves through, and so is a run of typing into one
//! text field: the step opens with the first change and closes once the
//! pointer is up, nothing is being dragged, and the text field that was being
//! typed into no longer has focus.

use std::any::Any;
use std::sync::Arc;

use egui::Id;

use crate::graph::Graph;

/// How many steps undo can walk back.
const DEPTH: usize = 64;

/// A `Graph<N>`, with `N` erased so the editor need not be generic over it.
type Snapshot = Arc<dyn Any + Send + Sync>;

#[derive(Clone, Default)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The step in progress, if one has begun and not yet closed.
    open: Option<Open>,
    /// Set by an undo or a redo, so the change it makes is not itself recorded
    /// as an edit.
    restored: bool,
}

#[derive(Clone)]
struct Open {
    /// The graph as it was before the step began.
    before: Snapshot,
    /// Whether anything has actually changed. A drag that never moved, or a
    /// grab that was cancelled, opens a step and then closes it unchanged.
    edited: bool,
    /// The text field being typed into, which keeps the step open while it
    /// has focus.
    typing: Option<Id>,
}

/// What a frame did, as far as history is concerned.
#[derive(Clone, Copy)]
pub(super) struct Frame {
    /// The graph's contents or layout changed.
    pub edited: bool,
    /// A drag, grab or resize is moving nodes and has not finished.
    pub in_flight: bool,
    /// Any pointer button is held.
    pub pointer_down: bool,
    /// The text field that has keyboard focus, if one does.
    pub typing: Option<Id>,
}

impl History {
    /// Whether the frame about to run needs a snapshot of the graph before it,
    /// in case it starts a step.
    pub fn wants_before(&self) -> bool {
        self.open.is_none()
    }

    /// Record what a frame did. `before` is the graph from the start of the
    /// frame, taken when [`History::wants_before`] asked for it.
    pub fn end_frame<N: crate::NodeData>(&mut self, before: Option<Graph<N>>, frame: Frame) {
        if std::mem::take(&mut self.restored) {
            return;
        }
        if self.open.is_none()
            && (frame.edited || frame.in_flight)
            && let Some(before) = before
        {
            self.open = Some(Open {
                before: Arc::new(before),
                edited: false,
                typing: None,
            });
        }
        let Some(open) = &mut self.open else {
            return;
        };
        if frame.edited {
            open.edited = true;
            open.typing = frame.typing;
        }
        let still_typing = frame.typing.is_some() && frame.typing == open.typing;
        if !(frame.in_flight || frame.pointer_down || still_typing) {
            self.close();
        }
    }

    /// Note an edit the editor makes without reporting it as a change, such as
    /// a resize.
    pub fn mark_edited(&mut self) {
        if let Some(open) = &mut self.open {
            open.edited = true;
        }
    }

    /// Close the step in progress, keeping it if anything changed.
    fn close(&mut self) {
        if let Some(open) = self.open.take()
            && open.edited
        {
            self.push(open.before);
        }
    }

    fn push(&mut self, snapshot: Snapshot) {
        if self.undo.len() == DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(snapshot);
        self.redo.clear();
    }

    /// Remember `graph` as it is now, before a change made from outside the
    /// editor.
    pub fn checkpoint<N: crate::NodeData>(&mut self, graph: &Graph<N>) {
        self.close();
        self.push(Arc::new(graph.clone()));
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.open.as_ref().is_some_and(|open| open.edited)
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Step back. Returns whether the graph changed.
    pub fn undo<N: crate::NodeData>(&mut self, graph: &mut Graph<N>) -> bool {
        self.close();
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        match swap(snapshot, graph) {
            Some(now) => {
                self.redo.push(now);
                self.restored = true;
                true
            }
            None => {
                self.clear();
                false
            }
        }
    }

    /// Step forward again. Returns whether the graph changed.
    pub fn redo<N: crate::NodeData>(&mut self, graph: &mut Graph<N>) -> bool {
        self.close();
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        match swap(snapshot, graph) {
            Some(now) => {
                self.undo.push(now);
                self.restored = true;
                true
            }
            None => {
                self.clear();
                false
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Put `snapshot` in place of `graph`, returning what was there. `None` if the
/// snapshot is of a different payload type, which only happens when one
/// editor is shown graphs of two types.
fn swap<N: crate::NodeData>(snapshot: Snapshot, graph: &mut Graph<N>) -> Option<Snapshot> {
    let then = snapshot.downcast::<Graph<N>>().ok()?;
    let then = Arc::try_unwrap(then).unwrap_or_else(|shared| (*shared).clone());
    Some(Arc::new(std::mem::replace(graph, then)))
}

impl std::fmt::Debug for History {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("History")
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .field("open", &self.open.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DynNode;
    use crate::template::{NodeLibrary, NodeTemplate, SocketSpec, Widget};
    use crate::NodeId;

    fn library() -> NodeLibrary {
        let mut library = NodeLibrary::new();
        let text = library.types.add("Text", egui::Color32::GRAY);
        library.register(
            NodeTemplate::new("text", "Text")
                .input(SocketSpec::new("value", text).editable(Widget::text())),
        );
        library
    }

    fn graph(library: &NodeLibrary) -> (Graph<DynNode>, NodeId) {
        let mut graph = Graph::new();
        let id = graph.add_node(library, library.id("text").unwrap(), egui::pos2(0.0, 0.0));
        (graph, id)
    }

    fn value(graph: &Graph<DynNode>, id: NodeId) -> String {
        graph.node(id).unwrap().input_value("value").unwrap().as_str().unwrap().to_owned()
    }

    fn set(graph: &mut Graph<DynNode>, id: NodeId, text: &str) {
        graph.node_mut(id).unwrap().set_input_value("value", text);
    }

    /// One frame: snapshot if asked, make the change, report it.
    fn frame(
        history: &mut History,
        graph: &mut Graph<DynNode>,
        change: impl FnOnce(&mut Graph<DynNode>),
        report: Frame,
    ) {
        let before = history.wants_before().then(|| graph.clone());
        change(graph);
        history.end_frame(before, report);
    }

    const EDIT: Frame = Frame { edited: true, in_flight: false, pointer_down: false, typing: None };
    const IDLE: Frame = Frame { edited: false, in_flight: false, pointer_down: false, typing: None };

    #[test]
    fn a_value_edit_undoes_and_redoes() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        set(&mut graph, id, "a");
        frame(&mut history, &mut graph, |g| set(g, id, "b"), EDIT);

        assert!(history.undo(&mut graph));
        assert_eq!(value(&graph, id), "a");
        assert!(history.redo(&mut graph));
        assert_eq!(value(&graph, id), "b");
        assert!(!history.redo(&mut graph));
    }

    #[test]
    fn typing_into_one_field_is_one_step() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        let field = Some(Id::new("field"));
        let typing = Frame { typing: field, ..EDIT };
        for text in ["h", "he", "hel"] {
            frame(&mut history, &mut graph, |g| set(g, id, text), typing);
        }
        // Still focused, nothing typed: the step stays open.
        frame(&mut history, &mut graph, |_| {}, Frame { typing: field, ..IDLE });
        // Focus leaves: the step closes.
        frame(&mut history, &mut graph, |_| {}, IDLE);

        assert!(history.undo(&mut graph));
        assert_eq!(value(&graph, id), "");
        assert!(!history.can_undo());
    }

    #[test]
    fn typing_into_a_second_field_is_a_second_step() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        let first = Frame { typing: Some(Id::new("first")), ..EDIT };
        let second = Frame { typing: Some(Id::new("second")), ..EDIT };
        frame(&mut history, &mut graph, |g| set(g, id, "a"), first);
        // The click that moves focus comes a frame before any typing.
        frame(&mut history, &mut graph, |_| {}, Frame { edited: false, ..second });
        frame(&mut history, &mut graph, |g| set(g, id, "ab"), second);
        frame(&mut history, &mut graph, |_| {}, IDLE);

        history.undo(&mut graph);
        assert_eq!(value(&graph, id), "a");
        history.undo(&mut graph);
        assert_eq!(value(&graph, id), "");
    }

    #[test]
    fn a_drag_is_one_step_and_a_still_one_is_none() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        let dragging = Frame { in_flight: true, pointer_down: true, ..IDLE };
        let nudge = |g: &mut Graph<DynNode>| g.node_mut(id).unwrap().position.x += 10.0;

        // Pressed and released without moving.
        frame(&mut history, &mut graph, |_| {}, dragging);
        frame(&mut history, &mut graph, |_| {}, IDLE);
        assert!(!history.can_undo());

        frame(&mut history, &mut graph, |_| {}, dragging);
        frame(&mut history, &mut graph, nudge, dragging);
        frame(&mut history, &mut graph, nudge, dragging);
        frame(&mut history, &mut graph, |_| {}, EDIT); // released: NodesMoved
        assert!(history.undo(&mut graph));
        assert_eq!(graph.node(id).unwrap().position.x, 0.0);
        assert!(!history.can_undo());
    }

    #[test]
    fn a_new_edit_drops_what_redo_had() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        frame(&mut history, &mut graph, |g| set(g, id, "a"), EDIT);
        history.undo(&mut graph);
        assert!(history.can_redo());
        frame(&mut history, &mut graph, |_| {}, IDLE); // the undo's own frame
        frame(&mut history, &mut graph, |g| set(g, id, "b"), EDIT);
        assert!(!history.can_redo());
    }

    #[test]
    fn a_checkpoint_makes_an_outside_change_undoable() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        history.checkpoint(&graph);
        graph.remove_node(id);
        assert!(history.undo(&mut graph));
        assert!(graph.node(id).is_some());
    }

    #[test]
    fn undo_closes_a_step_still_open() {
        let library = library();
        let (mut graph, id) = graph(&library);
        let mut history = History::default();
        let typing = Frame { typing: Some(Id::new("field")), ..EDIT };
        frame(&mut history, &mut graph, |g| set(g, id, "a"), typing);
        assert!(history.can_undo());
        assert!(history.undo(&mut graph));
        assert_eq!(value(&graph, id), "");
    }
}
