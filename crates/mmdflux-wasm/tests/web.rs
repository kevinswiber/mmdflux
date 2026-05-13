use mmdflux_wasm::{
    browser_text_metrics_request, detect, render, render_with_browser_text_metrics, validate,
    version,
};
use wasm_bindgen::JsCast;
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

fn dynamic_metrics_json_fixture() -> &'static str {
    r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}]}"#
}

fn dynamic_metrics_json_with_profile_fixture() -> &'static str {
    r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}],"profileId":"mmdflux-browser-canvas-v1"}"#
}

fn dynamic_multi_style_metrics_json_fixture() -> &'static str {
    r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"},{"id":"s1","fontFamily":"Verdana","fontSize":8,"fontStyle":"normal","fontWeight":"400","lineHeight":12,"cssFont":"8px Verdana"}],"profileId":"mmdflux-browser-canvas-v1"}"#
}

fn dynamic_mermaid_multi_font_metrics_json_fixture() -> &'static str {
    r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"},{"id":"s1","fontFamily":"Verdana","fontSize":8,"fontStyle":"normal","fontWeight":"400","lineHeight":12,"cssFont":"8px Verdana"},{"id":"s2","fontFamily":"Courier New","fontSize":20,"fontStyle":"normal","fontWeight":"400","lineHeight":30,"cssFont":"20px Courier New"},{"id":"s3","fontFamily":"Times New Roman","fontSize":32,"fontStyle":"normal","fontWeight":"400","lineHeight":48,"cssFont":"32px Times New Roman"}],"profileId":"mmdflux-browser-canvas-v1"}"#
}

fn dynamic_metrics_json_with_profile_fields(
    profile_id: &str,
    profile_version: u32,
    font_family: &str,
    font_size_px: f64,
    line_height_px: f64,
    font_style: &str,
    font_weight: &str,
) -> String {
    format!(
        r#"{{"defaultStyle":"s0","textStyles":[{{"id":"s0","fontFamily":"{font_family}","fontSize":{font_size_px},"fontStyle":"{font_style}","fontWeight":"{font_weight}","lineHeight":{line_height_px},"cssFont":"{font_style} {font_weight} {font_size_px}px {font_family}"}}],"profileId":"{profile_id}","profileVersion":{profile_version}}}"#
    )
}

fn callback(body: &str) -> js_sys::Function {
    js_sys::Function::new_with_args("text, cssFont", body)
}

#[wasm_bindgen_test]
fn browser_text_metrics_request_returns_style_set_for_multi_font_graph() {
    let input = include_str!("../../../tests/fixtures/flowchart/dynamic/multi_font_styles.mmd");
    let decision =
        browser_text_metrics_request(input, "svg", "{}").expect("resolver export should succeed");
    let value: serde_json::Value =
        serde_json::from_str(&decision).expect("decision should be JSON");

    assert_eq!(value["required"], true);
    let styles = value["browserTextMetrics"]["textStyles"]
        .as_array()
        .expect("styles should be an array");
    assert!(styles.iter().any(|style| {
        style["fontFamily"] == "Verdana" && style["cssFont"] == r#"normal 400 8px "Verdana""#
    }));
    assert!(
        styles
            .iter()
            .any(|style| style["fontFamily"] == "Courier New"
                && style["cssFont"] == r#"normal 400 20px "Courier New""#)
    );
    assert!(
        styles
            .iter()
            .any(|style| style["fontFamily"] == "Times New Roman"
                && style["cssFont"] == r#"normal 400 32px "Times New Roman""#)
    );
}

#[wasm_bindgen_test]
fn browser_text_metrics_request_returns_not_required_for_unstyled_svg() {
    let decision = browser_text_metrics_request("graph TD\nA-->B", "svg", "{}")
        .expect("resolver export should succeed");
    let value: serde_json::Value =
        serde_json::from_str(&decision).expect("decision should be JSON");

    assert_eq!(value["required"], false);
    assert!(value.get("browserTextMetrics").is_none());
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
fn dynamic_text_metrics_callback_changes_svg_geometry() {
    let input = "graph TD\nA -->|mmmm| B";
    let static_output = render(input, "svg", "{}").expect("static svg should render");
    let measure = callback("return text.includes('m') ? 100 : 8;");
    let dynamic_output = render_with_browser_text_metrics(
        input,
        "svg",
        "{}",
        dynamic_metrics_json_fixture(),
        &measure,
    )
    .expect("dynamic svg should render");

    assert_ne!(dynamic_output, static_output);
    assert!(dynamic_output.contains("<svg"));
    assert!(dynamic_output.contains("width=\"108.00\""));
}

#[wasm_bindgen_test]
fn render_with_browser_text_metrics_passes_per_style_css_font() {
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let calls_for_callback = calls.clone();
    let closure = wasm_bindgen::closure::Closure::<dyn FnMut(String, String) -> f64>::new(
        move |text: String, css_font: String| {
            if text == "Same" {
                calls_for_callback.borrow_mut().push(css_font.clone());
            }
            if css_font.contains("Verdana") {
                text.len() as f64 * 6.0
            } else {
                text.len() as f64 * 10.0
            }
        },
    );
    let measure = closure.as_ref().unchecked_ref::<js_sys::Function>();
    let output = render_with_browser_text_metrics(
        "graph TD\nA[Same] --> B[Same]\nstyle B font-family:Verdana,font-size:8px",
        "svg",
        "{}",
        dynamic_multi_style_metrics_json_fixture(),
        measure,
    )
    .expect("dynamic multi-style svg should render");

    assert!(output.contains("<svg"));
    let calls = calls.borrow();
    assert!(calls.iter().any(|font| font == "16px Inter"), "{calls:?}");
    assert!(calls.iter().any(|font| font == "8px Verdana"), "{calls:?}");
}

#[wasm_bindgen_test]
fn static_render_export_stays_byte_stable_after_dynamic_render() {
    let input = include_str!("../../../tests/fixtures/flowchart/labeled_edges.mmd");
    let before = render(input, "svg", "{}").expect("static svg should render before dynamic");
    let measure = callback("return text.length * 9;");
    let dynamic = render_with_browser_text_metrics(
        "graph TD\nA -->|mmmm| B",
        "svg",
        "{}",
        dynamic_metrics_json_fixture(),
        &measure,
    )
    .expect("dynamic svg should render");
    assert!(dynamic.contains("<svg"));
    let after = render(input, "svg", "{}").expect("static svg should render after dynamic");

    assert_eq!(after, before);
}

#[wasm_bindgen_test]
fn static_wasm_render_rejects_custom_provider_free_font_style() {
    for (format, expected) in [
        ("svg", "dynamic text metrics"),
        ("mmds", "dynamic text metrics"),
        ("text", "not supported"),
        ("ascii", "not supported"),
    ] {
        let error = render(
            "graph TD\nA-->B",
            format,
            r#"{"fontFamily":"Inter","fontSize":16}"#,
        )
        .expect_err("custom provider-free font style should fail");

        let message = error_debug(error);
        assert!(message.contains(expected), "{message}");
    }
}

#[wasm_bindgen_test]
fn static_wasm_render_accepts_static_descriptor_matching_font_style() {
    let output = render(
        "graph TD\nA-->B",
        "svg",
        r#"{"fontFamily":"\"trebuchet ms\", verdana, arial, sans-serif","fontSize":16}"#,
    )
    .expect("static descriptor matching style should render");

    assert!(output.contains("<svg"));
}

#[wasm_bindgen_test]
fn static_wasm_render_accepts_equivalent_static_descriptor_spelling_byte_stable() {
    let input = "graph TD\nA[mmmm]-->B[iiii]";
    let default_output = render(input, "svg", "{}").expect("default render");
    let styled_output = render(
        input,
        "svg",
        r#"{"fontFamily":"Trebuchet MS, Verdana, Arial, sans-serif","fontSize":16}"#,
    )
    .expect("equivalent descriptor spelling should render");

    assert_eq!(styled_output, default_output);
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_callback_throw_errors() {
    let measure = callback("throw new Error('canvas failed');");
    let err = render_with_browser_text_metrics(
        "graph TD\nA-->B",
        "svg",
        "{}",
        dynamic_metrics_json_fixture(),
        &measure,
    )
    .expect_err("callback throw should fail");

    assert!(error_debug(err).contains("canvas failed"));
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_rejects_terminal_formats_before_measurement() {
    for format in ["text", "ascii"] {
        let measure = callback("throw new Error('callback should not run');");
        let err = render_with_browser_text_metrics(
            "graph TD\nA[Alpha] --> B[Beta]",
            format,
            "{}",
            dynamic_metrics_json_with_profile_fixture(),
            &measure,
        )
        .expect_err("terminal dynamic output should reject");

        let message = error_debug(err);
        assert!(message.contains(format), "{message}");
        assert!(message.contains("terminal"), "{message}");
        assert!(message.contains("unsupported"), "{message}");
        assert!(!message.contains("callback should not run"), "{message}");
    }
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_rejects_sequence_before_measurement() {
    let measure = callback("throw new Error('callback should not run');");
    let err = render_with_browser_text_metrics(
        "sequenceDiagram\nAlice->>Bob: Hi",
        "svg",
        "{}",
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect_err("sequence dynamic output should reject");

    let message = error_debug(err);
    assert!(message.contains("sequence"), "{message}");
    assert!(message.contains("timeline"), "{message}");
    assert!(message.contains("unsupported"), "{message}");
    assert!(!message.contains("callback should not run"), "{message}");
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_callback_rejects_non_number_returns() {
    for (body, expected) in [
        ("return Promise.resolve(12);", "synchronous"),
        ("return { width: 12 };", "return a number"),
        ("return '12';", "return a number"),
        ("return null;", "return a number"),
        ("return undefined;", "return a number"),
    ] {
        let measure = callback(body);
        let err = render_with_browser_text_metrics(
            "graph TD\nA-->B",
            "svg",
            "{}",
            dynamic_metrics_json_fixture(),
            &measure,
        )
        .expect_err("non-number callback return should fail");

        let message = error_debug(err);
        assert!(message.contains(expected), "{message}");
    }
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_callback_rejects_invalid_widths() {
    for body in ["return Number.NaN;", "return Infinity;", "return -1;"] {
        let measure = callback(body);
        let err = render_with_browser_text_metrics(
            "graph TD\nA-->B",
            "svg",
            "{}",
            dynamic_metrics_json_fixture(),
            &measure,
        )
        .expect_err("invalid width should fail");

        let message = error_debug(err);
        assert!(message.contains("finite non-negative"), "{message}");
    }
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_callback_failure_does_not_fallback_to_static_metrics() {
    let measure = callback(
        "if (text === 'mmmm') { throw new Error('missing glyph mmmm'); } return text.length * 8;",
    );
    let err = render_with_browser_text_metrics(
        "graph TD\nA -->|mmmm| B",
        "svg",
        "{}",
        dynamic_metrics_json_fixture(),
        &measure,
    )
    .expect_err("callback failure should fail the dynamic render");

    let message = error_debug(err);
    assert!(message.contains("missing glyph mmmm"), "{message}");
    assert!(message.contains("mmmm"), "{message}");
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_callback_reentry_errors_cleanly() {
    let inner = callback("return 8;");
    let closure = wasm_bindgen::closure::Closure::<dyn FnMut(String, String) -> f64>::new(
        move |_text, _css_font| {
            let _ = render_with_browser_text_metrics(
                "graph TD\nA-->B",
                "svg",
                "{}",
                dynamic_metrics_json_fixture(),
                &inner,
            );
            8.0
        },
    );
    let measure = closure.as_ref().unchecked_ref::<js_sys::Function>();
    let err = render_with_browser_text_metrics(
        "graph TD\nA-->B",
        "svg",
        "{}",
        dynamic_metrics_json_fixture(),
        measure,
    )
    .expect_err("dynamic render re-entry should fail");

    let message = error_debug(err);
    assert!(message.contains("re-entered"), "{message}");
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_callback_can_call_validate_without_corruption() {
    let closure = wasm_bindgen::closure::Closure::<dyn FnMut(String, String) -> f64>::new(
        move |_text, _css_font| {
            let result = validate("graph TD\nA-->B");
            assert!(result.contains("\"valid\":true"));
            8.0
        },
    );
    let measure = closure.as_ref().unchecked_ref::<js_sys::Function>();
    let output = render_with_browser_text_metrics(
        "graph TD\nA-->B",
        "svg",
        "{}",
        dynamic_metrics_json_fixture(),
        measure,
    )
    .expect("validate re-entry should not corrupt dynamic render");

    assert!(output.contains("<svg"));
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_renders_provider_bound_mmds_output() {
    let measure = callback("return text.length * 8;");
    let output = render_with_browser_text_metrics(
        "graph TD\nA-->B",
        "mmds",
        "{}",
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("dynamic browser metrics should render provider-bound MMDS");

    assert!(output.contains("\"org.mmdflux.text-metrics.v1\""));
    assert!(output.contains("\"org.mmdflux.text-measurements.v1\""));
    assert!(output.contains("\"source\": \"dynamic\""));
    assert!(output.contains("\"id\": \"mmdflux-browser-canvas-v1\""));
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_replays_provider_bound_mmds_output() {
    let measure = callback("return text.length * 8;");
    let input = "graph TD\nA[Alpha] -->|a labeled edge| B[Beta]";
    let config = r#"{"geometryLevel":"routed"}"#;
    let direct_svg = render_with_browser_text_metrics(
        input,
        "svg",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("direct dynamic svg should render");
    let mmds = render_with_browser_text_metrics(
        input,
        "mmds",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("dynamic mmds should render");
    let replay_svg = render_with_browser_text_metrics(
        &mmds,
        "svg",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("dynamic mmds replay should render");

    assert_eq!(replay_svg, direct_svg);
}

#[wasm_bindgen_test]
fn browser_dynamic_mmds_replays_provider_free_through_static_render() {
    let measure = callback("return text.length * 8;");
    let input = "graph TD\nA[Alpha] -->|a labeled edge| B[Beta]";
    let config = r#"{"geometryLevel":"routed"}"#;
    let direct_svg = render_with_browser_text_metrics(
        input,
        "svg",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("direct dynamic svg should render");
    let mmds = render_with_browser_text_metrics(
        input,
        "mmds",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("dynamic mmds should render");

    let replay_svg =
        render(&mmds, "svg", config).expect("static render should replay measured dynamic MMDS");

    assert_eq!(replay_svg, direct_svg);
}

#[wasm_bindgen_test]
fn browser_dynamic_multifont_mmds_replays_provider_free_through_static_render() {
    let measure = callback(
        r#"
        if (cssFont.includes("Times New Roman")) return text.length * 30;
        if (cssFont.includes("Courier New")) return text.length * 15;
        if (cssFont.includes("Verdana")) return text.length * 4;
        return text.length * 8;
        "#,
    );
    let input = include_str!("../../../tests/fixtures/flowchart/dynamic/multi_font_styles.mmd");
    let config = r#"{"geometryLevel":"routed"}"#;
    let direct_svg = render_with_browser_text_metrics(
        input,
        "svg",
        config,
        dynamic_mermaid_multi_font_metrics_json_fixture(),
        &measure,
    )
    .expect("direct dynamic multi-font svg should render");
    let mmds = render_with_browser_text_metrics(
        input,
        "mmds",
        config,
        dynamic_mermaid_multi_font_metrics_json_fixture(),
        &measure,
    )
    .expect("dynamic multi-font mmds should render");

    assert!(direct_svg.contains("font-family=\"Verdana\""));
    assert!(direct_svg.contains("font-family=\"Courier New\""));
    assert!(direct_svg.contains("font-family=\"Times New Roman\""));
    assert!(mmds.contains("\"textStyles\""));
    assert!(mmds.contains("\"Times New Roman\""));

    let replay_svg =
        render(&mmds, "svg", config).expect("static render should replay measured dynamic MMDS");

    assert_eq!(replay_svg, direct_svg);
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_rejects_mismatched_mmds_provider_identity() {
    let measure = callback("return text.length * 8;");
    let config = r#"{"geometryLevel":"routed"}"#;
    let mmds = render_with_browser_text_metrics(
        "graph TD\nA-->B",
        "mmds",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect("dynamic mmds should render");

    let mismatched_metrics = [
        (
            "profile id",
            dynamic_metrics_json_with_profile_fields(
                "other-provider-v1",
                1,
                "Inter",
                16.0,
                24.0,
                "normal",
                "400",
            ),
            "metricsProfile.id",
        ),
        (
            "profile version",
            dynamic_metrics_json_with_profile_fields(
                "mmdflux-browser-canvas-v1",
                2,
                "Inter",
                16.0,
                24.0,
                "normal",
                "400",
            ),
            "metricsProfile.version",
        ),
        (
            "font family",
            dynamic_metrics_json_with_profile_fields(
                "mmdflux-browser-canvas-v1",
                1,
                "Arial",
                16.0,
                24.0,
                "normal",
                "400",
            ),
            "defaultTextStyle.font-family",
        ),
        (
            "font size",
            dynamic_metrics_json_with_profile_fields(
                "mmdflux-browser-canvas-v1",
                1,
                "Inter",
                18.0,
                24.0,
                "normal",
                "400",
            ),
            "defaultTextStyle.font-size",
        ),
        (
            "font style",
            dynamic_metrics_json_with_profile_fields(
                "mmdflux-browser-canvas-v1",
                1,
                "Inter",
                16.0,
                24.0,
                "italic",
                "400",
            ),
            "defaultTextStyle.font-style",
        ),
        (
            "font weight",
            dynamic_metrics_json_with_profile_fields(
                "mmdflux-browser-canvas-v1",
                1,
                "Inter",
                16.0,
                24.0,
                "normal",
                "700",
            ),
            "defaultTextStyle.font-weight",
        ),
        (
            "line height",
            dynamic_metrics_json_with_profile_fields(
                "mmdflux-browser-canvas-v1",
                1,
                "Inter",
                16.0,
                30.0,
                "normal",
                "400",
            ),
            "defaultTextStyle.line-height",
        ),
    ];

    for (name, metrics_json, expected_field) in mismatched_metrics {
        let err = render_with_browser_text_metrics(&mmds, "svg", config, &metrics_json, &measure)
            .expect_err("mismatched provider identity should fail");
        let message = error_debug(err);
        assert!(message.contains(expected_field), "{name}: {message}");
    }

    for (name, replay_config, expected_field) in [
        (
            "node padding x",
            r#"{"geometryLevel":"routed","svgNodePaddingX":20}"#,
            "layoutText.node-padding-x",
        ),
        (
            "node padding y",
            r#"{"geometryLevel":"routed","svgNodePaddingY":20}"#,
            "layoutText.node-padding-y",
        ),
    ] {
        let err = render_with_browser_text_metrics(
            &mmds,
            "svg",
            replay_config,
            dynamic_metrics_json_with_profile_fixture(),
            &measure,
        )
        .expect_err("mismatched provider layout should fail");
        let message = error_debug(err);
        assert!(message.contains(expected_field), "{name}: {message}");
    }

    let mut value: serde_json::Value = serde_json::from_str(&mmds).unwrap();
    value["extensions"]["org.mmdflux.text-metrics.v1"]["layoutText"]["label-padding-x"] =
        serde_json::json!(8.0);
    let mismatched_label_padding = serde_json::to_string(&value).unwrap();
    let err = render_with_browser_text_metrics(
        &mismatched_label_padding,
        "svg",
        config,
        dynamic_metrics_json_with_profile_fixture(),
        &measure,
    )
    .expect_err("persisted label padding mismatch should fail");
    assert!(error_debug(err).contains("layoutText.label-padding-x"));
}

#[wasm_bindgen_test]
fn dynamic_text_metrics_rejects_config_json_font_style() {
    let measure = callback("return text.length * 8;");

    for config_json in [
        r#"{"fontFamily":"Inter","fontSize":16}"#,
        r#"{"themeVariables":{"fontFamily":"Inter","fontSize":"16px"}}"#,
    ] {
        let err = render_with_browser_text_metrics(
            "graph TD\nA-->B",
            "svg",
            config_json,
            dynamic_metrics_json_fixture(),
            &measure,
        )
        .expect_err("dynamic export should reject configJson font style");

        let message = error_debug(err);
        assert!(message.contains("configJson"), "{message}");
        assert!(message.contains("metricsJson"), "{message}");
    }
}

#[wasm_bindgen_test]
fn renders_svg_with_font_metrics_profile_config() {
    for profile in ["mmdflux-heuristic-proportional-v1", "mmdflux-sans-v1"] {
        let output = render(
            "graph TD\nA[mmmm]-->B[iiii]",
            "svg",
            &format!(r#"{{"fontMetricsProfile":"{profile}"}}"#),
        )
        .expect("font metrics profile config should render");
        assert!(output.contains("<svg"));
    }
}

#[wasm_bindgen_test]
fn rejects_unsupported_font_metrics_profile_config() {
    let error = render(
        "graph TD\nA-->B",
        "svg",
        r#"{"fontMetricsProfile":"unknown-profile-v1"}"#,
    )
    .expect_err("unsupported font metrics profile should fail");
    assert!(error_debug(error).contains("unsupported text metrics profile 'unknown-profile-v1'"));
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
