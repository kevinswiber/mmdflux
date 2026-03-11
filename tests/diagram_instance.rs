use std::fs;
use std::path::Path;

use mmdflux::registry::{DiagramDefinition, DiagramInstance, DiagramRegistry};
use mmdflux::{DiagramFamily, GeometryLevel, OutputFormat, RenderConfig, RenderError};

struct MockDiagram {
    parsed: Option<String>,
}

fn mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

impl MockDiagram {
    fn new() -> Self {
        Self { parsed: None }
    }
}

impl DiagramInstance for MockDiagram {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.parsed = Some(input.to_string());
        Ok(())
    }

    fn render(&self, format: OutputFormat, _config: &RenderConfig) -> Result<String, RenderError> {
        let content = self.parsed.as_ref().ok_or("Not parsed")?;
        match format {
            OutputFormat::Text => Ok(format!("[TEXT] {}", content)),
            OutputFormat::Ascii => Ok(format!("[ASCII] {}", content)),
            OutputFormat::Svg | OutputFormat::Mmds | OutputFormat::Mermaid => {
                Err("Not supported".into())
            }
        }
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        matches!(format, OutputFormat::Text | OutputFormat::Ascii)
    }
}

#[test]
fn diagram_instance_parse_and_render() {
    let mut diagram = MockDiagram::new();
    diagram.parse("test input").unwrap();

    let output = diagram
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert_eq!(output, "[TEXT] test input");
}

#[test]
fn diagram_instance_supports_format() {
    let diagram = MockDiagram::new();
    assert!(diagram.supports_format(OutputFormat::Text));
    assert!(diagram.supports_format(OutputFormat::Ascii));
    assert!(!diagram.supports_format(OutputFormat::Svg));
}

#[test]
fn diagram_instance_unsupported_format_errors() {
    let mut diagram = MockDiagram::new();
    diagram.parse("test").unwrap();
    let result = diagram.render(OutputFormat::Svg, &RenderConfig::default());
    assert!(result.is_err());
}

#[test]
fn registry_create_returns_instance() {
    let mut registry = DiagramRegistry::new();

    registry.register(DiagramDefinition {
        id: "mock",
        family: DiagramFamily::Graph,
        detector: |_| true,
        factory: || Box::new(MockDiagram::new()),
        supported_formats: &[OutputFormat::Text, OutputFormat::Ascii],
    });

    let mut instance = registry.create("mock").expect("should create instance");
    instance.parse("hello").unwrap();
    let output = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .unwrap();
    assert_eq!(output, "[TEXT] hello");
}

#[test]
fn registry_create_unknown_returns_none() {
    let registry = DiagramRegistry::new();
    assert!(registry.create("unknown").is_none());
}

#[test]
fn mmds_module_exports_instance_type() {
    let _ = mmdflux::diagrams::mmds::MmdsInstance::default();
}

#[test]
fn mmds_instance_parse_accepts_minimal_layout_payload() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let input = mmds_fixture("minimal-layout.json");

    instance.parse(&input).expect("parse should succeed");
    assert!(instance.has_parsed_payload());
}

#[test]
fn mmds_instance_parse_rejects_invalid_json_with_stable_message() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let err = instance.parse("not json").unwrap_err();
    assert!(err.to_string().starts_with("MMDS parse error:"));
}

#[test]
fn mmds_layout_payload_renders_text() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let input = mmds_fixture("minimal-layout.json");
    instance.parse(&input).expect("parse should succeed");

    let rendered = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .expect("layout payload should render text");
    assert!(rendered.contains("Start"));
    assert!(rendered.contains("End"));
}

#[test]
fn mmds_routed_geometry_level_uses_direct_svg_path() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let input = mmds_fixture("positioned/routed-basic.json");
    instance.parse(&input).expect("parse should succeed");

    let svg = instance
        .render(OutputFormat::Svg, &RenderConfig::default())
        .expect("routed MMDS should render SVG");
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Start"));
}

#[test]
fn mmds_routed_geometry_level_renders_text_by_ignoring_paths() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let input = mmds_fixture("positioned/routed-basic.json");
    instance.parse(&input).expect("parse should succeed");

    let rendered = instance
        .render(OutputFormat::Text, &RenderConfig::default())
        .expect("routed MMDS should render text by ignoring paths");
    assert!(rendered.contains("Start"));
}

#[test]
fn mmds_routed_json_output_at_layout_level_strips_paths() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let input = mmds_fixture("positioned/routed-basic.json");
    instance.parse(&input).expect("parse should succeed");

    // Default geometry_level is Layout, so routed input should be downgraded.
    let json = instance
        .render(OutputFormat::Mmds, &RenderConfig::default())
        .expect("routed MMDS should render JSON at layout level");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["geometry_level"], "layout");
    // Routed-only fields should be stripped.
    let edge = &value["edges"][0];
    assert!(edge.get("path").is_none());
    assert!(edge.get("label_position").is_none());
    assert!(edge.get("is_backward").is_none());
}

#[test]
fn mmds_routed_json_output_at_routed_level_preserves_paths() {
    let mut instance = mmdflux::diagrams::mmds::MmdsInstance::default();
    let input = mmds_fixture("positioned/routed-basic.json");
    instance.parse(&input).expect("parse should succeed");

    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..Default::default()
    };
    let json = instance
        .render(OutputFormat::Mmds, &config)
        .expect("routed MMDS should pass through at routed level");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["geometry_level"], "routed");
    assert!(value["edges"][0].get("path").is_some());
}
