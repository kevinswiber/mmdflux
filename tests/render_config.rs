use mmdflux::{
    CornerStyle, Curve, EdgePreset, EngineAlgorithmId, OutputFormat, PathSimplification,
    RenderConfig, RenderError, RoutingStyle,
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
    assert!(config.curve.is_none());
    assert!(config.edge_radius.is_none());
    assert!(config.svg_diagram_padding.is_none());
    assert!(!config.show_ids);
}

#[test]
fn render_config_default_has_no_curve_override() {
    let config = RenderConfig::default();
    assert!(config.curve.is_none());
}

#[test]
fn render_config_accepts_explicit_curve_override() {
    let config = RenderConfig {
        curve: Some(Curve::Linear(CornerStyle::Rounded)),
        ..Default::default()
    };
    assert_eq!(config.curve, Some(Curve::Linear(CornerStyle::Rounded)));
}

#[test]
fn render_error_from_string() {
    let err: RenderError = "test error".into();
    assert_eq!(err.message, "test error");
    assert_eq!(err.to_string(), "test error");
}

#[test]
fn render_config_to_text_render_options_conversion() {
    let config = RenderConfig::default();
    let options: mmdflux::render::graph::TextRenderOptions = (&config).into();
    assert!(matches!(options.output_format, OutputFormat::Text));
}

#[test]
fn layout_config_accessible_from_flat_config_module() {
    let _ = mmdflux::config::LayoutConfig::default();
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

#[test]
fn svg_render_options_default_config_uses_orthogonal_routing() {
    let config = RenderConfig::default();
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
    assert_eq!(options.curve, Curve::Basis);
}

#[test]
fn svg_render_options_step_preset_expands_to_orthogonal_routing() {
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step),
        ..Default::default()
    };
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
    assert_eq!(options.curve, Curve::Linear(CornerStyle::Sharp));
}

#[test]
fn svg_render_options_basis_preset_expands_to_polyline_routing() {
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Basis),
        ..Default::default()
    };
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.routing_style, RoutingStyle::Polyline);
    assert_eq!(options.curve, Curve::Basis);
}

#[test]
fn svg_render_options_explicit_routing_style_overrides_preset_routing() {
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step),
        routing_style: Some(RoutingStyle::Polyline),
        ..Default::default()
    };
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.routing_style, RoutingStyle::Polyline);
    assert_eq!(options.curve, Curve::Linear(CornerStyle::Sharp));
}

#[test]
fn svg_render_options_explicit_curve_overrides_preset_curve() {
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step),
        curve: Some(Curve::Basis),
        ..Default::default()
    };
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.routing_style, RoutingStyle::Orthogonal);
    assert_eq!(options.curve, Curve::Basis);
}

#[test]
fn svg_render_options_path_simplification_is_preserved() {
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Polyline),
        path_simplification: PathSimplification::Lossless,
        ..Default::default()
    };
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.path_simplification, PathSimplification::Lossless);
}

#[test]
fn svg_render_options_mermaid_engine_uses_polyline_by_default() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
        ..Default::default()
    };
    let options: mmdflux::render::graph::SvgRenderOptions = (&config).into();
    assert_eq!(options.routing_style, RoutingStyle::Polyline);
    assert_eq!(options.curve, Curve::Basis);
}

#[test]
fn text_render_options_carry_padding_and_path_simplification() {
    let config = RenderConfig {
        padding: Some(4),
        routing_style: Some(RoutingStyle::Direct),
        path_simplification: PathSimplification::Lossless,
        ..Default::default()
    };
    let options: mmdflux::render::graph::TextRenderOptions = (&config).into();
    assert_eq!(options.padding, Some(4));
    assert_eq!(options.routing_style, RoutingStyle::Direct);
    assert_eq!(options.path_simplification, PathSimplification::Lossless);
}
