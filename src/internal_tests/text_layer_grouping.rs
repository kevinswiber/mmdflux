//! Text layers follow engine ranks when same-rank node sizes differ.
//!
//! BT and RL ranks are aligned on their far edge, so grouping layers on the
//! top-left corner splits a rank whose nodes differ in size.

use super::fit_measure::{layout_fixture, layout_source};
use crate::graph::grid::{GridLayout, geometry_to_grid_layout_with_routed};
use crate::render::graph::{TextRenderOptions, layout_config_for_diagram};

fn grid_layout(
    graph: &crate::graph::Graph,
    geometry: &crate::graph::geometry::GraphGeometry,
) -> GridLayout {
    let config = layout_config_for_diagram(graph, &TextRenderOptions::default());
    geometry_to_grid_layout_with_routed(graph, geometry, None, &config)
}

fn assert_same_layer(layout: &GridLayout, a: &str, b: &str) {
    let (pa, pb) = (layout.grid_positions[a], layout.grid_positions[b]);
    assert_eq!(pa.layer, pb.layer, "{a} at {pa:?}, {b} at {pb:?}");
}

#[test]
fn right_left_same_rank_nodes_share_a_text_layer() {
    let (graph, geometry) = layout_fixture("flowchart/rl_mixed_rank_sizes.mmd");
    assert_same_layer(&grid_layout(&graph, &geometry), "B", "C");
}

#[test]
fn bottom_top_same_rank_nodes_share_a_text_layer() {
    let (graph, geometry) = layout_fixture("flowchart/bt_mixed_rank_sizes.mmd");
    assert_same_layer(&grid_layout(&graph, &geometry), "B", "C");
}

#[test]
fn left_right_same_rank_nodes_share_a_text_layer() {
    let (graph, geometry) = layout_source(
        "graph LR\n  A --> B\n  A --> C[a very long label that is more than thirty cells wide]\n  B --> D\n  C --> D\n",
    );
    assert_same_layer(&grid_layout(&graph, &geometry), "B", "C");
}

#[test]
fn top_down_same_rank_nodes_share_a_text_layer() {
    let tall = (0..30)
        .map(|i| format!("l{i}"))
        .collect::<Vec<_>>()
        .join("<br>");
    let (graph, geometry) = layout_source(&format!(
        "graph TD\n  A --> B\n  A --> C[\"{tall}\"]\n  B --> D\n  C --> D\n"
    ));
    assert_same_layer(&grid_layout(&graph, &geometry), "B", "C");
}
