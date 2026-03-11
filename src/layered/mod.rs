//! Compatibility facade for the layered Sugiyama engine.
//!
//! The implementation now lives under [`crate::engines::graph::layered`].
//! This module preserves the pre-breakup API while the public Rust facade is
//! still being reset in plan task `2.3`.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{HashMap, HashSet};

use graph::LayoutGraph;

pub use crate::engines::graph::layered::{
    DiGraph, Direction, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point,
    Ranker, Rect, SelfEdge, SelfEdgeLayout, layout, layout_with_labels, normalize, types,
};
#[allow(unused_imports)]
pub(crate) use crate::engines::graph::layered::{
    acyclic, bk, border, graph, nesting, network_simplex, order, parent_dummy_chains, position,
    rank,
};

pub(crate) fn make_space_for_edge_labels(lg: &mut LayoutGraph) {
    crate::engines::graph::layered::make_space_for_edge_labels(lg);
}

pub(crate) fn make_space_for_labeled_edges(
    lg: &mut LayoutGraph,
    edge_labels: &HashMap<usize, normalize::EdgeLabelInfo>,
) {
    crate::engines::graph::layered::make_space_for_labeled_edges(lg, edge_labels);
}

pub(crate) fn switch_label_dummies(lg: &mut LayoutGraph, strategy: LabelDummyStrategy) {
    crate::engines::graph::layered::switch_label_dummies(lg, strategy);
}

pub(crate) fn select_label_sides(lg: &mut LayoutGraph) {
    crate::engines::graph::layered::select_label_sides(lg);
}

pub(crate) fn extract_self_edges(lg: &mut LayoutGraph) {
    crate::engines::graph::layered::extract_self_edges(lg);
}

fn insert_self_edge_dummies(lg: &mut LayoutGraph) {
    crate::engines::graph::layered::insert_self_edge_dummies(lg);
}

fn position_self_edges(lg: &LayoutGraph, config: &LayoutConfig) -> Vec<SelfEdgeLayout> {
    crate::engines::graph::layered::position_self_edges(lg, config)
}

fn translate_layout_result(
    result: &mut LayoutResult,
    margin_x: f64,
    margin_y: f64,
    direction: Direction,
) {
    crate::engines::graph::layered::translate_layout_result(result, margin_x, margin_y, direction);
}

fn assign_node_intersects(result: &mut LayoutResult) {
    crate::engines::graph::layered::assign_node_intersects(result);
}

pub(crate) fn count_forward_edges_per_gap(lg: &LayoutGraph) -> HashMap<i32, usize> {
    crate::engines::graph::layered::count_forward_edges_per_gap(lg)
}

pub(crate) fn compute_rank_sep_overrides(
    lg: &LayoutGraph,
    config: &LayoutConfig,
) -> HashMap<i32, f64> {
    crate::engines::graph::layered::compute_rank_sep_overrides(lg, config)
}

#[allow(dead_code)]
fn label_dummy_gap_ranks(lg: &LayoutGraph) -> HashSet<i32> {
    crate::engines::graph::layered::label_dummy_gap_ranks(lg)
}

#[cfg(test)]
mod tests {
    include!("compat_tests.rs");
}
