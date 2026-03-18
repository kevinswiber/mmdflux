use mmdflux::format::{CornerStyle, Curve, EdgePreset, RoutingStyle};
use mmdflux::simplification::PathSimplification;
use mmdflux::{EngineAlgorithmId, RenderConfig, RenderError};

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
fn layout_config_accessible_from_public_config_module() {
    let _ = mmdflux::LayoutConfig::default();
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
fn render_config_keeps_public_svg_style_fields() {
    let config = RenderConfig {
        edge_preset: Some(EdgePreset::Step),
        routing_style: Some(RoutingStyle::Polyline),
        curve: Some(Curve::Linear(CornerStyle::Rounded)),
        path_simplification: PathSimplification::Lossless,
        ..Default::default()
    };
    assert_eq!(config.edge_preset, Some(EdgePreset::Step));
    assert_eq!(config.routing_style, Some(RoutingStyle::Polyline));
    assert_eq!(config.curve, Some(Curve::Linear(CornerStyle::Rounded)));
    assert_eq!(config.path_simplification, PathSimplification::Lossless);
}
