use mmdflux::diagram::{
    DiagramFamily, DiagramModel, EngineConfig, OutputFormat, PathSimplification, RenderConfig,
};

#[test]
fn diagram_family_variants_exist() {
    let _graph = DiagramFamily::Graph;
    let _timeline = DiagramFamily::Timeline;
    let _chart = DiagramFamily::Chart;
    let _table = DiagramFamily::Table;
}

#[test]
fn output_format_default_is_text() {
    assert_eq!(OutputFormat::default(), OutputFormat::Text);
}

struct DummyModel;
impl DiagramModel for DummyModel {
    fn clear(&mut self) {}
    fn title(&self) -> Option<&str> {
        None
    }
    fn acc_title(&self) -> Option<&str> {
        None
    }
    fn acc_description(&self) -> Option<&str> {
        None
    }
}

#[test]
fn dummy_model_compiles() {
    let mut model = DummyModel;
    model.clear();
    assert!(model.title().is_none());
    assert!(model.acc_title().is_none());
    assert!(model.acc_description().is_none());
}

// --- EngineConfig tests (Task 1.2) ---

#[test]
fn engine_config_layered_variant_exists() {
    let layered_cfg = mmdflux::layered::LayoutConfig::default();
    let ec = EngineConfig::Layered(layered_cfg);
    assert!(matches!(ec, EngineConfig::Layered(_)));
}

#[test]
fn render_config_layout_converts_to_engine_config_layered() {
    let cfg: EngineConfig = RenderConfig::default().layout.into();
    assert!(matches!(cfg, EngineConfig::Layered(_)));
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
