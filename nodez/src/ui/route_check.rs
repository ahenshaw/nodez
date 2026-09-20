//! Invariants every routed wire has to hold, checked over generated graphs.
//!
//! The hand-built fixtures elsewhere pin down cases we already know about.
//! These tests go the other way: throw a few hundred shapes of graph at the
//! router and assert the two things a wire must never do, whatever the shape.
//!
//! Both checks run against the path the editor actually draws — sockets,
//! waypoints, rounded corners and all — because that is what a user sees. A
//! wire can have every waypoint in open space and still cut a node in half on
//! the segment between two of them.

use egui::{Pos2, Rect, pos2};

use super::geometry::{bezier_point, wire_path};
use super::style::EditorStyle;
use super::{node_size, socket_anchor};
use crate::graph::{Graph, NodeId, SocketKind};
use crate::layout::{LayoutOptions, RouteOptions, layered, route_links};
use crate::template::{NodeLibrary, NodeTemplate, ParamSpec, SocketSpec, TemplateId, Widget};

/// xorshift64*, so a failing seed reproduces exactly.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `lo..=hi`.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() % (hi - lo + 1) as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// Templates of deliberately different sizes. Height comes from the socket and
/// param count, width from the title, and both matter: on a tall node the
/// sockets are nowhere near the middle of its edge, which is where a router
/// that ignores anchors would aim.
fn library() -> (NodeLibrary, Vec<TemplateId>) {
    let mut library = NodeLibrary::new();
    let ty = library.types.add("T", egui::Color32::GRAY);

    let mut templates = Vec::new();
    for inputs in 1..=4 {
        for params in 0..=2 {
            let name = format!("n{inputs}x{params}");
            let title = "Node".repeat(inputs.min(3));
            let mut template = NodeTemplate::new(&name, &title)
                .input(SocketSpec::new("in", ty).multi())
                .output(SocketSpec::new("out", ty));
            for i in 0..inputs {
                template = template.input(SocketSpec::new(format!("s{i}"), ty));
            }
            for i in 0..params {
                template = template.param(ParamSpec::new(format!("p{i}"), Widget::text()));
            }
            templates.push(library.register(template));
        }
    }
    (library, templates)
}

/// A random DAG, generated as columns so there is always something for a long
/// wire to have to get past.
fn random_graph(rng: &mut Rng, library: &NodeLibrary, templates: &[TemplateId]) -> Graph {
    let mut graph = Graph::new();
    let depth = rng.range(2, 6);
    let columns: Vec<Vec<NodeId>> = (0..depth)
        .map(|_| {
            (0..rng.range(1, 5))
                .map(|_| {
                    let t = templates[rng.range(0, templates.len() - 1)];
                    graph.add_node(library, t, pos2(0.0, 0.0))
                })
                .collect()
        })
        .collect();

    for c in 1..columns.len() {
        for &node in &columns[c] {
            // One wire from the column immediately before, so the node lands
            // where it was generated rather than floating to depth zero.
            let prev = &columns[c - 1];
            let from = prev[rng.range(0, prev.len() - 1)];
            let _ = graph.connect(library, (from, "out"), (node, "in"));

            // Then the long ones: these are what has to be routed.
            for earlier in &columns[..c.saturating_sub(1)] {
                if rng.chance(60) {
                    let from = earlier[rng.range(0, earlier.len() - 1)];
                    let _ = graph.connect(library, (from, "out"), (node, "in"));
                }
            }
        }
    }
    graph
}

/// A graph laid out and routed, with everything the checks need to measure it.
struct Routed {
    graph: Graph,
    rects: Vec<(NodeId, Rect)>,
    style: EditorStyle,
    library: NodeLibrary,
}

fn route(rng: &mut Rng) -> Routed {
    route_with(rng, 0.0)
}

/// `jitter` shifts every node by up to that many pixels after layout, standing
/// in for a user who dragged things about before asking for a re-route.
fn route_with(rng: &mut Rng, jitter: f32) -> Routed {
    let (library, templates) = library();
    let mut graph = random_graph(rng, &library, &templates);
    let style = EditorStyle::default();
    let size = |g: &Graph, n: &crate::graph::Node| node_size(g, &library, n, &style);

    layered(&mut graph, &LayoutOptions::default(), size).unwrap();
    if jitter > 0.0 {
        // Nudge each node, but put it back if it lands on another one. A user
        // dragging nodes about produces a messy layout, not a pile; a pile has
        // no clean routing to find and would only prove the test's patience.
        for id in graph.nodes().map(|n| n.id).collect::<Vec<_>>() {
            let dx = (rng.range(0, 200) as f32 / 100.0 - 1.0) * jitter;
            let dy = (rng.range(0, 200) as f32 / 100.0 - 1.0) * jitter;
            let was = graph.node(id).unwrap().position;
            graph.node_mut(id).unwrap().position = was + egui::vec2(dx, dy);

            let box_of = |g: &Graph, id| {
                let n = g.node(id).unwrap();
                Rect::from_min_size(n.position, size(g, n))
            };
            let moved = box_of(&graph, id);
            let collides = graph
                .nodes()
                .filter(|n| n.id != id)
                .any(|n| Rect::from_min_size(n.position, size(&graph, n)).intersects(moved));
            if collides {
                graph.node_mut(id).unwrap().position = was;
            }
        }
    }
    route_links(
        &mut graph,
        &RouteOptions {
            curvature: style.wire_curvature,
            min_curve: style.wire_min_curve,
            max_curve: style.wire_max_curve,
            ..RouteOptions::default()
        },
        size,
        |g, socket, kind, slot| {
            let node = g.node(socket.node)?;
            socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
        },
    )
    .unwrap();

    let rects = graph
        .nodes()
        .map(|n| (n.id, Rect::from_min_size(n.position, size(&graph, n))))
        .collect();
    Routed {
        graph,
        rects,
        style,
        library,
    }
}

impl Routed {
    /// Whether the nudge dropped two nodes on top of each other. No routing
    /// is clean through that, and the arrangement is visibly broken already,
    /// so those graphs are not the router's to answer for.
    fn overlapping(&self) -> bool {
        self.rects
            .iter()
            .enumerate()
            .any(|(i, (_, a))| self.rects[i + 1..].iter().any(|(_, b)| a.intersects(*b)))
    }

    /// A node's box, for asking where a wire may turn.
    fn box_of(&self, id: NodeId) -> Rect {
        self.rects.iter().find(|(at, _)| *at == id).unwrap().1
    }

    /// Where a wire attaches at each end.
    fn ends(&self, conn: &crate::graph::Connection) -> (Pos2, Pos2) {
        let from = self.graph.node(conn.from.node).unwrap();
        let to = self.graph.node(conn.to.node).unwrap();
        let a = socket_anchor(
            &self.graph,
            &self.library,
            from,
            &self.style,
            SocketKind::Output,
            &conn.from.socket,
            None,
        )
        .unwrap();
        let b = socket_anchor(
            &self.graph,
            &self.library,
            to,
            &self.style,
            SocketKind::Input,
            &conn.to.socket,
            Some(conn.order),
        )
        .unwrap();
        (a, b)
    }

    /// The wire as drawn, densely sampled, in graph space.
    fn drawn(&self, conn: &crate::graph::Connection) -> Vec<Pos2> {
        let (a, b) = self.ends(conn);
        let mut out = Vec::new();
        for segment in wire_path(a, b, &conn.waypoints, &self.style, 1.0) {
            const SAMPLES: usize = 32;
            for i in 0..=SAMPLES {
                out.push(bezier_point(&segment, i as f32 / SAMPLES as f32));
            }
        }
        out
    }
}

/// How many graphs each check sweeps. Small graphs, so this stays quick.
const CASES: u64 = 300;

#[test]
fn a_wire_never_runs_under_a_node() {
    crossing_sweep(0.0, "straight out of the layout");
}

/// The arrangement a user actually re-routes: one they have dragged about.
/// `route_links` is documented to expect columns, but nudging a node does not
/// stop it being asked, and a wire cutting a node is a bug either way.
#[test]
fn a_nudged_graph_still_routes_clear() {
    crossing_sweep(30.0, "after nodes were nudged");
}

fn crossing_sweep(jitter: f32, what: &str) {
    let mut failures: Vec<String> = Vec::new();
    let mut routed_wires = 0;
    let (mut straight, mut bent) = (0, 0);
    let mut deepest = 0.0f32;
    let mut overlapped = 0u64;

    for seed in 1..=CASES {
        let case = route_with(&mut Rng::new(seed), jitter);
        if case.overlapping() {
            overlapped += 1;
            continue;
        }
        for conn in case.graph.connections() {
            if !conn.waypoints.is_empty() {
                routed_wires += 1;
            }
            let path = case.drawn(conn);
            for (id, rect) in &case.rects {
                if *id == conn.from.node || *id == conn.to.node {
                    continue;
                }
                // How far the wire gets inside this node at its deepest.
                let worst = path
                    .iter()
                    .filter(|p| rect.contains(**p))
                    .map(|p| {
                        let dx = (p.x - rect.left()).min(rect.right() - p.x);
                        let dy = (p.y - rect.top()).min(rect.bottom() - p.y);
                        dx.min(dy)
                    })
                    .fold(0.0f32, f32::max);
                // `Rect::contains` counts the outline itself, and a sampled
                // curve lands on it to within float noise, so tangency has to
                // be told apart from a wire that is genuinely inside.
                const TANGENT: f32 = 0.1;
                if worst > TANGENT {
                    deepest = deepest.max(worst);
                    if conn.waypoints.is_empty() {
                        straight += 1;
                    } else {
                        bent += 1;
                    }
                    failures.push(format!(
                        "seed {seed}: wire {:?} cuts {:.1}px into node {:?} ({} waypoints)",
                        conn.id,
                        worst,
                        id,
                        conn.waypoints.len()
                    ));
                }
            }
        }
    }

    // A sweep that skipped or straightened everything would pass without
    // having checked anything, so it has to say how much work it did.
    let checked = CASES - overlapped;
    assert!(
        checked * 2 >= CASES && routed_wires > 0,
        "only {checked} of {CASES} graphs were usable, with {routed_wires} routed wires: \
         the sweep proves nothing"
    );
    assert!(
        failures.is_empty(),
        "{} wires cross a node {what} ({straight} left straight, {bent} routed), \
         worst {deepest:.1}px deep; {overlapped} graphs skipped as overlapping:\n{}",
        failures.len(),
        report(&failures)
    );
}

/// A wire may climb to get past what is in its way, and no further. Detouring
/// over the whole canvas for two sockets a few pixels apart is the failure
/// this catches: it is clear of everything, so no other sweep objects.
#[test]
fn a_wire_climbs_no_further_than_it_must() {
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0;

    for seed in 1..=CASES {
        let case = route_with(&mut Rng::new(seed), 0.0);
        for conn in case.graph.connections() {
            if conn.waypoints.is_empty() {
                continue;
            }
            checked += 1;
            let (a, b) = case.ends(conn);

            // What the wire has to clear: its own two ends, and every node
            // standing in the stretch of canvas it crosses. Measured from the
            // sockets, so this says nothing about how the router works.
            let (lo, hi) = (a.x.min(b.x), a.x.max(b.x));
            let mut top = a.y.min(b.y);
            let mut bottom = a.y.max(b.y);
            for (id, rect) in &case.rects {
                if *id == conn.from.node || *id == conn.to.node {
                    continue;
                }
                if rect.right() >= lo && rect.left() <= hi {
                    top = top.min(rect.top());
                    bottom = bottom.max(rect.bottom());
                }
            }
            // Going clear of all that costs a margin; three is generous.
            let slack = RouteOptions::default().margin * 3.0;
            for point in &conn.waypoints {
                let strayed = (top - slack - point.y).max(point.y - (bottom + slack));
                if strayed > 0.0 {
                    failures.push(format!(
                        "seed {seed}: wire {:?} climbs {strayed:.0}px past anything in its way",
                        conn.id
                    ));
                }
            }
        }
    }

    assert!(checked > 0, "no routed wires; the sweep proves nothing");
    assert!(
        failures.is_empty(),
        "{} of {checked} routed wires take the long way round:\n{}",
        failures.len(),
        report(&failures)
    );
}

/// A routed wire has to arrive along its own height, from outside the node it
/// is landing on. Letting the climb sit on the target's edge costs the last
/// leg its length, and the wire then drops onto the socket down the face of
/// the node, crossing whatever other sockets it passes on the way — which
/// reads as if it fed every one of them.
#[test]
fn a_wire_meets_its_socket_level() {
    for (jitter, what) in [(0.0, "in the layout"), (30.0, "once nudged")] {
        let mut failures: Vec<String> = Vec::new();
        let mut checked = 0;

        for seed in 1..=CASES {
            let case = route_with(&mut Rng::new(seed), jitter);
            for conn in case.graph.connections() {
                let (Some(first), Some(last)) =
                    (conn.waypoints.first(), conn.waypoints.last())
                else {
                    continue; // Straight to the socket; nothing to arrive along.
                };
                checked += 1;
                let (a, b) = case.ends(conn);
                let from = case.box_of(conn.from.node);
                let to = case.box_of(conn.to.node);

                // Level with the socket, so the wire runs into it rather than
                // down onto it.
                const LEVEL: f32 = 0.5;
                if (last.y - b.y).abs() > LEVEL {
                    failures.push(format!(
                        "seed {seed}: wire {:?} arrives {:.1}px off its socket's height",
                        conn.id,
                        (last.y - b.y).abs()
                    ));
                }
                if (first.y - a.y).abs() > LEVEL {
                    failures.push(format!(
                        "seed {seed}: wire {:?} leaves {:.1}px off its socket's height",
                        conn.id,
                        (first.y - a.y).abs()
                    ));
                }
                // And from outside the node, not down its face.
                if last.x > to.left() {
                    failures.push(format!(
                        "seed {seed}: wire {:?} turns {:.1}px inside the node it lands on",
                        conn.id,
                        last.x - to.left()
                    ));
                }
                if first.x < from.right() {
                    failures.push(format!(
                        "seed {seed}: wire {:?} turns {:.1}px inside the node it leaves",
                        conn.id,
                        from.right() - first.x
                    ));
                }
            }
        }

        assert!(checked > 0, "no routed wires {what}; the sweep proves nothing");
        assert!(
            failures.is_empty(),
            "{} of {checked} routed wires meet a socket badly {what}:\n{}",
            failures.len(),
            report(&failures)
        );
    }
}

#[test]
fn a_wire_never_doubles_back() {
    let mut failures: Vec<String> = Vec::new();
    let (mut straight, mut bent) = (0, 0);

    for seed in 1..=CASES {
        let case = route(&mut Rng::new(seed));
        for conn in case.graph.connections() {
            let (a, b) = case.ends(conn);
            // Only forward wires have a direction to reverse. A wire whose
            // target sits left of its source has to come back on itself.
            if b.x <= a.x {
                continue;
            }
            let path = case.drawn(conn);
            // The furthest the wire ever retreats from the rightmost point it
            // has reached. Anything past a hairline reads as a kink.
            let mut high = f32::NEG_INFINITY;
            let mut worst = 0.0f32;
            for p in &path {
                high = high.max(p.x);
                worst = worst.max(high - p.x);
            }
            const TOLERANCE: f32 = 1.0;
            if worst > TOLERANCE {
                if conn.waypoints.is_empty() {
                    straight += 1;
                } else {
                    bent += 1;
                }
                failures.push(format!(
                    "seed {seed}: wire {:?} reverses {:.1}px ({} waypoints, span {:.0}px)",
                    conn.id,
                    worst,
                    conn.waypoints.len(),
                    b.x - a.x
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} wires double back ({straight} left straight, {bent} routed):\n{}",
        failures.len(),
        report(&failures)
    );
}

/// The first handful of failures, so a run names something actionable without
/// burying it in a thousand lines.
fn report(failures: &[String]) -> String {
    let shown = failures.len().min(10);
    let mut out = failures[..shown].join("\n");
    if failures.len() > shown {
        out.push_str(&format!("\n... and {} more", failures.len() - shown));
    }
    out
}

/// Routing must not depend on the order wires happen to be stored in: the same
/// picture has to come back every time, or a redraw shuffles the wires.
#[test]
fn routing_the_same_graph_twice_agrees() {
    for seed in 1..=50 {
        let first = route(&mut Rng::new(seed));
        let second = route(&mut Rng::new(seed));
        let a: Vec<_> = first
            .graph
            .connections()
            .map(|c| c.waypoints.clone())
            .collect();
        let b: Vec<_> = second
            .graph
            .connections()
            .map(|c| c.waypoints.clone())
            .collect();
        assert_eq!(a, b, "seed {seed} routes differently on a second run");
    }
}

/// Whatever the router does to the picture, the graph underneath is untouched.
#[test]
fn routing_never_changes_the_graph() {
    for seed in 1..=CASES {
        let mut rng = Rng::new(seed);
        let (library, templates) = library();
        let mut graph = random_graph(&mut rng, &library, &templates);
        let style = EditorStyle::default();
        let size = |g: &Graph, n: &crate::graph::Node| node_size(g, &library, n, &style);

        let before = graph.topological_order().unwrap();
        let wires: Vec<_> = graph
            .connections()
            .map(|c| (c.id, c.from.clone(), c.to.clone(), c.order))
            .collect();

        layered(&mut graph, &LayoutOptions::default(), size).unwrap();
        route_links(
            &mut graph,
            &RouteOptions::default(),
            size,
            |g, socket, kind, slot| {
                let node = g.node(socket.node)?;
                socket_anchor(g, &library, node, &style, kind, &socket.socket, slot)
            },
        )
        .unwrap();

        assert_eq!(graph.topological_order().unwrap(), before, "seed {seed}");
        let after: Vec<_> = graph
            .connections()
            .map(|c| (c.id, c.from.clone(), c.to.clone(), c.order))
            .collect();
        assert_eq!(after, wires, "seed {seed}");
    }
}
