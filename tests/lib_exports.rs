use std::collections::BTreeSet;
use std::path::Path;

use mmdflux::errors::ParseDiagnostic;
use mmdflux::format::{CornerStyle, Curve, EdgePreset, RoutingStyle};
use mmdflux::graph::style::{ColorToken, NodeStyle};
use mmdflux::graph::{Arrow, Direction, Edge, GeometryLevel, Graph, Node, Shape, Stroke};
use mmdflux::mmds::{MmdsToken, MmdsTokenError};
use mmdflux::payload::Diagram as Payload;
use mmdflux::registry::{DiagramFamily, DiagramInstance, ParsedDiagram};
use mmdflux::simplification::PathSimplification;
use mmdflux::{
    ColorWhen, EngineAlgorithmId, EngineId, OutputFormat, RenderConfig, RenderError,
    RuntimeConfigInput, SvgThemeConfig, SvgThemeMode, TextColorMode,
};

fn lib_rs_source() -> String {
    repo_file("src/lib.rs")
}

fn repo_file(relative_path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    std::fs::read_to_string(&path).unwrap()
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

fn lines_before(source: &str, needle: &str, count: usize) -> String {
    let index = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} should exist"));
    let mut lines: Vec<_> = source[..index].lines().rev().take(count).collect();
    lines.reverse();
    lines.join("\n")
}

fn assert_hidden_deprecated_item(source: &str, needle: &str, replacement: &str) {
    let attrs = lines_before(source, needle, 8);
    assert!(
        attrs.contains("#[doc(hidden)]"),
        "{needle} should be hidden from public docs"
    );
    assert!(
        attrs.contains("#[deprecated("),
        "{needle} should be deprecated"
    );
    assert!(
        attrs.contains(replacement),
        "{needle} should name replacement {replacement:?}"
    );
}

fn assert_non_exhaustive(source: &str, needle: &str) {
    let attrs = lines_before(source, needle, 8);
    assert!(
        attrs.contains("#[non_exhaustive]"),
        "{needle} should be #[non_exhaustive]"
    );
}

#[test]
fn crate_root_only_exports_supported_public_modules() {
    let modules = public_modules_for_test();

    for required in [
        "builtins",
        "commands",
        "errors",
        "format",
        "graph",
        "mmds",
        "payload",
        "registry",
        "simplification",
        "timeline",
        "views",
    ] {
        assert!(
            modules.contains(required),
            "{required} should remain public"
        );
    }

    for forbidden in [
        "config",
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
fn views_module_is_public_low_level_api() {
    let modules = public_modules_for_test();

    assert!(
        modules.contains("views"),
        "views should be a supported low-level public module"
    );
}

#[test]
fn view_contract_types_compile() {
    use mmdflux::views::{
        AnchorRef, BoundaryPolicy, CompoundPolicy, EdgeAnchor, LayoutMode, NodePredicate, Selector,
        TraversalDirection, ViewError, ViewEvent, ViewSpec, ViewStatement, project,
    };

    let spec = ViewSpec::default();
    let statement = ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 2,
    });
    let anchor = AnchorRef::Node("gateway".to_string());
    let edge_anchor_type = std::any::type_name::<EdgeAnchor>();
    let stub_policy = BoundaryPolicy::Stub {
        aggregate_threshold: 4,
    };
    let tag_predicate = NodePredicate::Tag("database".to_string());

    let _ = (
        spec,
        statement,
        anchor,
        edge_anchor_type,
        stub_policy,
        tag_predicate,
        LayoutMode::SharedCoordinates,
        CompoundPolicy::Preserve,
        ViewEvent::NodeLeftView {
            id: "internal".to_string(),
            reason: mmdflux::views::ElisionReason::Excluded,
        },
        ViewError::NotImplementedYet {
            feature: "edge anchors".to_string(),
        },
        project,
    );
}

#[test]
fn mmds_public_api_surface_is_explicit_before_2_4_0() {
    let _ = mmdflux::render_diagram;
    let _ = mmdflux::materialize_diagram;
    let _ = mmdflux::render_document;
    let _ = mmdflux::render_document_with_relayout;
    let _ = mmdflux::mmds::diff::union::union_document;
    let _ = std::any::type_name::<mmdflux::mmds::diff::union::UnionOptions>();
    let _ = mmdflux::detect_diagram;
    let _ = mmdflux::validate_diagram;

    let _ = mmdflux::commands::apply;
    let _ = mmdflux::commands::apply_with_config;
    let _ = std::any::type_name::<mmdflux::commands::Command>();
    let _ = std::any::type_name::<mmdflux::commands::EdgeSelector>();
    let _ = std::any::type_name::<mmdflux::commands::CommandApplyError>();

    let _ = std::any::type_name::<mmdflux::mmds::events::ModelEvent>();
    let _ = std::any::type_name::<mmdflux::mmds::events::ModelEventKind>();
    let _ = mmdflux::mmds::diff::diff_documents;
    let _ = std::any::type_name::<mmdflux::mmds::diff::Diff>();
    let _ = std::any::type_name::<mmdflux::mmds::diff::Change>();
    let _ = std::any::type_name::<mmdflux::mmds::diff::ChangeKind>();
    let _ = std::any::type_name::<mmdflux::mmds::Document>();
    let _ = std::any::type_name::<mmdflux::mmds::Subject>();
    let _ = std::any::type_name::<mmdflux::mmds::MmdsTokenError>();
    let _parse_shape: fn(&str) -> Result<Shape, MmdsTokenError> = <Shape as MmdsToken>::parse_mmds;

    let _ = mmdflux::views::project;
    let _ = std::any::type_name::<mmdflux::views::ViewSpec>();
    let _ = std::any::type_name::<mmdflux::views::ViewStatement>();
    let _ = std::any::type_name::<mmdflux::views::Selector>();
    let _ = std::any::type_name::<mmdflux::views::ViewEvent>();
    let _ = std::any::type_name::<mmdflux::views::ViewError>();

    let _render_document: fn(
        &mmdflux::mmds::Document,
        mmdflux::OutputFormat,
        &mmdflux::RenderConfig,
    ) -> Result<String, mmdflux::RenderError> = mmdflux::render_document;
}

#[test]
fn early_mmds_surface_is_non_exhaustive() {
    let commands = repo_file("src/commands.rs");
    assert_non_exhaustive(&commands, "pub enum Command");
    assert_non_exhaustive(&commands, "pub enum EdgeSelector");
    assert_non_exhaustive(&commands, "pub enum CommandApplyError");

    let events = repo_file("src/mmds/events.rs");
    assert_non_exhaustive(&events, "pub struct ModelEvent");
    assert_non_exhaustive(&events, "pub enum ModelEventKind");

    let diff = repo_file("src/mmds/diff/mod.rs");
    assert_non_exhaustive(&diff, "pub struct Diff");
    assert_non_exhaustive(&diff, "pub struct Change");
    assert_non_exhaustive(&diff, "pub enum ChangeKind");
    assert_non_exhaustive(&diff, "pub struct EdgeIds");

    let wire = repo_file("src/mmds/diff/wire.rs");
    assert_non_exhaustive(&wire, "pub enum WireLayer");
    assert_non_exhaustive(&wire, "pub struct WireOptions");

    let union = repo_file("src/mmds/diff/union.rs");
    assert_non_exhaustive(&union, "pub struct UnionOptions");

    let mmds = repo_file("src/mmds/mod.rs");
    assert_non_exhaustive(&mmds, "pub enum Subject");

    let token = repo_file("src/mmds/token.rs");
    assert_non_exhaustive(&token, "pub struct MmdsTokenError");

    let view_spec = repo_file("src/views/spec.rs");
    assert_non_exhaustive(&view_spec, "pub struct ViewSpec");
    assert_non_exhaustive(&view_spec, "pub enum ViewStatement");
    assert_non_exhaustive(&view_spec, "pub enum Selector");
    assert_non_exhaustive(&view_spec, "pub enum AnchorRef");
    assert_non_exhaustive(&view_spec, "pub struct EdgeAnchor");
    assert_non_exhaustive(&view_spec, "pub enum NodePredicate");
    assert_non_exhaustive(&view_spec, "pub enum LayoutMode");
    assert_non_exhaustive(&view_spec, "pub enum BoundaryPolicy");
    assert_non_exhaustive(&view_spec, "pub enum CompoundPolicy");

    let view_events = repo_file("src/views/events.rs");
    assert_non_exhaustive(&view_events, "pub enum ElisionReason");
    assert_non_exhaustive(&view_events, "pub enum ViewEvent");

    let view_error = repo_file("src/views/error.rs");
    assert_non_exhaustive(&view_error, "pub enum ViewError");
}

#[test]
fn crate_root_does_not_flatten_mmds_command_diff_event_or_view_types() {
    let exports = public_exports_for_test();

    assert_exports_exclude(
        &exports,
        &[
            "Command",
            "EdgeSelector",
            "CommandApplyError",
            "ModelEvent",
            "ModelEventKind",
            "Diff",
            "Change",
            "ChangeKind",
            "Subject",
            "MmdsToken",
            "MmdsTokenError",
            "ViewSpec",
            "ViewStatement",
            "Selector",
            "ViewEvent",
            "ViewError",
        ],
        "the crate-root export surface (MMDS commands, events, diffs, tokens, and views stay in home modules)",
    );
}

#[test]
fn deprecated_mmds_public_helpers_have_explicit_release_policy() {
    let mmds_mod = repo_file("src/mmds/mod.rs");
    let document = repo_file("src/mmds/document.rs");
    let hydrate = repo_file("src/mmds/hydrate.rs");

    let deprecated_count = [&mmds_mod, &document, &hydrate]
        .iter()
        .map(|source| source.matches("#[deprecated(").count())
        .sum::<usize>();
    assert_eq!(
        deprecated_count, 6,
        "the deprecated MMDS compatibility inventory should stay explicit"
    );

    assert_hidden_deprecated_item(&document, "pub type Output = Document;", "mmds::Document");
    assert_hidden_deprecated_item(
        &document,
        "pub fn to_json_typed_with_routing(",
        "materialize_diagram plus serde_json serialization",
    );
    assert_hidden_deprecated_item(
        &mmds_mod,
        "pub fn evaluate_profiles_for_output(",
        "evaluate_profiles_for_document",
    );
    assert_hidden_deprecated_item(&hydrate, "pub fn from_output(", "from_document");
    assert_hidden_deprecated_item(
        &hydrate,
        "pub fn hydrate_graph_geometry_from_output_with_diagram(",
        "hydrate_graph_geometry_from_document_with_diagram",
    );
    assert_hidden_deprecated_item(
        &hydrate,
        "pub fn hydrate_routed_geometry_from_output(",
        "hydrate_routed_geometry_from_document",
    );
}

#[test]
fn crate_root_rustdoc_names_public_workflows_without_unreleased_migration_guide() {
    let source = lib_rs_source();

    for required in [
        "Supported diagram types: **flowchart**, **class**, **state**, and **sequence**.",
        "# Stability",
        "render_diagram",
        "materialize_diagram",
        "render_document",
        "commands::apply",
        "mmds::diff::diff_documents",
        "views::project",
        "## Commands and Model Events",
        "## Views",
        "## Snapshot Diffs",
    ] {
        assert!(
            source.contains(required),
            "{required} should be named in crate docs"
        );
    }

    assert!(
        !source.contains("migration guide for views")
            && !source.contains("migration guide for commands")
            && !source.contains("migration guide for events"),
        "unreleased APIs should not be documented as migration-guide changes"
    );
    assert!(
        !source.contains("## Commands, Diffs, and Views")
            && !source.contains("## Commands and Views"),
        "command/event, view, and snapshot diff examples should stay separated"
    );
}

#[test]
fn public_module_rustdocs_name_contract_caveats() {
    let commands = repo_file("src/commands.rs");
    let diff = repo_file("src/mmds/diff/mod.rs");
    let events = repo_file("src/mmds/events.rs");
    let mmds = repo_file("src/mmds/mod.rs");
    let token = repo_file("src/mmds/token.rs");
    let views = repo_file("src/views/mod.rs");

    for source in [&commands, &diff, &events, &mmds, &token, &views] {
        assert!(
            source.contains("[Stability](crate#stability)"),
            "early public modules should link to the crate-level stability policy"
        );
    }

    assert!(commands.contains("model events"));
    assert!(commands.contains("not snapshot diff"));
    assert!(diff.contains("snapshot diff"));
    assert!(events.contains("not snapshot diffs"));
    assert!(token.contains("MMDS string token"));
    assert!(views.contains("projection"));
}

#[test]
fn readme_and_docs_list_public_rust_api_examples() {
    let readme = repo_file("README.md");
    let mmds_docs = repo_file("docs/mmds.md");

    for example in [
        "commands_events_views",
        "snapshot_diff",
        "materialized_view",
        "mmds_replay",
    ] {
        assert!(
            readme.contains(example),
            "{example} should be discoverable from README"
        );
        assert!(
            mmds_docs.contains(example),
            "{example} should be discoverable from docs/mmds.md"
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
            "SvgThemeConfig",
            "SvgThemeMode",
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
            "ViewSpec",
            "ViewStatement",
            "Selector",
            "AnchorRef",
            "LayoutMode",
            "BoundaryPolicy",
            "CompoundPolicy",
            "ViewEvent",
            "ViewError",
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
    let _ = Direction::parse_mmds("TD").unwrap();
    let _ = Edge::new("A", "B");
    let _ = Node::new("A").with_shape(Shape::Rectangle);
    let _ = Shape::parse_mmds("rectangle").unwrap();
    let _ = Stroke::parse_mmds("solid").unwrap();
    let _ = Arrow::parse_mmds("normal").unwrap();
    let _ = MmdsTokenError::new("shape", "invalid");
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
    let _ = SvgThemeMode::default();
    let _ = SvgThemeConfig::default();
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
    let payload = instance.parse(input).unwrap().into_payload().unwrap();
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
    let _ = std::any::type_name::<mmdflux::mmds::Document>();
    let _ = std::any::type_name::<mmdflux::mmds::HydrationError>();
    let _ = std::any::type_name::<mmdflux::mmds::GenerationError>();
    let _ = std::any::type_name::<mmdflux::mmds::diff::Diff>();
    let _ = std::any::type_name::<mmdflux::mmds::diff::Change>();
    let _ = std::any::type_name::<mmdflux::mmds::diff::ChangeKind>();
    let _ = std::any::type_name::<mmdflux::mmds::events::ModelEvent>();
    let _ = std::any::type_name::<mmdflux::mmds::events::ModelEventKind>();
    let _ = std::any::type_name::<mmdflux::mmds::MmdsTokenError>();
    let _ = std::any::type_name::<mmdflux::mmds::Subject>();
    let _ = std::any::type_name::<mmdflux::commands::Command>();
    let _ = std::any::type_name::<mmdflux::commands::EdgeSelector>();
    let _ = std::any::type_name::<mmdflux::commands::CommandApplyError>();

    let _parse_with_profiles: fn(
        &str,
    ) -> Result<
        (mmdflux::mmds::Document, mmdflux::mmds::ProfileNegotiation),
        mmdflux::mmds::ParseError,
    > = mmdflux::mmds::parse_with_profiles;
    let _validate_input: fn(&str) -> Result<(), mmdflux::RenderError> =
        mmdflux::mmds::validate_input;
    let _from_mmds_str: fn(&str) -> Result<mmdflux::graph::Graph, mmdflux::mmds::HydrationError> =
        mmdflux::mmds::from_str;
    let _from_mmds_document: fn(
        &mmdflux::mmds::Document,
    )
        -> Result<mmdflux::graph::Graph, mmdflux::mmds::HydrationError> =
        mmdflux::mmds::from_document;
    let _generate_mermaid_from_mmds_str: fn(
        &str,
    ) -> Result<String, mmdflux::mmds::GenerationError> = mmdflux::mmds::generate_mermaid_from_str;
    let _materialize_diagram: fn(
        &str,
        &mmdflux::RenderConfig,
    ) -> Result<mmdflux::mmds::Document, mmdflux::RenderError> = mmdflux::materialize_diagram;
    let _render_document: fn(
        &mmdflux::mmds::Document,
        mmdflux::OutputFormat,
        &mmdflux::RenderConfig,
    ) -> Result<String, mmdflux::RenderError> = mmdflux::render_document;
    let _diff_documents: fn(
        &mmdflux::mmds::Document,
        &mmdflux::mmds::Document,
    ) -> mmdflux::mmds::diff::Diff = mmdflux::mmds::diff::diff_documents;
    assert!(mmdflux::mmds::diff::ChangeKind::NodeMoved.is_geometry());
    assert!(!mmdflux::mmds::diff::ChangeKind::NodeMoved.is_model());
    let _apply: fn(
        &mmdflux::commands::Command,
        &mut mmdflux::mmds::Document,
    ) -> Result<
        Vec<mmdflux::mmds::events::ModelEvent>,
        mmdflux::commands::CommandApplyError,
    > = mmdflux::commands::apply;
}

#[test]
fn mmds_module_hides_geometry_coupled_helpers() {
    let source = repo_file("src/mmds/mod.rs");
    let document_source = repo_file("src/mmds/document.rs");

    // These geometry-coupled helpers must not appear on the public surface.
    // The single runtime helper (to_mmds_json_typed_with_routing) may appear
    // as a pub(crate) re-export but not as a pub re-export.
    for forbidden in [
        "to_mmds_layout",
        "to_mmds_layout_typed",
        "to_mmds_routed",
        "to_mmds_routed_typed",
        "hydrate_graph_geometry_from_mmds",
        "hydrate_routed_geometry_from_mmds",
    ] {
        assert!(
            !source.contains(forbidden),
            "{forbidden} should no longer be part of the public mmds surface"
        );
    }

    // to_mmds_json helpers should be pub or pub(crate) re-exports, never inlined.
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("to_mmds_json")
            && !trimmed.starts_with("pub use")
            && !trimmed.starts_with("pub(crate)")
            && !trimmed.starts_with("//")
        {
            panic!("to_mmds_json* helpers must be re-exports only, found: {trimmed}");
        }
    }

    let reexport_lines: Vec<_> = source.lines().map(str::trim).collect();
    let reexport_index = reexport_lines
        .iter()
        .position(|line| *line == "pub use document::to_json_typed_with_routing;")
        .expect("legacy typed JSON routing helper should remain re-exported for compatibility");
    let reexport_attrs = &reexport_lines[reexport_index.saturating_sub(3)..reexport_index];

    assert!(
        reexport_attrs.contains(&"#[doc(hidden)]"),
        "legacy typed JSON routing helper should stay hidden on the mmds public surface"
    );
    assert!(
        reexport_attrs.contains(&"#[allow(deprecated)]"),
        "legacy typed JSON routing helper re-export should suppress its own deprecation warning"
    );
    assert!(
        document_source.contains("#[deprecated(")
            && document_source.contains("pub fn to_json_typed_with_routing("),
        "legacy typed JSON routing helper should remain deprecated"
    );
}
