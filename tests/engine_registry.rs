//! Engine registry tests: typed engine IDs, parsing, availability, and registry lookup.

use mmdflux::format::{CornerStyle, Curve, EdgePreset, RoutingStyle};
use mmdflux::{AlgorithmId, EngineAlgorithmId, EngineId, OutputFormat, RenderConfig, RenderError};

// =============================================================================
// Engine selection through render path
// =============================================================================

/// Helper: parse + render with a specific engine algorithm ID string.
fn render_with_engine(input: &str, engine: &str) -> Result<String, RenderError> {
    let engine = EngineAlgorithmId::parse(engine)?;
    let config = RenderConfig {
        layout_engine: Some(engine),
        ..Default::default()
    };
    mmdflux::render_diagram(input, OutputFormat::Text, &config)
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
    let flux_out = mmdflux::render_diagram(
        &input,
        OutputFormat::Svg,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .unwrap();
    let mermaid_out = mmdflux::render_diagram(
        &input,
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
    assert!(caps.supports_subgraphs);
    assert!(caps.route_ownership.routes_edges());
    assert!(
        caps.supported_routing_styles
            .contains(&RoutingStyle::Orthogonal)
    );
}

#[test]
fn mermaid_layered_capabilities() {
    let id = EngineAlgorithmId::parse("mermaid-layered").unwrap();
    let caps = id.capabilities();
    assert!(caps.supports_subgraphs);
    assert!(!caps.route_ownership.routes_edges());
    assert_eq!(caps.supported_routing_styles, &[RoutingStyle::Polyline]);
}

#[test]
fn elk_layered_capabilities() {
    let id = EngineAlgorithmId::parse("elk-layered").unwrap();
    let caps = id.capabilities();
    assert!(caps.supports_subgraphs);
    assert!(caps.route_ownership.routes_edges());
    assert!(
        caps.supported_routing_styles
            .contains(&RoutingStyle::Orthogonal)
    );
}

#[test]
fn elk_mrtree_capabilities() {
    let id = EngineAlgorithmId::parse("elk-mrtree").unwrap();
    let caps = id.capabilities();
    assert!(!caps.supports_subgraphs);
    assert!(caps.route_ownership.routes_edges());
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
// Engine routing style capabilities and validation (plan-0081 Phase 7.3)
// =============================================================================

/// Helper: render with a specific engine, optional explicit routing style and/or preset.
fn render_with_engine_routing(
    input: &str,
    engine: &str,
    routing: Option<RoutingStyle>,
    preset: Option<EdgePreset>,
) -> Result<String, RenderError> {
    let engine_id = EngineAlgorithmId::parse(engine)?;
    let config = RenderConfig {
        layout_engine: Some(engine_id),
        routing_style: routing,
        edge_preset: preset,
        ..Default::default()
    };
    mmdflux::render_diagram(input, OutputFormat::Svg, &config)
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
    let caps = EngineAlgorithmId::parse("flux-layered")
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
    let config = RenderConfig {
        routing_style: Some(routing),
        curve: Some(curve),
        geometry_level: mmdflux::graph::GeometryLevel::Layout,
        ..Default::default()
    };
    mmdflux::render_diagram(&input, OutputFormat::Mmds, &config).expect("render should succeed")
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
