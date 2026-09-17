//! Automatic layout: arrange a graph left-to-right in dependency columns.
//!
//! Useful for graphs built in code, or for tidying one up after a load.

use std::collections::HashMap;

use egui::{Pos2, pos2};

use crate::graph::{CycleError, Graph, Node, NodeData, NodeId};

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

/// Lay the graph out in columns, one per dependency depth, and centre each
/// column vertically.
///
/// `height_of` supplies each node's drawn height. It is handed the graph as
/// well as the node so it can call [`crate::node_size`], which needs both:
///
/// ```no_run
/// # use nodez::{Graph, LayoutOptions, NodeLibrary, EditorStyle, node_size};
/// # fn demo(graph: &mut Graph, library: &NodeLibrary, style: &EditorStyle) {
/// nodez::layered(graph, &LayoutOptions::default(), |g, node| {
///     node_size(g, library, node, style).y
/// })
/// .unwrap();
/// # }
/// ```
pub fn layered<N: NodeData>(
    graph: &mut Graph<N>,
    options: &LayoutOptions,
    height_of: impl Fn(&Graph<N>, &Node<N>) -> f32,
) -> Result<(), CycleError> {
    let depths = graph.depths()?;
    if depths.is_empty() {
        return Ok(());
    }
    // Measure everything before mutating any positions.
    let heights_by_id: HashMap<NodeId, f32> = graph
        .nodes()
        .map(|node| (node.id, height_of(graph, node)))
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

    // Column widths come from the nodes themselves, so wide nodes get room.
    let mut column_x = Vec::with_capacity(column_count);
    let mut x = options.origin.x;
    for column in &columns {
        column_x.push(x);
        let width = column
            .iter()
            .filter_map(|id| graph.node(*id))
            .map(|n| n.width)
            .fold(0.0_f32, f32::max);
        x += width + options.column_gap;
    }

    let mut heights: Vec<Vec<f32>> = Vec::with_capacity(column_count);
    let mut column_height = Vec::with_capacity(column_count);
    for column in &columns {
        let hs: Vec<f32> = column
            .iter()
            .filter_map(|id| heights_by_id.get(id).copied())
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
/// nodes it connects to in the neighbouring column.
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
