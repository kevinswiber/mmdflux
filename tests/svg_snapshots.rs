use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::diagram::{
    CornerStyle, Curve, EngineAlgorithmId, OutputFormat, PathSimplification, RenderConfig,
    RoutingStyle,
};
use mmdflux::diagrams::flowchart::FlowchartInstance;
use mmdflux::diagrams::mmds::from_mmds_str;
use mmdflux::registry::DiagramInstance;
use mmdflux::render::{RenderOptions, render_svg};
use mmdflux::{build_diagram, parse_flowchart};

fn list_fixtures() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read fixtures dir: {e}"))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().is_some_and(|e| e == "mmd") {
                Some(path.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {name}: {e}"))
}

fn render_svg_fixture(name: &str) -> String {
    let input = load_fixture(name);
    let flowchart = parse_flowchart(&input).expect("Failed to parse fixture");
    let diagram = build_diagram(&flowchart);
    let mut options = RenderOptions::default_svg();
    options.path_simplification = PathSimplification::None;
    render_svg(&diagram, &options)
}

fn render_svg_fixture_with_curve(name: &str, routing: RoutingStyle, curve: Curve) -> String {
    let input = load_fixture(name);
    let flowchart = parse_flowchart(&input).expect("Failed to parse fixture");
    let diagram = build_diagram(&flowchart);
    let mut options = RenderOptions::default_svg();
    options.svg.routing_style = routing;
    options.svg.curve = curve;
    options.path_simplification = PathSimplification::None;
    render_svg(&diagram, &options)
}

fn render_svg_fixture_with_engine(name: &str, engine: &str) -> String {
    let input = load_fixture(name);
    let mut instance = FlowchartInstance::new();
    instance.parse(&input).expect("Failed to parse fixture");
    let config = RenderConfig {
        path_simplification: PathSimplification::None,
        layout_engine: Some(EngineAlgorithmId::parse(engine).unwrap()),
        ..RenderConfig::default()
    };
    instance
        .render(OutputFormat::Svg, &config)
        .expect("Failed to render SVG fixture")
}

fn render_svg_mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    let payload = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read MMDS fixture {}: {e}", path.display()));
    let diagram = from_mmds_str(&payload).expect("MMDS fixture should hydrate");
    let mut options = RenderOptions::default_svg();
    options.path_simplification = PathSimplification::None;
    render_svg(&diagram, &options)
}

fn render_svg_positioned_mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    let payload = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read MMDS fixture {}: {e}", path.display()));
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    instance.parse(&payload).expect("MMDS fixture should parse");
    instance
        .render(
            OutputFormat::Svg,
            &RenderConfig {
                path_simplification: PathSimplification::None,
                ..RenderConfig::default()
            },
        )
        .expect("positioned MMDS should render SVG")
}

fn assert_direct_vs_mmds_svg_parity(flowchart_fixture: &str, mmds_fixture: &str) {
    let direct_svg = render_svg_fixture(flowchart_fixture);
    let replay_svg = render_svg_mmds_fixture(mmds_fixture);
    assert_eq!(
        replay_svg, direct_svg,
        "MMDS replay diverged for flowchart fixture {flowchart_fixture} and MMDS fixture {mmds_fixture}"
    );
}

fn snapshot_path(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join("flowchart")
        .join(format!("{stem}.svg"))
}

fn preset_snapshot_path(preset: &str, stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join(format!("flowchart-{preset}"))
        .join(format!("{stem}.svg"))
}

fn mmds_snapshot_path(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join("mmds")
        .join(format!("{stem}.svg"))
}

fn assert_snapshot(fixture: &str) {
    let stem = fixture.trim_end_matches(".mmd");
    let output = render_svg_fixture(fixture);
    let path = snapshot_path(stem);

    if std::env::var("GENERATE_SVG_SNAPSHOTS").is_ok() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, &output).unwrap();
    }

    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Missing snapshot: {}", path.display()));
    assert_eq!(output, expected, "Snapshot mismatch for {fixture}");
}

fn assert_preset_snapshot(preset: &str, fixture: &str, routing: RoutingStyle, curve: Curve) {
    let stem = fixture.trim_end_matches(".mmd");
    let output = render_svg_fixture_with_curve(fixture, routing, curve);
    let path = preset_snapshot_path(preset, stem);

    if std::env::var("GENERATE_SVG_SNAPSHOTS").is_ok() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, &output).unwrap();
    }

    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Missing snapshot: {}", path.display()));
    assert_eq!(output, expected, "{preset} snapshot mismatch for {fixture}");
}

fn assert_mmds_snapshot(fixture: &str, snapshot_stem: &str) {
    let output = render_svg_positioned_mmds_fixture(fixture);
    let path = mmds_snapshot_path(snapshot_stem);

    if std::env::var("GENERATE_SVG_SNAPSHOTS").is_ok() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, &output).unwrap();
    }

    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Missing snapshot: {}", path.display()));
    assert_eq!(output, expected, "Snapshot mismatch for {fixture}");
}

#[test]
fn svg_snapshot_all_fixtures() {
    for fixture in list_fixtures() {
        assert_snapshot(&fixture);
    }
}

#[test]
fn mmds_replay_without_endpoint_intent_diverges_on_subgraph_as_node_edge_fixture() {
    let direct_svg = render_svg_fixture("subgraph_as_node_edge.mmd");
    let replay_svg = render_svg_mmds_fixture("subgraph-endpoint-intent-missing.json");

    assert_ne!(replay_svg, direct_svg);
}

#[test]
fn mmds_replay_without_endpoint_intent_diverges_on_subgraph_to_subgraph_fixture() {
    let direct_svg = render_svg_fixture("subgraph_to_subgraph_edge.mmd");
    let replay_svg = render_svg_mmds_fixture("subgraph-endpoint-subgraph-to-subgraph-missing.json");

    assert_ne!(replay_svg, direct_svg);
}

#[test]
fn mmds_replay_with_endpoint_intent_matches_subgraph_as_node_fixture() {
    assert_direct_vs_mmds_svg_parity(
        "subgraph_as_node_edge.mmd",
        "subgraph-endpoint-intent-present.json",
    );
}

#[test]
fn mmds_replay_with_endpoint_intent_matches_subgraph_to_subgraph_fixture() {
    assert_direct_vs_mmds_svg_parity(
        "subgraph_to_subgraph_edge.mmd",
        "subgraph-endpoint-subgraph-to-subgraph-present.json",
    );
}

#[test]
fn direct_and_mmds_replay_match_for_subgraph_endpoint_fixture_set() {
    // `subgraph_as_node_edge` covers both subgraph-as-target and subgraph-as-source
    // endpoint-intent cases. `subgraph_to_subgraph_edge` covers subgraph-to-subgraph.
    for (flowchart_fixture, mmds_fixture) in [
        (
            "subgraph_as_node_edge.mmd",
            "subgraph-endpoint-intent-present.json",
        ),
        (
            "subgraph_to_subgraph_edge.mmd",
            "subgraph-endpoint-subgraph-to-subgraph-present.json",
        ),
    ] {
        assert_direct_vs_mmds_svg_parity(flowchart_fixture, mmds_fixture);
    }
}

#[test]
fn positioned_mmds_svg_snapshot_routed_basic() {
    assert_mmds_snapshot("positioned/routed-basic.json", "routed-basic");
}

#[test]
fn svg_snapshot_all_fixtures_straight() {
    for fixture in list_fixtures() {
        assert_preset_snapshot(
            "straight",
            &fixture,
            RoutingStyle::Direct,
            Curve::Linear(CornerStyle::Sharp),
        );
    }
}

#[test]
fn svg_snapshot_all_fixtures_polyline() {
    for fixture in list_fixtures() {
        assert_preset_snapshot(
            "polyline",
            &fixture,
            RoutingStyle::Polyline,
            Curve::Linear(CornerStyle::Sharp),
        );
    }
}

#[test]
fn svg_snapshot_all_fixtures_step() {
    for fixture in list_fixtures() {
        assert_preset_snapshot(
            "step",
            &fixture,
            RoutingStyle::Orthogonal,
            Curve::Linear(CornerStyle::Sharp),
        );
    }
}

#[test]
fn svg_snapshot_all_fixtures_curved_step() {
    for fixture in list_fixtures() {
        assert_preset_snapshot(
            "curved-step",
            &fixture,
            RoutingStyle::Orthogonal,
            Curve::Basis,
        );
    }
}

#[test]
fn svg_snapshot_all_fixtures_smooth_step() {
    for fixture in list_fixtures() {
        assert_preset_snapshot(
            "smooth-step",
            &fixture,
            RoutingStyle::Orthogonal,
            Curve::Linear(CornerStyle::Rounded),
        );
    }
}

#[test]
fn svg_snapshot_all_fixtures_basis() {
    for fixture in list_fixtures() {
        assert_preset_snapshot("basis", &fixture, RoutingStyle::Polyline, Curve::Basis);
    }
}

#[test]
fn svg_snapshot_curve_bucket_basis_regression() {
    assert_preset_snapshot(
        "basis",
        "simple_cycle.mmd",
        RoutingStyle::Polyline,
        Curve::Basis,
    );
}

#[test]
fn svg_snapshot_curve_bucket_linear_rounded_regression() {
    assert_preset_snapshot(
        "smooth-step",
        "simple_cycle.mmd",
        RoutingStyle::Orthogonal,
        Curve::Linear(CornerStyle::Rounded),
    );
}

#[test]
fn svg_polyline_route_rollback_is_stable_across_repeated_renders() {
    for fixture in ["simple.mmd", "chain.mmd", "simple_cycle.mmd"] {
        let baseline = render_svg_fixture_with_engine(fixture, "mermaid-layered");
        let output = render_svg_fixture_with_engine(fixture, "mermaid-layered");
        assert_eq!(
            output, baseline,
            "polyline rollback should be stable for fixture {fixture}"
        );
    }
}
