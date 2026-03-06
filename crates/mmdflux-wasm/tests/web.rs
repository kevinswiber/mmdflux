use mmdflux_wasm::{detect, render, version};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn error_debug(error: wasm_bindgen::JsError) -> String {
    format!("{error:?}")
}

fn strip_ansi(input: &str) -> String {
    let mut stripped = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }

        stripped.push(ch);
    }

    stripped
}

#[wasm_bindgen_test]
fn renders_flowchart_text() {
    let output = render("graph TD\nA-->B", "text", "{}").expect("render should succeed");
    assert!(output.contains("A"));
    assert!(output.contains("B"));
}

#[wasm_bindgen_test]
fn renders_flowchart_text_with_color_policy_config() {
    let input = include_str!("../../../tests/fixtures/flowchart/style-basic.mmd");
    let plain = render(input, "text", r#"{"color":"off"}"#)
        .expect("render with plain color config should succeed");
    let auto = render(input, "text", r#"{"color":"auto"}"#)
        .expect("render with auto color should succeed");
    let ansi = render(input, "text", r#"{"color":"always"}"#)
        .expect("render with ansi color config should succeed");

    assert!(!plain.contains("\u{1b}["));
    assert!(!auto.contains("\u{1b}["));
    assert!(ansi.contains("38;2;"));
    assert!(ansi.contains("48;2;"));
    assert_eq!(plain, auto);
    assert_eq!(strip_ansi(&ansi), plain);
}

#[wasm_bindgen_test]
fn renders_flowchart_svg() {
    let output = render("graph TD\nA-->B", "svg", "{}").expect("svg render should succeed");
    assert!(output.contains("<svg"));
}

#[wasm_bindgen_test]
fn rejects_legacy_edge_routing_config_key() {
    let error = render(
        "graph TD\nA-->B",
        "svg",
        r#"{"edgeRouting":"orthogonal-preview"}"#,
    )
    .expect_err("legacy edgeRouting should be rejected");
    assert!(error_debug(error).contains("unknown field"));
}

#[wasm_bindgen_test]
fn detect_returns_flowchart_id() {
    assert_eq!(detect("graph TD\nA-->B"), Some("flowchart".to_string()));
}

#[wasm_bindgen_test]
fn detect_returns_none_for_unknown_input() {
    assert_eq!(detect("this is not mermaid"), None);
}

#[wasm_bindgen_test]
fn rejects_unknown_format() {
    let error = render("graph TD\nA-->B", "nope", "{}").expect_err("unknown format must error");
    assert!(error_debug(error).contains("unknown output format"));
}

#[wasm_bindgen_test]
fn rejects_unknown_diagram_type() {
    let error = render("not mermaid at all", "text", "{}").expect_err("unknown diagram must fail");
    assert!(error_debug(error).contains("unknown diagram type"));
}

#[wasm_bindgen_test]
fn rejects_invalid_config_json() {
    let error = render("graph TD\nA-->B", "text", "{").expect_err("invalid config must fail");
    assert!(error_debug(error).contains("invalid config_json"));
}

#[wasm_bindgen_test]
fn rejects_legacy_edge_style_config_key() {
    let error = render("graph TD\nA-->B", "svg", r#"{"edgeStyle":"straight"}"#)
        .expect_err("legacy edgeStyle should be rejected");
    assert!(error_debug(error).contains("unknown field"));
}

#[wasm_bindgen_test]
fn applies_geometry_level_and_path_simplification_for_mmds() {
    let output = render(
        "graph TD\nA-->B",
        "mmds",
        r#"{"geometryLevel":"routed","pathSimplification":"minimal"}"#,
    )
    .expect("mmds render with geometry/path config should succeed");
    assert!(output.contains("\"path\""));
}

#[wasm_bindgen_test]
fn version_matches_package_version() {
    assert_eq!(version(), env!("CARGO_PKG_VERSION"));
}
