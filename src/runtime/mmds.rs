//! MMDS replay rendering: hydrate to graph-family IR and re-render.
//!
//! The mmds module owns the interchange format (parse, hydrate, serialize).
//! This module owns the render dispatch for MMDS input.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;

use serde_json::{Map, Value};

use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::GeometryLevel;
use crate::graph::measure::{
    GraphTextStyleKey, LEGACY_MMDS_TEXT_METRICS_PROFILE_ID, ResolvedTextMetrics,
    TextMetricsLayoutDescriptor, TextMetricsProfileConfig, TextMetricsProfileDescriptor,
    TextMetricsProvider, TextMetricsStyleDescriptor, resolve_text_metrics_profile,
};
use crate::mmds::{
    Document, TEXT_MEASUREMENTS_EXTENSION_NAMESPACE, TEXT_METRICS_EXTENSION_NAMESPACE,
    from_document, generate_mermaid, hydrate_graph_geometry_from_document_with_diagram,
    hydrate_routed_geometry_from_document_with_provider, parse_input, resolve_logical_diagram_id,
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

#[cfg(feature = "unstable-text-metrics-provider")]
pub(in crate::runtime) fn render_input_with_dynamic_text_metrics(
    input: &str,
    format: OutputFormat,
    svg_options: &SvgRenderOptions,
    svg_theme: Option<&ResolvedSvgTheme>,
    provider_descriptor: &TextMetricsProfileDescriptor,
    provider: &dyn TextMetricsProvider,
) -> Result<String, RenderError> {
    let payload =
        parse_input(input).map_err(|error| prefixed_display_error("parse error", error))?;
    render_document_with_dynamic_text_metrics(
        &payload,
        format,
        svg_options,
        svg_theme,
        provider_descriptor,
        provider,
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
        validate_text_metrics_extension_shape(payload)?;
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
        resolve_text_metrics_for_replay(payload, format, requested_text_metrics_profile)?;

    match replay_text_metrics {
        ReplayTextMetrics::Static {
            resolved,
            from_extension,
        } => render_document_with_replay_provider(
            payload,
            format,
            text_options,
            svg_options,
            svg_theme,
            diagram_id,
            &resolved.descriptor,
            &resolved.metrics,
            from_extension,
        ),
        ReplayTextMetrics::Dynamic {
            descriptor,
            measurements,
        } => {
            let provider = PersistedTextMeasurementsProvider::new(&descriptor, &measurements);
            let output = render_document_with_replay_provider(
                payload,
                format,
                text_options,
                svg_options,
                svg_theme,
                diagram_id,
                &descriptor,
                &provider,
                true,
            )?;
            provider.finish()?;
            Ok(output)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_document_with_replay_provider(
    payload: &Document,
    format: OutputFormat,
    text_options: &TextRenderOptions,
    svg_options: &SvgRenderOptions,
    svg_theme: Option<&ResolvedSvgTheme>,
    diagram_id: &str,
    text_metrics_descriptor: &TextMetricsProfileDescriptor,
    text_metrics: &dyn TextMetricsProvider,
    from_extension: bool,
) -> Result<String, RenderError> {
    let mut diagram = from_document(payload).map_err(display_error)?;
    let has_routed_geometry = payload.geometry_level == GeometryLevel::Routed;

    // MMDS replay path runs the wrap pass so the hydrated graph's edge labels
    // carry the same `wrapped_label_lines` artifact the original render would
    // have. `wrapped_label_lines` is
    // `#[serde(skip)]` on the Edge, so round-tripping through MMDS drops
    // it; rehydrating it here keeps the SVG/text replay in lockstep with
    // the direct runtime render. New MMDS payloads persist the metrics
    // identity and layout-time values under the text-metrics extension; older
    // payloads fall back to the compatibility profile.
    crate::graph::label_wrap::prepare_wrapped_labels_with_provider(
        &mut diagram.edges,
        text_metrics,
        text_metrics_descriptor.layout_text.edge_label_max_width,
    );

    let geometry = hydrate_graph_geometry_from_document_with_diagram(payload, &diagram)
        .map_err(display_error)?;
    let routed = has_routed_geometry
        .then(|| hydrate_routed_geometry_from_document_with_provider(payload, text_metrics))
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
            let replay_svg_options = from_extension
                .then(|| svg_options_with_text_metrics(svg_options, text_metrics_descriptor));
            let svg_options = replay_svg_options.as_ref().unwrap_or(svg_options);

            Ok(match routed.as_ref() {
                Some(routed) => render_svg_from_routed_geometry_with_theme_and_metrics(
                    &diagram,
                    routed,
                    svg_options,
                    svg_theme,
                    text_metrics,
                ),
                None => render_svg_from_geometry_with_theme_routing_and_metrics(
                    &diagram,
                    &geometry,
                    svg_options,
                    edge_routing_from_style(svg_options.routing_style),
                    svg_theme,
                    text_metrics,
                ),
            })
        }
        _ => Err(RenderError {
            message: format!("{format} output is not supported for {diagram_id} diagrams"),
        }),
    }
}

#[cfg(feature = "unstable-text-metrics-provider")]
fn render_document_with_dynamic_text_metrics(
    payload: &Document,
    format: OutputFormat,
    svg_options: &SvgRenderOptions,
    svg_theme: Option<&ResolvedSvgTheme>,
    provider_descriptor: &TextMetricsProfileDescriptor,
    provider: &dyn TextMetricsProvider,
) -> Result<String, RenderError> {
    if !matches!(format, OutputFormat::Svg) {
        return Err(RenderError {
            message: format!(
                "dynamic text metrics MMDS replay only supports SVG output (requested {format})"
            ),
        });
    }

    let _diagram_id = resolve_logical_diagram_id(payload)?;
    let persisted_descriptor = dynamic_text_metrics_descriptor_for_replay(payload)?;
    ensure_persisted_descriptor_matches_dynamic_provider(
        &persisted_descriptor,
        provider_descriptor,
    )?;
    if let Some(measurements_extension) = payload
        .extensions
        .get(TEXT_MEASUREMENTS_EXTENSION_NAMESPACE)
    {
        parse_text_measurements_for_descriptor(measurements_extension, &persisted_descriptor)?;
    }

    let has_routed_geometry = payload.geometry_level == GeometryLevel::Routed;
    let mut diagram = from_document(payload).map_err(display_error)?;
    crate::graph::label_wrap::prepare_wrapped_labels_with_provider(
        &mut diagram.edges,
        provider,
        provider_descriptor.layout_text.edge_label_max_width,
    );

    let geometry = hydrate_graph_geometry_from_document_with_diagram(payload, &diagram)
        .map_err(display_error)?;
    let routed = has_routed_geometry
        .then(|| hydrate_routed_geometry_from_document_with_provider(payload, provider))
        .transpose()
        .map_err(display_error)?;
    let options = svg_options_with_text_metrics(svg_options, provider_descriptor);

    Ok(match routed.as_ref() {
        Some(routed) => render_svg_from_routed_geometry_with_theme_and_metrics(
            &diagram, routed, &options, svg_theme, provider,
        ),
        None => render_svg_from_geometry_with_theme_routing_and_metrics(
            &diagram,
            &geometry,
            &options,
            edge_routing_from_style(options.routing_style),
            svg_theme,
            provider,
        ),
    })
}

fn is_shared_coordinates_view(payload: &Document) -> bool {
    payload
        .extensions
        .get(VIEW_EXTENSION_NAMESPACE)
        .and_then(|extension| extension.get("layout_mode"))
        .and_then(serde_json::Value::as_str)
        == Some("shared_coordinates")
}

enum ReplayTextMetrics {
    Static {
        resolved: ResolvedTextMetrics,
        from_extension: bool,
    },
    Dynamic {
        descriptor: TextMetricsProfileDescriptor,
        measurements: PersistedTextMeasurements,
    },
}

#[derive(Debug)]
struct PersistedTextMeasurements {
    default_style: GraphTextStyleKey,
    style_ids_by_key: HashMap<GraphTextStyleKey, String>,
    line_widths: HashMap<(String, String), f64>,
    scalar_widths: HashMap<(String, char), f64>,
}

struct ParsedTextMeasurementStyles {
    default_style: GraphTextStyleKey,
    ids_by_key: HashMap<GraphTextStyleKey, String>,
    ids: HashSet<String>,
}

struct PersistedTextMeasurementsProvider<'a> {
    descriptor: &'a TextMetricsProfileDescriptor,
    measurements: &'a PersistedTextMeasurements,
    first_error: RefCell<Option<RenderError>>,
}

impl<'a> PersistedTextMeasurementsProvider<'a> {
    fn new(
        descriptor: &'a TextMetricsProfileDescriptor,
        measurements: &'a PersistedTextMeasurements,
    ) -> Self {
        Self {
            descriptor,
            measurements,
            first_error: RefCell::new(None),
        }
    }

    fn finish(&self) -> Result<(), RenderError> {
        match self.first_error.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn record_error(&self, error: RenderError) {
        let mut first_error = self.first_error.borrow_mut();
        if first_error.is_none() {
            *first_error = Some(error);
        }
    }
}

impl TextMetricsProvider for PersistedTextMeasurementsProvider<'_> {
    fn measure_line_width(&self, text: &str) -> f64 {
        self.measure_line_width_for_style(&self.measurements.default_style, text)
    }

    fn measure_scalar_width(&self, ch: char) -> f64 {
        self.measure_scalar_width_for_style(&self.measurements.default_style, ch)
    }

    fn default_text_style_key(&self) -> GraphTextStyleKey {
        self.measurements.default_style.clone()
    }

    fn measure_line_width_for_style(&self, style: &GraphTextStyleKey, text: &str) -> f64 {
        let Some(style_id) = self.measurements.style_ids_by_key.get(style) else {
            self.record_error(missing_persisted_text_measurement_style(
                "lineWidths",
                style,
            ));
            return 0.0;
        };
        match self
            .measurements
            .line_widths
            .get(&(style_id.clone(), text.to_string()))
            .copied()
        {
            Some(width) => width,
            None => {
                self.record_error(missing_persisted_text_measurement(
                    "lineWidths",
                    style_id,
                    text,
                ));
                0.0
            }
        }
    }

    fn measure_scalar_width_for_style(&self, style: &GraphTextStyleKey, ch: char) -> f64 {
        let Some(style_id) = self.measurements.style_ids_by_key.get(style) else {
            self.record_error(missing_persisted_text_measurement_style(
                "scalarWidths",
                style,
            ));
            return 0.0;
        };
        let mut buffer = [0_u8; 4];
        let text = ch.encode_utf8(&mut buffer);
        match self
            .measurements
            .scalar_widths
            .get(&(style_id.clone(), ch))
            .copied()
        {
            Some(width) => width,
            None => {
                self.record_error(missing_persisted_text_measurement(
                    "scalarWidths",
                    style_id,
                    text,
                ));
                0.0
            }
        }
    }

    fn font_size(&self) -> f64 {
        self.descriptor.default_text_style.font_size
    }

    fn line_height(&self) -> f64 {
        self.descriptor.default_text_style.line_height
    }

    fn node_padding_x(&self) -> f64 {
        self.descriptor.layout_text.node_padding_x
    }

    fn node_padding_y(&self) -> f64 {
        self.descriptor.layout_text.node_padding_y
    }

    fn label_padding_x(&self) -> f64 {
        self.descriptor.layout_text.label_padding_x
    }

    fn label_padding_y(&self) -> f64 {
        self.descriptor.layout_text.label_padding_y
    }
}

fn resolve_text_metrics_for_replay(
    payload: &Document,
    format: OutputFormat,
    requested_text_metrics_profile: Option<&str>,
) -> Result<ReplayTextMetrics, RenderError> {
    let Some(extension) = payload.extensions.get(TEXT_METRICS_EXTENSION_NAMESPACE) else {
        ensure_requested_profile_matches_replay(
            requested_text_metrics_profile,
            LEGACY_MMDS_TEXT_METRICS_PROFILE_ID,
        )?;
        return resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some(LEGACY_MMDS_TEXT_METRICS_PROFILE_ID),
            ..TextMetricsProfileConfig::default()
        })
        .map(|resolved| ReplayTextMetrics::Static {
            resolved,
            from_extension: false,
        })
        .map_err(display_error);
    };

    let persisted_descriptor = parse_text_metrics_descriptor(extension)?;
    let profile_id = persisted_descriptor.profile_id.as_str();
    if persisted_descriptor.source == "dynamic" {
        ensure_requested_profile_matches_replay(requested_text_metrics_profile, profile_id)?;
        if !matches!(format, OutputFormat::Svg) {
            return Err(dynamic_text_metrics_provider_required(profile_id));
        }
        let Some(measurements_extension) = payload
            .extensions
            .get(TEXT_MEASUREMENTS_EXTENSION_NAMESPACE)
        else {
            return Err(dynamic_text_metrics_provider_required(profile_id));
        };
        let measurements =
            parse_text_measurements_for_descriptor(measurements_extension, &persisted_descriptor)?;
        return Ok(ReplayTextMetrics::Dynamic {
            descriptor: persisted_descriptor,
            measurements,
        });
    }
    ensure_requested_profile_matches_replay(requested_text_metrics_profile, profile_id)?;

    let layout_text = &persisted_descriptor.layout_text;
    let resolved = resolve_text_metrics_profile(TextMetricsProfileConfig {
        profile_id: Some(profile_id),
        node_padding_x: layout_text.node_padding_x,
        node_padding_y: layout_text.node_padding_y,
        edge_label_max_width: layout_text.edge_label_max_width,
    })
    .map_err(display_error)?;
    ensure_persisted_descriptor_matches_static_profile(
        &persisted_descriptor,
        &resolved.descriptor,
    )?;

    Ok(ReplayTextMetrics::Static {
        resolved,
        from_extension: true,
    })
}

fn validate_text_metrics_extension_shape(payload: &Document) -> Result<(), RenderError> {
    let Some(extension) = payload.extensions.get(TEXT_METRICS_EXTENSION_NAMESPACE) else {
        if payload
            .extensions
            .contains_key(TEXT_MEASUREMENTS_EXTENSION_NAMESPACE)
        {
            return Err(invalid_text_measurements_extension(format!(
                "requires sibling {TEXT_METRICS_EXTENSION_NAMESPACE}"
            )));
        }
        return Ok(());
    };

    let descriptor = parse_text_metrics_descriptor(extension)?;
    if let Some(measurements) = payload
        .extensions
        .get(TEXT_MEASUREMENTS_EXTENSION_NAMESPACE)
    {
        parse_text_measurements_for_descriptor(measurements, &descriptor)?;
    }
    Ok(())
}

#[cfg(feature = "unstable-text-metrics-provider")]
fn dynamic_text_metrics_descriptor_for_replay(
    payload: &Document,
) -> Result<TextMetricsProfileDescriptor, RenderError> {
    let extension = payload
        .extensions
        .get(TEXT_METRICS_EXTENSION_NAMESPACE)
        .ok_or_else(|| invalid_text_metrics_extension("missing dynamic text metrics extension"))?;
    let descriptor = parse_text_metrics_descriptor(extension)?;
    if descriptor.source != "dynamic" {
        return Err(invalid_text_metrics_extension(format!(
            "metricsProfile.source {:?} is not dynamic",
            descriptor.source
        )));
    }
    Ok(descriptor)
}

fn dynamic_text_metrics_provider_required(profile_id: &str) -> RenderError {
    RenderError {
        message: format!(
            "dynamic text metrics profile '{profile_id}' requires a matching provider for MMDS replay"
        ),
    }
}

fn parse_text_metrics_descriptor(
    extension: &Map<String, Value>,
) -> Result<TextMetricsProfileDescriptor, RenderError> {
    let metrics_profile = required_object(extension, "metricsProfile")?;
    let profile_id = metrics_profile
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_text_metrics_extension("missing metricsProfile.id"))?;
    let source = required_string_field(metrics_profile, "metricsProfile", "source")?;
    let version = required_integer_field(metrics_profile, "metricsProfile", "version")?;

    let default_text_style = required_object(extension, "defaultTextStyle")?;
    validate_default_text_style(default_text_style)?;
    let layout_text = required_object(extension, "layoutText")?;
    validate_layout_text(layout_text)?;

    Ok(TextMetricsProfileDescriptor {
        profile_id: profile_id.to_string(),
        source: source.to_string(),
        version,
        default_text_style: TextMetricsStyleDescriptor {
            font_family: required_string_field(
                default_text_style,
                "defaultTextStyle",
                "font-family",
            )?
            .to_string(),
            font_size: required_number_field(default_text_style, "defaultTextStyle", "font-size")?,
            font_style: required_string_field(
                default_text_style,
                "defaultTextStyle",
                "font-style",
            )?
            .to_string(),
            font_weight: required_string_field(
                default_text_style,
                "defaultTextStyle",
                "font-weight",
            )?
            .to_string(),
            line_height: required_number_field(
                default_text_style,
                "defaultTextStyle",
                "line-height",
            )?,
        },
        layout_text: TextMetricsLayoutDescriptor {
            node_padding_x: required_number_field(layout_text, "layoutText", "node-padding-x")?,
            node_padding_y: required_number_field(layout_text, "layoutText", "node-padding-y")?,
            label_padding_x: required_number_field(layout_text, "layoutText", "label-padding-x")?,
            label_padding_y: required_number_field(layout_text, "layoutText", "label-padding-y")?,
            edge_label_max_width: required_number_or_null_field(
                layout_text,
                "layoutText",
                "edge-label-max-width",
            )?,
        },
    })
}

fn parse_text_measurements_for_descriptor(
    extension: &Map<String, Value>,
    descriptor: &TextMetricsProfileDescriptor,
) -> Result<PersistedTextMeasurements, RenderError> {
    if descriptor.source != "dynamic" {
        return Err(invalid_text_measurements_extension(format!(
            "profileRef.source \"dynamic\" does not match sibling text metrics source {:?}",
            descriptor.source
        )));
    }

    let profile_ref = required_measurements_object(extension, "profileRef")?;
    let profile_id = required_measurements_string_field(profile_ref, "profileRef", "id")?;
    ensure_text_measurements_string_ref(
        "profileRef.id",
        profile_id,
        descriptor.profile_id.as_str(),
    )?;
    let source = required_measurements_string_field(profile_ref, "profileRef", "source")?;
    ensure_text_measurements_string_ref("profileRef.source", source, descriptor.source.as_str())?;
    let version = required_measurements_integer_field(profile_ref, "profileRef", "version")?;
    if version != descriptor.version {
        return Err(invalid_text_measurements_extension(format!(
            "profileRef.version {version} does not match sibling text metrics profile version {}",
            descriptor.version
        )));
    }

    let styles = parse_measurement_text_styles(extension, descriptor)?;
    let line_widths = parse_line_width_entries(extension, &styles.ids)?;
    let scalar_widths = parse_scalar_width_entries(extension, &styles.ids)?;

    Ok(PersistedTextMeasurements {
        default_style: styles.default_style,
        style_ids_by_key: styles.ids_by_key,
        line_widths,
        scalar_widths,
    })
}

fn parse_measurement_text_styles(
    extension: &Map<String, Value>,
    descriptor: &TextMetricsProfileDescriptor,
) -> Result<ParsedTextMeasurementStyles, RenderError> {
    let entries = required_measurements_array(extension, "textStyles")?;
    let default_style = GraphTextStyleKey::from_descriptor(&descriptor.default_text_style)
        .map_err(invalid_text_measurements_extension)?;
    let mut style_ids = HashSet::new();
    let mut style_ids_by_key = HashMap::new();
    let mut has_default_style = false;
    for (index, entry) in entries.iter().enumerate() {
        let object = entry.as_object().ok_or_else(|| {
            invalid_text_measurements_extension(format!("textStyles[{index}] must be an object"))
        })?;
        ensure_text_style_entry_fields(object, index)?;
        let id = required_measurements_string_field(object, "textStyles", "id")?;
        if id.trim().is_empty() {
            return Err(invalid_text_measurements_extension(format!(
                "textStyles[{index}].id must not be empty"
            )));
        }
        if !style_ids.insert(id.to_string()) {
            return Err(invalid_text_measurements_extension(format!(
                "duplicate textStyles id {id:?}"
            )));
        }
        let style_key = GraphTextStyleKey::new(
            required_measurements_string_field(object, "textStyles", "fontFamily")?,
            required_measurements_number_field(object, "textStyles", "fontSize")?,
            required_measurements_number_field(object, "textStyles", "lineHeight")?,
            required_measurements_string_field(object, "textStyles", "fontStyle")?,
            required_measurements_string_field(object, "textStyles", "fontWeight")?,
        )
        .map_err(|message| {
            invalid_text_measurements_extension(format!(
                "textStyles[{index}] is invalid: {message}"
            ))
        })?;
        validate_non_empty_measurement_string(object, "textStyles", "cssFont", index)?;
        has_default_style |= style_key == default_style;
        if style_ids_by_key.insert(style_key, id.to_string()).is_some() {
            return Err(invalid_text_measurements_extension(format!(
                "duplicate textStyles style identity for id {id:?}"
            )));
        }
    }
    if !has_default_style {
        return Err(invalid_text_measurements_extension(
            "textStyles must include the sibling defaultTextStyle".to_string(),
        ));
    }
    Ok(ParsedTextMeasurementStyles {
        default_style,
        ids_by_key: style_ids_by_key,
        ids: style_ids,
    })
}

fn parse_line_width_entries(
    extension: &Map<String, Value>,
    known_styles: &HashSet<String>,
) -> Result<HashMap<(String, String), f64>, RenderError> {
    let entries = required_measurements_array(extension, "lineWidths")?;
    let mut widths = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let object = entry.as_object().ok_or_else(|| {
            invalid_text_measurements_extension(format!("lineWidths[{index}] must be an object"))
        })?;
        ensure_measurement_entry_fields(object, "lineWidths", index)?;
        let style = required_measurement_style_field(object, "lineWidths", index, known_styles)?;
        let text = required_measurements_string_field(object, "lineWidths", "text")?;
        let width = required_measurements_width(object, "lineWidths", index)?;
        if widths
            .insert((style.to_string(), text.to_string()), width)
            .is_some()
        {
            return Err(invalid_text_measurements_extension(format!(
                "duplicate lineWidths style {style:?} text {text:?}"
            )));
        }
    }
    Ok(widths)
}

fn parse_scalar_width_entries(
    extension: &Map<String, Value>,
    known_styles: &HashSet<String>,
) -> Result<HashMap<(String, char), f64>, RenderError> {
    let entries = required_measurements_array(extension, "scalarWidths")?;
    let mut widths = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let object = entry.as_object().ok_or_else(|| {
            invalid_text_measurements_extension(format!("scalarWidths[{index}] must be an object"))
        })?;
        ensure_measurement_entry_fields(object, "scalarWidths", index)?;
        let style = required_measurement_style_field(object, "scalarWidths", index, known_styles)?;
        let text = required_measurements_string_field(object, "scalarWidths", "text")?;
        let mut chars = text.chars();
        let Some(ch) = chars.next() else {
            return Err(invalid_text_measurements_extension(format!(
                "scalarWidths[{index}].text must contain exactly one Unicode scalar"
            )));
        };
        if chars.next().is_some() {
            return Err(invalid_text_measurements_extension(format!(
                "scalarWidths[{index}].text must contain exactly one Unicode scalar"
            )));
        }
        let width = required_measurements_width(object, "scalarWidths", index)?;
        if widths.insert((style.to_string(), ch), width).is_some() {
            return Err(invalid_text_measurements_extension(format!(
                "duplicate scalarWidths style {style:?} text {text:?}"
            )));
        }
    }
    Ok(widths)
}

fn ensure_measurement_entry_fields(
    object: &Map<String, Value>,
    array_name: &str,
    index: usize,
) -> Result<(), RenderError> {
    for field in object.keys() {
        if field != "style" && field != "text" && field != "width" {
            return Err(invalid_text_measurements_extension(format!(
                "{array_name}[{index}] contains unsupported field {field:?}"
            )));
        }
    }
    Ok(())
}

fn ensure_text_style_entry_fields(
    object: &Map<String, Value>,
    index: usize,
) -> Result<(), RenderError> {
    for field in object.keys() {
        if !matches!(
            field.as_str(),
            "id" | "fontFamily"
                | "fontSize"
                | "fontStyle"
                | "fontWeight"
                | "lineHeight"
                | "cssFont"
        ) {
            return Err(invalid_text_measurements_extension(format!(
                "textStyles[{index}] contains unsupported field {field:?}"
            )));
        }
    }
    Ok(())
}

fn required_measurements_object<'a>(
    extension: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Map<String, Value>, RenderError> {
    extension
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_text_measurements_extension(format!("missing {field}")))
}

fn required_measurements_array<'a>(
    extension: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Vec<Value>, RenderError> {
    extension
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_text_measurements_extension(format!("missing {field}")))
}

fn required_measurements_string_field<'a>(
    object: &'a Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<&'a str, RenderError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid_text_measurements_extension(format!("{object_name}.{field} must be a string"))
    })
}

fn required_measurements_number_field(
    object: &Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<f64, RenderError> {
    let value = object.get(field).and_then(Value::as_f64).ok_or_else(|| {
        invalid_text_measurements_extension(format!("{object_name}.{field} must be a number"))
    })?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid_text_measurements_extension(format!(
            "{object_name}.{field} must be finite"
        )))
    }
}

fn validate_non_empty_measurement_string(
    object: &Map<String, Value>,
    object_name: &str,
    field: &str,
    index: usize,
) -> Result<(), RenderError> {
    let value = required_measurements_string_field(object, object_name, field)?;
    if value.trim().is_empty() {
        return Err(invalid_text_measurements_extension(format!(
            "{object_name}[{index}].{field} must not be empty"
        )));
    }
    Ok(())
}

fn required_measurement_style_field<'a>(
    object: &'a Map<String, Value>,
    array_name: &str,
    index: usize,
    known_styles: &HashSet<String>,
) -> Result<&'a str, RenderError> {
    let style = required_measurements_string_field(object, array_name, "style")?;
    if style.trim().is_empty() {
        return Err(invalid_text_measurements_extension(format!(
            "{array_name}[{index}].style must not be empty"
        )));
    }
    if !known_styles.contains(style) {
        return Err(invalid_text_measurements_extension(format!(
            "{array_name}[{index}].style {style:?} is not present in textStyles"
        )));
    }
    Ok(style)
}

fn required_measurements_integer_field(
    object: &Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<u32, RenderError> {
    let Some(number) = object.get(field).and_then(Value::as_number) else {
        return Err(invalid_text_measurements_extension(format!(
            "{object_name}.{field} must be an integer"
        )));
    };

    number
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            invalid_text_measurements_extension(format!("{object_name}.{field} must be an integer"))
        })
}

fn required_measurements_width(
    object: &Map<String, Value>,
    array_name: &str,
    index: usize,
) -> Result<f64, RenderError> {
    let width = object.get("width").and_then(Value::as_f64).ok_or_else(|| {
        invalid_text_measurements_extension(format!("{array_name}[{index}].width must be a number"))
    })?;
    if width < 0.0 || !width.is_finite() {
        return Err(invalid_text_measurements_extension(format!(
            "{array_name}[{index}].width must be finite and non-negative"
        )));
    }
    Ok(width)
}

fn ensure_text_measurements_string_ref(
    field: &str,
    actual: &str,
    expected: &str,
) -> Result<(), RenderError> {
    if actual == expected {
        return Ok(());
    }
    Err(invalid_text_measurements_extension(format!(
        "{field} {actual:?} does not match sibling text metrics profile expected {expected:?}"
    )))
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
) -> Result<u32, RenderError> {
    let Some(number) = object.get(field).and_then(Value::as_number) else {
        return Err(invalid_text_metrics_extension(format!(
            "{object_name}.{field} must be an integer"
        )));
    };

    number
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            invalid_text_metrics_extension(format!("{object_name}.{field} must be an integer"))
        })
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

fn ensure_persisted_descriptor_matches_static_profile(
    persisted: &TextMetricsProfileDescriptor,
    expected: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    ensure_persisted_descriptor_matches_profile(persisted, expected, false)
}

#[cfg(feature = "unstable-text-metrics-provider")]
fn ensure_persisted_descriptor_matches_dynamic_provider(
    persisted: &TextMetricsProfileDescriptor,
    expected: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    ensure_persisted_descriptor_matches_profile(persisted, expected, true)
}

fn ensure_persisted_descriptor_matches_profile(
    persisted: &TextMetricsProfileDescriptor,
    expected: &TextMetricsProfileDescriptor,
    include_document_owned_layout: bool,
) -> Result<(), RenderError> {
    ensure_string_descriptor_field(
        "metricsProfile.id",
        &persisted.profile_id,
        &expected.profile_id,
        expected,
    )?;
    ensure_string_descriptor_field(
        "metricsProfile.source",
        &persisted.source,
        &expected.source,
        expected,
    )?;
    ensure_u32_descriptor_field(
        "metricsProfile.version",
        persisted.version,
        expected.version,
        expected,
    )?;
    ensure_string_descriptor_field(
        "defaultTextStyle.font-family",
        &persisted.default_text_style.font_family,
        &expected.default_text_style.font_family,
        expected,
    )?;
    ensure_f64_descriptor_field(
        "defaultTextStyle.font-size",
        persisted.default_text_style.font_size,
        expected.default_text_style.font_size,
        expected,
    )?;
    ensure_string_descriptor_field(
        "defaultTextStyle.font-style",
        &persisted.default_text_style.font_style,
        &expected.default_text_style.font_style,
        expected,
    )?;
    ensure_string_descriptor_field(
        "defaultTextStyle.font-weight",
        &persisted.default_text_style.font_weight,
        &expected.default_text_style.font_weight,
        expected,
    )?;
    ensure_f64_descriptor_field(
        "defaultTextStyle.line-height",
        persisted.default_text_style.line_height,
        expected.default_text_style.line_height,
        expected,
    )?;
    ensure_f64_descriptor_field(
        "layoutText.label-padding-x",
        persisted.layout_text.label_padding_x,
        expected.layout_text.label_padding_x,
        expected,
    )?;
    ensure_f64_descriptor_field(
        "layoutText.label-padding-y",
        persisted.layout_text.label_padding_y,
        expected.layout_text.label_padding_y,
        expected,
    )?;
    if include_document_owned_layout {
        ensure_f64_descriptor_field(
            "layoutText.node-padding-x",
            persisted.layout_text.node_padding_x,
            expected.layout_text.node_padding_x,
            expected,
        )?;
        ensure_f64_descriptor_field(
            "layoutText.node-padding-y",
            persisted.layout_text.node_padding_y,
            expected.layout_text.node_padding_y,
            expected,
        )?;
        ensure_optional_f64_descriptor_field(
            "layoutText.edge-label-max-width",
            persisted.layout_text.edge_label_max_width,
            expected.layout_text.edge_label_max_width,
            expected,
        )?;
    }
    Ok(())
}

fn ensure_string_descriptor_field(
    field: &str,
    actual: &str,
    expected_value: &str,
    expected: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    if actual == expected_value {
        return Ok(());
    }
    Err(descriptor_mismatch_error(
        field,
        format!("{actual:?}"),
        format!("{expected_value:?}"),
        expected,
    ))
}

fn ensure_u32_descriptor_field(
    field: &str,
    actual: u32,
    expected_value: u32,
    expected: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    if actual == expected_value {
        return Ok(());
    }
    Err(descriptor_mismatch_error(
        field,
        actual.to_string(),
        expected_value.to_string(),
        expected,
    ))
}

fn ensure_f64_descriptor_field(
    field: &str,
    actual: f64,
    expected_value: f64,
    expected: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    if (actual - expected_value).abs() <= 1e-9 {
        return Ok(());
    }
    Err(descriptor_mismatch_error(
        field,
        actual.to_string(),
        expected_value.to_string(),
        expected,
    ))
}

fn ensure_optional_f64_descriptor_field(
    field: &str,
    actual: Option<f64>,
    expected_value: Option<f64>,
    expected: &TextMetricsProfileDescriptor,
) -> Result<(), RenderError> {
    match (actual, expected_value) {
        (Some(actual), Some(expected_value)) if (actual - expected_value).abs() <= 1e-9 => Ok(()),
        (None, None) => Ok(()),
        _ => Err(descriptor_mismatch_error(
            field,
            format!("{actual:?}"),
            format!("{expected_value:?}"),
            expected,
        )),
    }
}

fn descriptor_mismatch_error(
    field: &str,
    actual: String,
    expected_value: String,
    expected: &TextMetricsProfileDescriptor,
) -> RenderError {
    invalid_text_metrics_extension(format!(
        "{field} {actual} does not match text metrics profile '{}' expected {expected_value}",
        expected.profile_id
    ))
}

fn invalid_text_metrics_extension(message: impl Into<String>) -> RenderError {
    RenderError {
        message: format!("invalid text metrics extension: {}", message.into()),
    }
}

fn invalid_text_measurements_extension(message: impl Into<String>) -> RenderError {
    RenderError {
        message: format!(
            "invalid {TEXT_MEASUREMENTS_EXTENSION_NAMESPACE}: {}",
            message.into()
        ),
    }
}

fn missing_persisted_text_measurement(kind: &str, style_id: &str, text: &str) -> RenderError {
    RenderError {
        message: format!(
            "missing persisted dynamic text measurement in {TEXT_MEASUREMENTS_EXTENSION_NAMESPACE}.{kind} for style {style_id:?} text {text:?}"
        ),
    }
}

fn missing_persisted_text_measurement_style(kind: &str, style: &GraphTextStyleKey) -> RenderError {
    RenderError {
        message: format!(
            "missing persisted dynamic text measurement style in {TEXT_MEASUREMENTS_EXTENSION_NAMESPACE}.textStyles for {kind} style {style:?}"
        ),
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::graph::measure::{
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID, RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
    };

    const LEGACY_LAYOUT_MMDS: &str = r##"{
      "version": 1,
      "profiles": ["mmds-core-v1"],
      "defaults": {
        "node": { "shape": "rectangle" },
        "edge": {
          "stroke": "solid",
          "arrow_start": "none",
          "arrow_end": "normal",
          "minlen": 1
        }
      },
      "geometry_level": "layout",
      "metadata": {
        "diagram_type": "flowchart",
        "direction": "TD",
        "bounds": { "width": 120.0, "height": 200.0 }
      },
      "nodes": [
        {
          "id": "A",
          "label": "Alpha",
          "position": { "x": 60.0, "y": 35.0 },
          "size": { "width": 99.16, "height": 54.0 }
        },
        {
          "id": "B",
          "label": "Beta",
          "position": { "x": 60.0, "y": 139.0 },
          "size": { "width": 88.0, "height": 54.0 }
        }
      ],
      "edges": [
        { "id": "e0", "source": "A", "target": "B", "label": "mmmm" }
      ]
    }"##;

    #[test]
    fn replay_without_text_metrics_extension_resolves_legacy_compatibility_profile() {
        let document = parse_input(LEGACY_LAYOUT_MMDS).expect("legacy MMDS should parse");
        let replay = resolve_text_metrics_for_replay(&document, OutputFormat::Svg, None)
            .expect("legacy replay metrics should resolve");

        let ReplayTextMetrics::Static {
            resolved,
            from_extension,
        } = replay
        else {
            panic!("legacy replay should resolve static metrics");
        };

        assert_eq!(
            resolved.descriptor.profile_id,
            COMPATIBILITY_TEXT_METRICS_PROFILE_ID
        );
        assert_eq!(resolved.descriptor.source, "heuristic");
        assert!(!from_extension);
    }

    #[test]
    fn replay_without_text_metrics_extension_rejects_recorded_request() {
        let document = parse_input(LEGACY_LAYOUT_MMDS).expect("legacy MMDS should parse");
        let err = match resolve_text_metrics_for_replay(
            &document,
            OutputFormat::Svg,
            Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID),
        ) {
            Ok(_) => panic!("legacy replay should reject recorded profile request"),
            Err(err) => err,
        };

        assert!(
            err.message.contains(&format!(
                "font metrics profile '{}' does not match MMDS replay profile '{}'",
                RECORDED_SANS_TEXT_METRICS_PROFILE_ID, COMPATIBILITY_TEXT_METRICS_PROFILE_ID
            )),
            "{err}"
        );
    }

    fn dynamic_descriptor() -> TextMetricsProfileDescriptor {
        TextMetricsProfileDescriptor {
            profile_id: "browser-test-v1".to_string(),
            source: "dynamic".to_string(),
            version: 1,
            default_text_style: TextMetricsStyleDescriptor {
                font_family: "Inter".to_string(),
                font_size: 16.0,
                font_style: "normal".to_string(),
                font_weight: "400".to_string(),
                line_height: 24.0,
            },
            layout_text: TextMetricsLayoutDescriptor {
                node_padding_x: 15.0,
                node_padding_y: 15.0,
                label_padding_x: 4.0,
                label_padding_y: 2.0,
                edge_label_max_width: Some(200.0),
            },
        }
    }

    fn text_measurements_extension() -> Map<String, Value> {
        json!({
            "profileRef": {
                "id": "browser-test-v1",
                "source": "dynamic",
                "version": 1
            },
            "textStyles": [
                {
                    "id": "s0",
                    "fontFamily": "Inter",
                    "fontSize": 16.0,
                    "fontStyle": "normal",
                    "fontWeight": "400",
                    "lineHeight": 24.0,
                    "cssFont": "16px Inter"
                }
            ],
            "lineWidths": [
                { "style": "s0", "text": "Alpha", "width": 42.31415926535897 },
                { "style": "s0", "text": "é", "width": 12.5 }
            ],
            "scalarWidths": [
                { "style": "s0", "text": "A", "width": 10.671875 },
                { "style": "s0", "text": " ", "width": 4.4453125 },
                { "style": "s0", "text": "é", "width": 12.5 }
            ]
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn text_measurements_extension_profile_ref_must_match_text_metrics_profile() {
        let descriptor = dynamic_descriptor();
        let mut extension = text_measurements_extension();
        extension
            .get_mut("profileRef")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("id".to_string(), json!("other-provider-v1"));

        let err = parse_text_measurements_for_descriptor(&extension, &descriptor)
            .expect_err("profileRef mismatch should fail");

        assert!(err.message.contains("org.mmdflux.text-measurements.v1"));
        assert!(err.message.contains("profileRef.id"), "{err}");
        assert!(err.message.contains("browser-test-v1"), "{err}");
    }

    #[test]
    fn text_measurements_scalar_width_text_must_be_one_unicode_scalar() {
        for text in ["", "AB", "e\u{0301}"] {
            let descriptor = dynamic_descriptor();
            let mut extension = text_measurements_extension();
            extension
                .get_mut("scalarWidths")
                .unwrap()
                .as_array_mut()
                .unwrap()[0]["text"] = json!(text);

            let err = parse_text_measurements_for_descriptor(&extension, &descriptor)
                .expect_err("invalid scalar text should fail");

            assert!(err.message.contains("scalarWidths"), "{err}");
            assert!(err.message.contains("one Unicode scalar"), "{err}");
        }
    }

    #[test]
    fn text_measurements_extension_rejects_duplicate_queries() {
        for array_name in ["lineWidths", "scalarWidths"] {
            let descriptor = dynamic_descriptor();
            let mut extension = text_measurements_extension();
            let entries = extension
                .get_mut(array_name)
                .unwrap()
                .as_array_mut()
                .unwrap();
            entries.push(entries[0].clone());

            let err = parse_text_measurements_for_descriptor(&extension, &descriptor)
                .expect_err("duplicate measurement queries should fail");

            assert!(err.message.contains(array_name), "{err}");
            assert!(err.message.contains("duplicate"), "{err}");
        }
    }

    #[test]
    fn text_measurements_extension_rejects_static_text_metrics_descriptor() {
        let mut descriptor = dynamic_descriptor();
        descriptor.source = "recorded".to_string();
        descriptor.profile_id = RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string();
        let extension = text_measurements_extension();

        let err = parse_text_measurements_for_descriptor(&extension, &descriptor)
            .expect_err("measurement sidecar should be dynamic-only");

        assert!(err.message.contains("org.mmdflux.text-measurements.v1"));
        assert!(err.message.contains("profileRef.source"));
        assert!(err.message.contains("recorded"));
    }

    #[test]
    fn persisted_measurement_widths_round_trip_subpixel_values_exactly() {
        let value = 42.31415926535897_f64;
        let json = serde_json::to_string(&json!({ "width": value })).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["width"].as_f64().unwrap().to_bits(), value.to_bits());
    }

    #[test]
    fn persisted_measurement_provider_does_not_normalize_query_text() {
        let descriptor = dynamic_descriptor();
        let extension = text_measurements_extension();
        let measurements = parse_text_measurements_for_descriptor(&extension, &descriptor).unwrap();
        let provider = PersistedTextMeasurementsProvider::new(&descriptor, &measurements);

        assert_eq!(provider.measure_line_width("é"), 12.5);
        assert_eq!(provider.measure_line_width("e\u{0301}"), 0.0);

        let err = provider
            .finish()
            .expect_err("decomposed form should be a distinct missing query");
        assert!(
            err.message
                .contains("missing persisted dynamic text measurement")
        );
        assert!(err.message.contains("e\\u{301}") || err.message.contains("e\u{301}"));
    }

    #[test]
    fn persisted_measurement_provider_rejects_missing_query() {
        let descriptor = dynamic_descriptor();
        let mut extension = text_measurements_extension();
        extension
            .get_mut("lineWidths")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .retain(|entry| entry["text"] != "Alpha");
        let measurements = parse_text_measurements_for_descriptor(&extension, &descriptor).unwrap();
        let provider = PersistedTextMeasurementsProvider::new(&descriptor, &measurements);

        assert_eq!(provider.measure_line_width("Alpha"), 0.0);
        let err = provider
            .finish()
            .expect_err("missing persisted line width should fail");

        assert!(
            err.message
                .contains("missing persisted dynamic text measurement")
        );
        assert!(err.message.contains("Alpha"), "{err}");
    }

    #[test]
    fn persisted_measurement_provider_rejects_missing_style_keyed_line_query() {
        let descriptor = dynamic_descriptor();
        let mut extension = text_measurements_extension();
        extension
            .get_mut("textStyles")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "s1",
                "fontFamily": "Verdana",
                "fontSize": 8.0,
                "fontStyle": "normal",
                "fontWeight": "400",
                "lineHeight": 12.0,
                "cssFont": "8px Verdana"
            }));
        let measurements = parse_text_measurements_for_descriptor(&extension, &descriptor).unwrap();
        let provider = PersistedTextMeasurementsProvider::new(&descriptor, &measurements);
        let style = GraphTextStyleKey::new("Verdana", 8.0, 12.0, "normal", "400").unwrap();

        assert_eq!(provider.measure_line_width_for_style(&style, "Alpha"), 0.0);
        let err = provider
            .finish()
            .expect_err("missing style-keyed line width should fail");

        assert!(err.message.contains("lineWidths"), "{err}");
        assert!(err.message.contains("s1"), "{err}");
        assert!(err.message.contains("Alpha"), "{err}");
    }
}
