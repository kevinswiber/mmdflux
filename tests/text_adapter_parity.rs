//! Parity tests for the text adapter.
//!
//! Verifies that `geometry_to_text_layout()` produces rendered output identical
//! to the existing `render()` path for all flowchart fixtures.
//!
//! Compares rendered text (not raw Layout fields) because the Sugiyama pipeline
//! has HashMap-derived non-determinism that can produce different intermediate
//! layouts for the same input across calls.

use std::path::Path;

use mmdflux::diagram::EngineConfig;
use mmdflux::diagrams::flowchart::engine::{MeasurementMode, run_layered_layout};
use mmdflux::diagrams::flowchart::geometry::GraphGeometry;
use mmdflux::render::{
    LayoutConfig, RenderOptions, geometry_to_text_layout, layout_config_for_diagram, render,
    render_text_from_layout,
};
use mmdflux::{Diagram, build_diagram, parse_flowchart};

/// Parse and build a diagram from raw Mermaid input.
fn parse_and_build(input: &str) -> Diagram {
    let flowchart = parse_flowchart(input).expect("Failed to parse");
    build_diagram(&flowchart)
}

/// Produce GraphGeometry via the engine path with text measurement mode.
fn produce_geometry_for_text(diagram: &Diagram, config: &LayoutConfig) -> GraphGeometry {
    use mmdflux::layered::types::{
        Direction as LayeredDirection, LayoutConfig as LayeredConfig, Ranker,
    };

    let direction = match diagram.direction {
        mmdflux::Direction::TopDown => LayeredDirection::TopBottom,
        mmdflux::Direction::BottomTop => LayeredDirection::BottomTop,
        mmdflux::Direction::LeftRight => LayeredDirection::LeftRight,
        mmdflux::Direction::RightLeft => LayeredDirection::RightLeft,
    };

    let mut rank_sep = config.rank_sep;
    if diagram.has_subgraphs() && config.cluster_rank_sep > 0.0 {
        rank_sep += config.cluster_rank_sep;
    }

    let layered_config = LayeredConfig {
        direction,
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: config.ranker.unwrap_or(Ranker::NetworkSimplex),
    };
    let engine_config = EngineConfig::Layered(layered_config);
    run_layered_layout(&MeasurementMode::Text, diagram, &engine_config)
        .expect("run_layered_layout failed")
}

#[test]
fn adapter_produces_identical_render_for_all_fixtures() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");

    let mut fixtures: Vec<_> = std::fs::read_dir(&fixtures_dir)
        .expect("Failed to read fixtures dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "mmd"))
        .map(|e| e.path())
        .collect();
    fixtures.sort();

    assert!(!fixtures.is_empty(), "No fixtures found");

    let mut tested = 0;
    for fixture_path in &fixtures {
        let name = fixture_path.file_name().unwrap().to_string_lossy();
        let input = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("Failed to read {name}: {e}"));

        let diagram = parse_and_build(&input);
        let options = RenderOptions::default();

        // Old path: render() computes layout internally
        let old_text = render(&diagram, &options);

        // New path: engine geometry → adapter → Layout → render from layout
        let mut config = layout_config_for_diagram(&diagram, &options);
        config.ranker = options.ranker;
        let geometry = produce_geometry_for_text(&diagram, &config);
        let new_layout = geometry_to_text_layout(&diagram, &geometry, &config);
        let new_text = render_text_from_layout(&diagram, &new_layout, &options);

        assert_eq!(old_text, new_text, "[{name}] rendered text mismatch");
        tested += 1;
    }

    assert!(tested >= 70, "Expected at least 70 fixtures, got {tested}");
}
