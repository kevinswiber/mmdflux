use std::fs;
use std::path::Path;

use mmdflux::mmds::SUPPORTED_OUTPUT_FORMATS as MMDS_SUPPORTED_OUTPUT_FORMATS;
use mmdflux::prepared::PreparedDiagram;
use mmdflux::registry::{DiagramDefinition, DiagramInstance, DiagramRegistry, ParsedDiagram};
use mmdflux::{DiagramFamily, GeometryLevel, OutputFormat, RenderConfig, RenderError};

struct MockDiagram;

struct MockParsedDiagram {
    content: String,
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
        Self
    }
}

impl DiagramInstance for MockDiagram {
    fn parse(
        self: Box<Self>,
        input: &str,
    ) -> Result<Box<dyn ParsedDiagram>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Box::new(MockParsedDiagram {
            content: input.to_string(),
        }))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        matches!(format, OutputFormat::Text | OutputFormat::Ascii)
    }
}

impl ParsedDiagram for MockParsedDiagram {
    fn prepare(self: Box<Self>, _config: &RenderConfig) -> Result<PreparedDiagram, RenderError> {
        Ok(PreparedDiagram::Text(format!("[TEXT] {}", self.content)))
    }
}

#[test]
fn diagram_instance_parse_and_prepare() {
    let prepared = Box::new(MockDiagram::new())
        .parse("test input")
        .unwrap()
        .prepare(&RenderConfig::default())
        .unwrap();
    assert!(matches!(
        prepared,
        PreparedDiagram::Text(text) if text == "[TEXT] test input"
    ));
}

#[test]
fn diagram_instance_prepare_returns_a_prepared_payload() {
    let prepared = Box::new(MockDiagram::new())
        .parse("test input")
        .unwrap()
        .prepare(&RenderConfig::default())
        .unwrap();
    assert!(matches!(prepared, PreparedDiagram::Text(_)));
}

#[test]
fn diagram_instance_trait_is_phase_split() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("registry.rs"),
    )
    .unwrap();
    let diagram_instance_block = source
        .split("pub trait ParsedDiagram")
        .next()
        .expect("DiagramInstance trait should precede ParsedDiagram");

    assert!(!source.contains("fn render(&self"));
    assert!(source.contains("fn parse(\n        self: Box<Self>,"));
    assert!(source.contains("pub trait ParsedDiagram"));
    assert!(!diagram_instance_block.contains("fn prepare("));
}

#[test]
fn diagram_instance_supports_format() {
    let diagram = MockDiagram::new();
    assert!(diagram.supports_format(OutputFormat::Text));
    assert!(diagram.supports_format(OutputFormat::Ascii));
    assert!(!diagram.supports_format(OutputFormat::Svg));
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

    let instance = registry.create("mock").expect("should create instance");
    let prepared = instance
        .parse("hello")
        .unwrap()
        .prepare(&RenderConfig::default())
        .unwrap();
    assert!(matches!(
        prepared,
        PreparedDiagram::Text(text) if text == "[TEXT] hello"
    ));
}

#[test]
fn registry_create_unknown_returns_none() {
    let registry = DiagramRegistry::new();
    assert!(registry.create("unknown").is_none());
}

#[test]
fn diagram_definitions_and_instances_share_one_format_contract() {
    let registry = mmdflux::registry_builtins::default_registry();
    let formats = [
        OutputFormat::Text,
        OutputFormat::Ascii,
        OutputFormat::Svg,
        OutputFormat::Mmds,
        OutputFormat::Mermaid,
    ];

    for diagram_id in registry.diagram_ids() {
        let definition = registry
            .get(diagram_id)
            .expect("registered diagrams should expose a definition");
        let instance = registry
            .create(diagram_id)
            .expect("registered diagrams should instantiate");

        for format in formats {
            assert_eq!(
                definition.supported_formats.contains(&format),
                instance.supports_format(format),
                "supported format drift for diagram {diagram_id} and format {format}"
            );
        }
    }

    for format in [
        OutputFormat::Text,
        OutputFormat::Ascii,
        OutputFormat::Svg,
        OutputFormat::Mmds,
        OutputFormat::Mermaid,
    ] {
        assert_eq!(
            MMDS_SUPPORTED_OUTPUT_FORMATS.contains(&format),
            mmdflux::mmds::supports_format(format),
            "MMDS format drift for {format}"
        );
    }
}

#[test]
fn mmds_module_exports_core_entrypoints() {
    let _ = mmdflux::mmds::parse_mmds_input;
    let _ = mmdflux::mmds::render_input;
}

#[test]
fn mmds_module_detects_minimal_layout_payload() {
    let input = mmds_fixture("minimal-layout.json");

    let diagram_id = mmdflux::mmds::detect_diagram_type(&input).expect("parse should succeed");
    assert_eq!(diagram_id, "flowchart");
}

#[test]
fn mmds_module_parse_rejects_invalid_json_with_stable_message() {
    let err = mmdflux::mmds::parse_mmds_input("not json").unwrap_err();
    assert!(err.to_string().starts_with("MMDS parse error:"));
}

#[test]
fn mmds_layout_payload_renders_text() {
    let input = mmds_fixture("minimal-layout.json");

    let rendered =
        mmdflux::mmds::render_input(&input, OutputFormat::Text, &RenderConfig::default())
            .expect("layout payload should render text");
    assert!(rendered.contains("Start"));
    assert!(rendered.contains("End"));
}

#[test]
fn mmds_routed_geometry_level_uses_direct_svg_path() {
    let input = mmds_fixture("positioned/routed-basic.json");

    let svg = mmdflux::mmds::render_input(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("routed MMDS should render SVG");
    assert!(svg.starts_with("<svg"));
    assert!(svg.contains("Start"));
}

#[test]
fn mmds_routed_geometry_level_renders_text_by_ignoring_paths() {
    let input = mmds_fixture("positioned/routed-basic.json");

    let rendered =
        mmdflux::mmds::render_input(&input, OutputFormat::Text, &RenderConfig::default())
            .expect("routed MMDS should render text by ignoring paths");
    assert!(rendered.contains("Start"));
}

#[test]
fn mmds_routed_json_output_at_layout_level_strips_paths() {
    let input = mmds_fixture("positioned/routed-basic.json");

    // Default geometry_level is Layout, so routed input should be downgraded.
    let json = mmdflux::mmds::render_input(&input, OutputFormat::Mmds, &RenderConfig::default())
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
    let input = mmds_fixture("positioned/routed-basic.json");

    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..Default::default()
    };
    let json = mmdflux::mmds::render_input(&input, OutputFormat::Mmds, &config)
        .expect("routed MMDS should pass through at routed level");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["geometry_level"], "routed");
    assert!(value["edges"][0].get("path").is_some());
}
