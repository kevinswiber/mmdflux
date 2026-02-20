use mmdflux::diagram::{
    CornerStyle, EdgePreset, EdgeRouting, EngineAlgorithmId, InterpolationStyle, OutputFormat,
    PathSimplification, RenderConfig, RenderError, RoutingStyle,
};

#[test]
fn render_config_default() {
    let config = RenderConfig::default();
    assert!(config.padding.is_none());
    assert!(config.cluster_ranksep.is_none());
    assert!(config.svg_scale.is_none());
    assert!(config.svg_node_padding_x.is_none());
    assert!(config.svg_node_padding_y.is_none());
    assert!(config.edge_preset.is_none());
    assert!(config.routing_style.is_none());
    assert!(config.interpolation_style.is_none());
    assert!(config.corner_style.is_none());
    assert!(config.edge_radius.is_none());
    assert!(config.svg_diagram_padding.is_none());
    assert!(!config.show_ids);
}

#[test]
fn render_error_from_string() {
    let err: RenderError = "test error".into();
    assert_eq!(err.message, "test error");
    assert_eq!(err.to_string(), "test error");
}

#[test]
fn render_config_to_render_options_conversion() {
    let config = RenderConfig::default();
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert!(matches!(options.output_format, OutputFormat::Text));
}

#[test]
fn diagram_layout_config_accessible_from_diagram_module() {
    let _ = mmdflux::diagram::LayoutConfig::default();
}

#[test]
fn render_config_with_engine_algorithm() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..Default::default()
    };
    assert_eq!(config.layout_engine.unwrap().to_string(), "flux-layered");
}

#[test]
fn render_config_default_layout_engine_is_none() {
    let config = RenderConfig::default();
    assert!(config.layout_engine.is_none());
}

// =============================================================================
// Phase 7.5: Style resolution in RenderOptions::from(RenderConfig)
// =============================================================================

#[test]
fn render_options_default_config_uses_orthogonal_routing() {
    // Default engine (flux-layered) defaults to Orthogonal routing + Bezier interpolation.
    let config = RenderConfig::default();
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(
        options.svg.routing_style,
        RoutingStyle::Orthogonal,
        "default flux-layered should produce orthogonal routing"
    );
    assert_eq!(
        options.svg.interpolation_style,
        InterpolationStyle::Bezier,
        "default flux-layered should produce bezier interpolation"
    );
    assert_eq!(
        options.svg.corner_style,
        CornerStyle::Sharp,
        "default flux-layered should produce sharp corners"
    );
}

#[test]
fn render_options_step_preset_expands_to_orthogonal_routing() {
    // EdgePreset::Step expands to (Orthogonal, Linear, Sharp).
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step),
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(options.svg.routing_style, RoutingStyle::Orthogonal);
    assert_eq!(options.svg.interpolation_style, InterpolationStyle::Linear);
    assert_eq!(options.svg.corner_style, CornerStyle::Sharp);
}

#[test]
fn render_options_bezier_preset_expands_to_polyline_routing() {
    // EdgePreset::Bezier expands to (Polyline, Bezier, Sharp).
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Bezier),
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(options.svg.routing_style, RoutingStyle::Polyline);
    assert_eq!(options.svg.interpolation_style, InterpolationStyle::Bezier);
}

#[test]
fn render_options_explicit_routing_style_overrides_preset_routing() {
    // Explicit --routing-style wins over the routing component of --edge-preset.
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step),         // Step → Orthogonal
        routing_style: Some(RoutingStyle::Polyline), // override → Polyline
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(
        options.svg.routing_style,
        RoutingStyle::Polyline,
        "explicit routing_style should override preset routing component"
    );
    // Interpolation still comes from preset (Linear for Step)
    assert_eq!(options.svg.interpolation_style, InterpolationStyle::Linear);
}

#[test]
fn render_options_explicit_interpolation_overrides_preset_interpolation() {
    // Explicit --interpolation-style wins over the interpolation component of --edge-preset.
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step), // Step → Linear
        interpolation_style: Some(InterpolationStyle::Bezier), // override → Bezier
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(options.svg.routing_style, RoutingStyle::Orthogonal); // from preset
    assert_eq!(
        options.svg.interpolation_style,
        InterpolationStyle::Bezier,
        "explicit interpolation_style should override preset interpolation component"
    );
}

#[test]
fn render_options_default_flux_engine_selects_orthogonal_route() {
    // Default flux-layered with no explicit style → Orthogonal → OrthogonalRoute.
    let config = RenderConfig::default();
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(
        options.edge_routing,
        Some(EdgeRouting::OrthogonalRoute),
        "default flux-layered should select OrthogonalRoute for orthogonal routing"
    );
}

#[test]
fn render_options_straight_preset_selects_direct_route_on_flux() {
    // Straight preset → Direct routing → DirectRoute on flux-layered (Native).
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Straight),
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(
        options.edge_routing,
        Some(EdgeRouting::DirectRoute),
        "Straight preset (Direct routing) on flux-layered should select DirectRoute"
    );
}

#[test]
fn render_options_polyline_preset_selects_polyline_route_on_flux() {
    // Polyline preset → Polyline routing → PolylineRoute on flux-layered (Native).
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Polyline),
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(
        options.edge_routing,
        Some(EdgeRouting::PolylineRoute),
        "Polyline preset on flux-layered should select PolylineRoute"
    );
}

#[test]
fn render_options_interpolation_change_does_not_affect_edge_routing_selection() {
    // Interpolation style is render-only; changing it does not affect EdgeRouting selection.
    // Both bezier and linear under the same routing style should produce the same EdgeRouting.
    let config_bezier = RenderConfig {
        edge_preset: Some(EdgePreset::Bezier), // Polyline + Bezier
        ..Default::default()
    };
    let config_polyline = RenderConfig {
        edge_preset: Some(EdgePreset::Polyline), // Polyline + Linear
        ..Default::default()
    };
    let opts_bezier: mmdflux::render::RenderOptions = (&config_bezier).into();
    let opts_polyline: mmdflux::render::RenderOptions = (&config_polyline).into();
    // Both are Polyline routing → both should get PolylineRoute (same edge routing).
    assert_eq!(
        opts_bezier.edge_routing, opts_polyline.edge_routing,
        "interpolation style change should not affect EdgeRouting selection (same routing style)"
    );
}

#[test]
fn render_options_path_simplification_is_orthogonal_to_style_selection() {
    let cases = [
        (Some(EdgePreset::Straight), None),
        (Some(EdgePreset::Polyline), None),
        (Some(EdgePreset::Step), None),
        (Some(EdgePreset::Bezier), None),
        (None, Some(RoutingStyle::Direct)),
        (None, Some(RoutingStyle::Polyline)),
        (None, Some(RoutingStyle::Orthogonal)),
    ];

    for (edge_preset, routing_style) in cases {
        let config = RenderConfig {
            edge_preset,
            routing_style,
            path_simplification: PathSimplification::Lossless,
            ..Default::default()
        };
        let options: mmdflux::render::RenderOptions = (&config).into();
        assert_eq!(
            options.path_simplification,
            PathSimplification::Lossless,
            "path simplification should remain unchanged for edge_preset={edge_preset:?} routing_style={routing_style:?}"
        );
    }
}

#[test]
fn render_options_mermaid_engine_uses_polyline_route_by_default() {
    // mermaid-layered uses HintDriven → always PolylineRoute regardless of routing style.
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
        ..Default::default()
    };
    let options: mmdflux::render::RenderOptions = (&config).into();
    assert_eq!(
        options.edge_routing,
        Some(EdgeRouting::PolylineRoute),
        "mermaid-layered (HintDriven) should always select PolylineRoute"
    );
}
