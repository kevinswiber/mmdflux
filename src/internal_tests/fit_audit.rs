//! Drawing audit calibration: the audit accepts as-authored drawings and
//! rejects candidates known to be broken.

use std::fs;
use std::path::Path;

use super::fit_measure::{layout_fixture, load_fixture};
use super::fit_spacing::{draw, gaps};
use crate::graph::Graph;
use crate::graph::geometry::GraphGeometry;
use crate::graph::grid::GridSpacingOverrides;
use crate::graph::measure::default_proportional_text_metrics;
use crate::mmds::{from_document, hydrate_graph_geometry_from_document_with_diagram};
use crate::render::graph::text::audit::DrawingAudit;
use crate::render::graph::{TextRenderOptions, render_text_from_geometry_measured};
use crate::{OutputFormat, RenderConfig};

fn graph_fixture_names() -> Vec<String> {
    let mut names = Vec::new();
    for family in ["flowchart", "class", "state"] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(family);
        let mut entries: Vec<String> = fs::read_dir(&dir)
            .expect("fixture dir")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.ends_with(".mmd").then(|| format!("{family}/{name}"))
            })
            .collect();
        entries.sort();
        names.extend(entries);
    }
    names
}

fn text_options() -> TextRenderOptions {
    RenderConfig::default().text_render_options(OutputFormat::Text)
}

fn audit_of(graph: &Graph, geometry: &GraphGeometry, options: &TextRenderOptions) -> DrawingAudit {
    render_text_from_geometry_measured(graph, geometry, None, options, true)
        .audit
        .expect("audit requested")
}

/// Materialize Mermaid source through the real runtime and hydrate the graph
/// and layout geometry back from the MMDS document.
fn replay_source(input: &str, strip_ranks: bool) -> (Graph, GraphGeometry) {
    let config = RenderConfig::default();
    let document = crate::materialize_diagram(input, &config).expect("materialize");
    let mut graph = from_document(&document).expect("hydrate graph");
    crate::graph::label_wrap::prepare_wrapped_labels_with_provider(
        &mut graph.edges,
        &default_proportional_text_metrics(),
        config.layout.edge_label_max_width,
    );
    let mut geometry =
        hydrate_graph_geometry_from_document_with_diagram(&document, &graph).expect("geometry");
    if strip_ranks && let Some(projection) = geometry.grid_projection.as_mut() {
        projection.node_ranks.clear();
    }
    (graph, geometry)
}

#[test]
fn fit_audit_as_authored_drawing_is_clean_against_itself() {
    let options = text_options();
    for fixture in graph_fixture_names() {
        let (graph, geometry) = layout_fixture(&fixture);
        let audit = audit_of(&graph, &geometry, &options);
        assert!(audit.regressions_against(&audit).is_empty(), "{fixture}");
    }
}

#[test]
fn fit_audit_as_authored_drawings_keep_labels_and_rank_order() {
    let options = text_options();
    for fixture in graph_fixture_names() {
        let (graph, geometry) = layout_fixture(&fixture);
        let audit = audit_of(&graph, &geometry, &options);
        assert!(audit.missing_node_labels.is_empty(), "{fixture}: {audit:?}");
        assert!(
            audit.rank_order_violations.is_empty(),
            "{fixture}: {audit:?}"
        );
        assert!(audit.node_overlaps.is_empty(), "{fixture}: {audit:?}");
    }
}

const REVERSED_DIRECTION_FIXTURES: &[&str] = &[
    "flowchart/bottom_top.mmd",
    "flowchart/right_left.mmd",
    "flowchart/subgraph_edges_bottom_top.mmd",
    "flowchart/label_clamp_bt_review.mmd",
    "flowchart/label_clamp_rl_review.mmd",
];

#[test]
fn fit_audit_reversed_directions_keep_rank_order() {
    let options = text_options();
    for fixture in REVERSED_DIRECTION_FIXTURES {
        let (graph, geometry) = layout_fixture(fixture);
        let direct = audit_of(&graph, &geometry, &options);
        assert!(direct.rank_order_violations.is_empty(), "{fixture} direct");

        let pinned = TextRenderOptions {
            use_pinned_ranks: true,
            ..options.clone()
        };
        for strip_ranks in [false, true] {
            let (graph, geometry) = replay_source(&load_fixture(fixture), strip_ranks);
            let replayed = audit_of(&graph, &geometry, &pinned);
            assert!(
                replayed.rank_order_violations.is_empty(),
                "{fixture} replayed (ranks stripped: {strip_ranks}): {replayed:?}"
            );
        }
    }
}

/// Transpose root and subgraph directions relative to their own axes (the
/// rotate-relative flip), then drop overrides equal to their parent's.
fn rotate_relative_flip(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let flipped = |dir: &str| {
                match dir {
                    "TD" | "TB" => "LR",
                    "LR" => "TD",
                    "BT" => "RL",
                    "RL" => "BT",
                    other => other,
                }
                .to_string()
            };
            if let Some(dir) = trimmed.strip_prefix("graph ") {
                format!("{indent}graph {}", flipped(dir.trim()))
            } else if let Some(dir) = trimmed.strip_prefix("flowchart ") {
                format!("{indent}flowchart {}", flipped(dir.trim()))
            } else if let Some(dir) = trimmed.strip_prefix("direction ") {
                format!("{indent}direction {}", flipped(dir.trim()))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fit_audit_rejects_rotate_relative_flip_of_nested_overrides() {
    let options = text_options();
    for fixture in [
        "flowchart/subgraph_direction_nested_mixed.mmd",
        "flowchart/subgraph_direction_nested_both.mmd",
    ] {
        let source = load_fixture(fixture);
        let (graph, geometry) = replay_source(&source, false);
        let authored = audit_of(&graph, &geometry, &options);
        let (flipped_graph, flipped_geometry) =
            replay_source(&rotate_relative_flip(&source), false);
        let candidate = audit_of(&flipped_graph, &flipped_geometry, &options);
        let regressions = candidate.regressions_against(&authored);
        assert!(!regressions.is_empty(), "{fixture}: flip accepted");
    }
}

#[test]
fn fit_audit_rejects_rank_gap_of_one_in_top_down() {
    for fixture in ["flowchart/fan_in.mmd", "flowchart/bidirectional.mmd"] {
        let authored = draw(fixture, GridSpacingOverrides::default())
            .audit
            .expect("audit requested");
        let squeezed = draw(fixture, gaps(1, 4)).audit.expect("audit requested");
        let regressions = squeezed.regressions_against(&authored);
        assert!(!regressions.is_empty(), "{fixture}: rank gap 1 accepted");
    }
}
