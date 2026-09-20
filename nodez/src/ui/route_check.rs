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

use std::cmp::Ordering;

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

/// The options the editor routes with.
fn options(style: &EditorStyle) -> RouteOptions {
    RouteOptions {
        curvature: style.wire_curvature,
        min_curve: style.wire_min_curve,
        max_curve: style.wire_max_curve,
        ..RouteOptions::default()
    }
}

/// `jitter` shifts every node by up to that many pixels after layout, standing
/// in for a user who dragged things about before asking for a re-route.
fn route_with(rng: &mut Rng, jitter: f32) -> Routed {
    route_costing(rng, jitter, RouteOptions::default().cross)
}

fn route_costing(rng: &mut Rng, jitter: f32, cross: f32) -> Routed {
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
            cross,
            ..options(&style)
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

    /// How many times one wire visibly goes over another, over the whole
    /// picture.
    ///
    /// Counted per pair of wires and per place they meet, not per pair of
    /// sampled segments: two wires running alongside each other touch at a
    /// dozen samples and cross once, if at all. Where wires share a socket
    /// they leave from or land on the same point, which is a fan rather than
    /// a crossing, so the ends are discounted.
    fn crossings(&self) -> usize {
        self.graph
            .connections()
            .enumerate()
            .map(|(i, conn)| {
                // Each pair once: only the wires after this one in the list.
                let later: Vec<_> = self.graph.connections().skip(i + 1).collect();
                self.cuts(&self.drawn(conn), self.ends(conn), &later)
            })
            .sum()
    }

    /// How many of `others` a wire drawn along `points` between `ends` goes
    /// over.
    ///
    /// Counted per wire and per place they meet, not per pair of sampled
    /// segments: two wires running alongside each other touch at a dozen
    /// samples and cross once, if at all. Where wires share a socket they
    /// leave from or land on the same point, which is a fan rather than a
    /// crossing, so the ends are discounted.
    fn cuts(
        &self,
        points: &[Pos2],
        (a, b): (Pos2, Pos2),
        others: &[&crate::graph::Connection],
    ) -> usize {
        const APART: f32 = 12.0;
        let margin = RouteOptions::default().margin;
        let mut total = 0;
        for other in others {
            let (c, d) = self.ends(other);
            let theirs = self.drawn(other);
            let mut meetings: Vec<Pos2> = Vec::new();
            for one in points.windows(2) {
                for two in theirs.windows(2) {
                    let Some(p) = crate::layout::meeting([one[0], one[1]], [two[0], two[1]]) else {
                        continue;
                    };
                    let shared = [a, b, c, d].iter().any(|s| p.distance(*s) <= margin);
                    if !shared && !meetings.iter().any(|q| q.distance(p) <= APART) {
                        meetings.push(p);
                    }
                }
            }
            total += meetings.len();
        }
        total
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

/// A routed wire must be no longer than the obvious way round.
///
/// The obvious way is the one anybody would draw by hand: out from the
/// socket, over the top of everything in the way (or under the bottom of it,
/// whichever is nearer), across, and in. It is always available and always
/// clear, so a router that spends more wire than that has gone wrong — and it
/// is worked out here from the node boxes, without asking the router anything,
/// so agreeing with it means something.
///
/// This is what catches a wire diving south to reach a socket level with where
/// it started: such a path is clear of every node, so no other sweep objects
/// to it, and it is simply long.
#[test]
fn a_wire_is_no_longer_than_the_obvious_way_round() {
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
            let margin = RouteOptions::default().margin;

            // Everything standing between the two sockets.
            let (lo, hi) = (a.x.min(b.x), a.x.max(b.x));
            let (mut top, mut bottom) = (a.y.min(b.y), a.y.max(b.y));
            for (id, rect) in &case.rects {
                if *id == conn.from.node || *id == conn.to.node {
                    continue;
                }
                if rect.right() >= lo && rect.left() <= hi {
                    top = top.min(rect.top());
                    bottom = bottom.max(rect.bottom());
                }
            }

            // Out, along at a height clear of all of it, and in.
            let by_way_of = |y: f32| (a.y - y).abs() + (hi - lo) + (y - b.y).abs() + margin * 2.0;
            let over_the_top = by_way_of(top - margin);
            let under_the_bottom = by_way_of(bottom + margin);
            let obvious = over_the_top.min(under_the_bottom);

            let mut drawn = a;
            let mut spent = 0.0;
            for p in conn.waypoints.iter().chain([&b]) {
                spent += (p.x - drawn.x).abs() + (p.y - drawn.y).abs();
                drawn = *p;
            }

            // The way round is measured along the top of the span and taken
            // on trust at the two ends, so it is a floor rather than a route
            // anyone could always draw. Corners cost the router something
            // too, keeping off another wire's line costs a little more, and
            // wire is what it spends to keep out of another's way at all —
            // which it decides on the picture as it finds it, not as the
            // picture ends up. Half again over the floor covers all of that;
            // the failures this catches run to twice it and beyond.
            let allowed = obvious * 3.0 / 2.0 + margin * 4.0;
            if spent <= allowed {
                continue;
            }

            // Wire is not the only thing a route spends, and the way round
            // is only cheap on length: it cuts across whatever happens to be
            // between the two sockets. A wire that went further than the
            // floor is allowed whatever the crossings it saved are worth,
            // priced the way the router prices them. Worked out only for the
            // wires that fail on length alone, because counting what every
            // wire crosses over three hundred graphs is not quick.
            let way_round = |y: f32| {
                vec![
                    a,
                    pos2(a.x + margin, a.y),
                    pos2(a.x + margin, y),
                    pos2(b.x - margin, y),
                    pos2(b.x - margin, b.y),
                    b,
                ]
            };
            let round = if over_the_top <= under_the_bottom {
                way_round(top - margin)
            } else {
                way_round(bottom + margin)
            };
            let others: Vec<_> = case
                .graph
                .connections()
                .filter(|other| other.id != conn.id)
                .collect();
            let ends = case.ends(conn);
            let saved = case.cuts(&round, ends, &others).saturating_sub(case.cuts(
                &case.drawn(conn),
                ends,
                &others,
            ));
            let cross = RouteOptions::default().cross;
            if spent > allowed + saved as f32 * cross {
                failures.push(format!(
                    "seed {seed}: wire {:?} spends {spent:.0}px where {obvious:.0}px goes round, \
                     and saves {saved} crossings doing it",
                    conn.id
                ));
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

/// Paying for crossings has to buy fewer of them.
///
/// Wire by wire the router can only be greedy — it prices what it can see —
/// so the way to tell whether the price is worth paying is to route a spread
/// of graphs both ways and count. Compared in total rather than graph by
/// graph: a penalty that pays off across a sweep can still lose on one
/// arrangement, and holding it to every single one would be holding it to
/// something it never promised.
#[test]
fn paying_for_crossings_buys_fewer_of_them() {
    // Fewer graphs than the other sweeps: counting what every wire crosses is
    // quadratic in the wires and in their samples, where the checks that only
    // ask about nodes are not.
    const GRAPHS: u64 = 30;

    let (mut free, mut priced, mut counted) = (0, 0, 0);
    for seed in 1..=GRAPHS {
        let without = route_costing(&mut Rng::new(seed), 0.0, 0.0);
        let with = route_costing(&mut Rng::new(seed), 0.0, RouteOptions::default().cross);
        if without.overlapping() {
            continue;
        }
        free += without.crossings();
        priced += with.crossings();
        counted += 1;
    }

    assert!(counted * 2 >= GRAPHS, "only {counted} graphs were usable");
    assert!(
        priced < free,
        "over {counted} graphs the router draws {priced} crossings when they cost \
         something and {free} when they are free"
    );
}

/// A routed wire mostly keeps the room it was asked to keep.
///
/// A wire drawn hard against a node it is only passing reads as part of that
/// node, which is worse than the detour that would have avoided it. The
/// router is allowed to squeeze — a wire boxed in on every side has to, and
/// giving up is worse still — so this is a proportion rather than a rule, and
/// a generous one: what it catches is the router deciding a squeeze is free.
#[test]
fn a_routed_wire_mostly_keeps_its_clearance() {
    // A third of the clearance in from the edge. Closer than this and the
    // wire is inside the gap that was supposed to be left around the node.
    const TOO_CLOSE: f32 = 0.5;
    const GRAPHS: u64 = 120;

    let margin = RouteOptions::default().margin;
    let (mut routed, mut squeezed) = (0, 0);
    for seed in 1..=GRAPHS {
        let case = route_with(&mut Rng::new(seed), 0.0);
        if case.overlapping() {
            continue;
        }
        for conn in case.graph.connections() {
            if conn.waypoints.is_empty() {
                continue;
            }
            routed += 1;
            let mut nearest = f32::INFINITY;
            for p in case.drawn(conn) {
                for (id, rect) in &case.rects {
                    if *id == conn.from.node || *id == conn.to.node {
                        continue;
                    }
                    let dx = (rect.left() - p.x).max(p.x - rect.right()).max(0.0);
                    let dy = (rect.top() - p.y).max(p.y - rect.bottom()).max(0.0);
                    nearest = nearest.min(dx.max(dy));
                }
            }
            if nearest < margin * TOO_CLOSE {
                squeezed += 1;
            }
        }
    }

    assert!(routed > 100, "only {routed} routed wires; this proves nothing");
    assert!(
        squeezed * 3 < routed,
        "{squeezed} of {routed} routed wires are drawn closer than half a clearance \
         to a node they are only passing"
    );
}

/// No node could be moved to a different column and shorten the picture.
///
/// Which is the whole of what the layering is for, and is exactly checkable.
/// Moving one node right by a column lengthens every wire coming into it and
/// shortens every wire leaving it, so a node with more weight behind it than
/// ahead belongs as far left as its neighbours allow, one with more ahead
/// belongs as far right, and one evenly balanced may sit anywhere between.
/// Anything else is wire spent for nothing.
///
/// This is necessary rather than sufficient — a ranking no single move can
/// improve is still only a local optimum, and what the solver promises is the
/// global one. It is what catches a solver that has quietly stopped solving.
#[test]
fn no_node_is_in_a_column_that_wastes_wire() {
    const GRAPHS: u64 = 100;
    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0;

    for seed in 1..=GRAPHS {
        let case = route_with(&mut Rng::new(seed), 0.0);

        // Columns, read back off the layout: nodes in one share an x.
        let mut xs: Vec<i64> = case.graph.nodes().map(|n| n.position.x as i64).collect();
        xs.sort_unstable();
        xs.dedup();
        let column = |id| {
            let x = case.graph.node(id).unwrap().position.x as i64;
            xs.iter().position(|v| *v == x).unwrap() as i64
        };

        for node in case.graph.nodes() {
            let (before, after) = (
                case.graph.predecessors(node.id),
                case.graph.successors(node.id),
            );
            // Weight is wires, not neighbours: two wires to one node pull twice.
            let weigh = |out: bool| {
                case.graph
                    .connections()
                    .filter(|c| if out { c.from.node } else { c.to.node } == node.id)
                    .count()
            };
            let (pull_back, pull_on) = (weigh(false), weigh(true));
            let want = match pull_back.cmp(&pull_on) {
                Ordering::Greater => before.iter().map(|&p| column(p) + 1).max(),
                Ordering::Less => after.iter().map(|&s| column(s) - 1).min(),
                Ordering::Equal => None,
            };
            let Some(want) = want else {
                continue;
            };
            checked += 1;
            let at = column(node.id);
            if at != want {
                failures.push(format!(
                    "seed {seed}: {:?} sits in column {at} where {want} costs less wire \
                     ({pull_back} in, {pull_on} out)",
                    node.id
                ));
            }
        }
    }

    assert!(checked > 100, "only {checked} nodes had a say; this proves nothing");
    assert!(
        failures.is_empty(),
        "{} of {checked} nodes are in a column that wastes wire:\n{}",
        failures.len(),
        report(&failures)
    );
}

