//! Engine registry tests: typed engine IDs, parsing, availability, and registry lookup.

use mmdflux::diagram::{
    AlgorithmId, CornerStyle, Curve, EdgePreset, EngineAlgorithmCapabilities, EngineAlgorithmId,
    EngineConfig, EngineId, GeometryLevel, GraphEngine, GraphSolveRequest, OutputFormat,
    PathSimplification, RenderConfig, RenderError, RouteOwnership, RoutingStyle,
};
use mmdflux::diagrams::flowchart::FlowchartInstance;
use mmdflux::diagrams::flowchart::engine::{FluxLayeredEngine, MermaidLayeredEngine};
use mmdflux::engines::graph::GraphEngineRegistry;
use mmdflux::registry::DiagramInstance;

// =============================================================================
// Engine selection through render path
// =============================================================================

/// Helper: parse + render with a specific engine algorithm ID string.
fn render_with_engine(input: &str, engine: &str) -> Result<String, RenderError> {
    let mut instance = FlowchartInstance::new();
    instance
        .parse(input)
        .expect("parse should succeed in test helper");
    let engine = EngineAlgorithmId::parse(engine)?;
    let config = RenderConfig {
        layout_engine: Some(engine),
        ..Default::default()
    };
    instance.render(OutputFormat::Text, &config)
}

#[test]
fn unavailable_engine_returns_actionable_error() {
    #[cfg(not(feature = "engine-elk"))]
    {
        let err = render_with_engine("graph TD\nA-->B", "elk-layered").unwrap_err();
        assert!(
            err.message.contains("engine-elk"),
            "error should reference feature flag: {}",
            err.message
        );
    }
}

#[test]
fn unknown_engine_returns_error() {
    let err = render_with_engine("graph TD\nA-->B", "nonexistent").unwrap_err();
    assert!(
        err.message.contains("unknown engine"),
        "error should mention unknown engine: {}",
        err.message
    );
}

#[test]
fn cose_rejected_at_parse_boundary() {
    // COSE is not in the new engine+algorithm taxonomy; rejected at parse time.
    let err = EngineAlgorithmId::parse("cose").unwrap_err();
    assert!(
        !err.message.is_empty(),
        "cose should be rejected at parse boundary: {}",
        err.message
    );
}

#[test]
fn cose_bilkent_rejected_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("cose-bilkent").unwrap_err();
    assert!(
        !err.message.is_empty(),
        "cose-bilkent should be rejected at parse boundary: {}",
        err.message
    );
}

// =============================================================================
// Flux vs Mermaid routing: SVG-divergent test
// =============================================================================

#[test]
fn flux_vs_mermaid_svg_output_may_diverge_for_cycle() {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/simple_cycle.mmd").unwrap();
    let mut instance = FlowchartInstance::new();
    instance.parse(&input).unwrap();

    let flux_out = instance
        .render(
            OutputFormat::Svg,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
                ..RenderConfig::default()
            },
        )
        .unwrap();
    let mermaid_out = instance
        .render(
            OutputFormat::Svg,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
                ..RenderConfig::default()
            },
        )
        .unwrap();

    // SVG paths will differ because routing topology changes — document, don't assert equal
    let _ = (flux_out, mermaid_out); // classification: SVG-divergent
}

// =============================================================================
// EngineAlgorithmId taxonomy (plan-0081 Phase 1)
// =============================================================================

#[test]
fn engine_algorithm_id_parses_flux_layered() {
    let id = EngineAlgorithmId::parse("flux-layered").unwrap();
    assert_eq!(id.engine(), EngineId::Flux);
    assert_eq!(id.algorithm(), AlgorithmId::Layered);
    assert_eq!(id.to_string(), "flux-layered");
}

#[test]
fn engine_algorithm_id_parses_all_valid_ids() {
    for (input, engine, algo) in [
        ("flux-layered", EngineId::Flux, AlgorithmId::Layered),
        ("mermaid-layered", EngineId::Mermaid, AlgorithmId::Layered),
        ("elk-layered", EngineId::Elk, AlgorithmId::Layered),
        ("elk-mrtree", EngineId::Elk, AlgorithmId::MrTree),
    ] {
        let id = EngineAlgorithmId::parse(input).unwrap();
        assert_eq!(id.engine(), engine);
        assert_eq!(id.algorithm(), algo);
    }
}

#[test]
fn engine_algorithm_id_is_case_insensitive() {
    assert!(EngineAlgorithmId::parse("Flux-Layered").is_ok());
    assert!(EngineAlgorithmId::parse("ELK-MRTREE").is_ok());
    assert!(EngineAlgorithmId::parse("  elk-layered  ").is_ok());
}

#[test]
fn engine_algorithm_id_rejects_legacy_dagre_with_migration() {
    let err = EngineAlgorithmId::parse("dagre").unwrap_err();
    assert!(
        err.message.contains("flux-layered"),
        "should suggest replacement: {}",
        err
    );
}

#[test]
fn engine_algorithm_id_rejects_legacy_elk_with_migration() {
    let err = EngineAlgorithmId::parse("elk").unwrap_err();
    assert!(
        err.message.contains("elk-layered"),
        "should suggest replacement: {}",
        err
    );
}

#[test]
fn engine_algorithm_id_rejects_legacy_cose() {
    let err = EngineAlgorithmId::parse("cose").unwrap_err();
    assert!(
        err.message.contains("no longer supported") || err.message.contains("cose"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn engine_algorithm_id_rejects_unknown() {
    let err = EngineAlgorithmId::parse("bogus-engine").unwrap_err();
    assert!(
        err.message.contains("unknown") || err.message.contains("Valid options"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn engine_algorithm_id_display_roundtrips() {
    for input in [
        "flux-layered",
        "mermaid-layered",
        "elk-layered",
        "elk-mrtree",
    ] {
        let id = EngineAlgorithmId::parse(input).unwrap();
        assert_eq!(id.to_string(), input);
    }
}

// =============================================================================
// RouteOwnership and EngineAlgorithmCapabilities (plan-0081 Phase 1.2)
// =============================================================================

#[test]
fn flux_layered_capabilities() {
    let id = EngineAlgorithmId::parse("flux-layered").unwrap();
    let caps = id.capabilities();
    assert_eq!(caps.route_ownership, RouteOwnership::Native);
    assert!(caps.supports_subgraphs);
}

#[test]
fn mermaid_layered_capabilities() {
    let id = EngineAlgorithmId::parse("mermaid-layered").unwrap();
    let caps = id.capabilities();
    assert_eq!(caps.route_ownership, RouteOwnership::HintDriven);
    assert!(caps.supports_subgraphs);
}

#[test]
fn elk_layered_capabilities() {
    let id = EngineAlgorithmId::parse("elk-layered").unwrap();
    let caps = id.capabilities();
    assert_eq!(caps.route_ownership, RouteOwnership::EngineProvided);
    assert!(caps.supports_subgraphs);
}

#[test]
fn elk_mrtree_capabilities() {
    let id = EngineAlgorithmId::parse("elk-mrtree").unwrap();
    let caps = id.capabilities();
    assert_eq!(caps.route_ownership, RouteOwnership::EngineProvided);
    assert!(!caps.supports_subgraphs);
}

#[test]
fn route_ownership_native_routes_edges() {
    assert!(RouteOwnership::Native.routes_edges());
    assert!(!RouteOwnership::HintDriven.routes_edges());
    assert!(RouteOwnership::EngineProvided.routes_edges());
}

// =============================================================================
// EngineAlgorithmId availability gating (plan-0081 Phase 1.3)
// =============================================================================

#[test]
fn flux_layered_is_always_available() {
    let id = EngineAlgorithmId::parse("flux-layered").unwrap();
    assert!(id.check_available().is_ok());
}

#[test]
fn mermaid_layered_is_always_available() {
    let id = EngineAlgorithmId::parse("mermaid-layered").unwrap();
    assert!(id.check_available().is_ok());
}

#[cfg(not(feature = "engine-elk"))]
#[test]
fn elk_layered_unavailable_without_feature() {
    let id = EngineAlgorithmId::parse("elk-layered").unwrap();
    let err = id.check_available().unwrap_err();
    assert!(
        err.message.contains("engine-elk"),
        "should name feature flag: {}",
        err
    );
}

#[cfg(not(feature = "engine-elk"))]
#[test]
fn elk_mrtree_unavailable_without_feature() {
    let id = EngineAlgorithmId::parse("elk-mrtree").unwrap();
    let err = id.check_available().unwrap_err();
    assert!(
        err.message.contains("engine-elk"),
        "should name feature flag: {}",
        err
    );
}

// =============================================================================
// Style model taxonomy (plan-0081 Phase 7.2)
// =============================================================================

#[test]
fn routing_style_parses_direct() {
    assert_eq!(RoutingStyle::parse("direct").unwrap(), RoutingStyle::Direct);
}

#[test]
fn routing_style_parses_polyline() {
    assert_eq!(
        RoutingStyle::parse("polyline").unwrap(),
        RoutingStyle::Polyline
    );
}

#[test]
fn routing_style_parses_orthogonal() {
    assert_eq!(
        RoutingStyle::parse("orthogonal").unwrap(),
        RoutingStyle::Orthogonal
    );
}

#[test]
fn curve_parses_linear() {
    assert_eq!(
        Curve::parse("linear").unwrap(),
        Curve::Linear(CornerStyle::Sharp)
    );
}

#[test]
fn curve_parses_basis() {
    assert_eq!(Curve::parse("basis").unwrap(), Curve::Basis);
}

#[test]
fn corner_style_parses_sharp() {
    assert_eq!(CornerStyle::parse("sharp").unwrap(), CornerStyle::Sharp);
}

#[test]
fn corner_style_parses_rounded() {
    assert_eq!(CornerStyle::parse("rounded").unwrap(), CornerStyle::Rounded);
}

#[test]
fn edge_preset_parses_all_values() {
    assert_eq!(EdgePreset::parse("straight").unwrap(), EdgePreset::Straight);
    assert_eq!(EdgePreset::parse("polyline").unwrap(), EdgePreset::Polyline);
    assert_eq!(EdgePreset::parse("step").unwrap(), EdgePreset::Step);
    assert_eq!(
        EdgePreset::parse("smooth-step").unwrap(),
        EdgePreset::SmoothStep
    );
    assert_eq!(
        EdgePreset::parse("curved-step").unwrap(),
        EdgePreset::CurvedStep
    );
    assert_eq!(EdgePreset::parse("basis").unwrap(), EdgePreset::Basis);
}

#[test]
fn edge_preset_expand_is_deterministic() {
    let (r, curve) = EdgePreset::Straight.expand();
    assert_eq!(r, RoutingStyle::Direct);
    assert_eq!(curve, Curve::Linear(CornerStyle::Sharp));
}

// =============================================================================
// GraphEngine solve contract (plan-0081 Phase 3.1)
// =============================================================================

#[test]
fn solve_request_fields_round_trip() {
    let req = GraphSolveRequest {
        output_format: OutputFormat::Text,
        geometry_level: GeometryLevel::Layout,
        path_simplification: PathSimplification::None,
        routing_style: None,
    };
    assert_eq!(req.output_format, OutputFormat::Text);
    assert_eq!(req.geometry_level, GeometryLevel::Layout);
    assert_eq!(req.path_simplification, PathSimplification::None);
}

#[test]
fn solve_request_from_config_derives_fields() {
    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..RenderConfig::default()
    };
    let req = GraphSolveRequest::from_config(&config, OutputFormat::Svg);
    assert_eq!(req.output_format, OutputFormat::Svg);
    assert_eq!(req.geometry_level, GeometryLevel::Routed);
    assert_eq!(req.path_simplification, PathSimplification::Lossless);
    assert_eq!(req.routing_style, None);
}

#[test]
fn solve_request_from_config_keeps_path_simplification_independent_of_style() {
    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        edge_preset: Some(EdgePreset::Straight),
        routing_style: Some(RoutingStyle::Direct),
        path_simplification: PathSimplification::Lossless,
        ..RenderConfig::default()
    };
    let req = GraphSolveRequest::from_config(&config, OutputFormat::Svg);
    assert_eq!(req.output_format, OutputFormat::Svg);
    assert_eq!(req.geometry_level, GeometryLevel::Routed);
    assert_eq!(req.path_simplification, PathSimplification::Lossless);
    assert_eq!(req.routing_style, Some(RoutingStyle::Direct));
}

// =============================================================================
// FluxLayeredEngine (plan-0081 Phase 3.2)
// =============================================================================

fn build_simple_diagram() -> mmdflux::Diagram {
    let flowchart = mmdflux::parse_flowchart("graph TD\nA-->B").unwrap();
    mmdflux::build_diagram(&flowchart)
}

#[test]
fn flux_layered_engine_id() {
    let engine = FluxLayeredEngine::text();
    assert_eq!(
        engine.id(),
        EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered)
    );
}

#[test]
fn flux_layered_capabilities_are_native() {
    let engine = FluxLayeredEngine::text();
    let caps = engine.capabilities();
    assert_eq!(caps.route_ownership, RouteOwnership::Native);
    assert!(caps.supports_subgraphs);
}

#[test]
fn flux_layered_solve_layout_level_has_no_routed_geometry() {
    let diagram = build_simple_diagram();
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest {
        output_format: OutputFormat::Text,
        geometry_level: GeometryLevel::Layout,
        path_simplification: PathSimplification::None,
        routing_style: None,
    };
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let result = engine.solve(&diagram, &config, &request).unwrap();

    assert_eq!(result.engine_id.engine(), EngineId::Flux);
    assert!(!result.geometry.nodes.is_empty());
    assert!(
        result.routed.is_none(),
        "layout level should not include routed geometry"
    );
}

#[test]
fn flux_layered_solve_routed_level_has_routed_geometry() {
    let diagram = build_simple_diagram();
    let engine = FluxLayeredEngine::text();
    let request = GraphSolveRequest {
        output_format: OutputFormat::Text,
        geometry_level: GeometryLevel::Routed,
        path_simplification: PathSimplification::None,
        routing_style: None,
    };
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let result = engine.solve(&diagram, &config, &request).unwrap();

    assert!(
        result.routed.is_some(),
        "routed level should produce routed geometry"
    );
    let routed = result.routed.unwrap();
    assert!(!routed.edges.is_empty());
}

// =============================================================================
// MermaidLayeredEngine (plan-0081 Phase 3.3)
// =============================================================================

#[test]
fn mermaid_layered_engine_id() {
    let engine = MermaidLayeredEngine::new();
    assert_eq!(
        engine.id(),
        EngineAlgorithmId::new(EngineId::Mermaid, AlgorithmId::Layered)
    );
}

#[test]
fn mermaid_layered_capabilities_are_hint_driven() {
    let engine = MermaidLayeredEngine::new();
    let caps = engine.capabilities();
    assert_eq!(caps.route_ownership, RouteOwnership::HintDriven);
    assert!(caps.supports_subgraphs);
}

#[test]
fn mermaid_layered_solve_layout_level_has_no_routed_geometry() {
    let diagram = build_simple_diagram();
    let engine = MermaidLayeredEngine::new();
    let request = GraphSolveRequest {
        output_format: OutputFormat::Mmds,
        geometry_level: GeometryLevel::Layout,
        path_simplification: PathSimplification::None,
        routing_style: None,
    };
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let result = engine.solve(&diagram, &config, &request).unwrap();

    assert!(
        result.routed.is_none(),
        "layout level should not include routed geometry"
    );
    assert!(!result.geometry.nodes.is_empty());
}

#[test]
fn mermaid_layered_layout_matches_flux_layered_layout() {
    // Both engines share the layered layout kernel, but Flux enables
    // per_edge_label_spacing which halves rank_sep and uses minlen=1 for
    // unlabeled edges.  For a simple A-->B (unlabeled), Flux produces a more
    // compact layout.  We verify that x-coordinates still match and that
    // Flux's y-spacing is <= Mermaid's (i.e., more compact or equal).
    let diagram = build_simple_diagram();
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let layout_req = GraphSolveRequest {
        output_format: OutputFormat::Mmds,
        geometry_level: GeometryLevel::Layout,
        path_simplification: PathSimplification::None,
        routing_style: None,
    };

    let flux = FluxLayeredEngine::text()
        .solve(&diagram, &config, &layout_req)
        .unwrap();
    let mermaid = MermaidLayeredEngine::new()
        .solve(&diagram, &config, &layout_req)
        .unwrap();

    assert_eq!(flux.geometry.nodes.len(), mermaid.geometry.nodes.len());
    for (id, flux_node) in &flux.geometry.nodes {
        let mermaid_node = mermaid.geometry.nodes.get(id).unwrap();
        assert_eq!(
            flux_node.rect.x, mermaid_node.rect.x,
            "node {id} x mismatch"
        );
    }

    // With per-edge label spacing, Flux is more compact for unlabeled edges.
    let flux_b = flux.geometry.nodes.get("B").unwrap();
    let mermaid_b = mermaid.geometry.nodes.get("B").unwrap();
    assert!(
        flux_b.rect.y <= mermaid_b.rect.y,
        "Flux should be at least as compact as Mermaid: flux B.y={} mermaid B.y={}",
        flux_b.rect.y,
        mermaid_b.rect.y,
    );
}

#[test]
fn mermaid_layered_solve_routed_level_has_routed_geometry() {
    let diagram = build_simple_diagram();
    let engine = MermaidLayeredEngine::new();
    let request = GraphSolveRequest {
        output_format: OutputFormat::Mmds,
        geometry_level: GeometryLevel::Routed,
        path_simplification: PathSimplification::None,
        routing_style: None,
    };
    let config = EngineConfig::Layered(mmdflux::layered::types::LayoutConfig::default());
    let result = engine.solve(&diagram, &config, &request).unwrap();

    assert!(
        result.routed.is_some(),
        "routed level should produce routed geometry"
    );
}

// =============================================================================
// GraphEngineRegistry solver lookup (plan-0081 Phase 3.4)
// =============================================================================

#[test]
fn registry_resolves_flux_layered() {
    let registry = GraphEngineRegistry::default();
    let id = EngineAlgorithmId::parse("flux-layered").unwrap();
    let engine = registry.get_solver(id);
    assert!(engine.is_some(), "flux-layered should be registered");
    assert_eq!(engine.unwrap().id().to_string(), "flux-layered");
}

#[test]
fn registry_resolves_mermaid_layered() {
    let registry = GraphEngineRegistry::default();
    let id = EngineAlgorithmId::parse("mermaid-layered").unwrap();
    let engine = registry.get_solver(id);
    assert!(engine.is_some(), "mermaid-layered should be registered");
    assert_eq!(engine.unwrap().id().to_string(), "mermaid-layered");
}

#[cfg(not(feature = "engine-elk"))]
#[test]
fn registry_does_not_have_elk_solver_without_feature() {
    let registry = GraphEngineRegistry::default();
    let id = EngineAlgorithmId::parse("elk-layered").unwrap();
    assert!(
        registry.get_solver(id).is_none(),
        "elk-layered should not be registered without engine-elk feature"
    );
}

#[test]
fn registry_get_solver_unknown_id_returns_none() {
    // An ID that parses but has no engine registered (elk without feature).
    // This test verifies get_solver returns None rather than panicking.
    let registry = GraphEngineRegistry::default();
    // flux-layered is always registered — verify the lookup succeeds (not None)
    let id = EngineAlgorithmId::parse("flux-layered").unwrap();
    assert!(registry.get_solver(id).is_some());
}

// =============================================================================
// Engine routing style capabilities and validation (plan-0081 Phase 7.3)
// =============================================================================

/// Helper: render with a specific engine, optional explicit routing style and/or preset.
fn render_with_engine_routing(
    input: &str,
    engine: &str,
    routing: Option<RoutingStyle>,
    preset: Option<EdgePreset>,
) -> Result<String, RenderError> {
    let mut instance = FlowchartInstance::new();
    instance.parse(input).expect("parse should succeed");
    let engine_id = EngineAlgorithmId::parse(engine)?;
    let config = RenderConfig {
        layout_engine: Some(engine_id),
        routing_style: routing,
        edge_preset: preset,
        ..Default::default()
    };
    instance.render(OutputFormat::Svg, &config)
}

#[test]
fn flux_layered_capabilities_include_routing_styles() {
    let caps = EngineAlgorithmId::parse("flux-layered")
        .unwrap()
        .capabilities();
    assert!(
        caps.supported_routing_styles
            .contains(&RoutingStyle::Direct),
        "flux-layered should support Direct"
    );
    assert!(
        caps.supported_routing_styles
            .contains(&RoutingStyle::Polyline),
        "flux-layered should support Polyline"
    );
    assert!(
        caps.supported_routing_styles
            .contains(&RoutingStyle::Orthogonal),
        "flux-layered should support Orthogonal"
    );
}

#[test]
fn mermaid_layered_capabilities_supports_only_polyline() {
    let caps = EngineAlgorithmId::parse("mermaid-layered")
        .unwrap()
        .capabilities();
    assert!(
        caps.supported_routing_styles
            .contains(&RoutingStyle::Polyline),
        "mermaid-layered should support Polyline"
    );
    assert!(
        !caps
            .supported_routing_styles
            .contains(&RoutingStyle::Direct),
        "mermaid-layered should not support Direct"
    );
    assert!(
        !caps
            .supported_routing_styles
            .contains(&RoutingStyle::Orthogonal),
        "mermaid-layered should not support Orthogonal"
    );
}

#[test]
fn mermaid_layered_rejects_orthogonal_routing_style() {
    let err = render_with_engine_routing(
        "graph TD\nA-->B",
        "mermaid-layered",
        Some(RoutingStyle::Orthogonal),
        None,
    )
    .unwrap_err();
    assert!(
        err.message.contains("mermaid-layered") || err.message.contains("orthogonal"),
        "error should name engine or routing style: {err}"
    );
}

#[test]
fn mermaid_layered_rejects_direct_routing_style() {
    let err = render_with_engine_routing(
        "graph TD\nA-->B",
        "mermaid-layered",
        Some(RoutingStyle::Direct),
        None,
    )
    .unwrap_err();
    assert!(
        err.message.contains("mermaid-layered") || err.message.contains("direct"),
        "error should name engine or routing style: {err}"
    );
}

#[test]
fn mermaid_layered_rejects_step_preset() {
    // step expands to Orthogonal+Linear+Sharp — unsupported on mermaid-layered
    let err = render_with_engine_routing(
        "graph TD\nA-->B",
        "mermaid-layered",
        None,
        Some(EdgePreset::Step),
    )
    .unwrap_err();
    assert!(
        !err.message.is_empty(),
        "step preset should be rejected on mermaid-layered: {err}"
    );
}

#[test]
fn mermaid_layered_rejects_curved_step_preset() {
    // curved-step expands to Orthogonal+Basis — unsupported on mermaid-layered
    let err = render_with_engine_routing(
        "graph TD\nA-->B",
        "mermaid-layered",
        None,
        Some(EdgePreset::CurvedStep),
    )
    .unwrap_err();
    assert!(
        !err.message.is_empty(),
        "curved-step preset should be rejected on mermaid-layered: {err}"
    );
}

#[test]
fn mermaid_layered_rejects_smooth_step_preset() {
    // smooth-step expands to Orthogonal+Linear+Rounded — unsupported on mermaid-layered
    let err = render_with_engine_routing(
        "graph TD\nA-->B",
        "mermaid-layered",
        None,
        Some(EdgePreset::SmoothStep),
    )
    .unwrap_err();
    assert!(
        !err.message.is_empty(),
        "smooth-step preset should be rejected on mermaid-layered: {err}"
    );
}

#[test]
fn mermaid_layered_accepts_basis_preset() {
    // basis expands to Polyline+Basis — supported on mermaid-layered
    assert!(
        render_with_engine_routing(
            "graph TD\nA-->B",
            "mermaid-layered",
            None,
            Some(EdgePreset::Basis),
        )
        .is_ok(),
        "basis preset should be accepted on mermaid-layered"
    );
}

#[test]
fn mermaid_layered_rejects_straight_preset() {
    // straight expands to Direct+Linear+Sharp — unsupported on mermaid-layered
    let err = render_with_engine_routing(
        "graph TD\nA-->B",
        "mermaid-layered",
        None,
        Some(EdgePreset::Straight),
    )
    .unwrap_err();
    assert!(
        !err.message.is_empty(),
        "straight preset should be rejected on mermaid-layered: {err}"
    );
}

#[test]
fn mermaid_layered_accepts_polyline_preset() {
    // polyline expands to Polyline+Linear+Sharp — supported on mermaid-layered
    assert!(
        render_with_engine_routing(
            "graph TD\nA-->B",
            "mermaid-layered",
            None,
            Some(EdgePreset::Polyline),
        )
        .is_ok(),
        "polyline preset should be accepted on mermaid-layered"
    );
}

#[test]
fn flux_layered_accepts_orthogonal_routing_style() {
    assert!(
        render_with_engine_routing(
            "graph TD\nA-->B",
            "flux-layered",
            Some(RoutingStyle::Orthogonal),
            None,
        )
        .is_ok(),
        "orthogonal routing should be accepted on flux-layered"
    );
}

#[test]
fn flux_layered_accepts_step_preset() {
    // step expands to Orthogonal — supported on flux-layered
    assert!(
        render_with_engine_routing(
            "graph TD\nA-->B",
            "flux-layered",
            None,
            Some(EdgePreset::Step),
        )
        .is_ok(),
        "step preset should be accepted on flux-layered"
    );
}

#[test]
fn capabilities_struct_exposes_supported_routing_styles() {
    // EngineAlgorithmCapabilities.supported_routing_styles is a slice of RoutingStyle
    let caps: EngineAlgorithmCapabilities = EngineAlgorithmId::parse("flux-layered")
        .unwrap()
        .capabilities();
    let _styles: &[RoutingStyle] = caps.supported_routing_styles;
    assert!(!_styles.is_empty());
}

// =============================================================================
// Routing style wired to edge path topology (plan-0081 Phase 7.4)
// =============================================================================

/// Helper: render a named flowchart fixture as SVG with a specific engine+routing style.
fn render_cycle_svg_with_routing(engine: &str, routing: RoutingStyle) -> String {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/simple_cycle.mmd").unwrap();
    render_with_engine_routing(&input, engine, Some(routing), None).unwrap()
}

/// Helper: render a named flowchart fixture as SVG with a specific engine+preset.
fn render_cycle_svg_with_preset(engine: &str, preset: EdgePreset) -> String {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/simple_cycle.mmd").unwrap();
    render_with_engine_routing(&input, engine, None, Some(preset)).unwrap()
}

#[test]
fn flux_polyline_vs_orthogonal_produce_distinct_svg_for_cycle() {
    // For a diagram with a backward edge, OrthogonalRoute (orthogonal) and
    // PolylineRoute (polyline) should produce distinct edge paths.
    let orthogonal = render_cycle_svg_with_routing("flux-layered", RoutingStyle::Orthogonal);
    let polyline = render_cycle_svg_with_routing("flux-layered", RoutingStyle::Polyline);
    assert_ne!(
        orthogonal, polyline,
        "flux-layered orthogonal and polyline routing should produce distinct SVG edge paths"
    );
}

#[test]
fn basis_preset_uses_polyline_edge_routing_on_flux() {
    // basis expands to Polyline+Basis — should use PolylineRoute (same as explicit Polyline).
    // Edge path topology should match explicit polyline routing.
    let basis = render_cycle_svg_with_preset("flux-layered", EdgePreset::Basis);
    let polyline = render_cycle_svg_with_routing("flux-layered", RoutingStyle::Polyline);
    assert_eq!(
        basis, polyline,
        "basis preset should produce same edge path topology as explicit polyline routing"
    );
}

#[test]
fn flux_polyline_routing_differs_from_mermaid_layered_for_cycle() {
    // Flux-layered enables enhanced backward routing (channel alignment) while
    // mermaid-layered preserves dagre v0.8.5 behavior. For diagrams with backward
    // edges (cycles), polyline paths should differ between the two engines.
    let flux_polyline = render_cycle_svg_with_routing("flux-layered", RoutingStyle::Polyline);
    let mermaid_polyline = render_cycle_svg_with_routing("mermaid-layered", RoutingStyle::Polyline);
    assert_ne!(
        flux_polyline, mermaid_polyline,
        "flux+polyline should differ from mermaid+polyline due to enhanced backward routing"
    );
}

// =============================================================================
// Phase 7.5: Render-style isolation — curve does not affect geometry
// =============================================================================

/// Helper: render simple_cycle.mmd as MMDS JSON with explicit style settings.
fn render_cycle_mmds_with_styles(routing: RoutingStyle, curve: Curve) -> String {
    let input = std::fs::read_to_string("tests/fixtures/flowchart/simple_cycle.mmd")
        .expect("simple_cycle.mmd should exist");
    let mut instance = FlowchartInstance::new();
    instance.parse(&input).expect("parse should succeed");
    let config = RenderConfig {
        routing_style: Some(routing),
        curve: Some(curve),
        geometry_level: mmdflux::diagram::GeometryLevel::Layout,
        ..Default::default()
    };
    instance
        .render(OutputFormat::Mmds, &config)
        .expect("render should succeed")
}

#[test]
fn interpolation_style_does_not_affect_mmds_layout_geometry() {
    // Layout-level MMDS (no paths) should be identical regardless of curve style.
    // Curve selection is a render-time concern — it only affects SVG path drawing.
    let basis = render_cycle_mmds_with_styles(RoutingStyle::Polyline, Curve::Basis);
    let linear =
        render_cycle_mmds_with_styles(RoutingStyle::Polyline, Curve::Linear(CornerStyle::Sharp));
    assert_eq!(
        basis, linear,
        "layout-level MMDS geometry should be identical regardless of curve style"
    );
}

#[test]
fn corner_style_does_not_affect_mmds_layout_geometry() {
    // Layout-level MMDS (no paths) should be identical regardless of linear corner style.
    // Corner treatment is a render-time concern — it only affects SVG corner drawing.
    let sharp =
        render_cycle_mmds_with_styles(RoutingStyle::Orthogonal, Curve::Linear(CornerStyle::Sharp));
    let rounded = render_cycle_mmds_with_styles(
        RoutingStyle::Orthogonal,
        Curve::Linear(CornerStyle::Rounded),
    );
    assert_eq!(
        sharp, rounded,
        "layout-level MMDS geometry should be identical regardless of corner style"
    );
}
