use std::collections::BTreeSet;
use std::path::Path;

use mmdflux::errors::ParseDiagnostic;
use mmdflux::format::{CornerStyle, Curve, EdgePreset, RoutingStyle};
use mmdflux::graph::style::{ColorToken, NodeStyle};
use mmdflux::graph::{Direction, Edge, GeometryLevel, Graph, Node, Shape};
use mmdflux::payload::Diagram as Payload;
use mmdflux::registry::{DiagramFamily, DiagramInstance, ParsedDiagram};
use mmdflux::simplification::PathSimplification;
use mmdflux::{
    ColorWhen, EngineAlgorithmId, EngineId, OutputFormat, RenderConfig, RenderError,
    RuntimeConfigInput, TextColorMode,
};

fn lib_rs_source() -> String {
    repo_file("src/lib.rs")
}

fn repo_file(relative_path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    std::fs::read_to_string(&path).unwrap()
}

fn baseline_manifest_modules() -> BTreeSet<String> {
    let manifest: serde_json::Value =
        serde_json::from_str(&repo_file("tests/baselines/manifest.json")).unwrap();
    manifest["rust_exports"]["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect()
}

fn public_exports_for_test() -> BTreeSet<String> {
    let content = lib_rs_source();
    let mut exports = BTreeSet::new();

    let joined = content.replace('\n', " ");
    for segment in joined.split("pub use ").skip(1) {
        let Some(stmt) = segment.split(';').next() else {
            continue;
        };
        let stmt = stmt.trim();

        if let Some(brace_start) = stmt.find('{') {
            let brace_end = stmt.find('}').unwrap_or(stmt.len());
            let symbols = &stmt[brace_start + 1..brace_end];
            for sym in symbols.split(',') {
                let sym = sym.trim();
                if !sym.is_empty() {
                    exports.insert(sym.to_string());
                }
            }
        } else if let Some(colon_pos) = stmt.rfind("::") {
            exports.insert(stmt[colon_pos + 2..].trim().to_string());
        }
    }

    exports
}

fn public_modules_for_test() -> BTreeSet<String> {
    let content = lib_rs_source();
    let mut modules = BTreeSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(module) = trimmed
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        {
            modules.insert(module.to_string());
        }
    }

    modules
}

fn assert_exports_include(exports: &BTreeSet<String>, required: &[&str]) {
    for name in required {
        assert!(
            exports.contains(*name),
            "{name} should remain in the crate-root export surface"
        );
    }
}

fn assert_exports_exclude(exports: &BTreeSet<String>, forbidden: &[&str], context: &str) {
    for name in forbidden {
        assert!(
            !exports.contains(*name),
            "{name} should stay out of {context}"
        );
    }
}

#[test]
fn crate_root_only_exports_supported_public_modules() {
    let modules = public_modules_for_test();

    for required in [
        "builtins",
        "config",
        "errors",
        "format",
        "graph",
        "mmds",
        "payload",
        "registry",
        "simplification",
        "timeline",
    ] {
        assert!(
            modules.contains(required),
            "{required} should remain public"
        );
    }

    for forbidden in [
        "diagrams",
        "engines",
        "frontends",
        "lint",
        "mermaid",
        "render",
        "runtime",
    ] {
        assert!(
            !modules.contains(forbidden),
            "{forbidden} should no longer be a public crate-root module"
        );
    }
}

#[test]
fn baseline_manifest_locks_supported_v2_public_modules() {
    let modules = baseline_manifest_modules();

    for required in [
        "builtins",
        "config",
        "errors",
        "format",
        "graph",
        "mmds",
        "payload",
        "registry",
        "simplification",
        "timeline",
    ] {
        assert!(
            modules.contains(required),
            "{required} should stay in the supported v2 public manifest lock"
        );
    }

    for forbidden in [
        "registry_builtins",
        "diagrams",
        "engines",
        "frontends",
        "lint",
        "mermaid",
        "render",
        "runtime",
    ] {
        assert!(
            !modules.contains(forbidden),
            "{forbidden} should stay out of the supported v2 public manifest lock"
        );
    }
}

#[test]
fn crate_root_reexports_curated_runtime_and_value_types() {
    let exports = public_exports_for_test();

    assert_exports_include(
        &exports,
        &[
            "RenderConfig",
            "RenderError",
            "OutputFormat",
            // Types from private modules — must stay re-exported.
            "AlgorithmId",
            "EngineAlgorithmId",
            "EngineId",
            "ColorWhen",
            "TextColorMode",
            "RuntimeConfigInput",
            "apply_svg_surface_defaults",
            // Runtime facade functions.
            "detect_diagram",
            "render_diagram",
            "validate_diagram",
        ],
    );

    // Types that moved to their home modules should NOT appear at the crate root.
    assert_exports_exclude(
        &exports,
        &[
            "ParseDiagnostic",
            "DiagramFamily",
            "CornerStyle",
            "Curve",
            "EdgePreset",
            "RoutingStyle",
            "Direction",
            "Edge",
            "GeometryLevel",
            "Node",
            "Shape",
            "MmdsGenerationError",
            "generate_mermaid_from_mmds",
            "generate_mermaid_from_mmds_str",
            "PathSimplification",
            "ColorToken",
            "NodeStyle",
        ],
        "the crate-root export surface (types moved to home modules)",
    );
}

#[test]
fn crate_root_does_not_reexport_internal_modules_or_registry_constructor() {
    let exports = public_exports_for_test();

    assert_exports_exclude(
        &exports,
        &[
            "default_registry",
            "parse_flowchart",
            "detect_diagram_type",
            "compile_to_graph",
            "to_mmds_json",
            "to_mmds_layout",
            "hydrate_graph_geometry_from_mmds",
            "hydrate_routed_geometry_from_mmds",
        ],
        "the crate-root export surface",
    );
}

#[test]
fn supported_root_exports_compile() {
    let _ = OutputFormat::default();
    let _ = RenderConfig::default();
    let _ = RenderError::from("surface");
    let _ = Graph::new(Direction::TopDown);
    let _ = Edge::new("A", "B");
    let _ = Node::new("A").with_shape(Shape::Rectangle);
    let _ = DiagramFamily::Graph;
    let _ = GeometryLevel::Layout;
    let _ = CornerStyle::Sharp;
    let _ = Curve::Basis;
    let _ = EdgePreset::Straight;
    let _ = RoutingStyle::Direct;
    let _ = EngineId::Flux;
    let _ = EngineAlgorithmId::parse("flux-layered").unwrap();
    let _ = ParseDiagnostic::warning(None, None, String::new());
    let _ = PathSimplification::default();
    let _ = ColorWhen::Auto;
    let _ = TextColorMode::Plain;
    let _ = ColorToken::parse("#fff").unwrap();
    let _ = NodeStyle::default();
    let _ = RuntimeConfigInput::default();
    let _ = std::any::type_name::<Box<dyn DiagramInstance>>();
    let _ = std::any::type_name::<Box<dyn ParsedDiagram>>();
}

#[test]
fn registry_api_works() {
    let registry = mmdflux::builtins::default_registry();
    let input = "graph TD\n    A-->B";

    let diagram_id = registry.detect(input).unwrap();
    assert_eq!(diagram_id, "flowchart");

    let instance = registry.create(diagram_id).unwrap();
    let payload = instance
        .parse(input)
        .unwrap()
        .into_payload(&RenderConfig::default())
        .unwrap();
    assert!(matches!(payload, Payload::Flowchart(_)));
}

#[test]
fn builtin_registry_module_is_public_and_registry_default_registry_is_gone() {
    let _ = mmdflux::builtins::default_registry();

    let registry_source = repo_file("src/registry.rs");
    assert!(
        !registry_source.contains("pub fn default_registry("),
        "src/registry.rs should stay contract-only"
    );
}

#[test]
fn mmds_module_keeps_supported_adapter_helpers_public() {
    let _ = std::any::type_name::<mmdflux::mmds::MmdsOutput>();
    let _ = std::any::type_name::<mmdflux::mmds::MmdsHydrationError>();
    let _ = std::any::type_name::<mmdflux::mmds::MmdsGenerationError>();

    let _parse_with_profiles: fn(
        &str,
    ) -> Result<
        (
            mmdflux::mmds::MmdsOutput,
            mmdflux::mmds::MmdsProfileNegotiation,
        ),
        mmdflux::mmds::MmdsParseError,
    > = mmdflux::mmds::parse_with_profiles;
    let _validate_input: fn(&str) -> Result<(), mmdflux::RenderError> =
        mmdflux::mmds::validate_input;
    let _from_mmds_str: fn(
        &str,
    )
        -> Result<mmdflux::graph::Graph, mmdflux::mmds::MmdsHydrationError> =
        mmdflux::mmds::from_mmds_str;
    let _render_input: fn(
        &str,
        mmdflux::OutputFormat,
        &mmdflux::RenderConfig,
    ) -> Result<String, mmdflux::RenderError> = mmdflux::mmds::render_input;
    let _render_output: fn(
        &mmdflux::mmds::MmdsOutput,
        mmdflux::OutputFormat,
        &mmdflux::RenderConfig,
    ) -> Result<String, mmdflux::RenderError> = mmdflux::mmds::render_output;
    let _generate_mermaid_from_mmds_str: fn(
        &str,
    )
        -> Result<String, mmdflux::mmds::MmdsGenerationError> =
        mmdflux::mmds::generate_mermaid_from_mmds_str;
}

#[test]
fn mmds_module_hides_geometry_coupled_helpers() {
    let source = repo_file("src/mmds/mod.rs");

    // These geometry-coupled helpers must not appear on the public surface.
    // The single runtime helper (to_mmds_json_typed_with_routing) may appear
    // as a pub(crate) re-export but not as a pub re-export.
    for forbidden in [
        "to_mmds_layout",
        "to_mmds_layout_typed",
        "to_mmds_routed",
        "to_mmds_routed_typed",
        "hydrate_graph_geometry_from_mmds",
        "hydrate_graph_geometry_from_output",
        "hydrate_graph_geometry_from_output_with_diagram",
        "hydrate_routed_geometry_from_mmds",
        "hydrate_routed_geometry_from_output",
    ] {
        assert!(
            !source.contains(forbidden),
            "{forbidden} should no longer be part of the public mmds surface"
        );
    }

    // to_mmds_json helpers may only appear as pub(crate), never pub.
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("to_mmds_json")
            && !trimmed.starts_with("pub(crate)")
            && !trimmed.starts_with("//")
        {
            panic!("to_mmds_json* helpers must be pub(crate) only, found: {trimmed}");
        }
    }
}
