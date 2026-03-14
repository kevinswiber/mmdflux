use mmdflux::config::{LabelDummyStrategy, LayoutConfig, LayoutDirection, Ranker};
use mmdflux::registry::DiagramFamily;
use mmdflux::simplification::PathSimplification;
use mmdflux::{OutputFormat, RenderConfig};

#[test]
fn diagram_family_variants_exist() {
    let _graph = DiagramFamily::Graph;
    let _timeline = DiagramFamily::Timeline;
}

#[test]
fn output_format_default_is_text() {
    assert_eq!(OutputFormat::default(), OutputFormat::Text);
}

#[test]
fn layout_config_public_defaults_are_accessible() {
    let cfg = LayoutConfig::default();
    assert_eq!(cfg.direction, LayoutDirection::TopBottom);
    assert_eq!(cfg.ranker, Ranker::NetworkSimplex);
    assert_eq!(cfg.label_dummy_strategy, LabelDummyStrategy::Midpoint);
    assert_eq!(cfg.rank_sep, 50.0);
}

#[test]
fn render_config_embeds_public_layout_config() {
    let cfg = RenderConfig::default();
    assert_eq!(cfg.layout.direction, LayoutDirection::TopBottom);
    assert_eq!(cfg.layout.ranker, Ranker::NetworkSimplex);
}

#[test]
fn render_config_default_layout_engine_is_none() {
    let cfg = RenderConfig::default();
    assert!(cfg.layout_engine.is_none());
}

#[test]
fn path_simplification_lossless_parses() {
    assert_eq!(
        PathSimplification::parse("lossless").unwrap(),
        PathSimplification::Lossless
    );
}
