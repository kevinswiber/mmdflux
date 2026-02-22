//! Parity tests for the text adapter.
//!
//! Verifies that `geometry_to_text_layout()` produces rendered output identical
//! to `compute_layout_from_geometry()` for all flowchart fixtures.
//!
//! Both paths use the same `GraphGeometry` input, ensuring the comparison is
//! meaningful despite HashMap-derived non-determinism in the Sugiyama pipeline.

use std::path::Path;

use mmdflux::diagram::EngineConfig;
use mmdflux::diagrams::flowchart::engine::{MeasurementMode, run_layered_layout};
use mmdflux::diagrams::flowchart::geometry::GraphGeometry;
use mmdflux::render::{
    LayoutConfig, RenderOptions, compute_layout_from_geometry, geometry_to_text_layout,
    layout_config_for_diagram, render_text_from_layout,
};
use mmdflux::{Diagram, build_diagram, parse_flowchart};

/// Parse and build a diagram from raw Mermaid input.
fn parse_and_build(input: &str) -> Diagram {
    let flowchart = parse_flowchart(input).expect("Failed to parse");
    build_diagram(&flowchart)
}

/// Produce GraphGeometry via the engine path with text measurement mode.
///
/// Note: rank_sep is NOT pre-adjusted for subgraphs here — the internal
/// round-trip through `layout_config_from_layered` → `layered_config_for_layout`
/// applies cluster_rank_sep automatically. Pre-adjusting would cause a
/// double-adjustment.
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

    let layered_config = LayeredConfig {
        direction,
        node_sep: config.node_sep,
        edge_sep: config.edge_sep,
        rank_sep: config.rank_sep,
        margin: config.margin,
        acyclic: true,
        ranker: config.ranker.unwrap_or(Ranker::NetworkSimplex),
    };
    let engine_config = EngineConfig::Layered(layered_config);
    run_layered_layout(&MeasurementMode::Text, diagram, &engine_config)
        .expect("run_layered_layout failed")
}

/// Verify the adapter produces identical rendered text as the reference path
/// for all flowchart fixtures, using the same geometry for both paths.
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
        let mut config = layout_config_for_diagram(&diagram, &options);
        config.ranker = options.ranker;

        // Produce geometry once, shared by both paths
        let geometry = produce_geometry_for_text(&diagram, &config);

        // Reference path: compute_layout_from_geometry → render
        let ref_layout = compute_layout_from_geometry(&diagram, &geometry, &config);
        let ref_text = render_text_from_layout(&diagram, &ref_layout, &options);

        // Adapter path: geometry_to_text_layout → render
        let adapter_layout = geometry_to_text_layout(&diagram, &geometry, &config);
        let adapter_text = render_text_from_layout(&diagram, &adapter_layout, &options);

        assert_eq!(
            ref_text, adapter_text,
            "[{name}] adapter text mismatch vs reference"
        );
        tested += 1;
    }

    assert!(tested >= 70, "Expected at least 70 fixtures, got {tested}");
}
