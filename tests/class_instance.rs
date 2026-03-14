use std::fs;
use std::path::Path;

use mmdflux::builtins::default_registry;
use mmdflux::graph::GeometryLevel;
use mmdflux::payload::Diagram;
use mmdflux::registry::DiagramInstance;
use mmdflux::{EngineAlgorithmId, OutputFormat, RenderConfig, RenderError};

fn render_class(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
) -> Result<String, RenderError> {
    mmdflux::render_diagram(input, format, config)
}

fn class_instance() -> Box<dyn DiagramInstance> {
    default_registry()
        .create("class")
        .expect("class should be registered")
}

#[test]
fn class_instance_parse_simple() {
    let result = class_instance().parse("classDiagram\nclass User");
    assert!(result.is_ok());
}

#[test]
fn class_instance_parse_error_on_invalid() {
    let result = class_instance().parse("not a class diagram");
    assert!(result.is_err());
}

#[test]
fn class_instance_into_payload_returns_graph_payload() {
    let payload = class_instance()
        .parse("classDiagram\nclass A\nclass B\nA --> B")
        .unwrap()
        .into_payload(&RenderConfig::default())
        .unwrap();
    let Diagram::Class(graph) = payload else {
        panic!("class should yield a class payload");
    };
    assert!(graph.nodes.contains_key("A"));
    assert!(graph.nodes.contains_key("B"));
}

#[test]
fn class_instance_render_ascii() {
    let out = render_class(
        "classDiagram\nA --> B",
        OutputFormat::Ascii,
        &RenderConfig::default(),
    )
    .unwrap();
    // ASCII mode should not contain Unicode box-drawing chars
    assert!(!out.contains('│'));
    assert!(!out.contains('─'));
}

#[test]
fn class_instance_render_svg() {
    let out = render_class(
        "classDiagram\nA --> B",
        OutputFormat::Svg,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(out.starts_with("<svg"));
    assert!(out.contains("<text"));
}

#[test]
fn class_instance_supports_text_ascii_svg() {
    let instance = class_instance();
    assert!(instance.supports_format(OutputFormat::Text));
    assert!(instance.supports_format(OutputFormat::Ascii));
    assert!(instance.supports_format(OutputFormat::Svg));
}

#[test]
fn class_instance_dependency_renders_dotted() {
    let out = render_class(
        "classDiagram\nA ..> B",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    // Dotted edges use ╎ or ┊ or similar in text mode
    assert!(out.contains('A'));
    assert!(out.contains('B'));
}

#[test]
fn class_instance_inheritance_renders() {
    let out = render_class(
        "classDiagram\nAnimal <|-- Dog",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();
    assert!(out.contains("Animal"));
    assert!(out.contains("Dog"));
}

#[test]
fn lollipop_relations_render_all_participating_classes() {
    let input = "classDiagram\nclass Class01 {\n  int amount\n  draw()\n}\nClass01 --() bar\nClass02 --() bar\nfoo ()-- Class01";
    let output = render_class(input, OutputFormat::Text, &RenderConfig::default()).unwrap();

    assert!(output.contains("Class02"));
    assert!(output.contains("foo"));
    assert!(!output.contains("│ bar │"));
    assert!(!output.contains("│ foo │"));
}

#[test]
fn lollipop_same_name_interfaces_render_as_distinct_endpoints() {
    let input = "classDiagram\nService --() InterfaceA\nClient --() InterfaceA";
    let output = render_class(input, OutputFormat::Text, &RenderConfig::default()).unwrap();

    assert_eq!(output.matches("InterfaceA").count(), 2);
    assert!(!output.contains("│ InterfaceA │"));
}

#[test]
fn namespace_blocks_render_namespace_titles() {
    let input = "\
classDiagram
namespace BaseShapes {
  class Triangle
  class Rectangle
}
Triangle --> Rectangle";
    let output = render_class(input, OutputFormat::Text, &RenderConfig::default()).unwrap();

    assert!(output.contains("BaseShapes"));
    assert!(output.contains("Triangle"));
    assert!(output.contains("Rectangle"));
}

#[test]
fn class_instance_via_registry() {
    let registry = default_registry();
    let instance = registry.create("class").unwrap();
    let payload = instance
        .parse("classDiagram\nclass User\nclass Order\nUser --> Order")
        .unwrap()
        .into_payload(&RenderConfig::default())
        .unwrap();
    assert!(matches!(payload, Diagram::Class(_)));
}

#[test]
fn class_instance_unknown_engine_rejected_at_parse_boundary() {
    let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
    assert!(err.message.contains("unknown engine"));
}

#[cfg(not(feature = "engine-elk"))]
#[test]
fn class_instance_unknown_engine_errors_cleanly() {
    let result = render_class(
        "classDiagram\nA --> B",
        OutputFormat::Text,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("elk-layered").unwrap()),
            ..RenderConfig::default()
        },
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message.contains("engine-elk") || err.message.contains("not available"),
        "error should be actionable: {}",
        err.message
    );
}

#[test]
fn class_routed_mmds_honors_edge_routing_override() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("class")
        .join("animal_hierarchy.mmd");
    let input = fs::read_to_string(&fixture).expect("class fixture should read");

    let full = render_class(
        &input,
        OutputFormat::Mmds,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("mermaid-layered mmds should render");
    let orthogonal = render_class(
        &input,
        OutputFormat::Mmds,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("flux-layered mmds should render");

    assert_ne!(
        full, orthogonal,
        "class routed MMDS should differ between mermaid-layered and flux-layered engines"
    );
}

#[test]
fn class_routed_mmds_honors_edge_routing_override_on_cycle() {
    let input = "classDiagram\nA --> B\nB --> C\nC --> A\n";
    let full = render_class(
        input,
        OutputFormat::Mmds,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("mermaid-layered mmds should render");
    let orthogonal = render_class(
        input,
        OutputFormat::Mmds,
        &RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("flux-layered mmds should render");

    assert_ne!(
        full, orthogonal,
        "class routed MMDS cycle output should differ between mermaid-layered and flux-layered"
    );
}

#[test]
fn class_svg_honors_edge_routing_override_on_cycle() {
    let input = "classDiagram\nA --> B\nB --> C\nC --> A\n";
    let full = render_class(
        input,
        OutputFormat::Svg,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("mermaid-layered svg should render");
    let orthogonal = render_class(
        input,
        OutputFormat::Svg,
        &RenderConfig {
            layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
            ..RenderConfig::default()
        },
    )
    .expect("flux-layered svg should render");

    assert!(full.starts_with("<svg"));
    assert!(orthogonal.starts_with("<svg"));
    assert_ne!(
        full, orthogonal,
        "class SVG cycle output should differ between mermaid-layered and flux-layered engines"
    );
}

// --- Task 4.4: Class diagram solve-path enablement ---

#[test]
fn class_render_text_through_solve_path() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..Default::default()
    };
    let output =
        render_class("classDiagram\nAnimal <|-- Dog", OutputFormat::Text, &config).unwrap();
    assert!(output.contains("Animal"));
    assert!(output.contains("Dog"));
}

#[test]
fn class_render_mmds_through_solve_path() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        geometry_level: GeometryLevel::Routed,
        ..Default::default()
    };
    let output =
        render_class("classDiagram\nAnimal <|-- Dog", OutputFormat::Mmds, &config).unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(json["edges"].is_array());
}

#[test]
fn class_default_engine_is_flux_layered() {
    let default_out = render_class(
        "classDiagram\nA <|-- B",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap();

    let explicit_config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("flux-layered").unwrap()),
        ..Default::default()
    };
    let explicit_out = render_class(
        "classDiagram\nA <|-- B",
        OutputFormat::Text,
        &explicit_config,
    )
    .unwrap();

    assert_eq!(default_out, explicit_out);
}

#[test]
fn class_mermaid_layered_compatibility() {
    let config = RenderConfig {
        layout_engine: Some(EngineAlgorithmId::parse("mermaid-layered").unwrap()),
        ..Default::default()
    };
    let output = render_class("classDiagram\nA <|-- B", OutputFormat::Text, &config).unwrap();
    assert!(output.contains('A'));
}
