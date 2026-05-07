//! MMDS replay rendering: hydrate to graph-family IR and re-render.
//!
//! The mmds module owns the interchange format (parse, hydrate, serialize).
//! This module owns the render dispatch for MMDS input.

use std::fmt::Display;

use serde_json::{Map, Value};

use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::GeometryLevel;
use crate::graph::measure::{
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID, DEFAULT_EDGE_LABEL_MAX_WIDTH,
    DEFAULT_PROPORTIONAL_NODE_PADDING_X, DEFAULT_PROPORTIONAL_NODE_PADDING_Y, ResolvedTextMetrics,
    TextMetricsProfileConfig, TextMetricsProfileDescriptor, resolve_text_metrics_profile,
};
use crate::mmds::{
    Document, TEXT_METRICS_EXTENSION_NAMESPACE, from_document, generate_mermaid,
    hydrate_graph_geometry_from_document_with_diagram, hydrate_routed_geometry_from_document,
    parse_input, resolve_logical_diagram_id,
};
use crate::render::graph::{
    SvgRenderOptions, TextRenderOptions, edge_routing_from_style,
    render_svg_from_geometry_with_theme_routing_and_metrics,
    render_svg_from_routed_geometry_with_theme_and_metrics, render_text_from_geometry,
};
use crate::render::svg::theme::ResolvedSvgTheme;
use crate::views::VIEW_EXTENSION_NAMESPACE;

/// Render MMDS input through the MMDS replay path.
pub(crate) fn render_input(
    input: &str,
    format: OutputFormat,
    geometry_level: GeometryLevel,
    text_options: &TextRenderOptions,
    svg_options: &SvgRenderOptions,
    svg_theme: Option<&ResolvedSvgTheme>,
    requested_text_metrics_profile: Option<&str>,
) -> Result<String, RenderError> {
    let payload =
        parse_input(input).map_err(|error| prefixed_display_error("parse error", error))?;
    render_document(
        &payload,
        format,
        geometry_level,
        text_options,
        svg_options,
        svg_theme,
        requested_text_metrics_profile,
    )
}

/// Render a parsed MMDS document through the MMDS replay path.
pub(crate) fn render_document(
    payload: &Document,
    format: OutputFormat,
    geometry_level: GeometryLevel,
    text_options: &TextRenderOptions,
    svg_options: &SvgRenderOptions,
    svg_theme: Option<&ResolvedSvgTheme>,
    requested_text_metrics_profile: Option<&str>,
) -> Result<String, RenderError> {
    let diagram_id = resolve_logical_diagram_id(payload)?;
    let has_routed_geometry = payload.geometry_level == GeometryLevel::Routed;

    if matches!(format, OutputFormat::Mmds) {
        let output = if has_routed_geometry && geometry_level == GeometryLevel::Layout {
            strip_routed_fields(payload)
        } else {
            payload.clone()
        };
        return serde_json::to_string_pretty(&output)
            .map_err(|error| prefixed_display_error("MMDS serialization error", error));
    }

    if matches!(format, OutputFormat::Mermaid) {
        return generate_mermaid(payload).map_err(display_error);
    }

    let replay_text_metrics =
        resolve_text_metrics_for_replay(payload, requested_text_metrics_profile)?;
    let text_metrics = &replay_text_metrics.resolved;
    let mut diagram = from_document(payload).map_err(display_error)?;

    // MMDS replay path runs the wrap pass so the hydrated graph's edge labels
    // carry the same `wrapped_label_lines` artifact the original render would
    // have. `wrapped_label_lines` is
    // `#[serde(skip)]` on the Edge, so round-tripping through MMDS drops
    // it; rehydrating it here keeps the SVG/text replay in lockstep with
    // the direct runtime render. New MMDS payloads persist the metrics
    // identity and layout-time values under the text-metrics extension; older
    // payloads fall back to the compatibility profile.
    crate::graph::label_wrap::prepare_wrapped_labels(
        &mut diagram.edges,
        &text_metrics.metrics,
        text_metrics.descriptor.layout_text.edge_label_max_width,
    );

    let geometry = hydrate_graph_geometry_from_document_with_diagram(payload, &diagram)
        .map_err(display_error)?;
    let routed = has_routed_geometry
        .then(|| hydrate_routed_geometry_from_document(payload))
        .transpose()
        .map_err(display_error)?;

    match format {
        OutputFormat::Text | OutputFormat::Ascii => {
            let mut options = text_options.clone();
            options.output_format = format;
            options.use_pinned_ranks =
                options.use_pinned_ranks || is_shared_coordinates_view(payload);
            Ok(render_text_from_geometry(
                &diagram,
                &geometry,
                routed.as_ref(),
                &options,
            ))
        }
        OutputFormat::Svg => {
            let replay_svg_options = replay_text_metrics
                .from_extension
                .then(|| svg_options_with_text_metrics(svg_options, &text_metrics.descriptor));
            let svg_options = replay_svg_options.as_ref().unwrap_or(svg_options);

            Ok(match routed.as_ref() {
                Some(routed) => render_svg_from_routed_geometry_with_theme_and_metrics(
                    &diagram,
                    routed,
                    svg_options,
                    svg_theme,
                    &text_metrics.metrics,
                ),
                None => render_svg_from_geometry_with_theme_routing_and_metrics(
                    &diagram,
                    &geometry,
                    svg_options,
                    edge_routing_from_style(svg_options.routing_style),
                    svg_theme,
                    &text_metrics.metrics,
                ),
            })
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
    }
}

fn is_shared_coordinates_view(payload: &Document) -> bool {
    payload
        .extensions
        .get(VIEW_EXTENSION_NAMESPACE)
        .and_then(|extension| extension.get("layout_mode"))
        .and_then(serde_json::Value::as_str)
        == Some("shared_coordinates")
}

struct ReplayTextMetrics {
    resolved: ResolvedTextMetrics,
    from_extension: bool,
}

fn resolve_text_metrics_for_replay(
    payload: &Document,
    requested_text_metrics_profile: Option<&str>,
) -> Result<ReplayTextMetrics, RenderError> {
    let Some(extension) = payload.extensions.get(TEXT_METRICS_EXTENSION_NAMESPACE) else {
        ensure_requested_profile_matches_replay(
            requested_text_metrics_profile,
            COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
        )?;
        return resolve_text_metrics_profile(TextMetricsProfileConfig::default())
            .map(|resolved| ReplayTextMetrics {
                resolved,
                from_extension: false,
            })
            .map_err(display_error);
    };

    let metrics_profile = required_object(extension, "metricsProfile")?;
    let profile_id = metrics_profile
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_text_metrics_extension("missing metricsProfile.id"))?;
    required_string_field(metrics_profile, "metricsProfile", "source")?;
    required_integer_field(metrics_profile, "metricsProfile", "version")?;
    validate_default_text_style(required_object(extension, "defaultTextStyle")?)?;
    let layout_text = required_object(extension, "layoutText")?;
    validate_layout_text(layout_text)?;
    ensure_requested_profile_matches_replay(requested_text_metrics_profile, profile_id)?;

    let resolved = resolve_text_metrics_profile(TextMetricsProfileConfig {
        profile_id: Some(profile_id),
        node_padding_x: optional_number_field(
            Some(layout_text),
            "node-padding-x",
            DEFAULT_PROPORTIONAL_NODE_PADDING_X,
        )?,
        node_padding_y: optional_number_field(
            Some(layout_text),
            "node-padding-y",
            DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
        )?,
        edge_label_max_width: optional_edge_label_max_width(Some(layout_text))?,
    })
    .map_err(display_error)?;

    Ok(ReplayTextMetrics {
        resolved,
        from_extension: true,
    })
}

fn required_object<'a>(
    extension: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Map<String, Value>, RenderError> {
    extension
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_text_metrics_extension(format!("missing {field}")))
}

fn required_string_field<'a>(
    object: &'a Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<&'a str, RenderError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid_text_metrics_extension(format!("{object_name}.{field} must be a string"))
    })
}

fn required_integer_field(
    object: &Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<(), RenderError> {
    let Some(number) = object.get(field).and_then(Value::as_number) else {
        return Err(invalid_text_metrics_extension(format!(
            "{object_name}.{field} must be an integer"
        )));
    };

    if number.as_i64().is_some() || number.as_u64().is_some() {
        Ok(())
    } else {
        Err(invalid_text_metrics_extension(format!(
            "{object_name}.{field} must be an integer"
        )))
    }
}

fn required_number_field(
    object: &Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<f64, RenderError> {
    object.get(field).and_then(Value::as_f64).ok_or_else(|| {
        invalid_text_metrics_extension(format!("{object_name}.{field} must be a number"))
    })
}

fn required_number_or_null_field(
    object: &Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<Option<f64>, RenderError> {
    match object.get(field) {
        Some(Value::Null) => Ok(None),
        Some(value) => value.as_f64().map(Some).ok_or_else(|| {
            invalid_text_metrics_extension(format!(
                "{object_name}.{field} must be a number or null"
            ))
        }),
        None => Err(invalid_text_metrics_extension(format!(
            "missing {object_name}.{field}"
        ))),
    }
}

fn validate_default_text_style(default_text_style: &Map<String, Value>) -> Result<(), RenderError> {
    required_string_field(default_text_style, "defaultTextStyle", "font-family")?;
    required_number_field(default_text_style, "defaultTextStyle", "font-size")?;
    required_string_field(default_text_style, "defaultTextStyle", "font-style")?;
    required_string_field(default_text_style, "defaultTextStyle", "font-weight")?;
    required_number_field(default_text_style, "defaultTextStyle", "line-height")?;
    Ok(())
}

fn validate_layout_text(layout_text: &Map<String, Value>) -> Result<(), RenderError> {
    required_number_field(layout_text, "layoutText", "node-padding-x")?;
    required_number_field(layout_text, "layoutText", "node-padding-y")?;
    required_number_field(layout_text, "layoutText", "label-padding-x")?;
    required_number_field(layout_text, "layoutText", "label-padding-y")?;
    required_number_or_null_field(layout_text, "layoutText", "edge-label-max-width")?;
    Ok(())
}

fn ensure_requested_profile_matches_replay(
    requested_profile: Option<&str>,
    replay_profile: &str,
) -> Result<(), RenderError> {
    if let Some(requested_profile) = requested_profile
        && requested_profile != replay_profile
    {
        return Err(RenderError {
            message: format!(
                "font metrics profile '{requested_profile}' does not match MMDS replay profile '{replay_profile}'"
            ),
        });
    }

    Ok(())
}

fn optional_number_field(
    layout_text: Option<&Map<String, Value>>,
    field: &str,
    default: f64,
) -> Result<f64, RenderError> {
    let Some(value) = layout_text.and_then(|layout_text| layout_text.get(field)) else {
        return Ok(default);
    };

    value.as_f64().ok_or_else(|| {
        invalid_text_metrics_extension(format!("layoutText.{field} must be a number"))
    })
}

fn optional_edge_label_max_width(
    layout_text: Option<&Map<String, Value>>,
) -> Result<Option<f64>, RenderError> {
    let Some(value) = layout_text.and_then(|layout_text| layout_text.get("edge-label-max-width"))
    else {
        return Ok(Some(DEFAULT_EDGE_LABEL_MAX_WIDTH));
    };

    match value {
        Value::Null => Ok(None),
        value => value.as_f64().map(Some).ok_or_else(|| {
            invalid_text_metrics_extension(
                "layoutText.edge-label-max-width must be a number or null",
            )
        }),
    }
}

fn invalid_text_metrics_extension(message: impl Into<String>) -> RenderError {
    RenderError {
        message: format!("invalid text metrics extension: {}", message.into()),
    }
}

fn svg_options_with_text_metrics(
    svg_options: &SvgRenderOptions,
    descriptor: &TextMetricsProfileDescriptor,
) -> SvgRenderOptions {
    let mut options = svg_options.clone();
    options.font_family = descriptor.default_text_style.font_family.clone();
    options.font_size = descriptor.default_text_style.font_size;
    options.node_padding_x = descriptor.layout_text.node_padding_x;
    options.node_padding_y = descriptor.layout_text.node_padding_y;
    options
}

fn display_error(error: impl Display) -> RenderError {
    RenderError {
        message: error.to_string(),
    }
}

fn prefixed_display_error(prefix: &str, error: impl Display) -> RenderError {
    RenderError {
        message: format!("{prefix}: {error}"),
    }
}

fn strip_routed_fields(payload: &Document) -> Document {
    let mut output = payload.clone();
    output.geometry_level = crate::graph::GeometryLevel::Layout;
    for edge in &mut output.edges {
        edge.path = None;
        edge.label_position = None;
        edge.is_backward = None;
        edge.source_port = None;
        edge.target_port = None;
        edge.label_rect = None;
    }
    for subgraph in &mut output.subgraphs {
        subgraph.bounds = None;
    }
    output
}
