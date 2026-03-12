use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use mmdflux::frontends::mermaid::{DiagramType, ParseError, detect_diagram_type};
// Verify core expected items are accessible from crate root.
use mmdflux::{
    Diagram,
    Direction,
    Edge,
    Node,
    OutputFormat,
    RenderConfig,
    RenderError,
    RenderRequest,
    Shape,
    // Diagrams
    diagrams::flowchart,
    graph::geometry::{
        EngineHints, FRect, GraphGeometry, LayeredHints, LayoutEdge, PositionedNode,
    },
    // Registry
    registry::DiagramInstance,
    // Render-only geometry API
    render::graph::{
        SvgRenderOptions, TextRenderOptions, render_svg_from_geometry, render_text_from_geometry,
    },
};

fn lib_rs_source() -> String {
    repo_file("src/lib.rs")
}

fn repo_file(relative_path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    std::fs::read_to_string(&path).unwrap()
}

fn public_exports_for_test() -> BTreeSet<String> {
    let content = lib_rs_source();
    let mut exports = BTreeSet::new();

    let joined = content.replace('\n', " ");
    for segment in joined.split("pub use ").skip(1) {
        let Some(stmt) = segment.split(';').next() else {
            continue;
        };
        let stmt = stmt.trim();

        if let Some(brace_start) = stmt.find('{') {
            let brace_end = stmt.find('}').unwrap_or(stmt.len());
            let symbols = &stmt[brace_start + 1..brace_end];
            for sym in symbols.split(',') {
                let sym = sym.trim();
                if !sym.is_empty() {
                    exports.insert(sym.to_string());
                }
            }
        } else if let Some(colon_pos) = stmt.rfind("::") {
            exports.insert(stmt[colon_pos + 2..].trim().to_string());
        }
    }

    exports
}

fn public_modules_for_test() -> BTreeSet<String> {
    let content = lib_rs_source();
    let mut modules = BTreeSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(module) = trimmed
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        {
            modules.insert(module.to_string());
        }
    }

    modules
}

fn assert_exports_include(exports: &BTreeSet<String>, required: &[&str]) {
    for name in required {
        assert!(
            exports.contains(*name),
            "{name} should remain in the crate-root export surface"
        );
    }
}

fn assert_exports_exclude(exports: &BTreeSet<String>, forbidden: &[&str], context: &str) {
    for name in forbidden {
        assert!(
            !exports.contains(*name),
            "{name} should stay out of {context}"
        );
    }
}

#[test]
fn all_exports_accessible() {
    // This test passes if it compiles
    let _ = OutputFormat::default();
    let _ = RenderConfig::default();
    let _ = RenderError::from("surface");
    let _ = RenderRequest::new("graph TD\nA-->B", OutputFormat::Text);
    let _: Box<dyn DiagramInstance> = Box::new(flowchart::FlowchartInstance::new());
}

#[test]
fn flat_public_contract_modules_are_accessible() {
    let _ = mmdflux::config::LayoutConfig::default();
    let _ = mmdflux::config::Ranker::NetworkSimplex;
    let _ = mmdflux::format::RoutingStyle::Polyline;
    let _ = mmdflux::errors::RenderError::from("surface");
    let _ = mmdflux::request::RenderRequest::new("graph TD\nA-->B", OutputFormat::Text);
    let _ = mmdflux::family::DiagramFamily::Graph;
    let _ = mmdflux::diagnostics::ParseDiagnostic::warning(None, None, String::new());
}

#[test]
fn crate_root_exports_only_the_curated_render_and_validation_surface() {
    let exports = public_exports_for_test();

    assert_exports_include(
        &exports,
        &[
            "OutputFormat",
            "RenderConfig",
            "RenderError",
            "RenderRequest",
        ],
    );
    assert_exports_exclude(
        &exports,
        &[
            "DiagramType",
            "ParseError",
            "parse_flowchart",
            "detect_diagram_type",
            "compile_to_graph",
            "default_registry",
        ],
        "the crate-root export surface",
    );
}

#[test]
fn crate_root_does_not_export_hidden_testing_module() {
    let modules = public_modules_for_test();

    assert!(
        !modules.contains("testing"),
        "src/lib.rs must not reintroduce a public testing module"
    );
}

#[test]
fn engine_level_solve_types_are_not_crate_root_api() {
    let exports = public_exports_for_test();

    assert_exports_exclude(
        &exports,
        &[
            "GraphEngine",
            "GraphSolveRequest",
            "GraphSolveResult",
            "EngineConfig",
            "EdgeRouting",
        ],
        "the crate root",
    );
}

#[test]
fn implementor_traits_and_engine_capabilities_are_not_crate_root_api() {
    let exports = public_exports_for_test();

    assert_exports_exclude(
        &exports,
        &[
            "DiagramModel",
            "DiagramParser",
            "DiagramRenderer",
            "EngineAlgorithmCapabilities",
            "RouteOwnership",
            "LayoutConfig",
        ],
        "the crate root",
    );
}

#[test]
fn registry_api_works() {
    let registry = mmdflux::registry_builtins::default_registry();
    let input = "graph TD\n    A-->B";

    let diagram_id = registry.detect(input).unwrap();
    assert_eq!(diagram_id, "flowchart");

    let mut instance = registry.create(diagram_id).unwrap();
    instance.parse(input).unwrap();
    let output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert!(output.contains('A'));
    assert!(output.contains('B'));
}

#[test]
fn builtin_registry_module_is_public_and_registry_default_registry_is_gone() {
    let _ = mmdflux::registry_builtins::default_registry();

    let registry_source = repo_file("src/registry.rs");
    assert!(
        !registry_source.contains("pub fn default_registry("),
        "src/registry.rs should stay contract-only"
    );
}

#[test]
fn crate_root_exports_registry_builtins_but_not_removed_advanced_helpers() {
    let modules = public_modules_for_test();
    let exports = public_exports_for_test();

    assert!(
        modules.contains("registry_builtins"),
        "registry_builtins should remain an explicit top-level advanced API module"
    );

    assert_exports_exclude(
        &exports,
        &["snap_path_to_grid_preview", "intersect", "default_registry"],
        "the crate-root export surface",
    );
}

#[test]
fn low_level_graph_engine_api_is_accessible_from_explicit_namespace() {
    let flowchart = mmdflux::frontends::mermaid::parse_flowchart("graph TD\nA-->B").unwrap();
    let diagram = mmdflux::diagrams::flowchart::compile_to_graph(&flowchart);
    let registry = mmdflux::engines::graph::registry::GraphEngineRegistry::default();
    let engine_id =
        mmdflux::EngineAlgorithmId::new(mmdflux::EngineId::Flux, mmdflux::AlgorithmId::Layered);
    let engine = registry.get_solver(engine_id).unwrap();
    let request = mmdflux::engines::graph::contracts::GraphSolveRequest::from_config(
        &RenderConfig::default(),
        OutputFormat::Text,
    );
    let config = mmdflux::engines::graph::contracts::EngineConfig::Layered(Default::default());
    let result = engine.solve(&diagram, &config, &request).unwrap();

    assert_eq!(result.engine_id, engine_id);
    assert!(!result.geometry.nodes.is_empty());
}

#[test]
fn low_level_text_replay_api_is_accessible_from_explicit_namespace() {
    let _ = mmdflux::render::graph::text_replay::GridLayoutConfig::default();
}

#[test]
fn routed_svg_api_is_accessible() {
    let _f: fn(
        &mmdflux::Diagram,
        &mmdflux::graph::geometry::RoutedGraphGeometry,
        &mmdflux::render::graph::SvgRenderOptions,
    ) -> String = mmdflux::render::graph::render_svg_from_routed_geometry;
}

#[test]
fn render_only_geometry_api_works() {
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("A").with_shape(Shape::Rectangle));
    diagram.add_node(Node::new("B").with_shape(Shape::Rectangle));
    diagram.add_edge(Edge::new("A", "B"));

    let geometry = GraphGeometry {
        nodes: HashMap::from([
            (
                "A".to_string(),
                PositionedNode {
                    id: "A".to_string(),
                    rect: FRect::new(0.0, 0.0, 9.0, 3.0),
                    shape: Shape::Rectangle,
                    label: "A".to_string(),
                    parent: None,
                },
            ),
            (
                "B".to_string(),
                PositionedNode {
                    id: "B".to_string(),
                    rect: FRect::new(20.0, 0.0, 9.0, 3.0),
                    shape: Shape::Rectangle,
                    label: "B".to_string(),
                    parent: None,
                },
            ),
        ]),
        edges: vec![LayoutEdge {
            index: 0,
            from: "A".to_string(),
            to: "B".to_string(),
            waypoints: vec![],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: None,
            preserve_orthogonal_topology: false,
        }],
        subgraphs: HashMap::new(),
        self_edges: vec![],
        direction: Direction::LeftRight,
        node_directions: HashMap::from([
            ("A".to_string(), Direction::LeftRight),
            ("B".to_string(), Direction::LeftRight),
        ]),
        bounds: FRect::new(0.0, 0.0, 30.0, 6.0),
        reversed_edges: vec![],
        engine_hints: Some(EngineHints::Layered(LayeredHints {
            node_ranks: HashMap::from([("A".to_string(), 0), ("B".to_string(), 1)]),
            rank_to_position: HashMap::from([(0, (0.0, 3.0)), (1, (20.0, 23.0))]),
            edge_waypoints: HashMap::new(),
            label_positions: HashMap::new(),
        })),
        grid_projection: None,
        rerouted_edges: HashSet::new(),
        enhanced_backward_routing: false,
    };

    let text = render_text_from_geometry(&diagram, &geometry, None, &TextRenderOptions::default());
    assert!(text.contains('A'));
    assert!(text.contains('B'));

    let svg = render_svg_from_geometry(&diagram, &geometry, &SvgRenderOptions::default());
    assert!(svg.contains("<svg"));
}

#[test]
fn render_text_geometry_api_does_not_require_engine_hints() {
    let mut diagram = Diagram::new(Direction::LeftRight);
    diagram.add_node(Node::new("A").with_shape(Shape::Rectangle));
    diagram.add_node(Node::new("B").with_shape(Shape::Rectangle));
    diagram.add_edge(Edge::new("A", "B"));

    let geometry = GraphGeometry {
        nodes: HashMap::from([
            (
                "A".to_string(),
                PositionedNode {
                    id: "A".to_string(),
                    rect: FRect::new(0.0, 0.0, 9.0, 3.0),
                    shape: Shape::Rectangle,
                    label: "A".to_string(),
                    parent: None,
                },
            ),
            (
                "B".to_string(),
                PositionedNode {
                    id: "B".to_string(),
                    rect: FRect::new(20.0, 0.0, 9.0, 3.0),
                    shape: Shape::Rectangle,
                    label: "B".to_string(),
                    parent: None,
                },
            ),
        ]),
        edges: vec![LayoutEdge {
            index: 0,
            from: "A".to_string(),
            to: "B".to_string(),
            waypoints: vec![],
            label_position: None,
            label_side: None,
            from_subgraph: None,
            to_subgraph: None,
            layout_path_hint: None,
            preserve_orthogonal_topology: false,
        }],
        subgraphs: HashMap::new(),
        self_edges: vec![],
        direction: Direction::LeftRight,
        node_directions: HashMap::from([
            ("A".to_string(), Direction::LeftRight),
            ("B".to_string(), Direction::LeftRight),
        ]),
        bounds: FRect::new(0.0, 0.0, 30.0, 6.0),
        reversed_edges: vec![],
        engine_hints: None,
        grid_projection: None,
        rerouted_edges: HashSet::new(),
        enhanced_backward_routing: false,
    };

    let text = render_text_from_geometry(&diagram, &geometry, None, &TextRenderOptions::default());
    assert!(text.contains('A'));
    assert!(text.contains('B'));
}

#[test]
fn core_types_accessible() {
    let _ = DiagramType::Flowchart;
    let _ = detect_diagram_type("graph TD\nA-->B");
    let _ = ParseError::UnexpectedEof;

    let mut diagram = Diagram::new(Direction::TopDown);
    diagram.add_node(Node::new("A").with_shape(Shape::Rectangle));
    diagram.add_node(Node::new("B"));
    diagram.add_edge(Edge::new("A", "B"));
}

#[test]
fn layered_engine_boundary_lives_under_engines_graph() {
    let mut graph = mmdflux::engines::graph::algorithms::layered::DiGraph::new();
    graph.add_node("A", (10.0, 10.0));
    graph.add_node("B", (10.0, 10.0));
    graph.add_edge("A", "B");

    let _ = mmdflux::engines::graph::algorithms::layered::Ranker::NetworkSimplex;
    let result = mmdflux::engines::graph::algorithms::layered::layout(
        &graph,
        &mmdflux::engines::graph::algorithms::layered::LayoutConfig::default(),
        |_, dims| *dims,
    );

    assert!(result.nodes.contains_key(
        &mmdflux::engines::graph::algorithms::layered::NodeId::from("A")
    ));
    assert!(result.nodes.contains_key(
        &mmdflux::engines::graph::algorithms::layered::NodeId::from("B")
    ));
}

#[test]
fn engine_adapters_live_in_explicit_modules() {
    let _ = mmdflux::engines::graph::flux::FluxLayeredEngine::text();
    let _ = mmdflux::engines::graph::mermaid::MermaidLayeredEngine::new();
}
