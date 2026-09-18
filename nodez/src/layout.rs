//! Automatic layout: arrange a graph left-to-right in dependency columns, and
//! tidy up a hand-placed selection with [`align`] and [`distribute`].
//!
//! Useful for graphs built in code, or for tidying one up after a load.

use std::collections::HashMap;

use egui::{Pos2, Rect, Vec2, pos2};

use crate::graph::{ConnectionId, CycleError, Graph, Node, NodeData, NodeId};

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

/// How much room [`route_links`] leaves around the nodes it steers wires past.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteOptions {
    /// Clearance kept outside a column when there is no channel to use.
    pub margin: f32,
    /// How far apart wires sharing a lane are fanned, so they stay countable.
    pub spread: f32,
    /// A gap narrower than this is not worth threading a wire through.
    pub min_lane: f32,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            margin: 24.0,
            spread: 7.0,
            min_lane: 20.0,
        }
    }
}

/// Bend every wire that spans more than one column around the nodes in
/// between, instead of letting it pass under them.
///
/// Expects a graph already arranged in columns, as [`layered`] leaves it, and
/// is meant to run straight after it. Wires that only reach the next column
/// are left straight, and re-running replaces the previous routing rather than
/// adding to it.
///
/// Only [`crate::Connection::waypoints`] changes, so nothing about traversal
/// or evaluation is affected.
pub fn route_links<N: NodeData>(
    graph: &mut Graph<N>,
    options: &RouteOptions,
    size_of: impl Fn(&Graph<N>, &Node<N>) -> Vec2,
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

    // Each column's nodes, top to bottom, and how far it reaches across.
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
    // alongside one, where it would run through the sockets.
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
        .map(|c| (c.id, c.from.node, c.to.node))
        .collect();
    links.sort_by_key(|(id, _, _)| id.0);

    // Pass one: which columns each wire has to cross, and the clear lane
    // through each of them.
    let mut routes: Vec<Route> = Vec::new();
    for (id, from, to) in links {
        let mut route = Route {
            link: id,
            crossings: Vec::new(),
        };
        let (Some(&start), Some(&end)) = (depths.get(&from), depths.get(&to)) else {
            routes.push(route);
            continue;
        };
        let (Some(from_rect), Some(to_rect)) = (rects.get(&from), rects.get(&to)) else {
            routes.push(route);
            continue;
        };
        // Neighboring columns already have a clear channel between them.
        if end > start + 1 {
            for column in start + 1..end {
                let Some(rects) = columns.get(&column) else {
                    continue;
                };
                // Where the wire would like to be by the time it gets here.
                let t = (column - start) as f32 / (end - start) as f32;
                let want =
                    from_rect.center().y + (to_rect.center().y - from_rect.center().y) * t;
                route.crossings.push(Crossing {
                    column,
                    lane: nearest_lane(rects, want, options),
                    want,
                });
            }
        }
        routes.push(route);
    }

    // Pass two: fan the wires that chose the same lane, ordered by where each
    // was heading, so a bundle neither overlaps nor crosses itself.
    let mut lanes: HashMap<(usize, i32), Vec<Sharer>> = HashMap::new();
    for (r, route) in routes.iter().enumerate() {
        for (c, crossing) in route.crossings.iter().enumerate() {
            lanes
                .entry((crossing.column, (crossing.lane / 4.0).round() as i32))
                .or_default()
                .push(Sharer {
                    route: r,
                    crossing: c,
                    want: crossing.want,
                });
        }
    }
    let mut offsets: HashMap<(usize, usize), f32> = HashMap::new();
    for sharers in lanes.values_mut() {
        sharers.sort_by(|a, b| a.want.total_cmp(&b.want).then(a.route.cmp(&b.route)));
        let last = sharers.len().saturating_sub(1) as f32;
        for (k, sharer) in sharers.iter().enumerate() {
            offsets.insert(
                (sharer.route, sharer.crossing),
                (k as f32 - last * 0.5) * options.spread,
            );
        }
    }

    for (r, route) in routes.into_iter().enumerate() {
        let mut waypoints = Vec::with_capacity(route.crossings.len() * 2);
        for (c, crossing) in route.crossings.into_iter().enumerate() {
            let y = crossing.lane + offsets.get(&(r, c)).copied().unwrap_or(0.0);
            // Enter and leave at the same height, so the wire runs flat past
            // the column rather than dipping through it.
            waypoints.push(pos2(channel_before(crossing.column), y));
            waypoints.push(pos2(channel_after(crossing.column), y));
        }
        if let Some(conn) = graph.connection_mut(route.link) {
            conn.waypoints = waypoints;
        }
    }
    Ok(())
}

/// One wire's plan: the columns it has to get past, in order.
struct Route {
    link: ConnectionId,
    crossings: Vec<Crossing>,
}

/// A column a wire crosses, and the lane it picked there.
struct Crossing {
    column: usize,
    lane: f32,
    /// Where the wire was heading, which orders it within a shared lane.
    want: f32,
}

/// One wire's claim on a lane another wire also wants.
struct Sharer {
    route: usize,
    crossing: usize,
    want: f32,
}

/// The clear horizontal lane through a column that sits closest to `want`.
///
/// Candidates are the gaps between the column's nodes, plus the open space
/// above the first and below the last.
fn nearest_lane(rects: &[Rect], want: f32, options: &RouteOptions) -> f32 {
    let mut lanes = Vec::with_capacity(rects.len() + 1);
    if let Some(first) = rects.first() {
        lanes.push(first.top() - options.margin);
    }
    for pair in rects.windows(2) {
        if pair[1].top() - pair[0].bottom() >= options.min_lane {
            lanes.push((pair[0].bottom() + pair[1].top()) * 0.5);
        }
    }
    if let Some(last) = rects.last() {
        lanes.push(last.bottom() + options.margin);
    }

    lanes
        .into_iter()
        .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()))
        .unwrap_or(want)
}
