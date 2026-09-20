//! Automatic layout: arrange a graph left-to-right in dependency columns, and
//! tidy up a hand-placed selection with [`align`] and [`distribute`].
//!
//! Useful for graphs built in code, or for tidying one up after a load.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

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
    /// How many passes are spent tidying: ordering each column so its wires
    /// cross as little as possible, then sliding each node to the height of
    /// what it is wired to. Zero leaves nodes in id order and each column
    /// centred.
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

/// Lay the graph out left to right in columns, each node as near as it can be
/// to the things it is wired to.
///
/// A node's column is bounded by dependency — nothing may sit level with or
/// left of what feeds it — but within those bounds it slides to the middle of
/// its neighbours rather than as far left as it can go. Depth alone strands
/// every source node in one tall first column, a whole graph away from the
/// one node each of them feeds; sliding them along is what puts an image node
/// beside the service that uses it.
///
/// Heights are settled the same way: each node wants to be level with the
/// average of what it is wired to, and each column is packed in that order
/// around that average, so a wire between two columns is close to straight.
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
    let depths = columns_of(graph)?;
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

    // Every column centred to start with, which is where they all stay if no
    // passes are asked for.
    let column_height = |column: &[NodeId]| {
        column
            .iter()
            .filter_map(|id| sizes.get(id))
            .map(|size| size.y)
            .sum::<f32>()
            + options.row_gap * (column.len().saturating_sub(1)) as f32
    };
    let tallest = columns
        .iter()
        .map(|column| column_height(column))
        .fold(0.0_f32, f32::max);

    let mut tops: HashMap<NodeId, f32> = HashMap::new();
    for column in &columns {
        let mut y = (tallest - column_height(column)) * 0.5;
        for id in column {
            tops.insert(*id, y);
            y += sizes.get(id).map_or(0.0, |size| size.y) + options.row_gap;
        }
    }

    settle_heights(graph, &columns, &sizes, &mut tops, options);

    // The settling moves whole columns about, so where the graph ends up is
    // only known now. `origin` is where its top-left corner goes.
    let top = tops.values().copied().fold(f32::INFINITY, f32::min);
    let shift = if top.is_finite() {
        options.origin.y - top
    } else {
        0.0
    };

    for (c, column) in columns.iter().enumerate() {
        for id in column {
            if let Some(node) = graph.node_mut(*id) {
                node.position = pos2(column_x[c], tops[id] + shift);
            }
        }
    }

    Ok(())
}

/// Which column each node sits in.
///
/// Dependency depth says only how early a node *may* be, and taking it at its
/// word puts every source in the first column — an image node a whole graph
/// away from the one service that uses it, with a wire the width of the
/// canvas to show for it. So depth is only the starting point: each node then
/// slides to the middle of everything it is wired to, as far as its own
/// neighbours allow it to go, until a pass changes nothing.
///
/// What comes out still respects dependency — a node is never level with or
/// left of what feeds it — because the range it may slide within is exactly
/// what its neighbours leave it.
fn columns_of<N: NodeData>(graph: &Graph<N>) -> Result<HashMap<NodeId, usize>, CycleError> {
    let mut at = graph.depths()?;
    if at.is_empty() {
        return Ok(at);
    }
    let order = graph.topological_order()?;
    let deepest = at.values().copied().max().unwrap_or(0);

    // A node only moves as far as its neighbours have moved already, so a run
    // of them shuffles along one pass at a time and the passes alternate
    // direction to let a run move either way. The loop stops as soon as a
    // pass changes nothing, which on the graphs this was built for is well
    // before the cap.
    const ROUNDS: usize = 8;
    for round in 0..ROUNDS {
        let mut moved = false;
        let sweep: Vec<NodeId> = if round % 2 == 0 {
            order.iter().rev().copied().collect()
        } else {
            order.clone()
        };
        for id in sweep {
            let (before, after) = (graph.predecessors(id), graph.successors(id));
            // As far left and as far right as it may go without drawing level
            // with anything it is wired to.
            let lo = before
                .iter()
                .filter_map(|p| at.get(p))
                .map(|c| c + 1)
                .max()
                .unwrap_or(0);
            let hi = after
                .iter()
                .filter_map(|s| at.get(s))
                .map(|c| c.saturating_sub(1))
                .min()
                .unwrap_or(deepest);
            if lo > hi {
                continue;
            }
            // The middle of what it is wired to, rather than the average: one
            // far-off neighbour should not drag a node away from the handful
            // it sits among.
            let mut want: Vec<usize> = before
                .iter()
                .chain(&after)
                .filter_map(|n| at.get(n).copied())
                .collect();
            if want.is_empty() {
                continue;
            }
            want.sort_unstable();
            let middle = want[want.len() / 2].clamp(lo, hi);
            if at.insert(id, middle) != Some(middle) {
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }

    // Sliding can empty a column, and an empty column is a blank stripe down
    // the middle of the picture.
    let mut used: Vec<usize> = at.values().copied().collect();
    used.sort_unstable();
    used.dedup();
    let packed: HashMap<usize, usize> = used.iter().enumerate().map(|(i, &c)| (c, i)).collect();
    Ok(at.into_iter().map(|(id, c)| (id, packed[&c])).collect())
}

/// Slide each column's nodes to the height of what they are wired to.
///
/// A column centred on the canvas puts every node in it nowhere in
/// particular; a node level with the socket it feeds makes its wire a
/// straight line. Each pass asks every node where it would like to be — the
/// average height of its neighbours — orders its column by that, and packs
/// the column back together in that order around the average of the wishes.
/// Packing is what keeps nodes from overlapping; the ordering is what keeps
/// the column from having to cross itself to grant them.
fn settle_heights<N: NodeData>(
    graph: &Graph<N>,
    columns: &[Vec<NodeId>],
    sizes: &HashMap<NodeId, Vec2>,
    tops: &mut HashMap<NodeId, f32>,
    options: &LayoutOptions,
) {
    let height = |id: &NodeId| sizes.get(id).map_or(0.0, |size| size.y);
    for round in 0..options.sweeps {
        // Either way along the graph, so a column answers to the one before
        // it as often as to the one after.
        let sweep: Vec<usize> = if round % 2 == 0 {
            (0..columns.len()).collect()
        } else {
            (0..columns.len()).rev().collect()
        };
        for c in sweep {
            let column = &columns[c];
            if column.is_empty() {
                continue;
            }
            let centre = |id: NodeId, tops: &HashMap<NodeId, f32>| {
                tops.get(&id).copied().unwrap_or(0.0) + height(&id) * 0.5
            };
            let mut wishes: Vec<(f32, NodeId)> = column
                .iter()
                .map(|&id| {
                    let mut neighbours = graph.predecessors(id);
                    neighbours.extend(graph.successors(id));
                    let want = if neighbours.is_empty() {
                        centre(id, tops)
                    } else {
                        neighbours.iter().map(|&n| centre(n, tops)).sum::<f32>()
                            / neighbours.len() as f32
                    };
                    (want, id)
                })
                .collect();
            // Ties by id, so the same graph settles the same way twice.
            wishes.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

            let total = column.iter().map(height).sum::<f32>()
                + options.row_gap * (column.len().saturating_sub(1)) as f32;
            let mean = wishes.iter().map(|(want, _)| want).sum::<f32>() / wishes.len() as f32;
            let mut y = mean - total * 0.5;
            for (_, id) in &wishes {
                tops.insert(*id, y);
                y += height(id) + options.row_gap;
            }
        }
    }
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
    /// What a corner costs, against a pixel of wire. Higher buys straighter
    /// paths at the price of longer ones.
    pub bend: f32,
    /// What going over a wire already drawn costs, in the same pixels. Higher
    /// buys a picture that is easier to follow at the price of wire that goes
    /// the long way round to keep out of another's way.
    pub cross: f32,
    /// What running hard against a node costs, per pixel of wire, against a
    /// pixel of wire in the open. Nothing is charged at a full clearance or
    /// beyond it; the charge comes on as a wire crosses into the room it was
    /// told to leave, and is worst where the wire is touching.
    ///
    /// This is what keeps a wire from disappearing into the body of a node it
    /// is only passing. A route that squeezes past is always shorter than one
    /// that goes round, so without a price on the squeeze it always wins.
    pub hug: f32,
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
            bend: 40.0,
            // Three corners' worth: enough that a wire will take the long way
            // round a bundle, not so much that it tours the canvas to dodge
            // one wire.
            cross: 120.0,
            // A run drawn against a node costs three times a run in the open:
            // itself, plus twice over for where it is.
            hug: 2.0,
            curvature: 0.5,
            min_curve: 30.0,
            max_curve: 180.0,
        }
    }
}

/// Steer wires around the nodes, and around each other.
///
/// A wire whose own curve already clears every node and goes over no other
/// wire is left alone, so simple graphs keep their plain noodles. Anything
/// else is given an orthogonal path found by search rather than by rule, and
/// keeps the curve only if the curve still works out cheaper.
///
/// The search runs over the grid of lines drawn a clearance out from every
/// node's own four sides. An optimal orthogonal path can always be laid on
/// those lines, so nothing is lost by looking only there, and a path is only
/// ever built from segments that miss every node — a wire through a node is
/// not something the search can express. Among the paths that exist it takes
/// the cheapest, counting length, a penalty per corner, and a penalty per
/// wire already drawn that it goes over — which is what keeps a wire from
/// touring the canvas to save a bend or a crossing.
///
/// Crossings are what a long diagonal costs and a plain length never shows: a
/// wire that sets off straight for a socket far away and below cuts whatever
/// runs between, where the same wire dropping to its socket's height first
/// and running across meets almost nothing. Priced against length and corners,
/// the router picks whichever actually reads better.
///
/// Wires are routed in turn, each preferring to keep off the segments already
/// spoken for, so a bundle running the same way spreads into its own lines
/// rather than stacking into one.
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
    // A cyclic graph has no business being laid out, and saying so here is
    // what callers have always got back.
    graph.depths()?;

    let rects: HashMap<NodeId, Rect> = graph
        .nodes()
        .map(|node| {
            (
                node.id,
                Rect::from_min_size(node.position, size_of(graph, node)),
            )
        })
        .collect();

    let mut links: Vec<_> = graph
        .connections()
        .map(|c| (c.id, c.from.clone(), c.to.clone(), c.order))
        .collect();
    links.sort_by_key(|(id, _, _, _)| id.0);

    // Routed twice over. The first pass has only the wires before it to keep
    // out of the way of, which is enough to fan a bundle but not enough to
    // judge a detour: a wire that goes the long way round to dodge three
    // crossings can land squarely in front of six wires not placed yet. The
    // second pass sees the whole picture and each wire answers to all of it.
    const PASSES: usize = 2;

    let mut plans: Vec<Plan> = Vec::new();
    for pass in 0..PASSES {
        for (at, (id, from, to, slot)) in links.iter().enumerate() {
            // Everything except this wire's own last go at it.
            let others = claims(&plans, at);

            // A link to a node that is not there has nowhere to be routed;
            // it has no shape either, so nothing else has to mind it.
            let (Some(from_rect), Some(to_rect)) = (rects.get(&from.node), rects.get(&to.node))
            else {
                keep(&mut plans, pass, at, Plan::straight(*id, Vec::new()));
                continue;
            };
            let a = anchor_of(graph, from, SocketKind::Output, None)
                .unwrap_or_else(|| pos2(from_rect.right(), from_rect.center().y));
            let b = anchor_of(graph, to, SocketKind::Input, Some(*slot))
                .unwrap_or_else(|| pos2(to_rect.left(), to_rect.center().y));

            // A wire may pass over the two nodes it belongs to; it has to, to
            // reach a socket on them.
            let obstacles: Vec<Rect> = rects
                .iter()
                .filter(|(id, _)| **id != from.node && **id != to.node)
                .map(|(_, rect)| *rect)
                .collect();

            let plan = plan_link(*id, a, b, &obstacles, &others, options);
            keep(&mut plans, pass, at, plan);
        }
    }

    spread_bundles(&mut plans, options);

    for Plan {
        link,
        path,
        clearance,
        ..
    } in plans
    {
        let path = guard_corners(&path, clearance);
        if let Some(conn) = graph.connection_mut(link) {
            // The sockets are where they are; only what happens between them
            // is the router's to say.
            conn.waypoints = if path.len() > 2 {
                path[1..path.len() - 1].to_vec()
            } else {
                Vec::new()
            };
        }
    }
    Ok(())
}

/// How one wire gets from `a` to `b`, given what the others are doing.
fn plan_link(
    link: ConnectionId,
    a: Pos2,
    b: Pos2,
    obstacles: &[Rect],
    others: &Claims,
    options: &RouteOptions,
) -> Plan {
    // What the plain curve would cost, if it can be drawn at all: how far it
    // runs, and how many wires it goes over on the way.
    let curve = curve_points(a, b, options);
    let plain = clears(&curve, obstacles, options).then(|| {
        (
            length(&curve),
            crossings(&curve, &others.drawn, a, b, options),
        )
    });

    // A curve that clears the nodes and cuts across nothing is already the
    // best this wire could do, and the search would only find something
    // longer. This is also the fast path, and the one every wire in an
    // uncrowded graph takes.
    if let Some((_, 0)) = plain {
        return Plan::straight(link, curve);
    }

    // A wire hemmed in on all sides may have no path at a full clearance and
    // an obvious one at half, so it is offered less room rather than given up
    // on: a wire drawn tight past a node beats one drawn through it.
    //
    // All three clearances are searched and the cheapest route taken, not the
    // first that works. Room to spare is worth something but it is not worth
    // everything, and insisting on it can send a wire the width of the canvas
    // to get round what it could have squeezed past — which costs the reader
    // far more than the gap it bought.
    const CLEARANCES: [f32; 3] = [1.0, 0.5, 0.125];
    let room = |scale: f32| (options.margin * scale).max(2.0);
    let found = CLEARANCES
        .into_iter()
        .filter_map(|scale| {
            let pad = room(scale);
            search(a, b, obstacles, others, pad, true, options).map(|route| (route, pad))
        })
        .min_by(|(one, _), (two, _)| one.1.total_cmp(&two.1))
        // And only then with the rule against turning back lifted — a wire
        // that jogs the wrong way for a moment still beats one drawn through
        // a node, and a wire boxed in on every side is the only one that ever
        // needs to.
        .or_else(|| {
            CLEARANCES.into_iter().find_map(|scale| {
                let pad = room(scale);
                search(a, b, obstacles, others, pad, false, options).map(|route| (route, pad))
            })
        });

    // The route and the curve are priced in the same pixels, so which of them
    // a wire gets is a comparison rather than a rule: a route has to save
    // enough crossings to pay for the ground and the corners it spends doing
    // it.
    match found {
        // Nothing orthogonal gets there either. The plain curve at least says
        // where the wire goes.
        None => Plan::straight(link, curve),
        Some(((path, cost), clearance)) => {
            if plain.is_some_and(|(len, over)| len + over as f32 * options.cross <= cost) {
                return Plan::straight(link, curve);
            }
            // Straightened away, so everything downstream sees whole
            // stretches rather than the string of grid steps they were found
            // as. Fanning one step out of a run would kink it.
            let path = simplify(&path);
            Plan {
                link,
                shape: path.clone(),
                path,
                clearance,
            }
        }
    }
}

/// Put a plan where it belongs: appended on the first pass over the wires,
/// and in place of the last one on every pass after it.
fn keep(plans: &mut Vec<Plan>, pass: usize, at: usize, plan: Plan) {
    if pass == 0 {
        plans.push(plan);
    } else {
        plans[at] = plan;
    }
}

/// What the other wires have claimed.
struct Claims {
    /// Every stretch of line they run along. A wire pays a little to share
    /// one, which is what fans a bundle out.
    taken: Vec<Run>,
    /// Every wire as it will actually be drawn, curve or route. A wire pays
    /// rather more to go over one.
    drawn: Vec<[Pos2; 2]>,
}

/// What every wire but one has claimed.
fn claims(plans: &[Plan], except: usize) -> Claims {
    let mut taken = Vec::new();
    let mut drawn = Vec::new();
    for (at, plan) in plans.iter().enumerate() {
        if at == except {
            continue;
        }
        taken.extend(runs(&plan.path).filter(Run::real));
        drawn.extend(legs(&plan.shape));
    }
    Claims { taken, drawn }
}

/// One wire's route, and how much room it was found with.
struct Plan {
    link: ConnectionId,
    path: Vec<Pos2>,
    /// The wire as the editor will draw it, route or curve, so the wires
    /// routed around it know where it actually runs.
    shape: Vec<Pos2>,
    /// The clearance the path keeps from every node, which is as far as any
    /// of it may later be nudged.
    clearance: f32,
}

impl Plan {
    /// A wire left to its own curve.
    fn straight(link: ConnectionId, curve: Vec<Pos2>) -> Self {
        Self {
            link,
            path: Vec::new(),
            shape: curve,
            clearance: 0.0,
        }
    }
}

/// One straight stretch of a routed wire, on the line it runs along.
#[derive(Clone, Copy)]
struct Run {
    /// Whether it runs across rather than down.
    across: bool,
    /// The line it sits on: its y if across, its x if down.
    line: f32,
    lo: f32,
    hi: f32,
}

/// The straight stretches a path is made of, one per leg and in step with the
/// path's own indices, so a leg can be found again to move it.
fn runs(path: &[Pos2]) -> impl Iterator<Item = Run> + '_ {
    path.windows(2).map(|leg| {
        let (p, q) = (leg[0], leg[1]);
        let across = (p.y - q.y).abs() <= (p.x - q.x).abs();
        let (line, lo, hi) = if across {
            (p.y, p.x.min(q.x), p.x.max(q.x))
        } else {
            (p.x, p.y.min(q.y), p.y.max(q.y))
        };
        Run {
            across,
            line,
            lo,
            hi,
        }
    })
}

impl Run {
    /// Whether it covers enough ground to be worth anyone's attention.
    fn real(&self) -> bool {
        self.hi - self.lo > 0.5
    }
}

/// Whether two runs lie along the same line and cover any of the same ground.
fn shares(a: &Run, b: &Run, options: &RouteOptions) -> bool {
    a.across == b.across && (a.line - b.line).abs() < options.spread && a.hi > b.lo && b.hi > a.lo
}

/// The cheapest orthogonal path from one socket to the other and what it
/// cost, or nothing if the sockets cannot be joined without crossing a node.
///
/// The wire leaves and arrives along its own height: the first and last steps
/// are across, so it meets each socket the way a socket expects to be met.
fn search(
    a: Pos2,
    b: Pos2,
    obstacles: &[Rect],
    others: &Claims,
    pad: f32,
    onward: bool,
    options: &RouteOptions,
) -> Option<(Vec<Pos2>, f32)> {
    // The lines to search: a clearance outside each node's sides, plus the
    // two sockets' own row and column so the path can start and finish.
    //
    // Only nodes near the wire contribute lines — a node the other side of
    // the canvas has nothing useful to offer this wire, and every line costs
    // the search. What blocks a step is still judged against every node, so
    // trimming the lines can only cost a wire a better path, never let it
    // through something.
    let (near_lo, near_hi) = (a.x.min(b.x) - pad * 6.0, a.x.max(b.x) + pad * 6.0);
    let near: Vec<&Rect> = obstacles
        .iter()
        .filter(|rect| rect.right() >= near_lo && rect.left() <= near_hi)
        .collect();
    let (mut loose_x, mut loose_y) = (vec![a.x + pad, b.x - pad], Vec::new());
    for rect in &near {
        loose_x.extend([rect.left() - pad, rect.right() + pad]);
        loose_y.extend([rect.top() - pad, rect.bottom() + pad]);
    }
    // And down the middle of every clear gap between the nodes. A wire has to
    // change height somewhere, and the open middle of a gap is the civil place
    // to do it: hard against a node's side it crowds that node's sockets, and
    // the node has nothing to do with the wire.
    loose_x.extend(gap_centers(&near));
    let xs = tidy(&[a.x, b.x], &loose_x);
    let ys = tidy(&[a.y, b.y], &loose_y);

    // How hemmed in each line is, so the search can prefer a roomy one. Every
    // way across costs the same length, so without this the choice of line is
    // settled by nothing at all.
    //
    // Measured against the clearance the wire was *asked* for, not the one
    // this attempt settled for: a route found at a quarter of the clearance
    // runs a quarter of a clearance from everything, and judging it by its
    // own reduced yardstick would call that roomy. It is the same graph and
    // the same eye looking at it either way.
    //
    // And it runs out at exactly that clearance, so a wire keeping the room
    // it was asked for pays nothing at all. What is priced is only the room a
    // wire gives up, which is the room the reader loses.
    let elbow = options.margin;
    let crowding = |lines: &[f32], lo: fn(&Rect) -> f32, hi: fn(&Rect) -> f32| -> Vec<f32> {
        lines
            .iter()
            .map(|&line| {
                let room = near
                    .iter()
                    .map(|rect| (lo(rect) - line).max(line - hi(rect)).max(0.0))
                    .fold(f32::INFINITY, f32::min);
                1.0 - (room / elbow).clamp(0.0, 1.0)
            })
            .collect()
    };
    // Both ways. Charging only the climbs left a wire free to run the length
    // of a node's top edge, close enough to read as part of it.
    let crowding_x = crowding(&xs, Rect::left, Rect::right);
    let crowding_y = crowding(&ys, Rect::top, Rect::bottom);

    // A dense enough graph can ask for more grid than the search is worth.
    const CELLS: usize = 40_000;
    if xs.len() * ys.len() > CELLS {
        return None;
    }

    // Where the wires already drawn cut each of those lines, worked out once
    // per line rather than once per step: a step is as short as the grid is
    // fine and there are thousands of them, where there are only ever a few
    // dozen lines.
    let over = Crossings::new(&xs, &ys, &others.drawn, a, b, options.margin);

    let (w, h) = (xs.len(), ys.len());
    let at = |x: f32, xs: &[f32]| xs.iter().position(|v| (v - x).abs() < f32::EPSILON);
    let (sx, sy) = (at(a.x, &xs)?, at(a.y, &ys)?);
    let (gx, gy) = (at(b.x, &xs)?, at(b.y, &ys)?);

    // Whether a step between two neighboring lines misses every node. The
    // grid lines sit exactly a clearance out from the sides, so a step along
    // one of them grazes without touching: the comparison has to be strict.
    let open = |across: bool, line: f32, lo: f32, hi: f32| {
        !obstacles.iter().any(|rect| {
            let rect = rect.expand(pad);
            if across {
                line > rect.top() && line < rect.bottom() && hi > rect.left() && lo < rect.right()
            } else {
                line > rect.left() && line < rect.right() && hi > rect.top() && lo < rect.bottom()
            }
        })
    };

    // What a step costs: its length, a penalty for turning to take it, and a
    // little for running where another wire already runs.
    let toll = |across: bool, line: f32, lo: f32, hi: f32| {
        let step = Run {
            across,
            line,
            lo,
            hi,
        };
        // A surcharge on the stretch shared, not a toll per step: steps are
        // as short as the grid is fine, and charging each one would price a
        // long run out of its own best line entirely.
        if others.taken.iter().any(|run| shares(run, &step, options)) {
            (hi - lo) * 0.25
        } else {
            0.0
        }
    };

    // Two states per crossing: one reached going across, one going down, so a
    // corner can be charged for.
    let state = |x: usize, y: usize, across: bool| (y * w + x) * 2 + usize::from(across);
    let mut best = vec![f32::INFINITY; w * h * 2];
    let mut came: Vec<Option<usize>> = vec![None; w * h * 2];
    let mut queue = BinaryHeap::new();

    let guess = |x: usize, y: usize| (xs[x] - b.x).abs() + (ys[y] - b.y).abs();
    let start = state(sx, sy, true);
    best[start] = 0.0;
    queue.push(Step {
        rank: Cost(guess(sx, sy)),
        state: start,
    });

    // Which way this wire is headed, and so which way its steps may go.
    let forward = b.x >= a.x;
    let goal = state(gx, gy, true);
    while let Some(Step { state: here, .. }) = queue.pop() {
        if here == goal {
            let cost = best[here];
            let mut path = vec![b];
            let mut walk = here;
            while let Some(prev) = came[walk] {
                let cell = prev / 2;
                path.push(pos2(xs[cell % w], ys[cell / w]));
                walk = prev;
            }
            path.reverse();
            return Some((path, cost));
        }
        let cell = here / 2;
        let (x, y, across) = (cell % w, cell / w, here % 2 == 1);
        let sofar = best[here];

        // Every other line, either way. Stepping to a line further off is a
        // longer step, not a different kind of one, so the search is free to
        // take the long way when the short one is blocked.
        for (nx, ny, moving) in neighbours(x, y, w, h) {
            // Leaving the socket is a step across and outward, always: a
            // wire that set off downwards would be leaving its socket
            // sideways, and one that set off backwards would be leaving it
            // through its own node.
            // Outward, whichever way the wire is ultimately headed: an
            // output socket is on its node's right side, so anything else is
            // a wire setting off back through its own node.
            if here == start && !(moving && nx > x) {
                continue;
            }
            // And it arrives the same way, so the last thing it does is run
            // into the socket rather than at it.
            if (nx, ny) == (gx, gy) && !(moving && nx > x) {
                continue;
            }
            // In between it only ever gets closer, where it can. A wire in a
            // graph drawn
            // left to right reads as flowing that way, and a step back the
            // way it came breaks that however short it is — a jog of one
            // clearance is still a wire that appears to change its mind.
            if onward && moving && (nx > x) != forward {
                continue;
            }
            let (line, lo, hi) = if moving {
                (ys[y], xs[x].min(xs[nx]), xs[x].max(xs[nx]))
            } else {
                (xs[x], ys[y].min(ys[ny]), ys[y].max(ys[ny]))
            };
            if !open(moving, line, lo, hi) {
                continue;
            }
            let turn = if moving == across { 0.0 } else { options.bend };
            // Running along a line that hugs a node costs more than running
            // along one in the open, by how close it is and how far it goes.
            let hug = options.hug
                * (hi - lo)
                * if moving { crowding_y[y] } else { crowding_x[x] };
            // What it crosses, on the line it is crossing them on.
            let cut = over.on(moving, if moving { y } else { x }, lo, hi) as f32 * options.cross;
            let cost = sofar + (hi - lo) + turn + hug + cut + toll(moving, line, lo, hi);
            let next = state(nx, ny, moving);
            if cost < best[next] {
                best[next] = cost;
                came[next] = Some(here);
                queue.push(Step {
                    rank: Cost(cost + guess(nx, ny)),
                    state: next,
                });
            }
        }
    }
    None
}

/// The middle of each clear stretch of x between the nodes.
///
/// Their sides give the lines that hug them; this gives the lines that do not.
fn gap_centers(rects: &[&Rect]) -> Vec<f32> {
    let mut spans: Vec<(f32, f32)> = rects.iter().map(|r| (r.left(), r.right())).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut out = Vec::new();
    let mut filled = f32::NEG_INFINITY;
    for (left, right) in spans {
        if left > filled && filled.is_finite() {
            out.push((filled + left) * 0.5);
        }
        filled = filled.max(right);
    }
    out
}

/// The four lines a step can reach from here, and whether reaching one means
/// moving across.
///
/// Neighbors only. A long run is a string of short steps that costs the same
/// in total, and a corner is only charged where the direction actually
/// changes, so nothing is lost by not reaching every line at once — and the
/// search stays small enough to run on every wire.
fn neighbours(
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) -> impl Iterator<Item = (usize, usize, bool)> {
    let across = [
        (x + 1 < w).then(|| (x + 1, y, true)),
        (x > 0).then(|| (x - 1, y, true)),
    ];
    let down = [
        (y + 1 < h).then(|| (x, y + 1, false)),
        (y > 0).then(|| (x, y - 1, false)),
    ];
    across.into_iter().chain(down).flatten()
}

/// The lines to search, sorted, with duplicates folded into one.
///
/// The sockets' own lines are kept exactly as given and everything else gives
/// way to them: a socket is where it is, and a line a fraction of a pixel off
/// one would have the wire arrive a fraction of a pixel off its socket.
///
/// Only lines the wire could not be drawn between are folded, though, and a
/// pixel is wider than that. Two nodes whose bottom edges are a pixel apart
/// give two lanes below them, one of which clears both and one of which
/// clears neither; folding the clear one into a socket's line a fraction away
/// loses the only way past, and the wire goes the long way round instead.
fn tidy(sockets: &[f32], loose: &[f32]) -> Vec<f32> {
    // Wide enough that a step between two lines survives [`simplify`]. A step
    // dropped as a doubled point takes its end with it, and the step that
    // followed it is left setting off from where the wire has not got to —
    // which is how a wire ends up leaving its socket sideways.
    const APART: f32 = CLOSE * 1.5;
    let mut out = sockets.to_vec();
    out.sort_by(f32::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
    // Sorted before they are sifted: which of two close lines survives would
    // otherwise depend on which the nodes happened to be visited in, and the
    // same graph would route differently from one run to the next.
    let mut loose = loose.to_vec();
    loose.sort_by(f32::total_cmp);
    for &line in &loose {
        if out.iter().all(|kept| (kept - line).abs() >= APART) {
            out.push(line);
        }
    }
    out.sort_by(f32::total_cmp);
    out
}

/// Two points closer together than this are the same point: a wire drawn
/// through both would show one.
const CLOSE: f32 = 0.5;

/// A wire's place in the search queue, cheapest first.
struct Step {
    rank: Cost,
    state: usize,
}

/// A cost that can be ordered, so the queue can hold it.
struct Cost(f32);

impl PartialEq for Cost {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Cost {}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed, so the heap gives up its cheapest rather than its dearest.
        other.0.total_cmp(&self.0)
    }
}

impl PartialEq for Step {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank
    }
}

impl Eq for Step {}

impl PartialOrd for Step {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Step {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank.cmp(&other.rank)
    }
}

/// Fan the wires that ended up running along the same line, so a bundle can
/// be counted rather than read as one thick wire.
///
/// Only the stretches in the middle of a wire move: the first and last are
/// what meet the sockets. A stretch is nudged no further than the clearance
/// its path was found with, so fanning cannot push any of it into a node, and
/// no further than its own neighbors can absorb — shifting a stretch drags
/// the two corners at its ends, and a neighbor shorter than the shift would
/// be turned back to front by it.
fn spread_bundles(plans: &mut [Plan], options: &RouteOptions) {
    // Every stretch that is allowed to move, with the wire it belongs to.
    let mut movable: Vec<(usize, usize, Run)> = Vec::new();
    for (w, plan) in plans.iter().enumerate() {
        if plan.path.len() < 4 {
            continue;
        }
        for (i, run) in runs(&plan.path).enumerate() {
            if i > 0 && i + 2 < plan.path.len() && run.real() {
                movable.push((w, i, run));
            }
        }
    }

    // Wires sharing a line, ordered along it so the fan does not cross itself.
    let mut spoken_for = vec![false; movable.len()];
    let mut shift: HashMap<(usize, usize), f32> = HashMap::new();
    for i in 0..movable.len() {
        if spoken_for[i] {
            continue;
        }
        let mut group = vec![i];
        for j in i + 1..movable.len() {
            if !spoken_for[j] && shares(&movable[i].2, &movable[j].2, options) {
                spoken_for[j] = true;
                group.push(j);
            }
        }
        spoken_for[i] = true;
        if group.len() < 2 {
            continue;
        }
        group.sort_by(|p, q| movable[*p].2.lo.total_cmp(&movable[*q].2.lo));
        let last = (group.len() - 1) as f32;
        for (k, &g) in group.iter().enumerate() {
            let (w, at, _) = movable[g];
            shift.insert((w, at), (k as f32 - last * 0.5) * options.spread);
        }
    }

    for (w, plan) in plans.iter_mut().enumerate() {
        if plan.path.len() < 4 {
            continue;
        }
        let legs: Vec<Run> = runs(&plan.path).collect();
        for (i, leg) in legs.iter().enumerate() {
            let Some(&want) = shift.get(&(w, i)) else {
                continue;
            };
            let by = allowed(&plan.path, &legs, i, want, plan.clearance);
            if by == 0.0 {
                continue;
            }
            for p in &mut plan.path[i..=i + 1] {
                if leg.across {
                    p.y += by;
                } else {
                    p.x += by;
                }
            }
        }
    }
}

/// Hold each corner's rounding down to what the wire has room for.
///
/// The editor rounds a corner by cutting across it, which takes the drawn wire
/// off the path and towards whatever the corner was drawn around. How far it
/// cuts follows the corner's radius, and the radius follows the length of the
/// two stretches meeting there — so a corner is kept from cutting too deep by
/// giving it short stretches to work with. The extra points sit on the path
/// and change nothing about where the wire goes; they only say where it may
/// start turning.
fn guard_corners(path: &[Pos2], clearance: f32) -> Vec<Pos2> {
    if path.len() < 3 || clearance <= 0.0 {
        return path.to_vec();
    }
    // Three times the clearance leaves a radius of one and a half, which cuts
    // well under a clearance at the corner.
    let hold = clearance * 3.0;
    let mut out = vec![path[0]];
    for (i, leg) in path.windows(2).enumerate() {
        let (from, to) = (leg[0], leg[1]);
        let span = (to - from).length();
        let step = |at: Pos2, towards: Pos2| at + (towards - at).normalized() * hold;
        // A stretch between two corners needs holding at both ends; one
        // running to a socket only at the corner end.
        if span > hold * 2.0 + 1.0 {
            if i > 0 {
                out.push(step(from, to));
            }
            if i + 2 < path.len() {
                out.push(step(to, from));
            }
        }
        out.push(to);
    }
    out
}

/// How much of a wanted nudge a stretch can actually take.
///
/// Bounded by the room the path was found with, and by what the stretches
/// either side have left to give: each of them is shortened at one end by the
/// nudge, and one shortened past nothing would run backwards.
fn allowed(path: &[Pos2], legs: &[Run], at: usize, want: f32, clearance: f32) -> f32 {
    const KEEP: f32 = 1.0;
    let room = (clearance * 0.5).max(0.0);
    let (mut lo, mut hi) = (-room, room);

    // The nudge moves this stretch sideways, which is along the length of the
    // two either side of it: one is lengthened by it and the other shortened,
    // and one shortened past nothing would run backwards. Measuring the
    // stretch before the wire's first corner is also what keeps a nudge from
    // dragging a corner back inside the node the wire set off from.
    let along = |p: Pos2| if legs[at].across { p.y } else { p.x };
    if at > 0 {
        let before = along(path[at]) - along(path[at - 1]);
        if before > 0.0 {
            lo = lo.max(KEEP - before);
        } else {
            hi = hi.min(-KEEP - before);
        }
    }
    if at + 2 < path.len() {
        let after = along(path[at + 2]) - along(path[at + 1]);
        if after > 0.0 {
            hi = hi.min(after - KEEP);
        } else {
            lo = lo.max(KEEP + after);
        }
    }
    if lo > hi { 0.0 } else { want.clamp(lo, hi) }
}

/// The curve the editor would draw between two sockets, as the run of short
/// segments everything here measures it by.
fn curve_points(a: Pos2, b: Pos2, options: &RouteOptions) -> Vec<Pos2> {
    let pull = ((b.x - a.x).abs() * options.curvature)
        .max(options.min_curve + (b.y - a.y).abs() * 0.15)
        .min(options.max_curve);
    let points = [a, pos2(a.x + pull, a.y), pos2(b.x - pull, b.y), b];

    const SAMPLES: usize = 48;
    (0..=SAMPLES)
        .map(|i| cubic(&points, i as f32 / SAMPLES as f32))
        .collect()
}

/// The straight segments a run of points is made of.
fn legs(points: &[Pos2]) -> impl Iterator<Item = [Pos2; 2]> + '_ {
    points.windows(2).map(|leg| [leg[0], leg[1]])
}

fn length(points: &[Pos2]) -> f32 {
    legs(points).map(|leg| leg[0].distance(leg[1])).sum()
}

/// Whether a wire drawn along these points clears every node in its way.
fn clears(points: &[Pos2], obstacles: &[Rect], options: &RouteOptions) -> bool {
    !points.iter().any(|p| {
        obstacles
            .iter()
            .any(|rect| rect.expand(options.margin * 0.5).contains(*p))
    })
}

/// How many of the wires already drawn a wire along these points goes over.
pub(crate) fn crossings(
    points: &[Pos2],
    drawn: &[[Pos2; 2]],
    a: Pos2,
    b: Pos2,
    options: &RouteOptions,
) -> usize {
    legs(points)
        .map(|leg| {
            drawn
                .iter()
                .filter(|other| {
                    meeting(leg, **other).is_some_and(|p| !fanning(p, a, b, options.margin))
                })
                .count()
        })
        .sum()
}

/// Where two segments cross, if they do.
///
/// Half open at one end of each, so two segments meeting at a shared corner
/// are counted once rather than by both of the halves that meet there.
pub(crate) fn meeting(one: [Pos2; 2], two: [Pos2; 2]) -> Option<Pos2> {
    let (r, s) = (one[1] - one[0], two[1] - two[0]);
    let denominator = r.x * s.y - r.y * s.x;
    if denominator.abs() < f32::EPSILON {
        return None;
    }
    let gap = two[0] - one[0];
    let t = (gap.x * s.y - gap.y * s.x) / denominator;
    let u = (gap.x * r.y - gap.y * r.x) / denominator;
    ((0.0..1.0).contains(&t) && (0.0..1.0).contains(&u)).then(|| one[0] + r * t)
}

/// Whether a meeting point is only where wires fan out of a socket they
/// share. Every wire off one output leaves from the same place; that they
/// touch there says nothing about where either of them goes.
fn fanning(p: Pos2, a: Pos2, b: Pos2, pad: f32) -> bool {
    p.distance(a) <= pad || p.distance(b) <= pad
}

/// Where the wires already drawn cut each line the search may run along.
struct Crossings {
    /// For each x line, the heights at which a wire cuts it, sorted.
    down: Vec<Vec<f32>>,
    /// For each y line, the same across.
    across: Vec<Vec<f32>>,
}

impl Crossings {
    fn new(xs: &[f32], ys: &[f32], drawn: &[[Pos2; 2]], a: Pos2, b: Pos2, pad: f32) -> Self {
        let mut down = vec![Vec::new(); xs.len()];
        let mut across = vec![Vec::new(); ys.len()];
        // Only the wires running through the ground this search covers can be
        // crossed by anything it finds.
        let (Some(&left), Some(&right), Some(&top), Some(&bottom)) =
            (xs.first(), xs.last(), ys.first(), ys.last())
        else {
            return Self { down, across };
        };
        let ground = Rect::from_min_max(pos2(left, top), pos2(right, bottom));

        for &[p, q] in drawn {
            if !Rect::from_two_pos(p, q).intersects(ground) {
                continue;
            }
            // Half open, so a wire whose own corner sits exactly on a line is
            // counted by one of the two segments that meet there, not both.
            for (i, &x) in xs.iter().enumerate() {
                if (p.x <= x) != (q.x <= x) {
                    let y = p.y + (q.y - p.y) * (x - p.x) / (q.x - p.x);
                    if !fanning(pos2(x, y), a, b, pad) {
                        down[i].push(y);
                    }
                }
            }
            for (i, &y) in ys.iter().enumerate() {
                if (p.y <= y) != (q.y <= y) {
                    let x = p.x + (q.x - p.x) * (y - p.y) / (q.y - p.y);
                    if !fanning(pos2(x, y), a, b, pad) {
                        across[i].push(x);
                    }
                }
            }
        }
        for line in down.iter_mut().chain(&mut across) {
            line.sort_by(f32::total_cmp);
        }
        Self { down, across }
    }

    /// How many wires a step along one of the lines goes over.
    fn on(&self, moving: bool, index: usize, lo: f32, hi: f32) -> usize {
        let line = if moving {
            &self.across[index]
        } else {
            &self.down[index]
        };
        // Half open again, so a wire crossed exactly where two steps meet is
        // charged to one of them rather than to both.
        line.partition_point(|&at| at < hi) - line.partition_point(|&at| at < lo)
    }
}

fn cubic(points: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    pos2(
        a * points[0].x + b * points[1].x + c * points[2].x + d * points[3].x,
        a * points[0].y + b * points[1].y + c * points[2].y + d * points[3].y,
    )
}

/// Drop waypoints that say nothing: a corner that doubles a neighbor, or one
/// that sits in the middle of a straight run.
fn simplify(points: &[Pos2]) -> Vec<Pos2> {
    let Some((&last, rest)) = points.split_last() else {
        return Vec::new();
    };
    // The ends are sockets, and a socket is where it is. Only what happens in
    // between is there to be tidied.
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len());
    for p in rest {
        if out.last().is_none_or(|prev| prev.distance(*p) > CLOSE) {
            out.push(*p);
        }
    }
    if out.last().is_some_and(|prev| prev.distance(last) <= CLOSE) {
        out.pop();
    }
    out.push(last);

    let mut i = 1;
    while i + 1 < out.len() {
        let (before, at, after) = (out[i - 1], out[i], out[i + 1]);
        let straight = ((at.x - before.x).abs() < CLOSE && (after.x - at.x).abs() < CLOSE)
            || ((at.y - before.y).abs() < CLOSE && (after.y - at.y).abs() < CLOSE);
        if straight {
            out.remove(i);
        } else {
            i += 1;
        }
    }
    out
}
