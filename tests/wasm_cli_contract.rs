use std::collections::HashMap;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    dependencies: Vec<CargoDependency>,
    features: HashMap<String, Vec<String>>,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoDependency {
    name: String,
    optional: bool,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    #[serde(rename = "required-features", default)]
    required_features: Vec<String>,
}

fn mmdflux_metadata() -> CargoPackage {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run `cargo metadata`");

    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: CargoMetadata =
        serde_json::from_slice(&output.stdout).expect("failed to parse `cargo metadata` JSON");

    metadata
        .packages
        .into_iter()
        .find(|package| package.name == "mmdflux")
        .expect("mmdflux package missing from `cargo metadata` output")
}

#[test]
fn clap_dependency_is_optional() {
    let package = mmdflux_metadata();
    let clap = package
        .dependencies
        .iter()
        .find(|dependency| dependency.name == "clap")
        .expect("clap dependency not found");

    assert!(
        clap.optional,
        "clap must be optional so non-CLI consumers can disable it"
    );
}

#[test]
fn cli_feature_enables_clap_dependency() {
    let package = mmdflux_metadata();
    let cli_feature = package
        .features
        .get("cli")
        .expect("`cli` feature must be declared");
    let default_features = package
        .features
        .get("default")
        .expect("`default` feature set must be declared");

    assert!(
        cli_feature.iter().any(|feature| feature == "dep:clap"),
        "`cli` must include `dep:clap`"
    );
    assert!(
        default_features.iter().any(|feature| feature == "cli"),
        "`default` must include `cli` to preserve existing CLI behavior"
    );
}

#[test]
fn mmdflux_binary_requires_cli_feature() {
    let package = mmdflux_metadata();
    let mmdflux_bin = package
        .targets
        .iter()
        .find(|target| target.name == "mmdflux" && target.kind.iter().any(|kind| kind == "bin"))
        .expect("mmdflux binary target missing");

    assert!(
        mmdflux_bin
            .required_features
            .iter()
            .any(|feature| feature == "cli"),
        "mmdflux binary must require `cli` feature"
    );
}

// ── Facade contract tests ──────────────────────────────────────────

#[test]
fn facade_render_returns_expected_text() {
    use mmdflux::{OutputFormat, RenderConfig, render_diagram};

    let input = "graph TD\nA-->B";
    let config = RenderConfig::default();

    let facade_result = render_diagram(input, OutputFormat::Text, &config).unwrap();
    assert!(facade_result.contains('A'));
    assert!(facade_result.contains('B'));
}

#[test]
fn facade_detect_matches_registry() {
    use mmdflux::detect_diagram;

    assert_eq!(detect_diagram("graph TD\nA-->B"), Some("flowchart"));
    assert_eq!(detect_diagram("not a diagram"), None);
}

#[test]
fn cli_and_wasm_use_the_same_render_config_contract() {
    use mmdflux::format::EdgePreset;
    use mmdflux::{RenderConfig, RuntimeConfigInput};

    // JSON config as WASM receives it.
    let json = r#"{"edgePreset":"smooth-step"}"#;
    let input: RuntimeConfigInput = serde_json::from_str(json).unwrap();
    let wasm_config = input.into_render_config().unwrap();

    // Equivalent config built manually as the CLI would.
    let cli_config = RenderConfig {
        edge_preset: Some(EdgePreset::SmoothStep),
        ..RenderConfig::default()
    };

    assert_eq!(wasm_config.edge_preset, cli_config.edge_preset);
    assert_eq!(wasm_config.layout.node_sep, cli_config.layout.node_sep);
}

#[test]
fn wasm_adapter_contains_no_private_config_mirror_types() {
    let source = std::fs::read_to_string("crates/mmdflux-wasm/src/lib.rs").unwrap();
    assert!(
        !source.contains("struct WasmRenderConfig"),
        "wasm crate must not define WasmRenderConfig locally"
    );
}

#[test]
fn config_input_rejects_legacy_fields() {
    use mmdflux::RuntimeConfigInput;

    let err = serde_json::from_str::<RuntimeConfigInput>(r#"{"interpolationStyle":"linear"}"#)
        .unwrap_err();
    assert!(err.to_string().contains("unknown field"));

    let err =
        serde_json::from_str::<RuntimeConfigInput>(r#"{"cornerStyle":"rounded"}"#).unwrap_err();
    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn facade_validate_returns_valid_json() {
    use mmdflux::validate_diagram;

    let result = validate_diagram("graph TD\nA-->B");
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["valid"], true);
}

#[test]
fn facade_validate_returns_diagnostics_for_invalid_input() {
    use mmdflux::validate_diagram;

    let result = validate_diagram("graph TD\n!!!");
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["valid"], false);
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn facade_render_unknown_diagram_uses_lowercase_error() {
    use mmdflux::{OutputFormat, RenderConfig, render_diagram};

    let err = render_diagram(
        "not a diagram",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap_err();

    // The facade must use lowercase "unknown diagram type" to match the
    // published WASM contract (web.rs line 92).
    assert!(
        err.message.contains("unknown diagram type"),
        "facade error must use lowercase: got {:?}",
        err.message
    );
}

#[test]
fn facade_render_parse_failure_uses_lowercase_prefix() {
    use mmdflux::{OutputFormat, RenderConfig, render_diagram};

    let err = render_diagram(
        "graph TD\n!!!",
        OutputFormat::Text,
        &RenderConfig::default(),
    )
    .unwrap_err();

    assert!(
        err.message.starts_with("parse error:"),
        "facade parse error must use lowercase prefix: got {:?}",
        err.message
    );
}
