//! Experimental dynamic text metrics contract for callback-backed adapters.
//!
//! This module is feature-gated and doc-hidden because its callback-backed
//! provider API is not part of the stable Rust facade.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::Deserialize;

use crate::builtins::default_registry;
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::frontends::{InputFrontend, detect_input_frontend};
use crate::graph::measure::{
    DEFAULT_LABEL_PADDING_X, DEFAULT_LABEL_PADDING_Y, DEFAULT_PROPORTIONAL_NODE_PADDING_X,
    DEFAULT_PROPORTIONAL_NODE_PADDING_Y, GraphTextStyleKey, TextMeasurementCache,
    TextMeasurementStyle, TextMetricsLayoutDescriptor, TextMetricsProfileDescriptor,
    TextMetricsProvider, TextMetricsStyleDescriptor,
};
use crate::payload::Diagram;
use crate::registry::DiagramFamily;
use crate::render::graph::SvgRenderOptions;
use crate::runtime::config::RenderConfig;
use crate::runtime::config_input::apply_svg_surface_defaults;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicMetricsInput {
    pub default_style: String,
    pub text_styles: Vec<DynamicTextStyleInput>,
    pub profile_id: Option<String>,
    pub profile_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicTextStyleInput {
    pub id: String,
    pub font_family: String,
    pub font_size: f64,
    pub font_style: String,
    pub font_weight: String,
    pub line_height: f64,
    pub css_font: String,
}

impl DynamicMetricsInput {
    pub fn validate(&self) -> Result<(), RenderError> {
        validate_non_empty("defaultStyle", &self.default_style)?;
        if self.text_styles.is_empty() {
            return Err(RenderError {
                message: "dynamic text metrics field `textStyles` must not be empty".to_string(),
            });
        }
        if let Some(profile_id) = &self.profile_id {
            validate_non_empty("profileId", profile_id)?;
        }
        if matches!(self.profile_version, Some(0)) {
            return Err(RenderError {
                message: "dynamic text metrics field `profileVersion` must be a positive integer"
                    .to_string(),
            });
        }
        let mut ids = std::collections::HashSet::new();
        let mut style_keys = std::collections::HashSet::new();
        let mut has_default = false;
        for style in &self.text_styles {
            style.validate()?;
            if !ids.insert(style.id.as_str()) {
                return Err(RenderError {
                    message: format!(
                        "dynamic text metrics field `textStyles` contains duplicate id {:?}",
                        style.id
                    ),
                });
            }
            let key = style.style_key()?;
            if !style_keys.insert(key) {
                return Err(RenderError {
                    message:
                        "dynamic text metrics field `textStyles` contains duplicate style identity"
                            .to_string(),
                });
            }
            has_default |= style.id == self.default_style;
        }
        if !has_default {
            return Err(RenderError {
                message: format!(
                    "dynamic text metrics field `defaultStyle` {:?} must reference a textStyles id",
                    self.default_style
                ),
            });
        }
        Ok(())
    }

    pub(crate) fn profile_version_or_default(&self) -> u32 {
        self.profile_version.unwrap_or(1)
    }

    pub(crate) fn require_profile_id(&self, operation: &str) -> Result<&str, RenderError> {
        match self.profile_id.as_deref() {
            Some(profile_id) if !profile_id.trim().is_empty() => Ok(profile_id),
            _ => Err(RenderError {
                message: format!(
                    "dynamic text metrics field `profileId` is required for {operation}"
                ),
            }),
        }
    }

    fn default_text_style(&self) -> Result<&DynamicTextStyleInput, RenderError> {
        self.text_styles
            .iter()
            .find(|style| style.id == self.default_style)
            .ok_or_else(|| RenderError {
                message: format!(
                    "dynamic text metrics field `defaultStyle` {:?} must reference a textStyles id",
                    self.default_style
                ),
            })
    }

    fn style_by_key(
        &self,
    ) -> Result<HashMap<GraphTextStyleKey, DynamicTextStyleInput>, RenderError> {
        let mut styles = HashMap::new();
        for style in &self.text_styles {
            styles.insert(style.style_key()?, style.clone());
        }
        Ok(styles)
    }

    pub(crate) fn text_metrics_descriptor_for_layout(
        &self,
        node_padding_x: f64,
        node_padding_y: f64,
        edge_label_max_width: Option<f64>,
    ) -> Result<TextMetricsProfileDescriptor, RenderError> {
        self.validate()?;
        let profile_id = self.require_profile_id("dynamic MMDS descriptor")?;
        let default_style = self.default_text_style()?;
        Ok(TextMetricsProfileDescriptor {
            profile_id: profile_id.to_string(),
            source: "dynamic".to_string(),
            version: self.profile_version_or_default(),
            default_text_style: TextMetricsStyleDescriptor {
                font_family: default_style.font_family.clone(),
                font_size: default_style.font_size,
                font_style: default_style.font_style.clone(),
                font_weight: default_style.font_weight.clone(),
                line_height: default_style.line_height,
            },
            layout_text: TextMetricsLayoutDescriptor {
                node_padding_x,
                node_padding_y,
                label_padding_x: DEFAULT_LABEL_PADDING_X,
                label_padding_y: DEFAULT_LABEL_PADDING_Y,
                edge_label_max_width,
            },
        })
    }
}

impl DynamicTextStyleInput {
    fn validate(&self) -> Result<(), RenderError> {
        validate_non_empty("textStyles.id", &self.id)?;
        validate_non_empty("textStyles.fontFamily", &self.font_family)?;
        validate_positive_finite("textStyles.fontSize", self.font_size)?;
        validate_non_empty("textStyles.fontStyle", &self.font_style)?;
        validate_non_empty("textStyles.fontWeight", &self.font_weight)?;
        validate_positive_finite("textStyles.lineHeight", self.line_height)?;
        validate_non_empty("textStyles.cssFont", &self.css_font)?;
        self.style_key()?;
        Ok(())
    }

    fn style_key(&self) -> Result<GraphTextStyleKey, RenderError> {
        GraphTextStyleKey::new(
            &self.font_family,
            self.font_size,
            self.line_height,
            &self.font_style,
            &self.font_weight,
        )
        .map_err(|message| RenderError {
            message: format!(
                "dynamic text metrics textStyles entry {:?} is invalid: {message}",
                self.id
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicTextMetricsError {
    message: String,
}

impl DynamicTextMetricsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DynamicTextMetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DynamicTextMetricsError {}

pub struct CallbackTextMetricsProvider<F>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    input: DynamicMetricsInput,
    default_style: GraphTextStyleKey,
    styles_by_key: HashMap<GraphTextStyleKey, DynamicTextStyleInput>,
    callback: RefCell<F>,
    line_cache: RefCell<HashMap<StyleLineCacheKey, f64>>,
    scalar_cache: RefCell<HashMap<StyleScalarCacheKey, f64>>,
    first_error: RefCell<Option<DynamicTextMetricsError>>,
    in_callback: Cell<bool>,
    node_padding_x: f64,
    node_padding_y: f64,
}

impl<F> CallbackTextMetricsProvider<F>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    pub fn new(input: DynamicMetricsInput, callback: F) -> Self {
        Self::with_node_padding(
            input,
            DEFAULT_PROPORTIONAL_NODE_PADDING_X,
            DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
            callback,
        )
    }

    pub(crate) fn with_node_padding(
        input: DynamicMetricsInput,
        node_padding_x: f64,
        node_padding_y: f64,
        callback: F,
    ) -> Self {
        let validation_error = input
            .validate()
            .err()
            .map(|err| DynamicTextMetricsError::new(err.message));
        let default_style = input
            .default_text_style()
            .and_then(DynamicTextStyleInput::style_key)
            .unwrap_or_else(|_| {
                GraphTextStyleKey::new("invalid dynamic default", 1.0, 1.0, "normal", "400")
                    .expect("fallback dynamic style key should be valid")
            });
        // Preserve the first validation error in `finish()`. If validation
        // failed, the lookup map is intentionally empty so measurement returns
        // harmless widths until the recorded error is surfaced.
        let styles_by_key = input.style_by_key().unwrap_or_default();
        Self {
            input,
            default_style,
            styles_by_key,
            callback: RefCell::new(callback),
            line_cache: RefCell::new(HashMap::new()),
            scalar_cache: RefCell::new(HashMap::new()),
            first_error: RefCell::new(validation_error),
            in_callback: Cell::new(false),
            node_padding_x,
            node_padding_y,
        }
    }

    pub fn finish(&self) -> Result<(), RenderError> {
        match self.first_error.borrow().clone() {
            Some(error) => Err(RenderError {
                message: error.to_string(),
            }),
            None => Ok(()),
        }
    }

    pub(crate) fn measurement_cache_snapshot(&self) -> Result<TextMeasurementCache, RenderError> {
        self.finish()?;
        // Provider-free replay can need the default space scalar during wrap
        // reconstruction even if direct rendering did not otherwise query it.
        self.measure_scalar_char_for_style(&self.default_style.clone(), ' ');
        self.finish()?;
        let mut text_styles = BTreeMap::new();
        for style in &self.input.text_styles {
            text_styles.insert(
                style.id.clone(),
                TextMeasurementStyle {
                    id: style.id.clone(),
                    style: style.style_key().map_err(|error| RenderError {
                        message: error.to_string(),
                    })?,
                    css_font: style.css_font.clone(),
                },
            );
        }
        Ok(TextMeasurementCache {
            default_style: self.input.default_style.clone(),
            text_styles,
            line_widths: self
                .line_cache
                .borrow()
                .iter()
                .map(|(key, width)| ((key.style_id.clone(), key.text.clone()), *width))
                .collect(),
            scalar_widths: self
                .scalar_cache
                .borrow()
                .iter()
                .map(|(key, width)| ((key.style_id.clone(), key.ch), *width))
                .collect(),
        })
    }

    fn measure_line_text(&self, text: &str) -> f64 {
        self.measure_line_text_for_style(&self.default_style.clone(), text)
    }

    fn measure_line_text_for_style(&self, style: &GraphTextStyleKey, text: &str) -> f64 {
        let Some(style_input) = self.style_input_for_key(style, Some(text)) else {
            return 0.0;
        };
        let key = StyleLineCacheKey {
            style_id: style_input.id.clone(),
            text: text.to_string(),
        };
        if let Some(width) = self.line_cache.borrow().get(&key).copied() {
            return width;
        }

        match self.measure_uncached_text(text, style_input) {
            Some(width) => {
                self.line_cache.borrow_mut().insert(key, width);
                width
            }
            None => 0.0,
        }
    }

    fn measure_scalar_char(&self, ch: char) -> f64 {
        self.measure_scalar_char_for_style(&self.default_style.clone(), ch)
    }

    fn measure_scalar_char_for_style(&self, style: &GraphTextStyleKey, ch: char) -> f64 {
        let mut text = [0_u8; 4];
        let text = ch.encode_utf8(&mut text);
        let Some(style_input) = self.style_input_for_key(style, Some(text)) else {
            return 0.0;
        };
        let key = StyleScalarCacheKey {
            style_id: style_input.id.clone(),
            ch,
        };
        if let Some(width) = self.scalar_cache.borrow().get(&key).copied() {
            return width;
        }

        match self.measure_uncached_text(text, style_input) {
            Some(width) => {
                self.scalar_cache.borrow_mut().insert(key, width);
                width
            }
            None => 0.0,
        }
    }

    fn measure_uncached_text(&self, text: &str, style: &DynamicTextStyleInput) -> Option<f64> {
        if self.in_callback.get() {
            self.record_error(DynamicTextMetricsError::new(format!(
                "dynamic text measurement callback re-entered while measuring {text:?} with font {:?}",
                style.css_font
            )));
            return None;
        }

        self.in_callback.set(true);
        let result = {
            let mut callback = self.callback.borrow_mut();
            callback(text, &style.css_font)
        };
        self.in_callback.set(false);

        let validated = match result {
            Ok(width) => self.validate_width(text, style, width),
            Err(error) => Err(self.contextualize_callback_error(text, style, error)),
        };

        match validated {
            Ok(width) => Some(width),
            Err(error) => {
                self.record_error(error);
                None
            }
        }
    }

    fn contextualize_callback_error(
        &self,
        text: &str,
        style: &DynamicTextStyleInput,
        error: DynamicTextMetricsError,
    ) -> DynamicTextMetricsError {
        DynamicTextMetricsError::new(format!(
            "dynamic text measurement callback failed for {text:?} with font {:?}: {error}",
            style.css_font
        ))
    }

    fn validate_width(
        &self,
        text: &str,
        style: &DynamicTextStyleInput,
        width: f64,
    ) -> Result<f64, DynamicTextMetricsError> {
        if !width.is_finite() || width < 0.0 {
            return Err(DynamicTextMetricsError::new(format!(
                "dynamic text measurement for {text:?} with font {:?} must return a finite non-negative width",
                style.css_font
            )));
        }
        Ok(width)
    }

    fn style_input_for_key(
        &self,
        style: &GraphTextStyleKey,
        text: Option<&str>,
    ) -> Option<&DynamicTextStyleInput> {
        match self.styles_by_key.get(style) {
            Some(style_input) => Some(style_input),
            None => {
                let text = text.unwrap_or("");
                if let Some((registered_style, style_input)) =
                    self.style_input_for_key_except_line_height(style)
                {
                    self.record_error(DynamicTextMetricsError::new(format!(
                        "dynamic text metrics style set has lineHeight mismatch for text {text:?}: Mermaid font-size-only styles derive lineHeight {:.2}px from the default style ratio, but CSS font {:?} was registered with lineHeight {:.2}px",
                        style.line_height_px(),
                        style_input.css_font,
                        registered_style.line_height_px()
                    )));
                } else {
                    self.record_error(DynamicTextMetricsError::new(format!(
                        "dynamic text metrics style set has no CSS font for text {text:?} and style {:?}",
                        style
                    )));
                }
                None
            }
        }
    }

    fn style_input_for_key_except_line_height(
        &self,
        style: &GraphTextStyleKey,
    ) -> Option<(&GraphTextStyleKey, &DynamicTextStyleInput)> {
        self.styles_by_key.iter().find(|(registered_style, _)| {
            registered_style.font_family == style.font_family
                && registered_style.font_size_mpx == style.font_size_mpx
                && registered_style.font_style == style.font_style
                && registered_style.font_weight == style.font_weight
        })
    }

    fn record_error(&self, error: DynamicTextMetricsError) {
        let mut first_error = self.first_error.borrow_mut();
        if first_error.is_none() {
            *first_error = Some(error);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StyleLineCacheKey {
    style_id: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StyleScalarCacheKey {
    style_id: String,
    ch: char,
}

impl<F> TextMetricsProvider for CallbackTextMetricsProvider<F>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    fn measure_line_width(&self, text: &str) -> f64 {
        self.measure_line_text(text)
    }

    fn measure_scalar_width(&self, ch: char) -> f64 {
        self.measure_scalar_char(ch)
    }

    fn default_text_style_key(&self) -> GraphTextStyleKey {
        self.default_style.clone()
    }

    fn measure_line_width_for_style(&self, style: &GraphTextStyleKey, text: &str) -> f64 {
        self.measure_line_text_for_style(style, text)
    }

    fn measure_scalar_width_for_style(&self, style: &GraphTextStyleKey, ch: char) -> f64 {
        self.measure_scalar_char_for_style(style, ch)
    }

    fn font_size(&self) -> f64 {
        self.styles_by_key.get(&self.default_style).map_or_else(
            || self.default_style.font_size_px(),
            |style| style.font_size,
        )
    }

    fn line_height(&self) -> f64 {
        self.styles_by_key.get(&self.default_style).map_or_else(
            || self.default_style.line_height_px(),
            |style| style.line_height,
        )
    }

    fn font_size_for_style(&self, style: &GraphTextStyleKey) -> f64 {
        self.styles_by_key
            .get(style)
            .map_or_else(|| style.font_size_px(), |style| style.font_size)
    }

    fn line_height_for_style(&self, style: &GraphTextStyleKey) -> f64 {
        self.styles_by_key
            .get(style)
            .map_or_else(|| style.line_height_px(), |style| style.line_height)
    }

    fn node_padding_x(&self) -> f64 {
        self.node_padding_x
    }

    fn node_padding_y(&self) -> f64 {
        self.node_padding_y
    }

    fn label_padding_x(&self) -> f64 {
        DEFAULT_LABEL_PADDING_X
    }

    fn label_padding_y(&self) -> f64 {
        DEFAULT_LABEL_PADDING_Y
    }
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), RenderError> {
    if value.trim().is_empty() {
        return Err(RenderError {
            message: format!("dynamic text metrics field `{field}` must not be empty"),
        });
    }
    Ok(())
}

fn validate_positive_finite(field: &str, value: f64) -> Result<(), RenderError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(RenderError {
            message: format!(
                "dynamic text metrics field `{field}` must be a finite positive number"
            ),
        });
    }
    Ok(())
}

fn unsupported_dynamic_output_format(format: OutputFormat) -> RenderError {
    let message = match format {
        OutputFormat::Text | OutputFormat::Ascii => format!(
            "dynamic text metrics unsupported for {format} output; Text and ASCII are terminal grid outputs and remain static-profile based"
        ),
        _ => format!(
            "dynamic text metrics supports SVG and provider-bound MMDS output (requested {format})"
        ),
    };

    RenderError { message }
}

fn unsupported_dynamic_diagram_family(diagram_id: &str) -> RenderError {
    RenderError {
        message: format!(
            "dynamic text metrics unsupported for {diagram_id}/timeline-family diagrams; sequence requires a separate timeline metrics plan"
        ),
    }
}

fn mmds_input_diagram_type(input: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    value
        .get("metadata")
        .and_then(|metadata| metadata.get("diagram_type"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

pub fn render_graph_family_svg_with_dynamic_text_metrics<F>(
    input: &str,
    config: &RenderConfig,
    dynamic_input: DynamicMetricsInput,
    callback: F,
) -> Result<String, RenderError>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    render_graph_family_with_dynamic_text_metrics(
        input,
        OutputFormat::Svg,
        config,
        dynamic_input,
        callback,
    )
}

pub fn render_graph_family_with_dynamic_text_metrics<F>(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
    dynamic_input: DynamicMetricsInput,
    callback: F,
) -> Result<String, RenderError>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    if !matches!(format, OutputFormat::Svg | OutputFormat::Mmds) {
        return Err(unsupported_dynamic_output_format(format));
    }
    if config.layout_engine.is_some() {
        return Err(RenderError {
            message:
                "dynamic text metrics does not accept layoutEngine; it always uses flux-layered SVG"
                    .to_string(),
        });
    }
    if config.font_metrics_profile.is_some() {
        return Err(RenderError {
            message: "dynamic text metrics does not accept fontMetricsProfile; provider measurement is selected by this API"
                .to_string(),
        });
    }
    if config.graph_text_style.is_some() {
        return Err(RenderError {
            message: "dynamic text metrics uses DynamicMetricsInput for font identity; do not pass RenderConfig.graph_text_style"
                .to_string(),
        });
    }

    dynamic_input.validate()?;
    super::validate_render_config(config)?;

    match detect_input_frontend(input) {
        Some(InputFrontend::Mmds) => {
            let mut effective_config = super::effective_render_config(input, format, config);
            if matches!(format, OutputFormat::Svg) {
                apply_svg_surface_defaults(OutputFormat::Svg, &mut effective_config, true);
            }
            let default_style = dynamic_input.default_text_style()?;
            let font_family = default_style.font_family.clone();
            let font_size = default_style.font_size;
            let node_padding_x = effective_config
                .svg_node_padding_x
                .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_X);
            let node_padding_y = effective_config
                .svg_node_padding_y
                .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_Y);
            let text_metrics_descriptor = dynamic_input.text_metrics_descriptor_for_layout(
                node_padding_x,
                node_padding_y,
                effective_config.layout.edge_label_max_width,
            )?;
            if mmds_input_diagram_type(input).as_deref() == Some("sequence") {
                return Err(unsupported_dynamic_diagram_family("sequence"));
            }
            let provider = CallbackTextMetricsProvider::with_node_padding(
                dynamic_input,
                node_padding_x,
                node_padding_y,
                callback,
            );
            let mut options = effective_config.svg_render_options();
            options.font_family = font_family;
            options.font_size = font_size;
            let svg_theme = super::resolve_configured_svg_theme(&effective_config)?;
            let rendered = crate::runtime::mmds::render_input_with_dynamic_text_metrics(
                input,
                format,
                &options,
                svg_theme.as_ref(),
                &text_metrics_descriptor,
                &provider,
            )?;
            provider.finish()?;
            return Ok(rendered);
        }
        Some(InputFrontend::Mermaid) => {}
        None => {
            return Err(RenderError {
                message: "unknown diagram type".to_string(),
            });
        }
    }

    let registry = default_registry();
    let resolved = registry.resolve(input).ok_or_else(|| RenderError {
        message: "unknown diagram type".to_string(),
    })?;
    if !matches!(resolved.family(), DiagramFamily::Graph) {
        return Err(unsupported_dynamic_diagram_family(resolved.diagram_id()));
    }
    if !resolved.supported_formats().contains(&format) {
        return Err(RenderError {
            message: format!(
                "{} diagrams do not support {format} output",
                resolved.diagram_id()
            ),
        });
    }

    let instance = registry
        .create(resolved.diagram_id())
        .ok_or_else(|| RenderError {
            message: format!(
                "no implementation for diagram type: {}",
                resolved.diagram_id()
            ),
        })?;
    let parsed = instance.parse(input).map_err(|error| RenderError {
        message: format!("parse error: {error}"),
    })?;

    let mut effective_config = super::effective_render_config(input, format, config);
    if matches!(format, OutputFormat::Svg) {
        // Match the existing Wasm SVG surface while keeping dynamic-measured
        // layout on this separate export instead of accepting a caller
        // layoutEngine knob.
        apply_svg_surface_defaults(OutputFormat::Svg, &mut effective_config, true);
    }
    let default_style = dynamic_input.default_text_style()?;
    let font_family = default_style.font_family.clone();
    let font_size = default_style.font_size;

    let node_padding_x = effective_config
        .svg_node_padding_x
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_X);
    let node_padding_y = effective_config
        .svg_node_padding_y
        .unwrap_or(DEFAULT_PROPORTIONAL_NODE_PADDING_Y);
    let text_metrics_descriptor = if matches!(format, OutputFormat::Mmds) {
        Some(dynamic_input.text_metrics_descriptor_for_layout(
            node_padding_x,
            node_padding_y,
            effective_config.layout.edge_label_max_width,
        )?)
    } else {
        None
    };
    let provider = CallbackTextMetricsProvider::with_node_padding(
        dynamic_input,
        node_padding_x,
        node_padding_y,
        callback,
    );
    let mut options = effective_config.svg_render_options();
    options.font_family = font_family;
    options.font_size = font_size;
    let render_context = DynamicGraphFamilyRenderContext {
        format,
        config: &effective_config,
        svg_options: &options,
        text_metrics_descriptor: text_metrics_descriptor.as_ref(),
        provider: &provider,
    };

    let payload =
        super::payload::prepare_payload_for_render(parsed.into_payload()?, &effective_config);
    let rendered = match payload {
        Diagram::Flowchart(mut graph) => render_graph_family_payload_with_dynamic_metrics(
            "flowchart",
            &mut graph,
            &render_context,
        ),
        Diagram::Class(mut graph) => {
            render_graph_family_payload_with_dynamic_metrics("class", &mut graph, &render_context)
        }
        Diagram::State(mut graph) => {
            render_graph_family_payload_with_dynamic_metrics("state", &mut graph, &render_context)
        }
        Diagram::Sequence(_) => Err(unsupported_dynamic_diagram_family("sequence")),
    }?;

    provider.finish()?;
    Ok(rendered)
}

struct DynamicGraphFamilyRenderContext<'a, F>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    format: OutputFormat,
    config: &'a RenderConfig,
    svg_options: &'a SvgRenderOptions,
    text_metrics_descriptor: Option<&'a TextMetricsProfileDescriptor>,
    provider: &'a CallbackTextMetricsProvider<F>,
}

fn render_graph_family_payload_with_dynamic_metrics<F>(
    diagram_id: &str,
    graph: &mut crate::graph::Graph,
    context: &DynamicGraphFamilyRenderContext<'_, F>,
) -> Result<String, RenderError>
where
    F: FnMut(&str, &str) -> Result<f64, DynamicTextMetricsError>,
{
    match context.format {
        OutputFormat::Svg => crate::runtime::graph_family::render_graph_family_svg_with_provider(
            diagram_id,
            graph,
            context.config,
            context.svg_options,
            context.provider,
        ),
        OutputFormat::Mmds => {
            let descriptor = context.text_metrics_descriptor.ok_or_else(|| RenderError {
                message:
                    "dynamic text metrics field `profileId` is required for dynamic MMDS descriptor"
                        .to_string(),
            })?;
            crate::runtime::graph_family::render_graph_family_mmds_with_provider_and_measurements(
                diagram_id,
                graph,
                context.config,
                descriptor,
                context.provider,
                || context.provider.measurement_cache_snapshot(),
            )
        }
        _ => unreachable!("format validation restricts dynamic text metrics output"),
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::engines::graph::EngineAlgorithmId;
    use crate::format::OutputFormat;
    use crate::graph::GeometryLevel;
    use crate::graph::measure::{
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID, DEFAULT_GRAPH_FONT_FAMILY, GraphTextStyleKey,
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID, TextMetricsProfileConfig, TextMetricsProvider,
        resolve_text_metrics_profile,
    };
    use crate::runtime::config::{GraphTextStyleConfig, RenderConfig};
    use crate::runtime::render_diagram;

    fn valid_input() -> DynamicMetricsInput {
        serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":24,
                  "cssFont":"16px Inter"
                }
              ]
            }"#,
        )
        .unwrap()
    }

    fn profiled_input(profile_id: &str) -> DynamicMetricsInput {
        serde_json::from_str(&format!(
            r#"{{
              "defaultStyle":"s0",
              "textStyles":[
                {{
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":24,
                  "cssFont":"16px Inter"
                }}
              ],
              "profileId":"{profile_id}"
            }}"#
        ))
        .unwrap()
    }

    fn style_set_input() -> DynamicMetricsInput {
        serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":8,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":12,
                  "cssFont":"400 normal 8px Inter"
                },
                {
                  "id":"s1",
                  "fontFamily":"Inter",
                  "fontSize":32,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":48,
                  "cssFont":"400 normal 32px Inter"
                }
              ],
              "profileId":"browser-test-v1"
            }"#,
        )
        .unwrap()
    }

    fn multi_font_style_set_input() -> DynamicMetricsInput {
        serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":24,
                  "cssFont":"16px Inter"
                },
                {
                  "id":"s1",
                  "fontFamily":"Verdana",
                  "fontSize":8,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":12,
                  "cssFont":"8px Verdana"
                },
                {
                  "id":"s2",
                  "fontFamily":"Courier New",
                  "fontSize":20,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":30,
                  "cssFont":"20px Courier New"
                }
              ],
              "profileId":"browser-multifont-v1"
            }"#,
        )
        .unwrap()
    }

    fn multi_font_same_text_input() -> &'static str {
        "graph TD\nA[Same] --> B[Same]\nstyle A font-family:Verdana,font-size:8px\nstyle B font-family:Courier New,font-size:20px"
    }

    fn multi_font_mermaid_input() -> &'static str {
        include_str!("../../tests/fixtures/flowchart/dynamic/multi_font_styles.mmd")
    }

    fn multi_font_mermaid_style_set_input() -> DynamicMetricsInput {
        serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":24,
                  "cssFont":"16px Inter"
                },
                {
                  "id":"s1",
                  "fontFamily":"Verdana",
                  "fontSize":8,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":12,
                  "cssFont":"8px Verdana"
                },
                {
                  "id":"s2",
                  "fontFamily":"Courier New",
                  "fontSize":20,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":30,
                  "cssFont":"20px Courier New"
                },
                {
                  "id":"s3",
                  "fontFamily":"Times New Roman",
                  "fontSize":32,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":48,
                  "cssFont":"32px Times New Roman"
                }
              ],
              "profileId":"browser-multifont-v1"
            }"#,
        )
        .unwrap()
    }

    fn multi_font_width(text: &str, css_font: &str) -> Result<f64, DynamicTextMetricsError> {
        let per_char = if css_font.contains("Courier New") {
            18.0
        } else if css_font.contains("Verdana") {
            6.0
        } else {
            10.0
        };
        Ok(text.len() as f64 * per_char)
    }

    fn multi_font_mermaid_width(
        text: &str,
        css_font: &str,
    ) -> Result<f64, DynamicTextMetricsError> {
        let per_char = if css_font.contains("Times New Roman") {
            30.0
        } else if css_font.contains("Courier New") {
            15.0
        } else if css_font.contains("Verdana") {
            4.0
        } else {
            8.0
        };
        Ok(text.chars().count() as f64 * per_char)
    }

    fn multi_font_dynamic_mmds_fixture() -> String {
        render_graph_family_with_dynamic_text_metrics(
            multi_font_same_text_input(),
            OutputFormat::Mmds,
            &routed_config(),
            multi_font_style_set_input(),
            multi_font_width,
        )
        .expect("dynamic multi-font MMDS fixture should render")
    }

    fn style_8px() -> GraphTextStyleKey {
        GraphTextStyleKey::new("Inter", 8.0, 12.0, "normal", "400").unwrap()
    }

    fn style_32px() -> GraphTextStyleKey {
        GraphTextStyleKey::new("Inter", 32.0, 48.0, "normal", "400").unwrap()
    }

    fn deterministic_width(text: &str, _css_font: &str) -> Result<f64, DynamicTextMetricsError> {
        Ok(text.len() as f64 * 8.0)
    }

    fn routed_config() -> RenderConfig {
        RenderConfig {
            geometry_level: GeometryLevel::Routed,
            ..RenderConfig::default()
        }
    }

    fn dynamic_mmds_fixture() -> String {
        render_graph_family_with_dynamic_text_metrics(
            "graph TD\nA[Alpha] -->|a labeled edge| B[Beta]",
            OutputFormat::Mmds,
            &routed_config(),
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect("dynamic MMDS fixture should render")
    }

    fn static_equivalent_input() -> DynamicMetricsInput {
        let metrics = resolve_text_metrics_profile(TextMetricsProfileConfig::default())
            .expect("default recorded text metrics should resolve")
            .metrics;
        DynamicMetricsInput {
            default_style: "s0".to_string(),
            text_styles: vec![DynamicTextStyleInput {
                id: "s0".to_string(),
                font_family: DEFAULT_GRAPH_FONT_FAMILY.to_string(),
                font_size: metrics.font_size(),
                font_style: "normal".to_string(),
                font_weight: "400".to_string(),
                line_height: metrics.line_height(),
                css_font: format!("{}px {}", metrics.font_size(), DEFAULT_GRAPH_FONT_FAMILY),
            }],
            profile_id: None,
            profile_version: None,
        }
    }

    #[test]
    fn dynamic_metrics_input_accepts_optional_provider_identity_for_svg() {
        let input: DynamicMetricsInput = serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"italic",
                  "fontWeight":"700",
                  "lineHeight":24,
                  "cssFont":"italic 700 16px Inter"
                }
              ],
              "profileId":"browser-inter-v1",
              "profileVersion":2
            }"#,
        )
        .unwrap();

        input.validate().unwrap();
        assert_eq!(
            input.require_profile_id("dynamic MMDS").unwrap(),
            "browser-inter-v1"
        );
        assert_eq!(input.profile_version_or_default(), 2);
        let default_style = input.default_text_style().unwrap();
        assert_eq!(default_style.font_style, "italic");
        assert_eq!(default_style.font_weight, "700");
    }

    #[test]
    fn dynamic_metrics_input_accepts_style_set_svg_metrics_json() {
        let input = valid_input();

        input.validate().unwrap();
        assert_eq!(input.profile_version_or_default(), 1);
        let default_style = input.default_text_style().unwrap();
        assert_eq!(default_style.font_style, "normal");
        assert_eq!(default_style.font_weight, "400");
        let err = input
            .require_profile_id("dynamic MMDS")
            .expect_err("profile id should only be required for descriptor operations");
        assert!(err.message.contains("profileId"), "{err}");
        assert!(err.message.contains("dynamic MMDS"), "{err}");
        assert!(!err.message.contains("browser"), "{err}");
    }

    #[test]
    fn dynamic_metrics_input_rejects_zero_profile_version() {
        let input: DynamicMetricsInput = serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":24,
                  "cssFont":"16px Inter"
                }
              ],
              "profileVersion":0
            }"#,
        )
        .unwrap();

        let err = input
            .validate()
            .expect_err("zero profileVersion should fail");
        assert!(err.message.contains("profileVersion"), "{err}");
        assert!(err.message.contains("positive"), "{err}");
    }

    #[test]
    fn dynamic_metrics_input_rejects_empty_optional_identity_fields() {
        for (field, json) in [
            (
                "profileId",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}],"profileId":" "}"#,
            ),
            (
                "textStyles.fontStyle",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":" ","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}]}"#,
            ),
            (
                "textStyles.fontWeight",
                r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":" ","lineHeight":24,"cssFont":"16px Inter"}]}"#,
            ),
        ] {
            let input: DynamicMetricsInput = serde_json::from_str(json).unwrap();
            let err = input
                .validate()
                .expect_err("empty optional field should fail");
            assert!(err.message.contains(field), "{err}");
        }
    }

    #[test]
    fn dynamic_metrics_input_builds_provider_bound_descriptor() {
        let input: DynamicMetricsInput = serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"normal",
                  "fontWeight":"400",
                  "lineHeight":24,
                  "cssFont":"16px Inter"
                }
              ],
              "profileId":"browser-inter-v1"
            }"#,
        )
        .unwrap();

        let descriptor = input
            .text_metrics_descriptor_for_layout(15.0, 16.0, Some(200.0))
            .unwrap();

        assert_eq!(descriptor.profile_id, "browser-inter-v1");
        assert_eq!(descriptor.source, "dynamic");
        assert_eq!(descriptor.version, 1);
        assert_eq!(descriptor.default_text_style.font_family, "Inter");
        assert_eq!(descriptor.default_text_style.font_size, 16.0);
        assert_eq!(descriptor.default_text_style.font_style, "normal");
        assert_eq!(descriptor.default_text_style.font_weight, "400");
        assert_eq!(descriptor.default_text_style.line_height, 24.0);
        assert_eq!(descriptor.layout_text.node_padding_x, 15.0);
        assert_eq!(descriptor.layout_text.node_padding_y, 16.0);
        assert_eq!(
            descriptor.layout_text.label_padding_x,
            DEFAULT_LABEL_PADDING_X
        );
        assert_eq!(
            descriptor.layout_text.label_padding_y,
            DEFAULT_LABEL_PADDING_Y
        );
        assert_eq!(descriptor.layout_text.edge_label_max_width, Some(200.0));
    }

    #[test]
    fn dynamic_metrics_descriptor_mirrors_adapter_owned_style_and_version() {
        let input: DynamicMetricsInput = serde_json::from_str(
            r#"{
              "defaultStyle":"s0",
              "textStyles":[
                {
                  "id":"s0",
                  "fontFamily":"Inter",
                  "fontSize":16,
                  "fontStyle":"italic",
                  "fontWeight":"700",
                  "lineHeight":24,
                  "cssFont":"italic 700 16px Inter"
                }
              ],
              "profileId":"browser-inter-v2",
              "profileVersion":2
            }"#,
        )
        .unwrap();

        let descriptor = input
            .text_metrics_descriptor_for_layout(12.0, 13.0, None)
            .unwrap();

        assert_eq!(descriptor.profile_id, "browser-inter-v2");
        assert_eq!(descriptor.version, 2);
        assert_eq!(descriptor.default_text_style.font_style, "italic");
        assert_eq!(descriptor.default_text_style.font_weight, "700");
        assert_eq!(descriptor.layout_text.edge_label_max_width, None);
    }

    #[test]
    fn dynamic_metrics_descriptor_requires_profile_id() {
        let err = valid_input()
            .text_metrics_descriptor_for_layout(15.0, 15.0, Some(200.0))
            .expect_err("descriptor construction should require profileId");

        assert!(err.message.contains("profileId"), "{err}");
        assert!(err.message.contains("dynamic MMDS descriptor"), "{err}");
        assert!(!err.message.contains("browser"), "{err}");
    }

    #[test]
    fn dynamic_metrics_input_rejects_unknown_fields() {
        let err = serde_json::from_str::<DynamicMetricsInput>(
            r#"{"defaultStyle":"s0","textStyles":[{"id":"s0","fontFamily":"Inter","fontSize":16,"fontStyle":"normal","fontWeight":"400","lineHeight":24,"cssFont":"16px Inter"}],"extra":true}"#,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("unknown field"), "{err}");
        assert!(err.contains("extra"), "{err}");
    }

    #[test]
    fn dynamic_metrics_input_validates_required_style_fields() {
        valid_input().validate().expect("valid input should pass");

        for (field, input) in [
            ("textStyles.cssFont", {
                let mut input = valid_input();
                input.text_styles[0].css_font = " ".to_string();
                input
            }),
            ("textStyles.fontFamily", {
                let mut input = valid_input();
                input.text_styles[0].font_family = " ".to_string();
                input
            }),
            ("textStyles.fontSize", {
                let mut input = valid_input();
                input.text_styles[0].font_size = 0.0;
                input
            }),
            ("textStyles.lineHeight", {
                let mut input = valid_input();
                input.text_styles[0].line_height = f64::INFINITY;
                input
            }),
        ] {
            let err = input.validate().expect_err("invalid field should fail");
            assert!(err.message.contains(field), "{err}");
        }
    }

    #[test]
    fn callback_provider_caches_repeated_measurements() {
        let calls = Rc::new(Cell::new(0));
        let observed_calls = Rc::clone(&calls);
        let provider = CallbackTextMetricsProvider::new(valid_input(), move |text, _css_font| {
            calls.set(calls.get() + 1);
            Ok(text.len() as f64)
        });

        assert_eq!(provider.measure_line_width("Alpha"), 5.0);
        assert_eq!(provider.measure_line_width("Alpha"), 5.0);
        provider.finish().expect("cached measurements should pass");
        assert_eq!(observed_calls.get(), 1);
    }

    #[test]
    fn callback_provider_finish_reports_unvalidated_input_errors() {
        let mut input = valid_input();
        input.default_style = "missing-style".to_string();
        let provider = CallbackTextMetricsProvider::new(input, |_text, _css_font| Ok(1.0));

        let err = provider
            .finish()
            .expect_err("provider should preserve validation errors");

        assert!(err.message.contains("defaultStyle"), "{err}");
        assert!(err.message.contains("missing-style"), "{err}");
    }

    #[test]
    fn dynamic_provider_caches_same_text_separately_by_style() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let observed_calls = Rc::clone(&calls);
        let provider =
            CallbackTextMetricsProvider::new(style_set_input(), move |text, css_font| {
                calls
                    .borrow_mut()
                    .push((text.to_string(), css_font.to_string()));
                Ok(if css_font.contains("32px") {
                    64.0
                } else {
                    16.0
                })
            });

        assert_eq!(
            provider.measure_line_width_for_style(&style_8px(), "Same"),
            16.0
        );
        assert_eq!(
            provider.measure_line_width_for_style(&style_32px(), "Same"),
            64.0
        );
        assert_eq!(
            provider.measure_line_width_for_style(&style_8px(), "Same"),
            16.0
        );
        provider.finish().expect("style-keyed cache should pass");

        assert_eq!(
            observed_calls.borrow().as_slice(),
            [
                ("Same".to_string(), "400 normal 8px Inter".to_string()),
                ("Same".to_string(), "400 normal 32px Inter".to_string()),
            ]
        );
    }

    #[test]
    fn dynamic_provider_snapshot_preserves_style_identity() {
        let provider = CallbackTextMetricsProvider::new(style_set_input(), |text, css_font| {
            Ok(if css_font.contains("32px") {
                text.len() as f64 * 20.0
            } else {
                text.len() as f64 * 5.0
            })
        });

        assert_eq!(
            provider.measure_line_width_for_style(&style_8px(), "Same"),
            20.0
        );
        assert_eq!(
            provider.measure_line_width_for_style(&style_32px(), "Same"),
            80.0
        );

        let snapshot = provider
            .measurement_cache_snapshot()
            .expect("successful provider should export measurements");
        assert_eq!(snapshot.line_width_for_style("s0", "Same"), Some(20.0));
        assert_eq!(snapshot.line_width_for_style("s1", "Same"), Some(80.0));
    }

    #[test]
    fn dynamic_provider_errors_name_text_and_css_font_for_style() {
        let provider = CallbackTextMetricsProvider::new(style_set_input(), |text, css_font| {
            Err(DynamicTextMetricsError::new(format!(
                "failed {text} with {css_font}"
            )))
        });

        assert_eq!(
            provider.measure_line_width_for_style(&style_32px(), "Styled"),
            0.0
        );
        let err = provider
            .finish()
            .expect_err("callback error should be recorded");

        assert!(err.message.contains("Styled"), "{err}");
        assert!(err.message.contains("400 normal 32px Inter"), "{err}");
    }

    #[test]
    fn dynamic_provider_reports_line_height_mismatches_clearly() {
        let mut input = multi_font_style_set_input();
        let small_style = input
            .text_styles
            .iter_mut()
            .find(|style| style.id == "s1")
            .expect("small style should exist");
        small_style.line_height = 14.0;
        let provider = CallbackTextMetricsProvider::new(input, |_text, _css_font| Ok(8.0));
        let mermaid_style = GraphTextStyleKey::new("Verdana", 8.0, 12.0, "normal", "400")
            .expect("test style is valid");

        assert_eq!(
            provider.measure_line_width_for_style(&mermaid_style, "Small"),
            0.0
        );
        let err = provider
            .finish()
            .expect_err("line-height mismatch should be recorded");

        assert!(err.message.contains("lineHeight mismatch"), "{err}");
        assert!(err.message.contains("default style ratio"), "{err}");
        assert!(err.message.contains("12.00px"), "{err}");
        assert!(err.message.contains("14.00px"), "{err}");
        assert!(err.message.contains("8px Verdana"), "{err}");
    }

    #[test]
    fn dynamic_metrics_rejects_config_graph_text_style_even_with_mermaid_styles() {
        let config = RenderConfig {
            graph_text_style: Some(GraphTextStyleConfig::new("Arial", 16.0)),
            ..RenderConfig::default()
        };

        let err = render_graph_family_with_dynamic_text_metrics(
            "graph TD\nA[Alpha] --> B[Beta]\nstyle A font-family:Inter,font-size:16px",
            OutputFormat::Svg,
            &config,
            style_set_input(),
            |_text, _css_font| Ok(8.0),
        )
        .unwrap_err();

        assert!(
            err.message.contains("graph_text_style")
                || err.message.contains("graphTextStyle")
                || err.message.contains("RenderConfig.graph_text_style"),
            "{err}"
        );
    }

    #[test]
    fn callback_provider_records_invalid_width_errors_with_text_and_font() {
        for width in [f64::NAN, f64::INFINITY, -1.0] {
            let provider =
                CallbackTextMetricsProvider::new(valid_input(), move |_text, _font| Ok(width));

            assert_eq!(provider.measure_line_width("Alpha"), 0.0);
            let err = provider
                .finish()
                .expect_err("invalid width should be recorded");
            assert!(err.message.contains("Alpha"), "{err}");
            assert!(err.message.contains("16px Inter"), "{err}");
            assert!(err.message.contains("finite non-negative width"), "{err}");
        }
    }

    #[test]
    fn callback_provider_records_callback_errors_with_text_and_font() {
        let provider = CallbackTextMetricsProvider::new(valid_input(), |_text, _font| {
            Err(DynamicTextMetricsError::new("canvas failed"))
        });

        assert_eq!(provider.measure_line_width("Beta"), 0.0);
        let err = provider
            .finish()
            .expect_err("callback error should be recorded");
        assert!(err.message.contains("Beta"), "{err}");
        assert!(err.message.contains("16px Inter"), "{err}");
        assert!(err.message.contains("canvas failed"), "{err}");
    }

    #[test]
    fn callback_provider_reentry_errors_cleanly() {
        let provider =
            CallbackTextMetricsProvider::new(valid_input(), |text, _font| Ok(text.len() as f64));

        provider.in_callback.set(true);
        assert_eq!(provider.measure_line_width("Gamma"), 0.0);
        provider.in_callback.set(false);

        let err = provider.finish().expect_err("re-entry should be recorded");
        assert!(err.message.contains("re-entered"), "{err}");
        assert!(err.message.contains("Gamma"), "{err}");
        assert!(err.message.contains("16px Inter"), "{err}");
    }

    #[test]
    fn callback_provider_caches_line_and_scalar_measurements() {
        let calls = Rc::new(Cell::new(0));
        let observed_calls = Rc::clone(&calls);
        let provider = CallbackTextMetricsProvider::new(valid_input(), move |text, _font| {
            calls.set(calls.get() + 1);
            Ok(text.len() as f64)
        });

        for line in ["Alpha", "Beta", "Alpha", "Beta"] {
            provider.measure_line_width(line);
        }
        for ch in "Alpha Beta".chars().chain("Alpha Beta".chars()) {
            provider.measure_scalar_width(ch);
        }

        provider.finish().expect("cached measurements should pass");
        let unique_lines = 2;
        let unique_scalars = "Alpha Beta"
            .chars()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert_eq!(observed_calls.get(), unique_lines + unique_scalars);
    }

    #[test]
    fn callback_provider_exports_line_and_scalar_queries_after_success() {
        let provider = CallbackTextMetricsProvider::new(valid_input(), |text, _font| {
            Ok(text.len() as f64 * 8.0)
        });

        assert_eq!(provider.measure_line_width("Alpha"), 40.0);
        assert_eq!(provider.measure_scalar_width('A'), 8.0);
        assert_eq!(provider.measure_scalar_width(' '), 8.0);
        provider.finish().unwrap();

        let snapshot = provider
            .measurement_cache_snapshot()
            .expect("successful provider should export measurements");
        assert_eq!(snapshot.line_width("Alpha"), Some(40.0));
        assert_eq!(snapshot.scalar_width('A'), Some(8.0));
        assert_eq!(snapshot.scalar_width(' '), Some(8.0));
    }

    #[test]
    fn callback_provider_keeps_line_and_scalar_query_keys_distinct() {
        let calls = Rc::new(Cell::new(0));
        let observed_calls = Rc::clone(&calls);
        let provider = CallbackTextMetricsProvider::new(valid_input(), move |text, _font| {
            calls.set(calls.get() + 1);
            Ok(text.len() as f64 * 8.0)
        });

        assert_eq!(provider.measure_line_width("A"), 8.0);
        assert_eq!(provider.measure_scalar_width('A'), 8.0);
        provider.finish().unwrap();

        let snapshot = provider
            .measurement_cache_snapshot()
            .expect("successful provider should export measurements");
        assert_eq!(snapshot.line_width("A"), Some(8.0));
        assert_eq!(snapshot.scalar_width('A'), Some(8.0));
        assert_eq!(snapshot.scalar_width(' '), Some(8.0));
        assert_eq!(observed_calls.get(), 3);
    }

    #[test]
    fn callback_provider_refuses_measurement_cache_snapshot_after_error() {
        let provider = CallbackTextMetricsProvider::new(valid_input(), |_text, _font| {
            Err(DynamicTextMetricsError::new("canvas failed"))
        });

        assert_eq!(provider.measure_line_width("Alpha"), 0.0);
        let err = provider
            .measurement_cache_snapshot()
            .expect_err("failed provider should not export measurements");

        assert!(err.message.contains("Alpha"), "{err}");
        assert!(err.message.contains("canvas failed"), "{err}");
    }

    #[test]
    fn dynamic_svg_bridge_changes_label_background_width() {
        let input = "graph TD\nA -->|mmmm| B";
        let static_svg =
            render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).unwrap();
        let dynamic_svg = render_graph_family_svg_with_dynamic_text_metrics(
            input,
            &RenderConfig::default(),
            valid_input(),
            |text, _css_font| Ok(if text.contains('m') { 100.0 } else { 8.0 }),
        )
        .unwrap();

        assert_ne!(dynamic_svg, static_svg);
        assert!(dynamic_svg.contains("<svg"), "{dynamic_svg}");
        assert!(dynamic_svg.contains("width=\"108.00\""), "{dynamic_svg}");
        assert!(!dynamic_svg.contains("metricsProfile"), "{dynamic_svg}");
        assert!(
            !dynamic_svg.contains("org.mmdflux.text-measurements.v1"),
            "{dynamic_svg}"
        );
    }

    #[test]
    fn dynamic_svg_bridge_rejects_unsupported_output_formats() {
        let err = render_graph_family_with_dynamic_text_metrics(
            "graph TD\nA-->B",
            OutputFormat::Mermaid,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("unsupported output format should fail");
        assert!(
            err.message
                .contains("supports SVG and provider-bound MMDS output"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_text_metrics_rejects_terminal_formats_before_measurement() {
        for format in [OutputFormat::Text, OutputFormat::Ascii] {
            let mut calls = 0;
            let err = render_graph_family_with_dynamic_text_metrics(
                "graph TD\nA[Alpha] --> B[Beta]",
                format,
                &RenderConfig::default(),
                profiled_input("browser-test-v1"),
                |_text, _css_font| {
                    calls += 1;
                    Ok(8.0)
                },
            )
            .expect_err("terminal dynamic output should reject");

            assert_eq!(calls, 0, "dynamic callback must not run for {format}");
            assert!(err.message.contains(&format.to_string()), "{err}");
            assert!(err.message.contains("terminal"), "{err}");
            assert!(err.message.contains("unsupported"), "{err}");
        }
    }

    #[test]
    fn static_terminal_output_stays_byte_stable_after_dynamic_render() {
        let input = "graph TD\nA[Alpha] -->|edge label| B[Beta]";

        for profile in [
            None,
            Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID),
            Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID),
        ] {
            let config = RenderConfig {
                font_metrics_profile: profile.map(str::to_string),
                ..RenderConfig::default()
            };
            let text_before =
                render_diagram(input, OutputFormat::Text, &config).expect("text render before");
            let ascii_before =
                render_diagram(input, OutputFormat::Ascii, &config).expect("ascii render before");

            let dynamic_svg = render_graph_family_with_dynamic_text_metrics(
                input,
                OutputFormat::Svg,
                &RenderConfig::default(),
                profiled_input("browser-test-v1"),
                |text, _css_font| Ok(text.len() as f64 * 11.0),
            )
            .expect("dynamic SVG render should succeed");
            assert!(dynamic_svg.contains("<svg"), "{dynamic_svg}");

            assert_eq!(
                render_diagram(input, OutputFormat::Text, &config).expect("text render after"),
                text_before
            );
            assert_eq!(
                render_diagram(input, OutputFormat::Ascii, &config).expect("ascii render after"),
                ascii_before
            );
        }
    }

    #[test]
    fn dynamic_mmds_output_emits_text_measurements_sidecar() {
        let output = render_graph_family_with_dynamic_text_metrics(
            "graph TD\nA[Alpha] -->|edge label| B[Beta]",
            OutputFormat::Mmds,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            |text, _css_font| Ok(text.len() as f64 * 8.0),
        )
        .expect("dynamic MMDS output should render");

        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        let extension = &value["extensions"]["org.mmdflux.text-metrics.v1"];
        assert_eq!(extension["metricsProfile"]["source"], "dynamic");
        assert_eq!(extension["metricsProfile"]["id"], "browser-test-v1");
        assert_eq!(extension["metricsProfile"]["version"], 1);
        assert_eq!(extension["defaultTextStyle"]["font-family"], "Inter");
        assert_eq!(extension["defaultTextStyle"]["font-size"], 16.0);
        assert_eq!(extension["defaultTextStyle"]["line-height"], 24.0);

        let sidecar = &value["extensions"]["org.mmdflux.text-measurements.v1"];
        assert_eq!(sidecar["profileRef"]["id"], "browser-test-v1");
        assert_eq!(sidecar["profileRef"]["source"], "dynamic");
        assert_eq!(sidecar["profileRef"]["version"], 1);
        assert!(
            sidecar["lineWidths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| {
                    entry["text"] == "Alpha" && entry["width"].as_f64().unwrap() > 0.0
                })
        );
        assert!(
            sidecar["scalarWidths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| { entry["text"] == " " && entry["width"].as_f64().unwrap() > 0.0 })
        );
        let line_texts: Vec<_> = sidecar["lineWidths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["text"].as_str().unwrap().to_string())
            .collect();
        let mut sorted_line_texts = line_texts.clone();
        sorted_line_texts.sort();
        assert_eq!(line_texts, sorted_line_texts);
        let scalar_texts: Vec<_> = sidecar["scalarWidths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["text"].as_str().unwrap().to_string())
            .collect();
        let mut sorted_scalar_texts = scalar_texts.clone();
        sorted_scalar_texts.sort();
        assert_eq!(scalar_texts, sorted_scalar_texts);
        assert!(
            value["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "mmdflux-text-measurements-v1")
        );
    }

    #[test]
    fn static_mmds_output_does_not_emit_text_measurements_sidecar() {
        let output = render_diagram(
            "graph TD\nA[Alpha] -->|edge label| B[Beta]",
            OutputFormat::Mmds,
            &RenderConfig::default(),
        )
        .expect("static MMDS output should render");

        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(
            value["extensions"]
                .get("org.mmdflux.text-measurements.v1")
                .is_none()
        );
        assert!(
            !value["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "mmdflux-text-measurements-v1")
        );
    }

    #[test]
    fn dynamic_mmds_with_measurements_replays_provider_free_to_same_svg() {
        let input = "graph TD\nA[Alpha] -->|edge label| B[Beta]";
        let config = routed_config();

        let direct_svg = render_graph_family_with_dynamic_text_metrics(
            input,
            OutputFormat::Svg,
            &config,
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect("direct dynamic SVG should render");
        let dynamic_mmds = render_graph_family_with_dynamic_text_metrics(
            input,
            OutputFormat::Mmds,
            &config,
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect("dynamic MMDS output should render");

        let replay_svg = render_diagram(&dynamic_mmds, OutputFormat::Svg, &RenderConfig::default())
            .expect("persisted measurements should allow provider-free replay");

        assert_eq!(replay_svg, direct_svg);
    }

    #[test]
    fn dynamic_measurement_sidecar_round_trips_same_text_under_two_styles() {
        let input = multi_font_same_text_input();
        let config = routed_config();
        let direct_svg = render_graph_family_with_dynamic_text_metrics(
            input,
            OutputFormat::Svg,
            &config,
            multi_font_style_set_input(),
            multi_font_width,
        )
        .expect("direct multi-font SVG should render");
        let dynamic_mmds = render_graph_family_with_dynamic_text_metrics(
            input,
            OutputFormat::Mmds,
            &config,
            multi_font_style_set_input(),
            multi_font_width,
        )
        .expect("dynamic multi-font MMDS should render");

        let value: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        let sidecar = &value["extensions"]["org.mmdflux.text-measurements.v1"];
        assert!(
            sidecar["textStyles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|style| style["id"] == "s1" && style["fontFamily"] == "Verdana"),
            "{sidecar}"
        );
        assert!(
            sidecar["textStyles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|style| style["id"] == "s2" && style["fontFamily"] == "Courier New"),
            "{sidecar}"
        );
        assert!(
            sidecar["lineWidths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["style"] == "s1" && entry["text"] == "Same"),
            "{sidecar}"
        );
        assert!(
            sidecar["lineWidths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["style"] == "s2" && entry["text"] == "Same"),
            "{sidecar}"
        );

        let replay_svg = render_diagram(&dynamic_mmds, OutputFormat::Svg, &RenderConfig::default())
            .expect("style-keyed sidecar should allow provider-free replay");

        assert_eq!(replay_svg, direct_svg);
    }

    #[test]
    fn dynamic_multifont_mermaid_fixture_round_trips_and_records_all_styles() {
        use std::cell::RefCell;
        use std::collections::BTreeMap;

        let calls = Rc::new(RefCell::new(BTreeMap::<(String, String), usize>::new()));
        let calls_for_svg = Rc::clone(&calls);
        let direct_svg = render_graph_family_with_dynamic_text_metrics(
            multi_font_mermaid_input(),
            OutputFormat::Svg,
            &routed_config(),
            multi_font_mermaid_style_set_input(),
            move |text, css_font| {
                *calls_for_svg
                    .borrow_mut()
                    .entry((css_font.to_string(), text.to_string()))
                    .or_default() += 1;
                multi_font_mermaid_width(text, css_font)
            },
        )
        .expect("direct dynamic multi-font SVG should render");
        assert!(
            direct_svg.contains("font-family=\"Verdana\""),
            "{direct_svg}"
        );
        assert!(
            direct_svg.contains("font-family=\"Courier New\""),
            "{direct_svg}"
        );
        assert!(
            direct_svg.contains("font-family=\"Times New Roman\""),
            "{direct_svg}"
        );
        assert!(
            direct_svg.contains("width=\"128.00\""),
            "edge label background should reflect Times New Roman width; {direct_svg}"
        );
        assert!(
            calls.borrow().values().all(|count| *count == 1),
            "{:?}",
            calls.borrow()
        );

        let dynamic_mmds = render_graph_family_with_dynamic_text_metrics(
            multi_font_mermaid_input(),
            OutputFormat::Mmds,
            &routed_config(),
            multi_font_mermaid_style_set_input(),
            multi_font_mermaid_width,
        )
        .expect("dynamic multi-font MMDS should render");
        let value: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        let sidecar = &value["extensions"]["org.mmdflux.text-measurements.v1"];
        for (style, family, text, width) in [
            ("s1", "Verdana", "Regular", 28.0),
            ("s2", "Courier New", "Styled Node", 165.0),
            ("s3", "Times New Roman", "link", 120.0),
        ] {
            assert!(
                sidecar["textStyles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["id"] == style && entry["fontFamily"] == family),
                "{sidecar}"
            );
            assert!(
                sidecar["lineWidths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| {
                        entry["style"] == style
                            && entry["text"] == text
                            && (entry["width"].as_f64().unwrap() - width).abs() < f64::EPSILON
                    }),
                "{sidecar}"
            );
        }

        let replay_svg = render_diagram(&dynamic_mmds, OutputFormat::Svg, &RenderConfig::default())
            .expect("provider-free sidecar replay should render");
        assert_eq!(replay_svg, direct_svg);
    }

    #[test]
    fn dynamic_mmds_without_measurements_still_requires_provider_free_replay_provider() {
        let dynamic_mmds = dynamic_mmds_fixture();
        let mut value: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        value["extensions"]
            .as_object_mut()
            .unwrap()
            .remove("org.mmdflux.text-measurements.v1");
        value["profiles"]
            .as_array_mut()
            .unwrap()
            .retain(|profile| profile != "mmdflux-text-measurements-v1");
        let without_measurements = serde_json::to_string_pretty(&value).unwrap();

        let err = render_diagram(
            &without_measurements,
            OutputFormat::Svg,
            &RenderConfig::default(),
        )
        .expect_err("dynamic MMDS without persisted measurements should still require provider");

        assert!(
            err.message.contains("requires a matching provider"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_mmds_with_measurements_rejects_provider_free_text_replay() {
        let dynamic_mmds = dynamic_mmds_fixture();

        for format in [OutputFormat::Text, OutputFormat::Ascii] {
            let err = render_diagram(&dynamic_mmds, format, &RenderConfig::default())
                .expect_err("provider-free dynamic text metrics replay stays SVG-only");

            assert!(
                err.message.contains("requires a matching provider"),
                "{err}"
            );
        }
    }

    #[test]
    fn live_provider_replay_does_not_consume_stale_measurement_sidecar() {
        let dynamic_mmds = dynamic_mmds_fixture();
        let mut stale: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        stale["extensions"]["org.mmdflux.text-measurements.v1"]["lineWidths"][0]["width"] =
            serde_json::json!(9999.0);
        let stale = serde_json::to_string_pretty(&stale).unwrap();

        let expected = render_graph_family_with_dynamic_text_metrics(
            &dynamic_mmds,
            OutputFormat::Svg,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect("matching live provider replay should render");
        let actual = render_graph_family_with_dynamic_text_metrics(
            &stale,
            OutputFormat::Svg,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect("matching live provider remains authoritative");

        assert_eq!(actual, expected);
    }

    #[test]
    fn live_provider_replay_validates_measurement_sidecar_shape_when_present() {
        let dynamic_mmds = dynamic_mmds_fixture();
        let mut malformed: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        malformed["extensions"]["org.mmdflux.text-measurements.v1"]["profileRef"]["id"] =
            serde_json::json!("wrong-provider");
        let malformed = serde_json::to_string_pretty(&malformed).unwrap();

        let err = render_graph_family_with_dynamic_text_metrics(
            &malformed,
            OutputFormat::Svg,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect_err("recognized malformed sidecar should reject provider-bound replay");

        assert!(err.message.contains("profileRef.id"), "{err}");
        assert!(
            err.message.contains("org.mmdflux.text-measurements.v1"),
            "{err}"
        );
    }

    #[test]
    fn mmds_to_mmds_pass_through_preserves_measurements_sidecar() {
        let dynamic_mmds = dynamic_mmds_fixture();

        let output = render_diagram(&dynamic_mmds, OutputFormat::Mmds, &RenderConfig::default())
            .expect("pass-through does not measure text");

        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(
            value["extensions"]
                .get("org.mmdflux.text-measurements.v1")
                .is_some()
        );
    }

    #[test]
    fn mmds_to_mmds_pass_through_validates_measurements_sidecar_shape() {
        let dynamic_mmds = dynamic_mmds_fixture();
        let mut malformed: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        malformed["extensions"]["org.mmdflux.text-measurements.v1"]["lineWidths"][0]["width"] =
            serde_json::json!("wide");
        let malformed = serde_json::to_string_pretty(&malformed).unwrap();

        let err = render_diagram(&malformed, OutputFormat::Mmds, &RenderConfig::default())
            .expect_err("pass-through should validate recognized sidecar shape");

        assert!(err.message.contains("lineWidths[0].width"), "{err}");
        assert!(
            err.message.contains("org.mmdflux.text-measurements.v1"),
            "{err}"
        );
    }

    #[test]
    fn provider_free_dynamic_replay_rejects_missing_scalar_measurement() {
        let dynamic_mmds = dynamic_mmds_fixture();
        let mut missing_query: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        missing_query["extensions"]["org.mmdflux.text-measurements.v1"]["scalarWidths"] =
            serde_json::json!([]);
        let missing_query = serde_json::to_string_pretty(&missing_query).unwrap();

        let err = render_diagram(&missing_query, OutputFormat::Svg, &RenderConfig::default())
            .expect_err("missing measured query should reject provider-free replay");

        assert!(err.message.contains("scalarWidths"), "{err}");
        assert!(err.message.contains("missing persisted"), "{err}");
    }

    #[test]
    fn dynamic_measurement_sidecar_rejects_duplicate_style_text_entries() {
        let dynamic_mmds = multi_font_dynamic_mmds_fixture();
        let mut duplicate: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        let line_widths = duplicate["extensions"]["org.mmdflux.text-measurements.v1"]["lineWidths"]
            .as_array_mut()
            .unwrap();
        let repeated = line_widths
            .iter()
            .find(|entry| entry["style"] == "s1" && entry["text"] == "Same")
            .unwrap()
            .clone();
        line_widths.push(repeated);
        let duplicate = serde_json::to_string_pretty(&duplicate).unwrap();

        let err = render_diagram(&duplicate, OutputFormat::Svg, &RenderConfig::default())
            .expect_err("duplicate style-keyed line width should reject");

        assert!(err.message.contains("duplicate lineWidths"), "{err}");
        assert!(err.message.contains("s1"), "{err}");
        assert!(err.message.contains("Same"), "{err}");
    }

    #[test]
    fn dynamic_measurement_sidecar_rejects_unknown_style_references() {
        let dynamic_mmds = multi_font_dynamic_mmds_fixture();
        let mut unknown_style: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        unknown_style["extensions"]["org.mmdflux.text-measurements.v1"]["lineWidths"][0]["style"] =
            serde_json::json!("missing-style");
        let unknown_style = serde_json::to_string_pretty(&unknown_style).unwrap();

        let err = render_diagram(&unknown_style, OutputFormat::Svg, &RenderConfig::default())
            .expect_err("unknown sidecar style id should reject");

        assert!(err.message.contains("lineWidths"), "{err}");
        assert!(err.message.contains("missing-style"), "{err}");
        assert!(err.message.contains("textStyles"), "{err}");
    }

    #[test]
    fn provider_free_dynamic_replay_rejects_sidecar_profile_ref_mismatch() {
        let dynamic_mmds = dynamic_mmds_fixture();
        let mut mismatched: serde_json::Value = serde_json::from_str(&dynamic_mmds).unwrap();
        mismatched["extensions"]["org.mmdflux.text-measurements.v1"]["profileRef"]["id"] =
            serde_json::json!("wrong-provider");
        let mismatched = serde_json::to_string_pretty(&mismatched).unwrap();

        let err = render_diagram(&mismatched, OutputFormat::Svg, &RenderConfig::default())
            .expect_err("profileRef mismatch should reject provider-free replay");

        assert!(err.message.contains("profileRef.id"), "{err}");
        assert!(
            err.message.contains("org.mmdflux.text-measurements.v1"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_mmds_output_requires_profile_id() {
        let err = render_graph_family_with_dynamic_text_metrics(
            "graph TD\nA-->B",
            OutputFormat::Mmds,
            &RenderConfig::default(),
            valid_input(),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("dynamic MMDS output should require provider identity");

        assert!(err.message.contains("profileId"), "{err}");
        assert!(err.message.contains("dynamic MMDS descriptor"), "{err}");
    }

    #[test]
    fn dynamic_mmds_output_rejects_sequence_input() {
        let mut calls = 0;
        let err = render_graph_family_with_dynamic_text_metrics(
            "sequenceDiagram\nAlice->>Bob: Hi",
            OutputFormat::Mmds,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            |_text, _css_font| {
                calls += 1;
                Ok(8.0)
            },
        )
        .expect_err("sequence input should fail");

        assert_eq!(calls, 0, "dynamic callback must not run for sequence");
        assert!(
            err.message.contains("sequence")
                && err.message.contains("timeline")
                && err.message.contains("unsupported"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_mmds_output_rejects_layout_engine_override() {
        let err = render_graph_family_with_dynamic_text_metrics(
            "graph TD\nA-->B",
            OutputFormat::Mmds,
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::MERMAID_LAYERED),
                ..RenderConfig::default()
            },
            profiled_input("browser-test-v1"),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("layout engine override should fail");

        assert!(
            err.message.contains("does not accept layoutEngine"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_mmds_replay_with_matching_provider_matches_direct_dynamic_svg() {
        let input = "graph TD\nA[Alpha] -->|a labeled edge| B[Beta]";
        let config = routed_config();
        let metrics = profiled_input("browser-test-v1");
        let direct_svg = render_graph_family_with_dynamic_text_metrics(
            input,
            OutputFormat::Svg,
            &config,
            metrics.clone(),
            deterministic_width,
        )
        .expect("direct dynamic SVG should render");
        let mmds = render_graph_family_with_dynamic_text_metrics(
            input,
            OutputFormat::Mmds,
            &config,
            metrics.clone(),
            deterministic_width,
        )
        .expect("dynamic MMDS should render");

        let replay_svg = render_graph_family_with_dynamic_text_metrics(
            &mmds,
            OutputFormat::Svg,
            &config,
            metrics,
            deterministic_width,
        )
        .expect("dynamic MMDS replay should render with matching provider");

        assert_eq!(replay_svg, direct_svg);
    }

    #[test]
    fn dynamic_mmds_replay_rejects_provider_descriptor_mismatches() {
        let mmds = dynamic_mmds_fixture();

        let mut version = profiled_input("browser-test-v1");
        version.profile_version = Some(2);
        let mut font_family = profiled_input("browser-test-v1");
        font_family.text_styles[0].font_family = "Arial".to_string();
        let mut font_size = profiled_input("browser-test-v1");
        font_size.text_styles[0].font_size = 18.0;
        let mut font_style = profiled_input("browser-test-v1");
        font_style.text_styles[0].font_style = "italic".to_string();
        let mut font_weight = profiled_input("browser-test-v1");
        font_weight.text_styles[0].font_weight = "700".to_string();
        let mut line_height = profiled_input("browser-test-v1");
        line_height.text_styles[0].line_height = 30.0;

        for (name, metrics, expected_field) in [
            (
                "profile id",
                profiled_input("browser-other-v1"),
                "metricsProfile.id",
            ),
            ("version", version, "metricsProfile.version"),
            ("font family", font_family, "defaultTextStyle.font-family"),
            ("font size", font_size, "defaultTextStyle.font-size"),
            ("font style", font_style, "defaultTextStyle.font-style"),
            ("font weight", font_weight, "defaultTextStyle.font-weight"),
            ("line height", line_height, "defaultTextStyle.line-height"),
        ] {
            let err = render_graph_family_with_dynamic_text_metrics(
                &mmds,
                OutputFormat::Svg,
                &routed_config(),
                metrics,
                deterministic_width,
            )
            .unwrap_err();
            assert!(err.message.contains(expected_field), "{name}: {err}");
        }

        let mut node_padding_config = routed_config();
        node_padding_config.svg_node_padding_x = Some(20.0);
        let err = render_graph_family_with_dynamic_text_metrics(
            &mmds,
            OutputFormat::Svg,
            &node_padding_config,
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect_err("node padding mismatch should fail");
        assert!(err.message.contains("layoutText.node-padding-x"), "{err}");

        let mut edge_width_config = routed_config();
        edge_width_config.layout.edge_label_max_width = Some(120.0);
        let err = render_graph_family_with_dynamic_text_metrics(
            &mmds,
            OutputFormat::Svg,
            &edge_width_config,
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect_err("edge label max width mismatch should fail");
        assert!(
            err.message.contains("layoutText.edge-label-max-width"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_mmds_replay_rejects_persisted_label_padding_mismatch() {
        let mut value: serde_json::Value = serde_json::from_str(&dynamic_mmds_fixture()).unwrap();
        value["extensions"]["org.mmdflux.text-metrics.v1"]["layoutText"]["label-padding-x"] =
            serde_json::json!(8.0);
        let mmds = serde_json::to_string(&value).unwrap();

        let err = render_graph_family_with_dynamic_text_metrics(
            &mmds,
            OutputFormat::Svg,
            &routed_config(),
            profiled_input("browser-test-v1"),
            deterministic_width,
        )
        .expect_err("label padding mismatch should fail");

        assert!(err.message.contains("layoutText.label-padding-x"), "{err}");
    }

    #[test]
    fn dynamic_mmds_replay_rejects_static_mmds_input() {
        let mmds = render_diagram(
            "graph TD\nA-->B",
            OutputFormat::Mmds,
            &RenderConfig::default(),
        )
        .unwrap();

        let err = render_graph_family_svg_with_dynamic_text_metrics(
            &mmds,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("static MMDS input should fail on the dynamic replay path");

        assert!(err.message.contains("metricsProfile.source"), "{err}");
        assert!(err.message.contains("not dynamic"), "{err}");
    }

    #[test]
    fn dynamic_svg_bridge_rejects_sequence_input() {
        let mut calls = 0;
        let err = render_graph_family_svg_with_dynamic_text_metrics(
            "sequenceDiagram\nAlice->>Bob: Hi",
            &RenderConfig::default(),
            valid_input(),
            |_text, _css_font| {
                calls += 1;
                Ok(8.0)
            },
        )
        .expect_err("sequence input should fail");

        assert_eq!(calls, 0, "dynamic callback must not run for sequence");
        assert!(
            err.message.contains("sequence")
                && err.message.contains("timeline")
                && err.message.contains("unsupported"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_mmds_replay_rejects_sequence_input_before_measurement() {
        let sequence_mmds = render_diagram(
            "sequenceDiagram\nAlice->>Bob: Hi",
            OutputFormat::Mmds,
            &RenderConfig::default(),
        )
        .expect("sequence MMDS should render");

        let mut calls = 0;
        let err = render_graph_family_with_dynamic_text_metrics(
            &sequence_mmds,
            OutputFormat::Svg,
            &RenderConfig::default(),
            profiled_input("browser-test-v1"),
            |_text, _css_font| {
                calls += 1;
                Ok(8.0)
            },
        )
        .expect_err("sequence MMDS should fail on dynamic replay path");

        assert_eq!(calls, 0, "dynamic callback must not run for sequence MMDS");
        assert!(
            err.message.contains("sequence")
                && err.message.contains("timeline")
                && err.message.contains("unsupported"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_svg_bridge_rejects_layout_engine_override() {
        let err = render_graph_family_svg_with_dynamic_text_metrics(
            "graph TD\nA-->B",
            &RenderConfig {
                layout_engine: Some(EngineAlgorithmId::MERMAID_LAYERED),
                ..RenderConfig::default()
            },
            valid_input(),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("layout engine override should fail");

        assert!(
            err.message.contains("does not accept layoutEngine"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_svg_bridge_rejects_static_font_metrics_profile() {
        let err = render_graph_family_svg_with_dynamic_text_metrics(
            "graph TD\nA-->B",
            &RenderConfig {
                font_metrics_profile: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID.to_string()),
                ..RenderConfig::default()
            },
            valid_input(),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("static font metrics profile should fail");

        assert!(
            err.message.contains("does not accept fontMetricsProfile"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_svg_rejects_config_graph_text_style() {
        let config = RenderConfig {
            graph_text_style: Some(GraphTextStyleConfig::new("Inter", 16.0)),
            ..RenderConfig::default()
        };

        let err = render_graph_family_svg_with_dynamic_text_metrics(
            "graph TD\nA-->B",
            &config,
            valid_input(),
            |_text, _css_font| Ok(8.0),
        )
        .expect_err("config graph text style should fail");

        assert!(err.message.contains("DynamicMetricsInput"), "{err}");
        assert!(
            err.message.contains("RenderConfig.graph_text_style"),
            "{err}"
        );
    }

    #[test]
    fn dynamic_svg_bridge_matches_static_svg_with_recorded_callback() {
        let input = "graph TD\nA[mmmm]\nB[iiii]";
        let static_svg =
            render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).unwrap();
        let recorded = resolve_text_metrics_profile(TextMetricsProfileConfig::default())
            .expect("default recorded text metrics should resolve")
            .metrics;
        let dynamic_svg = render_graph_family_svg_with_dynamic_text_metrics(
            input,
            &RenderConfig::default(),
            static_equivalent_input(),
            move |text, _css_font| Ok(recorded.measure_line_width(text)),
        )
        .unwrap();

        assert_eq!(dynamic_svg, static_svg);
    }
}
