//! Grid-native compaction levers: cell gaps and label-aware spacing.

use super::fit_measure::{layout_fixture, layout_source, load_fixture};
use crate::graph::grid::{
    GridGaps, GridLayout, GridSpacingOverrides, geometry_to_grid_layout_with_spacing,
};
use crate::graph::measure::default_proportional_text_metrics;
use crate::graph::routing::route_graph_geometry;
use crate::render::graph::text::audit::DrawingAudit;
use crate::render::graph::text::render_text_from_grid_layout_measured;
use crate::render::graph::{
    GraphTextDrawing, TextRenderOptions, edge_routing_from_style, layout_config_for_diagram,
    render_text_from_geometry_measured,
};
use crate::render::text::CellExtent;
use crate::{OutputFormat, RenderConfig};

fn options_with(spacing: GridSpacingOverrides) -> TextRenderOptions {
    TextRenderOptions {
        grid_spacing: spacing,
        ..RenderConfig::default().text_render_options(OutputFormat::Text)
    }
}

pub(crate) fn gaps(rank_gap: usize, node_gap: usize) -> GridSpacingOverrides {
    GridSpacingOverrides {
        label_aware: false,
        gaps: Some(GridGaps { rank_gap, node_gap }),
    }
}

pub(crate) fn draw(fixture: &str, spacing: GridSpacingOverrides) -> GraphTextDrawing {
    let (graph, geometry) = layout_fixture(fixture);
    render_text_from_geometry_measured(&graph, &geometry, None, &options_with(spacing), true)
}

fn extent(fixture: &str, spacing: GridSpacingOverrides) -> CellExtent {
    draw(fixture, spacing).extent
}

#[test]
fn fit_spacing_node_gap_narrows_top_down_fan_out() {
    let authored = extent(
        "flowchart/five_fan_out.mmd",
        GridSpacingOverrides::default(),
    );
    let compact = extent("flowchart/five_fan_out.mmd", gaps(2, 1));
    assert!(
        compact.width < authored.width,
        "{compact:?} vs {authored:?}"
    );
}

#[test]
fn fit_spacing_gaps_alone_do_not_narrow_labeled_left_right() {
    let authored = extent("flowchart/ci_pipeline.mmd", GridSpacingOverrides::default());
    let compact = extent("flowchart/ci_pipeline.mmd", gaps(2, 2));
    assert!(
        compact.width >= authored.width,
        "{compact:?} vs {authored:?}"
    );
}

pub(crate) fn label_aware(rank_gap: usize, node_gap: usize) -> GridSpacingOverrides {
    GridSpacingOverrides {
        label_aware: true,
        gaps: Some(GridGaps { rank_gap, node_gap }),
    }
}

fn grid_layout(input: &str, spacing: GridSpacingOverrides) -> GridLayout {
    let (graph, geometry) = layout_source(input);
    let options = options_with(spacing);
    let routed = route_graph_geometry(
        &graph,
        &geometry,
        edge_routing_from_style(options.routing_style),
        &default_proportional_text_metrics(),
    );
    let config = layout_config_for_diagram(&graph, &options);
    geometry_to_grid_layout_with_spacing(&graph, &geometry, Some(&routed), &config, &spacing)
}

#[test]
fn fit_spacing_label_aware_narrows_labeled_left_right() {
    let gaps_only = extent("flowchart/ci_pipeline.mmd", gaps(2, 2));
    let aware = extent("flowchart/ci_pipeline.mmd", label_aware(2, 2));
    assert!(aware.width < gaps_only.width, "{aware:?} vs {gaps_only:?}");
}

#[test]
fn fit_spacing_label_aware_sizes_only_the_labeled_gap() {
    let layout = grid_layout(
        "graph LR\n    A -->|deploy release| B\n    B --> C\n",
        label_aware(2, 2),
    );
    let bounds = |id: &str| layout.node_bounds[id];
    let (a, b, c) = (bounds("A"), bounds("B"), bounds("C"));
    let labeled_gap = b.x - (a.x + a.width);
    let unlabeled_gap = c.x - (b.x + b.width);
    assert!(labeled_gap >= "deploy release".len() + 4, "{labeled_gap}");
    assert!(
        unlabeled_gap < labeled_gap,
        "{unlabeled_gap} vs {labeled_gap}"
    );

    let authored = grid_layout(
        "graph LR\n    A -->|deploy release| B\n    B --> C\n",
        GridSpacingOverrides::default(),
    );
    let authored_gap = authored.node_bounds["C"].x
        - (authored.node_bounds["B"].x + authored.node_bounds["B"].width);
    assert!(
        unlabeled_gap < authored_gap,
        "{unlabeled_gap} vs {authored_gap}"
    );
}

#[test]
fn fit_spacing_label_aware_shortens_labeled_top_down() {
    let authored = extent(
        "flowchart/labeled_edges.mmd",
        GridSpacingOverrides::default(),
    );
    let aware = extent(
        "flowchart/labeled_edges.mmd",
        GridSpacingOverrides {
            label_aware: true,
            gaps: None,
        },
    );
    assert!(aware.height < authored.height, "{aware:?} vs {authored:?}");
}

/// Audit a fixture drawn with `spacing`, optionally dropping the label
/// projection anchors so labels follow the uniform scale.
fn audit_with_anchors(fixture: &str, spacing: GridSpacingOverrides, anchors: bool) -> DrawingAudit {
    let input = load_fixture(fixture);
    let (graph, geometry) = layout_source(&input);
    let options = options_with(spacing);
    let routed = route_graph_geometry(
        &graph,
        &geometry,
        edge_routing_from_style(options.routing_style),
        &default_proportional_text_metrics(),
    );
    let config = layout_config_for_diagram(&graph, &options);
    let mut layout =
        geometry_to_grid_layout_with_spacing(&graph, &geometry, Some(&routed), &config, &spacing);
    if !anchors {
        layout.grid_projection.primary_anchors.clear();
    }
    render_text_from_grid_layout_measured(&graph, &layout, Some(&routed), &options, true)
        .audit
        .expect("audit requested")
}

#[test]
fn fit_spacing_label_aware_keeps_labels_on_their_edges() {
    for fixture in [
        "flowchart/ci_pipeline.mmd",
        "flowchart/labeled_edges.mmd",
        "flowchart/inline_label_flowchart.mmd",
    ] {
        let authored = draw(fixture, GridSpacingOverrides::default())
            .audit
            .expect("audit requested");
        for spacing in [label_aware(2, 2), label_aware(4, 2)] {
            let candidate = audit_with_anchors(fixture, spacing, true);
            assert!(
                candidate
                    .displaced_edge_labels
                    .is_subset(&authored.displaced_edge_labels),
                "{fixture} {spacing:?}: {candidate:?}"
            );
            assert!(
                candidate
                    .missing_edge_label_words
                    .is_subset(&authored.missing_edge_label_words),
                "{fixture} {spacing:?}: {candidate:?}"
            );
        }
    }
}

#[test]
fn fit_spacing_label_projection_anchors_are_load_bearing() {
    let fixture = "flowchart/inline_label_flowchart.mmd";
    let authored = draw(fixture, GridSpacingOverrides::default())
        .audit
        .expect("audit requested");
    let uniform = audit_with_anchors(fixture, label_aware(2, 2), false);
    assert!(
        !uniform
            .displaced_edge_labels
            .is_subset(&authored.displaced_edge_labels),
        "{uniform:?}"
    );
}
