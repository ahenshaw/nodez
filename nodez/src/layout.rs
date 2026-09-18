//! Automatic layout: arrange a graph left-to-right in dependency columns, and
//! tidy up a hand-placed selection with [`align`] and [`distribute`].
//!
//! Useful for graphs built in code, or for tidying one up after a load.

use std::collections::HashMap;

use egui::{Pos2, Rect, Vec2, pos2};

use crate::graph::{
    ConnectionId, CycleError, Graph, Node, NodeData, NodeId, SocketKind, SocketRef,
};

/// Spacing knobs for [`layered`].
#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions {
    /// Horizontal gap between columns.
    pub column_gap: f32,
    /// Vertical gap between nodes in a column.
    pub row_gap: f32,
    /// Where the top-left of the laid-out graph ends up.
    pub origin: Pos2,
    /// Crossing-reduction sweeps. Zero keeps nodes in id order.
    pub sweeps: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            column_gap: 60.0,
            row_gap: 24.0,
            origin: pos2(0.0, 0.0),
            sweeps: 4,
        }
    }
}

/// Lay the graph out in columns, one per dependency depth, and center each
/// column vertically.
///
/// `size_of` supplies each node's drawn size. It is handed the graph as well as
/// the node so it can call [`crate::node_size`], which needs both:
///
/// ```no_run
/// # use nodez::{Graph, LayoutOptions, NodeLibrary, EditorStyle, node_size};
/// # fn demo(graph: &mut Graph, library: &NodeLibrary, style: &EditorStyle) {
/// nodez::layered(graph, &LayoutOptions::default(), |g, node| {
///     node_size(g, library, node, style)
/// })
/// .unwrap();
/// # }
/// ```
pub fn layered<N: NodeData>(
    graph: &mut Graph<N>,
    options: &LayoutOptions,
    size_of: impl Fn(&Graph<N>, &Node<N>) -> Vec2,
) -> Result<(), CycleError> {
    let depths = graph.depths()?;
    if depths.is_empty() {
        return Ok(());
    }
    // Measure everything before mutating any positions.
    let sizes: HashMap<NodeId, Vec2> = graph
        .nodes()
        .map(|node| (node.id, size_of(graph, node)))
        .collect();

    let column_count = depths.values().copied().max().unwrap_or(0) + 1;
    let mut columns: Vec<Vec<NodeId>> = vec![Vec::new(); column_count];
    let mut ids: Vec<_> = depths.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        columns[depths[&id]].push(id);
    }

    for _ in 0..options.sweeps {
        order_by_barycenter(graph, &mut columns, true);
        order_by_barycenter(graph, &mut columns, false);
    }

    // Column widths come from the measured sizes, so wide nodes get room and a
    // collapsed one only takes the width it is drawn at.
    let mut column_x = Vec::with_capacity(column_count);
    let mut x = options.origin.x;
    for column in &columns {
        column_x.push(x);
        let width = column
            .iter()
            .filter_map(|id| sizes.get(id))
            .map(|size| size.x)
            .fold(0.0_f32, f32::max);
        x += width + options.column_gap;
    }

    let mut heights: Vec<Vec<f32>> = Vec::with_capacity(column_count);
    let mut column_height = Vec::with_capacity(column_count);
    for column in &columns {
        let hs: Vec<f32> = column
            .iter()
            .filter_map(|id| sizes.get(id).map(|size| size.y))
            .collect();
        let total =
            hs.iter().sum::<f32>() + options.row_gap * (hs.len().saturating_sub(1)) as f32;
        heights.push(hs);
        column_height.push(total);
    }
    let tallest = column_height.iter().copied().fold(0.0_f32, f32::max);

    for (c, column) in columns.iter().enumerate() {
        let mut y = options.origin.y + (tallest - column_height[c]) * 0.5;
        for (r, id) in column.iter().enumerate() {
            if let Some(node) = graph.node_mut(*id) {
                node.position = pos2(column_x[c], y);
            }
            y += heights[c].get(r).copied().unwrap_or(0.0) + options.row_gap;
        }
    }

    Ok(())
}

/// One crossing-reduction sweep: order each column by the mean row of the
/// nodes it connects to in the neighboring column.
fn order_by_barycenter<N: NodeData>(
    graph: &Graph<N>,
    columns: &mut [Vec<NodeId>],
    forward: bool,
) {
    let range: Vec<usize> = if forward {
        (1..columns.len()).collect()
    } else {
        (0..columns.len().saturating_sub(1)).rev().collect()
    };

    for c in range {
        let reference = if forward { c - 1 } else { c + 1 };
        let rows: HashMap<NodeId, f32> = columns[reference]
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i as f32))
            .collect();

        let mut scored: Vec<(f32, NodeId)> = columns[c]
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let neighbors = if forward {
                    graph.predecessors(id)
                } else {
                    graph.successors(id)
                };
                let sum: Vec<f32> = neighbors
                    .iter()
                    .filter_map(|n| rows.get(n).copied())
                    .collect();
                let score = if sum.is_empty() {
                    i as f32
                } else {
                    sum.iter().sum::<f32>() / sum.len() as f32
                };
                (score, id)
            })
            .collect();

        scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        columns[c] = scored.into_iter().map(|(_, id)| id).collect();
    }
}

/// Which edge or axis a selection lines up on, for [`align`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Top,
    Bottom,
    /// Line the vertical center lines up.
    CenterX,
    /// Line the horizontal center lines up.
    CenterY,
}

impl Align {
    /// Whether this alignment moves nodes horizontally.
    fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::CenterX)
    }
}

/// Which way [`distribute`] spreads nodes out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// How much room [`distribute`] leaves between nodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    /// Equal gaps, with the two outermost nodes left where they are.
    Even,
    /// A fixed gap, growing from the first node along the axis.
    Fixed(f32),
}

/// Line a selection up on one edge, or on its center line.
///
/// The target comes from the bounding box of `nodes`, so aligning left moves
/// everything to the leftmost node's edge. Returns the nodes that actually
/// moved, which is what [`crate::EditorAction::NodesMoved`] wants.
///
/// `size_of` measures each node, as it does for [`layered`]. Only `Right`,
/// `Bottom` and the centers consult it; the other edges are the position
/// itself.
pub fn align<N: NodeData>(
    graph: &mut Graph<N>,
    nodes: impl IntoIterator<Item = NodeId>,
    to: Align,
    size_of: impl Fn(&Graph<N>, &Node<N>) -> Vec2,
) -> Vec<NodeId> {
    let measured = measure(graph, nodes, size_of);
    if measured.len() < 2 {
        return Vec::new();
    }

    // One number to line everything up on, read off the selection's bounds.
    let edge = match to {
        Align::Left => min_by(&measured, |p, _| p.x),
        Align::Top => min_by(&measured, |p, _| p.y),
        Align::Right => max_by(&measured, |p, s| p.x + s.x),
        Align::Bottom => max_by(&measured, |p, s| p.y + s.y),
        Align::CenterX => {
            (min_by(&measured, |p, _| p.x) + max_by(&measured, |p, s| p.x + s.x)) * 0.5
        }
        Align::CenterY => {
            (min_by(&measured, |p, _| p.y) + max_by(&measured, |p, s| p.y + s.y)) * 0.5
        }
    };

    let mut moved = Vec::new();
    for (id, position, size) in measured {
        let target = match to {
            Align::Left | Align::Top => edge,
            Align::Right => edge - size.x,
            Align::Bottom => edge - size.y,
            Align::CenterX => edge - size.x * 0.5,
            Align::CenterY => edge - size.y * 0.5,
        };
        let mut next = position;
        if to.is_horizontal() {
            next.x = target;
        } else {
            next.y = target;
        }
        if next != position
            && let Some(node) = graph.node_mut(id)
        {
            node.position = next;
            moved.push(id);
        }
    }
    moved
}

/// Spread a selection out along one axis.
///
/// Gaps are measured between bounding boxes, not between centers, so nodes of
/// different heights end up evenly spaced rather than evenly staggered.
/// Returns the nodes that actually moved.
pub fn distribute<N: NodeData>(
    graph: &mut Graph<N>,
    nodes: impl IntoIterator<Item = NodeId>,
    axis: Axis,
    spacing: Spacing,
    size_of: impl Fn(&Graph<N>, &Node<N>) -> Vec2,
) -> Vec<NodeId> {
    let mut measured = measure(graph, nodes, size_of);
    if measured.len() < 2 {
        return Vec::new();
    }
    let extent = |size: Vec2| match axis {
        Axis::X => size.x,
        Axis::Y => size.y,
    };
    let leading = |position: Pos2| match axis {
        Axis::X => position.x,
        Axis::Y => position.y,
    };
    // Order by where they already are, so nobody jumps past a neighbor.
    measured.sort_by(|a, b| leading(a.1).total_cmp(&leading(b.1)).then(a.0.cmp(&b.0)));

    let gap = match spacing {
        Spacing::Fixed(gap) => gap,
        Spacing::Even => {
            // Pin the outermost two and share out what is left between them.
            let first = &measured[0];
            let last = &measured[measured.len() - 1];
            let span = (leading(last.1) + extent(last.2)) - leading(first.1);
            let filled: f32 = measured.iter().map(|(_, _, size)| extent(*size)).sum();
            (span - filled) / (measured.len() - 1) as f32
        }
    };

    let mut moved = Vec::new();
    let mut cursor = leading(measured[0].1);
    for (id, position, size) in measured {
        let mut next = position;
        match axis {
            Axis::X => next.x = cursor,
            Axis::Y => next.y = cursor,
        }
        cursor += extent(size) + gap;
        if next != position
            && let Some(node) = graph.node_mut(id)
        {
            node.position = next;
            moved.push(id);
        }
    }
    moved
}

/// Collect the position and drawn size of each node that still exists.
fn measure<N: NodeData>(
    graph: &Graph<N>,
    nodes: impl IntoIterator<Item = NodeId>,
    size_of: impl Fn(&Graph<N>, &Node<N>) -> Vec2,
) -> Vec<(NodeId, Pos2, Vec2)> {
    let mut seen = std::collections::HashSet::new();
    nodes
        .into_iter()
        .filter(|id| seen.insert(*id))
        .filter_map(|id| graph.node(id))
        .map(|node| (node.id, node.position, size_of(graph, node)))
        .collect()
}

fn min_by(measured: &[(NodeId, Pos2, Vec2)], f: impl Fn(Pos2, Vec2) -> f32) -> f32 {
    measured
        .iter()
        .map(|(_, p, s)| f(*p, *s))
        .fold(f32::INFINITY, f32::min)
}

fn max_by(measured: &[(NodeId, Pos2, Vec2)], f: impl Fn(Pos2, Vec2) -> f32) -> f32 {
    measured
        .iter()
        .map(|(_, p, s)| f(*p, *s))
        .fold(f32::NEG_INFINITY, f32::max)
}

/// How [`route_links`] steers wires around nodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteOptions {
    /// Clearance kept around the nodes a wire is routed past.
    pub margin: f32,
    /// How far apart wires sharing a lane are fanned, so they stay countable.
    pub spread: f32,
    /// A gap narrower than this is not worth threading a wire through.
    pub min_lane: f32,
    /// The wire shape the editor draws, so the router can tell whether a wire
    /// left alone would actually clear the nodes between its ends. These
    /// mirror `EditorStyle`'s `wire_curvature`, `wire_min_curve` and
    /// `wire_max_curve`; change them there and change them here.
    pub curvature: f32,
    pub min_curve: f32,
    pub max_curve: f32,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            margin: 16.0,
            spread: 7.0,
            min_lane: 18.0,
            curvature: 0.5,
            min_curve: 30.0,
            max_curve: 180.0,
        }
    }
}

/// Steer wires around the nodes they would otherwise cross.
///
/// Expects a graph already arranged in columns, as [`layered`] leaves it, and
/// is meant to run straight after it.
///
/// A wire whose own curve already clears everything between its ends is left
/// alone, so simple graphs keep their plain noodles. Anything else is pinned
/// into the clear channels between columns and threaded through a gap in each
/// column it crosses.
///
/// `anchor_of` gives a socket's position in graph space — [`crate::socket_anchor`]
/// supplies it. Without it the router aims at the middle of a node's edge, and
/// on a tall node that is nowhere near where the wire actually attaches. It is
/// handed the slot a link lands on, so a multi-input is routed to the
/// attachment point the wire really uses rather than to the first of them.
///
/// Only [`crate::Connection::waypoints`] changes, so nothing about traversal
/// or evaluation is affected.
pub fn route_links<N: NodeData>(
    graph: &mut Graph<N>,
    options: &RouteOptions,
    size_of: impl Fn(&Graph<N>, &Node<N>) -> Vec2,
    anchor_of: impl Fn(&Graph<N>, &SocketRef, SocketKind, Option<u32>) -> Option<Pos2>,
) -> Result<(), CycleError> {
    let depths = graph.depths()?;
    if depths.is_empty() {
        return Ok(());
    }

    // Measure first: the plan is built against the graph, then written back.
    let rects: HashMap<NodeId, Rect> = graph
        .nodes()
        .map(|node| (node.id, Rect::from_min_size(node.position, size_of(graph, node))))
        .collect();

    let mut columns: HashMap<usize, Vec<Rect>> = HashMap::new();
    for (id, depth) in &depths {
        if let Some(rect) = rects.get(id) {
            columns.entry(*depth).or_default().push(*rect);
        }
    }
    for rects in columns.values_mut() {
        rects.sort_by(|a, b| a.top().total_cmp(&b.top()));
    }
    let extent: HashMap<usize, (f32, f32)> = columns
        .iter()
        .map(|(depth, rects)| {
            let left = rects.iter().map(|r| r.left()).fold(f32::INFINITY, f32::min);
            let right = rects.iter().map(|r| r.right()).fold(f32::NEG_INFINITY, f32::max);
            (*depth, (left, right))
        })
        .collect();

    // A wire changes height in the clear channel between two columns, never
    // alongside one, where the sockets are.
    let channel_before = |column: usize| -> f32 {
        let (left, _) = extent[&column];
        match column.checked_sub(1).and_then(|prev| extent.get(&prev)) {
            Some((_, prev_right)) => (prev_right + left) * 0.5,
            None => left - options.margin,
        }
    };
    let channel_after = |column: usize| -> f32 {
        let (_, right) = extent[&column];
        match extent.get(&(column + 1)) {
            Some((next_left, _)) => (right + next_left) * 0.5,
            None => right + options.margin,
        }
    };

    let mut links: Vec<_> = graph
        .connections()
        .map(|c| (c.id, c.from.clone(), c.to.clone(), c.order))
        .collect();
    links.sort_by_key(|(id, _, _, _)| id.0);

    // Pass one: decide what each wire needs.
    let mut routes: Vec<Route> = Vec::new();
    for (id, from, to, slot) in links {
        let mut route = Route {
            link: id,
            lane: None,
        };
        let (Some(&start), Some(&end)) = (depths.get(&from.node), depths.get(&to.node)) else {
            routes.push(route);
            continue;
        };
        let (Some(from_rect), Some(to_rect)) = (rects.get(&from.node), rects.get(&to.node))
        else {
            routes.push(route);
            continue;
        };

        // Where the wire really leaves and arrives.
        let a = anchor_of(graph, &from, SocketKind::Output, None)
            .unwrap_or_else(|| pos2(from_rect.right(), from_rect.center().y));
        let b = anchor_of(graph, &to, SocketKind::Input, Some(slot))
            .unwrap_or_else(|| pos2(to_rect.left(), to_rect.center().y));

        // Everything this wire could run into.
        let obstacles: Vec<Rect> = rects
            .iter()
            .filter(|(id, _)| **id != from.node && **id != to.node)
            .map(|(_, rect)| *rect)
            .collect();
        if direct_is_clear(a, b, &obstacles, options) {
            routes.push(route);
            continue;
        }

        // One height clear of every column in the way, so the wire crosses
        // them all in a single run instead of stepping between lanes.
        let mut clear = vec![(f32::NEG_INFINITY, f32::INFINITY)];
        for column in start + 1..end {
            if let Some(rects) = columns.get(&column) {
                clear = intersect(&clear, &clear_intervals(rects, options));
            }
        }
        let (lane, gap) = choose_lane(&clear, a.y, b.y);
        route.lane = Some(Lane {
            exit: channel_after(start),
            entry: channel_before(end),
            y: lane,
            gap,
            from: a.y,
            to: b.y,
        });
        routes.push(route);
    }

    // Pass two: fan the wires that chose the same height, ordered by where
    // each was heading, so a bundle neither overlaps nor crosses itself.
    let mut shared: HashMap<i32, Vec<Sharer>> = HashMap::new();
    for (r, route) in routes.iter().enumerate() {
        if let Some(lane) = &route.lane {
            shared
                .entry((lane.y / 4.0).round() as i32)
                .or_default()
                .push(Sharer {
                    route: r,
                    want: lane.from,
                });
        }
    }
    let mut offsets: HashMap<usize, f32> = HashMap::new();
    for sharers in shared.values_mut() {
        sharers.sort_by(|a, b| a.want.total_cmp(&b.want).then(a.route.cmp(&b.route)));
        let last = sharers.len().saturating_sub(1) as f32;
        for (k, sharer) in sharers.iter().enumerate() {
            offsets.insert(sharer.route, (k as f32 - last * 0.5) * options.spread);
        }
    }

    for (r, route) in routes.into_iter().enumerate() {
        let waypoints = match route.lane {
            None => Vec::new(),
            Some(lane) => {
                // Fanning must not push a wire back over the nodes the lane
                // was picked to clear.
                let y = (lane.y + offsets.get(&r).copied().unwrap_or(0.0))
                    .clamp(lane.gap.0, lane.gap.1);
                // Climb inside the channels, where there is room, and meet
                // both sockets level. Dropping to the socket over the last
                // stretch instead would hook the wire into it sideways.
                simplify(&[
                    pos2(lane.exit, lane.from),
                    pos2(lane.exit, y),
                    pos2(lane.entry, y),
                    pos2(lane.entry, lane.to),
                ])
            }
        };
        if let Some(conn) = graph.connection_mut(route.link) {
            conn.waypoints = waypoints;
        }
    }
    Ok(())
}

/// One wire's plan: nothing, or the one height it crosses everything at.
struct Route {
    link: ConnectionId,
    lane: Option<Lane>,
}

/// The single run a routed wire makes between the channels either side.
struct Lane {
    exit: f32,
    entry: f32,
    y: f32,
    /// The clear strip `y` sits in, which fanning may not leave.
    gap: (f32, f32),
    /// The height the wire starts at, which orders a shared lane.
    from: f32,
    /// The height it has to arrive at.
    to: f32,
}

/// One wire's claim on a height another wire also wants.
struct Sharer {
    route: usize,
    want: f32,
}

/// The strips of height that clear every node in a column.
fn clear_intervals(rects: &[Rect], options: &RouteOptions) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(rects.len() + 1);
    let mut cursor = f32::NEG_INFINITY;
    for rect in rects {
        let top = rect.top() - options.margin;
        if top > cursor {
            out.push((cursor, top));
        }
        cursor = cursor.max(rect.bottom() + options.margin);
    }
    out.push((cursor, f32::INFINITY));
    out
}

/// The strips clear in both of two columns.
fn intersect(a: &[(f32, f32)], b: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if hi > lo {
            out.push((lo, hi));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// The clear height that costs the wire the least climbing.
///
/// A height level with either end is worth most: the wire then leaves or
/// arrives flat, and that bend disappears entirely.
fn choose_lane(intervals: &[(f32, f32)], from: f32, to: f32) -> (f32, (f32, f32)) {
    let (low, high) = (from.min(to), from.max(to));
    let mut best: Option<(f32, f32, (f32, f32))> = None;
    for &(lo, hi) in intervals {
        // The point in this strip needing the least detour from the straight
        // run between the two ends.
        let y = if hi < low {
            hi
        } else if lo > high {
            lo
        } else {
            from.clamp(lo.max(low), hi.min(high))
        };
        let cost = (y - from).abs() + (y - to).abs();
        if best.as_ref().is_none_or(|(c, _, _)| cost < *c) {
            best = Some((cost, y, (lo, hi)));
        }
    }
    best.map_or((from, (from, from)), |(_, y, gap)| (y, gap))
}

/// Whether the curve the editor would draw between two sockets already clears
/// everything in its way.
fn direct_is_clear(a: Pos2, b: Pos2, obstacles: &[Rect], options: &RouteOptions) -> bool {
    let pull = ((b.x - a.x).abs() * options.curvature)
        .max(options.min_curve + (b.y - a.y).abs() * 0.15)
        .min(options.max_curve);
    let points = [a, pos2(a.x + pull, a.y), pos2(b.x - pull, b.y), b];

    const SAMPLES: usize = 48;
    (0..=SAMPLES).all(|i| {
        let p = cubic(&points, i as f32 / SAMPLES as f32);
        !obstacles
            .iter()
            .any(|rect| rect.expand(options.margin * 0.5).contains(p))
    })
}

fn cubic(points: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    pos2(
        a * points[0].x + b * points[1].x + c * points[2].x + d * points[3].x,
        a * points[0].y + b * points[1].y + c * points[2].y + d * points[3].y,
    )
}

/// Drop waypoints that say nothing: a wire already at the right height needs
/// no corner to get there.
fn simplify(points: &[Pos2]) -> Vec<Pos2> {
    const CLOSE: f32 = 0.5;
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_none_or(|last| last.distance(*p) > CLOSE) {
            out.push(*p);
        }
    }
    out
}
