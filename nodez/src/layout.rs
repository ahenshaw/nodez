//! Automatic layout: arrange a graph left-to-right in dependency columns, and
//! tidy up a hand-placed selection with [`align`] and [`distribute`].
//!
//! Useful for graphs built in code, or for tidying one up after a load.

use std::cmp::Ordering::Less;
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
    // alongside one, where the sockets are. Channel `k` is the gap after
    // column `k`.
    let channel = |k: usize| -> Channel {
        let after = extent.get(&k).map(|(_, right)| *right);
        let before = extent.get(&(k + 1)).map(|(left, _)| *left);
        match (after, before) {
            (Some(right), Some(left)) => Channel {
                center: (right + left) * 0.5,
                lo: (right + options.margin).min((right + left) * 0.5),
                hi: (left - options.margin).max((right + left) * 0.5),
            },
            (Some(right), None) => Channel {
                center: right + options.margin,
                lo: right + options.margin,
                hi: right + options.margin * 3.0,
            },
            (None, Some(left)) => Channel {
                center: left - options.margin,
                lo: left - options.margin * 3.0,
                hi: left - options.margin,
            },
            (None, None) => Channel {
                center: 0.0,
                lo: 0.0,
                hi: 0.0,
            },
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
            ends: None,
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

        // One height clear of everything in the way, so the wire crosses it
        // all in a single run instead of stepping between lanes.
        //
        // What counts as in the way is the stretch of x the wire actually
        // runs across, not the columns it nominally spans. Asking the columns
        // takes a node's depth for its position, and the two part company the
        // moment anyone drags a node: it keeps its depth while its box moves
        // into a channel this wire climbs, or out of one it was blocking.
        // Measuring the boxes means a nudged layout routes as well as a
        // pristine one.
        let run = (channel(start).lo, channel(end - 1).hi);
        let mut blocking: Vec<Rect> = obstacles
            .iter()
            .filter(|rect| rect.right() >= run.0 && rect.left() <= run.1)
            .copied()
            .collect();
        blocking.sort_by(|p, q| p.top().total_cmp(&q.top()));
        let clear = clear_intervals(&blocking, options);
        let (lane, gap) = choose_lane(&clear, a.y, b.y);
        route.lane = Some(Lane {
            exit_channel: start,
            entry_channel: end - 1,
            y: lane,
            gap,
            from: a.y,
            to: b.y,
        });
        route.ends = Some((a, b, from.node, to.node));
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

    // The height each wire settles on, once fanning has had its say.
    let height = |r: usize, lane: &Lane| -> f32 {
        (lane.y + offsets.get(&r).copied().unwrap_or(0.0)).clamp(lane.gap.0, lane.gap.1)
    };

    // Pass three: wires climbing the same channel each get their own line down
    // it, the way they each get their own height across. Two wires sharing a
    // channel would otherwise be drawn one on top of the other.
    let mut climbs: HashMap<usize, Vec<Climb>> = HashMap::new();
    for (r, route) in routes.iter().enumerate() {
        let Some(lane) = &route.lane else {
            continue;
        };
        let y = height(r, lane);
        let mut claim = |k: usize, a: f32, b: f32, exit: bool, entry: bool| {
            if (a - b).abs() > 1.0 {
                climbs.entry(k).or_default().push(Climb {
                    route: r,
                    lo: a.min(b),
                    hi: a.max(b),
                    exit,
                    entry,
                });
            }
        };
        if lane.exit_channel == lane.entry_channel {
            // One channel does the whole climb, so it needs one line.
            let lo = lane.from.min(y).min(lane.to);
            let hi = lane.from.max(y).max(lane.to);
            claim(lane.exit_channel, lo, hi, true, true);
        } else {
            claim(lane.exit_channel, lane.from, y, true, false);
            claim(lane.entry_channel, y, lane.to, false, true);
        }
    }

    let mut lines: HashMap<(usize, bool), f32> = HashMap::new();
    for (k, sharing) in &mut climbs {
        let gap = channel(*k);
        // Innermost first, so wires nest rather than cross on the way down.
        sharing.sort_by(|a, b| {
            a.lo.total_cmp(&b.lo)
                .then(b.hi.total_cmp(&a.hi))
                .then(a.route.cmp(&b.route))
        });
        let last = sharing.len().saturating_sub(1) as f32;
        // Spread as far as the channel allows, and no further.
        let step = if last > 0.0 {
            options.spread.min((gap.hi - gap.lo).max(0.0) / last)
        } else {
            0.0
        };
        for (i, climb) in sharing.iter().enumerate() {
            let x = (gap.center + (i as f32 - last * 0.5) * step).clamp(gap.lo, gap.hi);
            if climb.exit {
                lines.insert((climb.route, true), x);
            }
            if climb.entry {
                lines.insert((climb.route, false), x);
            }
        }
    }

    for (r, route) in routes.into_iter().enumerate() {
        let waypoints = match (route.lane, route.ends) {
            (Some(lane), Some((a, b, from_node, to_node))) => {
                let near: Vec<Rect> = rects
                    .iter()
                    .filter(|(id, _)| **id != from_node && **id != to_node)
                    .map(|(_, rect)| *rect)
                    .collect();
                let channel_x = |k: usize, exit: bool| {
                    lines
                        .get(&(r, exit))
                        .copied()
                        .unwrap_or_else(|| channel(k).center)
                };
                // Where a climb may stand. A climb on the target's own edge
                // gives the last leg no length at all, and the wire then drops
                // onto the socket from above, down the face of the node and
                // through whatever other sockets it passes, reading as if it
                // fed every one of them. `outer` is the hard limit: outside
                // both of the wire's own nodes, which are not obstacles to it
                // and so would otherwise be fair game to climb down the face
                // of. `room` keeps a socket's worth of clearance inside that,
                // which is what lets the wire meet each end level.
                let span = |lo: f32, hi: f32| {
                    if lo <= hi {
                        (lo, hi)
                    } else {
                        let mid = (lo + hi) * 0.5;
                        (mid, mid)
                    }
                };
                let from_rect = rects.get(&from_node).copied().unwrap_or(Rect::NOTHING);
                let to_rect = rects.get(&to_node).copied().unwrap_or(Rect::NOTHING);
                let outer = span(from_rect.right(), to_rect.left());
                let room = span(outer.0 + options.margin, outer.1 - options.margin);
                let room = if room.0 <= room.1 { room } else { outer };
                let hold = |x: f32, (lo, hi): (f32, f32)| x.clamp(lo, hi);
                let reach = |x: f32| hold(x, room);
                // The channel as the columns give it, and the channel held
                // back to leave the sockets their room. Both are offered:
                // holding it back is what meets a socket properly, but it can
                // also be what puts the climb inside a node.
                let raw = (
                    hold(channel_x(lane.exit_channel, true), outer),
                    hold(channel_x(lane.entry_channel, false), outer),
                );
                let held = (reach(raw.0), reach(raw.1));

                // Heights worth trying: the lane the wire was given, then
                // clear over the top of what is in its way, then under the
                // bottom of it.
                //
                // In its way means the boxes standing in the stretch this wire
                // actually crosses. Clearing everything on the canvas instead
                // sends a wire over the whole graph and back down for the sake
                // of two sockets a few pixels apart.
                let mut heights = vec![height(r, &lane)];
                let crossing: Vec<&Rect> = near
                    .iter()
                    // Strictly inside the corridor. A node whose edge the wire
                    // sets off from is beside it, not in its way; counting a
                    // whole column that way is what sends a wire diving under
                    // all of it to reach a socket level with where it started.
                    .filter(|rect| rect.right() > outer.0 && rect.left() < outer.1)
                    .collect();
                if !crossing.is_empty() {
                    let top = crossing
                        .iter()
                        .map(|r| r.top())
                        .fold(f32::INFINITY, f32::min);
                    let bottom = crossing
                        .iter()
                        .map(|r| r.bottom())
                        .fold(f32::NEG_INFINITY, f32::max);
                    heights.extend([top - options.margin, bottom + options.margin]);
                }

                // The legs the wire would run, given a height and the two
                // x it climbs at.
                let legs = |y: f32, exit: f32, entry: f32| {
                    [
                        a,
                        pos2(exit, lane.from),
                        pos2(exit, y),
                        pos2(entry, y),
                        pos2(entry, lane.to),
                        b,
                    ]
                };

                // How far short of `room` a pair of climbs leaves the two
                // sockets. Zero when both are met level with a leg long
                // enough to see.
                let stub = |exit: f32, entry: f32| {
                    (room.0 - exit).max(0.0) + (entry - room.1).max(0.0)
                };


                // Each height, climbed four ways: measured against the boxes
                // with the sockets' room kept and with it given up, and the
                // two channels unmeasured. None of them may stand inside the
                // wire's own two nodes. Ranked by how deeply buried the
                // wire is first, how stubby its approach second and how much
                // wire it spends last: a wire under a node is worse than one
                // that meets its socket abruptly, and either is worse than one
                // that merely takes the long way round.
                // How much wire it takes. Two clear answers are not equally
                // good: the one that gets there without touring the canvas is
                // the one to draw.
                let length = |points: &[Pos2; 6]| -> f32 {
                    points
                        .windows(2)
                        .map(|leg| (leg[1].x - leg[0].x).abs() + (leg[1].y - leg[0].y).abs())
                        .sum()
                };

                let mut best: Option<((f32, f32, f32), Vec<Pos2>)> = None;
                for &y in &heights {
                    let tries = [
                        (
                            clear_climb(held.0, a, y, room, &near, options),
                            clear_climb(held.1, b, y, room, &near, options),
                        ),
                        (
                            clear_climb(raw.0, a, y, outer, &near, options),
                            clear_climb(raw.1, b, y, outer, &near, options),
                        ),
                        held,
                        raw,
                    ];
                    for (exit, entry) in tries {
                        let points = legs(y, exit, entry);
                        let cost = (
                            intrusion(&points, &near, options),
                            stub(exit, entry),
                            length(&points),
                        );
                        let better = best
                            .as_ref()
                            .is_none_or(|(worst, _)| cost.partial_cmp(worst) == Some(Less));
                        if better {
                            best = Some((cost, simplify(&points[1..5])));
                        }
                    }
                }
                best.map(|(_, points)| points).unwrap_or_default()
            }
            _ => Vec::new(),
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
    /// Where the wire leaves and arrives, and the two nodes it is allowed to
    /// touch. The last pass needs both to check its own work.
    ends: Option<(Pos2, Pos2, NodeId, NodeId)>,
}

/// The single run a routed wire makes between the channels either side.
struct Lane {
    /// The gap after the column the wire starts in.
    exit_channel: usize,
    /// The gap before the column it ends in.
    entry_channel: usize,
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

/// The clear gap between two columns, where wires change height.
#[derive(Clone, Copy)]
struct Channel {
    center: f32,
    lo: f32,
    hi: f32,
}

/// One wire's climb down a channel, which wants a line of its own.
struct Climb {
    route: usize,
    lo: f32,
    hi: f32,
    /// Whether this is the wire's outward channel, its homeward one, or both.
    exit: bool,
    entry: bool,
}

/// How far a wire's legs reach into the boxes they are meant to avoid; zero
/// when the wire is clear.
///
/// What is measured is how far under a box the wire sits, which is what reads
/// as a wire disappearing rather than clipping a corner.
fn intrusion(points: &[Pos2], boxes: &[Rect], options: &RouteOptions) -> f32 {
    let pad = options.margin * 0.5;
    // The legs, plus the shortcut across each corner. The drawn wire rounds
    // its corners, which takes it diagonally across the turn and past boxes
    // neither leg goes near; the chord between where it starts and stops
    // turning covers that ground.
    let mut runs: Vec<Rect> = Vec::with_capacity(points.len() * 2);
    for leg in points.windows(2) {
        runs.push(Rect::from_two_pos(leg[0], leg[1]));
    }
    for corner in points.windows(3) {
        let (back, at, on) = (corner[0], corner[1], corner[2]);
        let radius = ((at - back).length().min((on - at).length()) * 0.5).min(options.min_curve);
        let step = |towards: Pos2| {
            let away = towards - at;
            if away.length_sq() < f32::EPSILON {
                at
            } else {
                at + away.normalized() * radius
            }
        };
        runs.push(Rect::from_two_pos(step(back), step(on)));
    }

    runs.iter()
        .map(|run| {
            // A run has extent along one axis only, so it is the other one
            // that says how deep it is buried. A corner's chord has both, and
            // the shallower reading is the fair one.
            let horizontal = run.width() >= run.height();
            boxes
                .iter()
                .map(|rect| {
                    let rect = rect.expand(pad);
                    if !rect.intersects(*run) {
                        return 0.0;
                    }
                    let depth = if horizontal {
                        (run.top() - rect.top()).min(rect.bottom() - run.bottom())
                    } else {
                        (run.left() - rect.left()).min(rect.right() - run.right())
                    };
                    // Any touch at all has to cost something, or a run
                    // grazing an edge would score as a clean miss.
                    depth.max(0.0) + 1.0
                })
                .sum::<f32>()
        })
        .sum()
}

/// Slide a climb sideways until it clears the boxes it would run through.
///
/// The wire reaches `socket` along its own height and then climbs to `y`, so
/// both the reach and the climb have to be clear. Candidates are the edges of
/// whatever is in the way, nearest first, which keeps the wire as close to the
/// channel it was given as the boxes allow. `room` is the stretch of x the
/// climb may use, which stops it landing so close to a socket that the wire
/// has no room left to meet it level. If nothing works the wire keeps its
/// channel: a drawn-through node beats a wire flung across the canvas.
fn clear_climb(
    want: f32,
    socket: Pos2,
    y: f32,
    room: (f32, f32),
    boxes: &[Rect],
    options: &RouteOptions,
) -> f32 {
    let want = want.clamp(room.0, room.1);
    let (lo, hi) = (socket.y.min(y), socket.y.max(y));
    let pad = options.margin * 0.5;
    let blocked = |x: f32| {
        boxes.iter().any(|rect| {
            let rect = rect.expand(pad);
            // The climb itself.
            let climbs_through =
                rect.left() <= x && x <= rect.right() && rect.top() < hi && lo < rect.bottom();
            // And the run out to it, level with the socket.
            let reaches_through = rect.top() <= socket.y
                && socket.y <= rect.bottom()
                && rect.left() <= socket.x.max(x)
                && socket.x.min(x) <= rect.right();
            climbs_through || reaches_through
        })
    };
    if !blocked(want) {
        return want;
    }
    let mut best: Option<f32> = None;
    let edges = boxes
        .iter()
        .flat_map(|rect| {
            let rect = rect.expand(pad);
            [rect.left() - pad, rect.right() + pad]
        })
        .chain([room.0, room.1]);
    for candidate in edges {
        {
            if candidate < room.0 || candidate > room.1 || blocked(candidate) {
                continue;
            }
            if best.is_none_or(|b: f32| (candidate - want).abs() < (b - want).abs()) {
                best = Some(candidate);
            }
        }
    }
    best.unwrap_or(want)
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
