mod common;

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
fn unstable_text_metrics_provider_feature_is_non_default() {
    let package = mmdflux_metadata();
    let feature = package
        .features
        .get("unstable-text-metrics-provider")
        .expect("unstable text metrics provider feature must be declared");
    let default_features = package
        .features
        .get("default")
        .expect("`default` feature set must be declared");

    assert!(
        feature.is_empty(),
        "unstable text metrics provider feature should not enable dependencies yet"
    );
    assert!(
        !default_features
            .iter()
            .any(|feature| feature == "unstable-text-metrics-provider"),
        "`default` must not enable the unstable text metrics provider bridge"
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
    use mmdflux::{RenderConfig, RuntimeConfigInput, SvgThemeConfig, SvgThemeMode};

    // JSON config as Wasm receives it.
    let json = r##"{
        "edgePreset":"smooth-step",
        "svgTheme": {
            "name": "dark",
            "mode": "dynamic",
            "bg": "#101418",
            "accent": "#7dd3fc"
        }
    }"##;
    let input: RuntimeConfigInput = serde_json::from_str(json).unwrap();
    let wasm_config = input.into_render_config().unwrap();

    // Equivalent config built manually as the CLI would.
    let cli_config = RenderConfig {
        edge_preset: Some(EdgePreset::SmoothStep),
        svg_theme: Some(SvgThemeConfig {
            name: Some("dark".to_string()),
            mode: SvgThemeMode::Dynamic,
            bg: Some("#101418".to_string()),
            fg: None,
            line: None,
            accent: Some("#7dd3fc".to_string()),
            muted: None,
            surface: None,
            border: None,
        }),
        ..RenderConfig::default()
    };

    assert_eq!(wasm_config.edge_preset, cli_config.edge_preset);
    assert_eq!(wasm_config.svg_theme, cli_config.svg_theme);
    assert_eq!(wasm_config.layout.node_sep, cli_config.layout.node_sep);
}

#[test]
fn runtime_config_input_parses_nested_svg_theme() {
    use mmdflux::{RuntimeConfigInput, SvgThemeMode};

    let input: RuntimeConfigInput = serde_json::from_str(
        r##"{
            "svgTheme": {
                "name": "dark",
                "mode": "dynamic",
                "bg": "#101418",
                "accent": "#7dd3fc"
            }
        }"##,
    )
    .unwrap();

    let config = input.into_render_config().unwrap();
    let theme = config.svg_theme.expect("svg theme should be present");
    assert_eq!(theme.name.as_deref(), Some("dark"));
    assert_eq!(theme.mode, SvgThemeMode::Dynamic);
    assert_eq!(theme.bg.as_deref(), Some("#101418"));
    assert_eq!(theme.accent.as_deref(), Some("#7dd3fc"));
}

#[test]
fn runtime_config_input_accepts_font_metrics_profile() {
    use mmdflux::RuntimeConfigInput;
    use mmdflux::graph::measure::{
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
    };

    for profile_id in [
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
    ] {
        let input: RuntimeConfigInput =
            serde_json::from_str(&format!(r#"{{"fontMetricsProfile":"{profile_id}"}}"#)).unwrap();
        let config = input.into_render_config().unwrap();

        assert_eq!(config.font_metrics_profile.as_deref(), Some(profile_id));
    }
}

#[test]
fn runtime_config_input_rejects_unsupported_font_metrics_profile() {
    use mmdflux::RuntimeConfigInput;

    let input: RuntimeConfigInput =
        serde_json::from_str(r#"{"fontMetricsProfile":"mermaid-sans-v1"}"#).unwrap();
    let err = input.into_render_config().unwrap_err();

    assert!(
        err.message
            .contains("unsupported text metrics profile 'mermaid-sans-v1'"),
        "{err}"
    );
}

#[test]
fn runtime_config_input_accepts_canonical_graph_font_style() {
    use mmdflux::RuntimeConfigInput;

    let input: RuntimeConfigInput =
        serde_json::from_str(r#"{"fontFamily":"Inter, system-ui","fontSize":14}"#).unwrap();
    let config = input.into_render_config().unwrap();
    let style = config.graph_text_style.expect("graph text style");

    assert_eq!(style.font_family, "Inter, system-ui");
    assert_eq!(style.font_size_px, 14.0);
}

#[test]
fn runtime_config_input_fills_missing_canonical_graph_font_style_defaults() {
    use mmdflux::RuntimeConfigInput;
    use mmdflux::graph::measure::{DEFAULT_GRAPH_FONT_FAMILY, DEFAULT_PROPORTIONAL_FONT_SIZE};

    let input: RuntimeConfigInput = serde_json::from_str(r#"{"fontFamily":"Inter"}"#).unwrap();
    let config = input.into_render_config().unwrap();
    let style = config.graph_text_style.expect("graph text style");
    assert_eq!(style.font_family, "Inter");
    assert_eq!(style.font_size_px, DEFAULT_PROPORTIONAL_FONT_SIZE);

    let input: RuntimeConfigInput = serde_json::from_str(r#"{"fontSize":14}"#).unwrap();
    let config = input.into_render_config().unwrap();
    let style = config.graph_text_style.expect("graph text style");
    assert_eq!(style.font_family, DEFAULT_GRAPH_FONT_FAMILY);
    assert_eq!(style.font_size_px, 14.0);
}

#[test]
fn runtime_config_input_rejects_invalid_canonical_graph_font_style() {
    use mmdflux::RuntimeConfigInput;

    for (json, expected) in [
        (r#"{"fontFamily":"","fontSize":14}"#, "fontFamily"),
        (
            r#"{"fontFamily":"Inter, , Arial","fontSize":14}"#,
            "fontFamily",
        ),
        (r#"{"fontFamily":"Inter","fontSize":0}"#, "fontSize"),
        (r#"{"fontFamily":"Inter","fontSize":-1}"#, "fontSize"),
    ] {
        let input: RuntimeConfigInput = serde_json::from_str(json).unwrap();
        let err = input.into_render_config().unwrap_err();
        assert!(err.message.contains(expected), "{err}");
    }
}

#[test]
fn runtime_config_input_accepts_theme_variables_font_aliases() {
    use mmdflux::RuntimeConfigInput;

    for (json, expected_size) in [
        (
            r#"{"themeVariables":{"fontFamily":"Inter","fontSize":14}}"#,
            14.0,
        ),
        (
            r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"14"}}"#,
            14.0,
        ),
        (
            r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"14px"}}"#,
            14.0,
        ),
        (
            r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"14PX"}}"#,
            14.0,
        ),
        (
            r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"14.5 px"}}"#,
            14.5,
        ),
    ] {
        let input: RuntimeConfigInput = serde_json::from_str(json).unwrap();
        let config = input.into_render_config().unwrap();
        let style = config.graph_text_style.expect("graph text style");
        assert_eq!(style.font_family, "Inter");
        assert_eq!(style.font_size_px, expected_size);
    }
}

#[test]
fn runtime_config_input_rejects_unknown_theme_variables_and_units() {
    use mmdflux::RuntimeConfigInput;

    let unknown = serde_json::from_str::<RuntimeConfigInput>(
        r##"{"themeVariables":{"primaryColor":"#fff"}}"##,
    )
    .unwrap_err();
    assert!(unknown.to_string().contains("unknown field `primaryColor`"));

    let input: RuntimeConfigInput =
        serde_json::from_str(r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"1rem"}}"#)
            .unwrap();
    let err = input.into_render_config().unwrap_err();
    assert!(err.message.contains("themeVariables.fontSize"), "{err}");

    for json in [
        r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"+14px"}}"#,
        r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"1.4e1"}}"#,
        r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"px"}}"#,
    ] {
        let input: RuntimeConfigInput = serde_json::from_str(json).unwrap();
        let err = input.into_render_config().unwrap_err();
        assert!(err.message.contains("themeVariables.fontSize"), "{err}");
    }
}

#[test]
fn runtime_config_input_rejects_conflicting_font_aliases() {
    use mmdflux::RuntimeConfigInput;

    for json in [
        r#"{"fontFamily":"Inter","fontSize":14,"themeVariables":{"fontFamily":"Arial","fontSize":"14px"}}"#,
        r#"{"fontFamily":"Inter","fontSize":14,"themeVariables":{"fontFamily":"Inter","fontSize":"16px"}}"#,
    ] {
        let input: RuntimeConfigInput = serde_json::from_str(json).unwrap();
        let err = input.into_render_config().unwrap_err();
        assert!(err.message.contains("conflicting"), "{err}");
    }
}

#[test]
fn runtime_config_input_accepts_equivalent_font_family_alias_spelling() {
    use mmdflux::RuntimeConfigInput;

    let input: RuntimeConfigInput = serde_json::from_str(
        r#"{"fontFamily":"\"Trebuchet MS\", Verdana, Arial, sans-serif","fontSize":16,"themeVariables":{"fontFamily":"trebuchet ms,verdana,arial,sans-serif","fontSize":"16px"}}"#,
    )
    .unwrap();

    input.into_render_config().unwrap();
}

#[cfg(feature = "unstable-text-metrics-provider")]
#[test]
fn dynamic_metrics_input_rejects_unknown_fields() {
    use mmdflux::dynamic_text_metrics::DynamicMetricsInput;

    let err = serde_json::from_str::<DynamicMetricsInput>(
        r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}],"extra":true}"#,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("unknown field"), "{err}");
    assert!(err.contains("extra"), "{err}");
}

#[test]
fn dynamic_text_metrics_bridge_is_feature_gated_and_doc_hidden() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let cargo_toml = std::fs::read_to_string(format!("{manifest_dir}/Cargo.toml")).unwrap();
    let lib_source = std::fs::read_to_string(format!("{manifest_dir}/src/lib.rs")).unwrap();
    let runtime_source =
        std::fs::read_to_string(format!("{manifest_dir}/src/runtime/mod.rs")).unwrap_or_default();
    let measure_source =
        std::fs::read_to_string(format!("{manifest_dir}/src/graph/measure.rs")).unwrap();

    assert!(
        cargo_toml.contains("unstable-text-metrics-provider"),
        "root Cargo.toml should declare the non-default unstable bridge feature"
    );
    assert!(
        lib_source.contains("unstable-text-metrics-provider")
            || runtime_source.contains("unstable-text-metrics-provider"),
        "dynamic text metrics bridge should be gated by the unstable feature"
    );
    assert!(
        lib_source.contains("#[doc(hidden)]") || runtime_source.contains("#[doc(hidden)]"),
        "dynamic text metrics bridge should stay doc-hidden"
    );
    assert!(
        measure_source.contains("pub(crate) trait TextMetricsProvider"),
        "TextMetricsProvider must stay crate-private"
    );
    assert!(
        !measure_source.contains("pub trait TextMetricsProvider"),
        "TextMetricsProvider must not become normal public API"
    );
}

#[test]
fn runtime_config_input_rejects_cli_only_svg_theme_auto_field() {
    use mmdflux::RuntimeConfigInput;

    let err =
        serde_json::from_str::<RuntimeConfigInput>(r#"{"svgThemeAuto":"light:default,dark:dark"}"#)
            .unwrap_err();
    assert!(err.to_string().contains("unknown field"));
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
    // published Wasm contract (web.rs line 92).
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
