//! Regression tests for the configurable subgraph title margin.
//!
//! The knob mirrors Mermaid's `flowchart.subGraphTitleMargin: { top, bottom }`
//! and adjusts the rank-axis space reserved between a subgraph's title and
//! its first member row. Defaults preserve the original measurement-driven
//! value so existing renders stay byte-identical; explicit values widen or
//! narrow the title gap.

use mmdflux::{
    EngineAlgorithmId, LayoutConfig, OutputFormat, RenderConfig, RuntimeConfigInput,
    SubgraphTitleMargin, render_diagram,
};

const SUBGRAPH_INPUT: &str = "flowchart TD\nsubgraph S [Group]\nA-->B\nend\n";

/// Render the fixture as SVG, where the visual-contract code path consumes
/// the `subgraph_title_margin` knob.
fn render_svg(config: RenderConfig) -> String {
    render_diagram(SUBGRAPH_INPUT, OutputFormat::Svg, &config)
        .expect("rendering SVG should succeed")
}

/// Extract the `height` attribute of the first `<rect>` element following
/// the cluster-class container marker (`class="cluster `). The cluster rect
/// width/height comes directly from the routed subgraph bounds, so growth
/// in this number reflects the rank-axis title margin expansion.
fn cluster_rect_height(svg: &str) -> f64 {
    let cluster_marker = svg
        .find("class=\"cluster ")
        .or_else(|| svg.find("class=\"cluster\""))
        .expect("SVG should mark the subgraph cluster group");
    let rect_open = svg[cluster_marker..]
        .find("<rect")
        .map(|offset| cluster_marker + offset)
        .expect("cluster group should embed a <rect>");
    let height_attr = svg[rect_open..]
        .find("height=\"")
        .map(|offset| rect_open + offset + "height=\"".len())
        .expect("cluster rect should set a height attribute");
    let height_end = svg[height_attr..]
        .find('"')
        .map(|offset| height_attr + offset)
        .expect("height attribute should close");
    svg[height_attr..height_end]
        .parse::<f64>()
        .expect("height attribute should parse as f64")
}

#[test]
fn subgraph_title_margin_defaults_to_none() {
    let config = RenderConfig::default();
    assert!(config.layout.subgraph_title_margin.is_none());
}

#[test]
fn explicit_subgraph_title_margin_grows_subgraph_height() {
    let default_svg = render_svg(RenderConfig::default());
    let widened_svg = render_svg(RenderConfig {
        layout: LayoutConfig {
            // Drive the total margin well above the default
            // measurement-driven value (~font_size, 14-16 px) so the
            // difference outruns any sub-pixel rounding.
            subgraph_title_margin: Some(SubgraphTitleMargin::new(40.0, 40.0)),
            ..LayoutConfig::default()
        },
        ..RenderConfig::default()
    });

    let default_height = cluster_rect_height(&default_svg);
    let widened_height = cluster_rect_height(&widened_svg);

    assert!(
        widened_height > default_height,
        "explicit subgraph_title_margin should grow rank-axis cluster height: \
         default = {default_height}, widened = {widened_height}"
    );
}

#[test]
fn explicit_subgraph_title_margin_uses_total_of_top_and_bottom() {
    let split_svg = render_svg(RenderConfig {
        layout: LayoutConfig {
            subgraph_title_margin: Some(SubgraphTitleMargin::new(30.0, 30.0)),
            ..LayoutConfig::default()
        },
        ..RenderConfig::default()
    });
    let totalled_svg = render_svg(RenderConfig {
        layout: LayoutConfig {
            subgraph_title_margin: Some(SubgraphTitleMargin::new(0.0, 60.0)),
            ..LayoutConfig::default()
        },
        ..RenderConfig::default()
    });

    // Mermaid's `subGraphTitleTotalMargin = top + bottom`; only the sum
    // matters for rank-axis spacing today, so the two configurations must
    // produce identical cluster heights.
    assert_eq!(
        cluster_rect_height(&split_svg),
        cluster_rect_height(&totalled_svg),
    );
}

#[test]
fn runtime_config_input_accepts_subgraph_title_margin_json() {
    let input = serde_json::from_str::<RuntimeConfigInput>(
        r#"{ "layout": { "subGraphTitleMargin": { "top": 12.5, "bottom": 7.5 } } }"#,
    )
    .expect("subGraphTitleMargin JSON should parse");
    let config = input
        .into_render_config()
        .expect("conversion to RenderConfig should succeed");

    let margin = config
        .layout
        .subgraph_title_margin
        .expect("subgraph_title_margin should be configured");
    assert_eq!(margin.top, 12.5);
    assert_eq!(margin.bottom, 7.5);
    assert_eq!(margin.total(), 20.0);
}

#[test]
fn runtime_config_input_rejects_negative_subgraph_title_margin() {
    let input = serde_json::from_str::<RuntimeConfigInput>(
        r#"{ "layout": { "subGraphTitleMargin": { "top": -1.0 } } }"#,
    )
    .expect("subGraphTitleMargin JSON should parse");

    let err = input
        .into_render_config()
        .expect_err("negative top margin should be rejected");
    assert!(
        err.message.contains("subGraphTitleMargin.top"),
        "{}",
        err.message
    );
    assert!(err.message.contains("non-negative"), "{}", err.message);
}

#[test]
fn runtime_config_input_rejects_non_finite_subgraph_title_margin_at_parse() {
    // serde_json's f64 parser rejects out-of-range literals (`1e400`,
    // `-1e400`) before they reach validation, and `NaN`/`Infinity` are
    // not valid JSON tokens. This pins that defense: the validation in
    // `SubgraphTitleMarginInput::into_config` is layered safety for
    // hand-constructed input structs, but no non-finite value can leak
    // through the JSON entry point in the first place.
    let parse_overflow = serde_json::from_str::<RuntimeConfigInput>(
        r#"{ "layout": { "subGraphTitleMargin": { "top": 1e400 } } }"#,
    );
    assert!(
        parse_overflow.is_err(),
        "overflow literal must not deserialize"
    );

    let parse_neg_overflow = serde_json::from_str::<RuntimeConfigInput>(
        r#"{ "layout": { "subGraphTitleMargin": { "bottom": -1e400 } } }"#,
    );
    assert!(
        parse_neg_overflow.is_err(),
        "negative overflow literal must not deserialize"
    );

    let parse_nan_token = serde_json::from_str::<RuntimeConfigInput>(
        r#"{ "layout": { "subGraphTitleMargin": { "top": NaN } } }"#,
    );
    assert!(
        parse_nan_token.is_err(),
        "NaN token is not legal JSON and must not deserialize"
    );
}

#[test]
fn explicit_subgraph_title_margin_routes_through_mermaid_layered_engine() {
    // The Flux tests above default to layout_engine = None (Flux). This
    // pins the Mermaid-layered forwarding path at
    // `src/engines/graph/mermaid.rs:182`, which copies user margin onto
    // the engine's profile flags.
    let mermaid = EngineAlgorithmId::parse("mermaid-layered")
        .expect("mermaid-layered should be a registered engine algorithm");

    let default_svg = render_svg(RenderConfig {
        layout_engine: Some(mermaid),
        ..RenderConfig::default()
    });
    let widened_svg = render_svg(RenderConfig {
        layout_engine: Some(mermaid),
        layout: LayoutConfig {
            subgraph_title_margin: Some(SubgraphTitleMargin::new(40.0, 40.0)),
            ..LayoutConfig::default()
        },
        ..RenderConfig::default()
    });

    assert!(
        cluster_rect_height(&widened_svg) > cluster_rect_height(&default_svg),
        "mermaid-layered engine should honor subgraph_title_margin"
    );
}
