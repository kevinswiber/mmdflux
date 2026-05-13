//! Shared graph-family measurement primitives.
//!
//! Graph-owned measurement stays renderer-agnostic: engines use grid
//! measurement for discrete replay layouts and proportional measurement for
//! float-space geometry.

#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::graph::font_metrics::{
    RECORDED_SANS_CSS_LINE_HEIGHT_RATIO, RECORDED_SANS_PROFILE_ID, RECORDED_SANS_PROFILE_SOURCE,
    RecordedMetricsProfile,
};
use crate::graph::style::{EdgeStyle, NodeStyle, parse_font_size_px};
use crate::graph::{Direction, Edge, Node, Shape};

/// Compatibility profile for the existing function-backed proportional heuristic.
pub const COMPATIBILITY_TEXT_METRICS_PROFILE_ID: &str = "mmdflux-heuristic-proportional-v1";
/// Recorded profile backed by static generated mmdflux sans metrics.
pub const RECORDED_SANS_TEXT_METRICS_PROFILE_ID: &str = RECORDED_SANS_PROFILE_ID;
/// Direct graph-family default text metrics profile.
pub const DEFAULT_TEXT_METRICS_PROFILE_ID: &str = RECORDED_SANS_TEXT_METRICS_PROFILE_ID;
/// Legacy fallback profile for MMDS documents that predate text-metrics metadata.
pub const LEGACY_MMDS_TEXT_METRICS_PROFILE_ID: &str = COMPATIBILITY_TEXT_METRICS_PROFILE_ID;
/// Supported graph-family text metrics profile identifiers.
pub const SUPPORTED_TEXT_METRICS_PROFILE_IDS: &[&str] = &[
    COMPATIBILITY_TEXT_METRICS_PROFILE_ID,
    RECORDED_SANS_TEXT_METRICS_PROFILE_ID,
];
/// Human-readable supported profile list used in validation messages.
pub const SUPPORTED_TEXT_METRICS_PROFILE_IDS_TEXT: &str =
    "mmdflux-heuristic-proportional-v1, mmdflux-sans-v1";
/// Default graph-family SVG font family used by proportional measurement.
pub const DEFAULT_GRAPH_FONT_FAMILY: &str = "\"trebuchet ms\", verdana, arial, sans-serif";
/// Default font size used for proportional measurement.
pub const DEFAULT_PROPORTIONAL_FONT_SIZE: f64 = 16.0;
/// Default horizontal node padding used for proportional measurement.
pub const DEFAULT_PROPORTIONAL_NODE_PADDING_X: f64 = 15.0;
/// Default vertical node padding used for proportional measurement.
pub const DEFAULT_PROPORTIONAL_NODE_PADDING_Y: f64 = 15.0;
/// Default horizontal padding applied around edge labels (per side).
///
/// This matches the `<rect class="graph-edge-label-bg">` horizontal padding
/// emitted by `render::graph::svg::text` so layout-time dummy reservations and
/// render-time backgrounds stay in lockstep.
pub const DEFAULT_LABEL_PADDING_X: f64 = 4.0;
/// Default vertical padding applied around edge labels (per side).
pub const DEFAULT_LABEL_PADDING_Y: f64 = 2.0;
/// Default maximum width used for graph-family edge-label wrapping.
pub const DEFAULT_EDGE_LABEL_MAX_WIDTH: f64 = 200.0;
/// Scale factor applied to approximate Mermaid's measured text widths.
const TEXT_WIDTH_SCALE: f64 = 1.16;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ProportionalTextMetrics {
    pub font_size: f64,
    pub line_height: f64,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub label_padding_x: f64,
    pub label_padding_y: f64,
    width_model: ProportionalWidthModel,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GraphTextStyleKey {
    pub(crate) font_family: String,
    pub(crate) font_size_mpx: u32,
    pub(crate) line_height_mpx: u32,
    pub(crate) font_style: String,
    pub(crate) font_weight: String,
}

impl GraphTextStyleKey {
    pub(crate) fn new(
        font_family: impl Into<String>,
        font_size_px: f64,
        line_height_px: f64,
        font_style: impl Into<String>,
        font_weight: impl Into<String>,
    ) -> Result<Self, String> {
        let font_family = non_empty_style_string("font-family", font_family.into())?;
        let font_style = non_empty_style_string("font-style", font_style.into())?;
        let font_weight = non_empty_style_string("font-weight", font_weight.into())?;
        Ok(Self {
            font_family,
            font_size_mpx: px_to_millipixels("font-size", font_size_px)?,
            line_height_mpx: px_to_millipixels("line-height", line_height_px)?,
            font_style,
            font_weight,
        })
    }

    pub(crate) fn default_profile_style(metrics: &ProportionalTextMetrics) -> Self {
        Self::new(
            DEFAULT_GRAPH_FONT_FAMILY,
            metrics.font_size,
            metrics.line_height,
            "normal",
            "400",
        )
        .expect("default profile text style is valid")
    }

    pub(crate) fn default_provider_style(provider: &dyn TextMetricsProvider) -> Self {
        provider.default_text_style_key()
    }

    pub(crate) fn from_descriptor(descriptor: &TextMetricsStyleDescriptor) -> Result<Self, String> {
        Self::new(
            &descriptor.font_family,
            descriptor.font_size,
            descriptor.line_height,
            &descriptor.font_style,
            &descriptor.font_weight,
        )
    }

    pub(crate) fn font_size_px(&self) -> f64 {
        self.font_size_mpx as f64 / 1000.0
    }

    pub(crate) fn line_height_px(&self) -> f64 {
        self.line_height_mpx as f64 / 1000.0
    }

    #[cfg(test)]
    pub(crate) fn test(name: &str) -> Self {
        Self::new(name, DEFAULT_PROPORTIONAL_FONT_SIZE, 24.0, "normal", "400")
            .expect("test style is valid")
    }
}

fn non_empty_style_string(field: &str, value: String) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{field} cannot be empty"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn px_to_millipixels(field: &str, value: f64) -> Result<u32, String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{field} must be a positive finite number"));
    }
    let rounded = (value * 1000.0).round();
    if rounded > u32::MAX as f64 {
        return Err(format!("{field} is too large"));
    }
    Ok(rounded as u32)
}

pub(crate) fn node_text_style_key(
    provider: &dyn TextMetricsProvider,
    node: &Node,
) -> GraphTextStyleKey {
    text_style_key_from_node_style(provider, &node.style)
}

pub(crate) fn edge_text_style_key(
    provider: &dyn TextMetricsProvider,
    edge: &Edge,
) -> GraphTextStyleKey {
    text_style_key_from_edge_style(provider, &edge.style)
}

fn text_style_key_from_node_style(
    provider: &dyn TextMetricsProvider,
    style: &NodeStyle,
) -> GraphTextStyleKey {
    text_style_key_from_parts(
        provider,
        style.font_family.as_deref(),
        style.font_size.as_deref(),
        style.font_style.as_deref(),
        style.font_weight.as_deref(),
    )
}

fn text_style_key_from_edge_style(
    provider: &dyn TextMetricsProvider,
    style: &EdgeStyle,
) -> GraphTextStyleKey {
    text_style_key_from_parts(
        provider,
        style.font_family.as_deref(),
        style.font_size.as_deref(),
        style.font_style.as_deref(),
        style.font_weight.as_deref(),
    )
}

fn text_style_key_from_parts(
    provider: &dyn TextMetricsProvider,
    font_family: Option<&str>,
    font_size: Option<&str>,
    font_style: Option<&str>,
    font_weight: Option<&str>,
) -> GraphTextStyleKey {
    let default_style = provider.default_text_style_key();
    let font_size_px = font_size
        .and_then(|value| parse_font_size_px(value).ok())
        .unwrap_or_else(|| default_style.font_size_px());
    let line_height_px = provider_line_height_for_font_size(provider, font_size_px);
    GraphTextStyleKey::new(
        font_family.unwrap_or(default_style.font_family.as_str()),
        font_size_px,
        line_height_px,
        font_style.unwrap_or(default_style.font_style.as_str()),
        font_weight.unwrap_or(default_style.font_weight.as_str()),
    )
    .expect("graph text style fields are valid after parser validation and defaults")
}

fn provider_line_height_for_font_size(
    provider: &dyn TextMetricsProvider,
    font_size_px: f64,
) -> f64 {
    let base_font_size = provider.font_size();
    let base_line_height = provider.line_height();
    if base_font_size.is_finite()
        && base_font_size > 0.0
        && base_line_height.is_finite()
        && base_line_height > 0.0
    {
        font_size_px * (base_line_height / base_font_size)
    } else {
        font_size_px * 1.5
    }
}

/// Internal graph-family text measurement seam.
///
/// This is intentionally crate-local until browser/provider-backed measurement
/// has a public API contract.
pub(crate) trait TextMetricsProvider {
    fn measure_line_width(&self, text: &str) -> f64;
    fn measure_scalar_width(&self, ch: char) -> f64;
    fn font_size(&self) -> f64;
    fn line_height(&self) -> f64;
    fn node_padding_x(&self) -> f64;
    fn node_padding_y(&self) -> f64;
    fn label_padding_x(&self) -> f64;
    fn label_padding_y(&self) -> f64;

    fn measure_space_width(&self) -> f64 {
        self.measure_scalar_width(' ')
    }

    fn default_text_style_key(&self) -> GraphTextStyleKey {
        GraphTextStyleKey::new(
            DEFAULT_GRAPH_FONT_FAMILY,
            self.font_size(),
            self.line_height(),
            "normal",
            "400",
        )
        .expect("default provider text style is valid")
    }

    fn measure_line_width_for_style(&self, _style: &GraphTextStyleKey, text: &str) -> f64 {
        self.measure_line_width(text)
    }

    fn measure_scalar_width_for_style(&self, _style: &GraphTextStyleKey, ch: char) -> f64 {
        self.measure_scalar_width(ch)
    }

    fn font_size_for_style(&self, _style: &GraphTextStyleKey) -> f64 {
        self.font_size()
    }

    fn line_height_for_style(&self, _style: &GraphTextStyleKey) -> f64 {
        self.line_height()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextMeasurementCache {
    pub default_style: String,
    pub text_styles: BTreeMap<String, TextMeasurementStyle>,
    pub line_widths: BTreeMap<(String, String), f64>,
    pub scalar_widths: BTreeMap<(String, char), f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextMeasurementStyle {
    pub(crate) id: String,
    pub(crate) style: GraphTextStyleKey,
    pub(crate) css_font: String,
}

impl TextMeasurementCache {
    pub(crate) fn line_width(&self, text: &str) -> Option<f64> {
        self.line_width_for_style(&self.default_style, text)
    }

    pub(crate) fn scalar_width(&self, ch: char) -> Option<f64> {
        self.scalar_width_for_style(&self.default_style, ch)
    }

    pub(crate) fn line_width_for_style(&self, style_id: &str, text: &str) -> Option<f64> {
        self.line_widths
            .get(&(style_id.to_string(), text.to_string()))
            .copied()
    }

    pub(crate) fn scalar_width_for_style(&self, style_id: &str, ch: char) -> Option<f64> {
        self.scalar_widths.get(&(style_id.to_string(), ch)).copied()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ProportionalWidthModel {
    CompatibilityHeuristic { scale: f64 },
    Recorded(RecordedMetricsProfile),
}

impl ProportionalWidthModel {
    fn measure_line_width(&self, font_size: f64, text: &str) -> f64 {
        match self {
            Self::CompatibilityHeuristic { scale } => {
                text.chars()
                    .map(|c| compatibility_char_width_ratio(c) * font_size)
                    .sum::<f64>()
                    * scale
            }
            Self::Recorded(profile) => text
                .chars()
                .map(|c| profile.measure_scalar_width(font_size, c))
                .sum::<f64>(),
        }
    }

    fn measure_scalar_width(&self, font_size: f64, ch: char) -> f64 {
        match self {
            Self::CompatibilityHeuristic { scale } => {
                compatibility_char_width_ratio(ch) * font_size * scale
            }
            Self::Recorded(profile) => profile.measure_scalar_width(font_size, ch),
        }
    }
}

impl ProportionalTextMetrics {
    pub fn new(font_size: f64, node_padding_x: f64, node_padding_y: f64) -> Self {
        Self::compatibility(font_size, node_padding_x, node_padding_y)
    }

    fn compatibility(font_size: f64, node_padding_x: f64, node_padding_y: f64) -> Self {
        Self {
            font_size,
            line_height: font_size * 1.5,
            node_padding_x,
            node_padding_y,
            label_padding_x: DEFAULT_LABEL_PADDING_X,
            label_padding_y: DEFAULT_LABEL_PADDING_Y,
            width_model: ProportionalWidthModel::CompatibilityHeuristic {
                scale: TEXT_WIDTH_SCALE,
            },
        }
    }

    fn recorded_sans(font_size: f64, node_padding_x: f64, node_padding_y: f64) -> Self {
        Self {
            font_size,
            line_height: font_size * RECORDED_SANS_CSS_LINE_HEIGHT_RATIO,
            node_padding_x,
            node_padding_y,
            label_padding_x: DEFAULT_LABEL_PADDING_X,
            label_padding_y: DEFAULT_LABEL_PADDING_Y,
            width_model: ProportionalWidthModel::Recorded(RecordedMetricsProfile::mmdflux_sans_v1()),
        }
    }

    pub fn measure_text_with_padding(
        &self,
        text: &str,
        padding_x: f64,
        padding_y: f64,
    ) -> (f64, f64) {
        measure_text_with_padding_for_provider(self, text, padding_x, padding_y)
    }

    pub fn edge_label_dimensions(&self, label: &str) -> (f64, f64) {
        edge_label_dimensions_for_provider(self, label)
    }

    /// Dimensions of an edge label that has already been wrapped into `lines`.
    ///
    /// Width = max width across lines; height = `line_height * lines.len()`
    /// plus symmetric label padding on both axes. Mirrors
    /// [`Self::edge_label_dimensions`] so the wrapped path is a drop-in
    /// replacement wherever `wrapped_label_lines` is populated.
    pub fn edge_label_dimensions_wrapped(&self, lines: &[String]) -> (f64, f64) {
        edge_label_dimensions_wrapped_for_provider(self, lines)
    }

    pub(crate) fn measure_line_width(&self, text: &str) -> f64 {
        self.width_model.measure_line_width(self.font_size, text)
    }

    pub(crate) fn measure_scalar_width(&self, c: char) -> f64 {
        self.width_model.measure_scalar_width(self.font_size, c)
    }

    pub(crate) fn measure_space_width(&self) -> f64 {
        self.measure_scalar_width(' ')
    }

    pub(crate) fn char_width_ratio(&self, c: char) -> f64 {
        compatibility_char_width_ratio(c)
    }
}

impl TextMetricsProvider for ProportionalTextMetrics {
    fn measure_line_width(&self, text: &str) -> f64 {
        // Use the inherent method explicitly; `self.measure_line_width(...)`
        // would recurse into this trait method.
        ProportionalTextMetrics::measure_line_width(self, text)
    }

    fn measure_scalar_width(&self, ch: char) -> f64 {
        // Use the inherent method explicitly; `self.measure_scalar_width(...)`
        // would recurse into this trait method.
        ProportionalTextMetrics::measure_scalar_width(self, ch)
    }

    fn font_size(&self) -> f64 {
        self.font_size
    }

    fn line_height(&self) -> f64 {
        self.line_height
    }

    fn node_padding_x(&self) -> f64 {
        self.node_padding_x
    }

    fn node_padding_y(&self) -> f64 {
        self.node_padding_y
    }

    fn label_padding_x(&self) -> f64 {
        self.label_padding_x
    }

    fn label_padding_y(&self) -> f64 {
        self.label_padding_y
    }
}

fn compatibility_char_width_ratio(c: char) -> f64 {
    match c {
        'i' | 'l' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' => 0.25,
        'f' | 'j' | 't' | 'r' => 0.32,
        'm' | 'w' | 'M' | 'W' => 0.7,
        'A'..='Z' => 0.48,
        _ => 0.46,
    }
}

/// Caller-provided inputs for resolving a concrete text metrics profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetricsProfileConfig<'a> {
    pub profile_id: Option<&'a str>,
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub edge_label_max_width: Option<f64>,
}

impl Default for TextMetricsProfileConfig<'_> {
    fn default() -> Self {
        Self {
            profile_id: None,
            node_padding_x: DEFAULT_PROPORTIONAL_NODE_PADDING_X,
            node_padding_y: DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
            edge_label_max_width: Some(DEFAULT_EDGE_LABEL_MAX_WIDTH),
        }
    }
}

/// Fully resolved text metrics identity and measurement implementation.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTextMetrics {
    pub descriptor: TextMetricsProfileDescriptor,
    pub metrics: ProportionalTextMetrics,
}

/// Persistable identity for the text metrics used by graph-family layout.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetricsProfileDescriptor {
    pub profile_id: String,
    pub source: String,
    pub version: u32,
    pub default_text_style: TextMetricsStyleDescriptor,
    pub layout_text: TextMetricsLayoutDescriptor,
}

/// Default text style associated with a text metrics profile.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetricsStyleDescriptor {
    pub font_family: String,
    pub font_size: f64,
    pub font_style: String,
    pub font_weight: String,
    pub line_height: f64,
}

/// Layout-time padding and wrapping parameters associated with a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetricsLayoutDescriptor {
    pub node_padding_x: f64,
    pub node_padding_y: f64,
    pub label_padding_x: f64,
    pub label_padding_y: f64,
    pub edge_label_max_width: Option<f64>,
}

/// Error returned when a requested text metrics profile is not implemented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedTextMetricsProfile {
    profile_id: String,
}

impl UnsupportedTextMetricsProfile {
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }
}

impl std::fmt::Display for UnsupportedTextMetricsProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unsupported text metrics profile '{}' (supported: {})",
            self.profile_id,
            supported_text_metrics_profile_ids()
        )
    }
}

impl std::error::Error for UnsupportedTextMetricsProfile {}

pub fn validate_text_metrics_profile_id(
    profile_id: &str,
) -> Result<(), UnsupportedTextMetricsProfile> {
    if is_supported_text_metrics_profile_id(profile_id) {
        Ok(())
    } else {
        Err(UnsupportedTextMetricsProfile {
            profile_id: profile_id.to_string(),
        })
    }
}

fn is_supported_text_metrics_profile_id(profile_id: &str) -> bool {
    SUPPORTED_TEXT_METRICS_PROFILE_IDS.contains(&profile_id)
}

pub fn supported_text_metrics_profile_ids() -> &'static str {
    SUPPORTED_TEXT_METRICS_PROFILE_IDS_TEXT
}

pub fn resolve_text_metrics_profile(
    config: TextMetricsProfileConfig<'_>,
) -> Result<ResolvedTextMetrics, UnsupportedTextMetricsProfile> {
    let profile_id = config.profile_id.unwrap_or(DEFAULT_TEXT_METRICS_PROFILE_ID);
    validate_text_metrics_profile_id(profile_id)?;

    let (metrics, source) = match profile_id {
        COMPATIBILITY_TEXT_METRICS_PROFILE_ID => (
            ProportionalTextMetrics::compatibility(
                DEFAULT_PROPORTIONAL_FONT_SIZE,
                config.node_padding_x,
                config.node_padding_y,
            ),
            "heuristic",
        ),
        RECORDED_SANS_TEXT_METRICS_PROFILE_ID => (
            ProportionalTextMetrics::recorded_sans(
                DEFAULT_PROPORTIONAL_FONT_SIZE,
                config.node_padding_x,
                config.node_padding_y,
            ),
            RECORDED_SANS_PROFILE_SOURCE,
        ),
        _ => unreachable!("profile validation restricts supported IDs"),
    };
    let descriptor =
        text_metrics_descriptor(profile_id, source, &metrics, config.edge_label_max_width);

    Ok(ResolvedTextMetrics {
        descriptor,
        metrics,
    })
}

fn text_metrics_descriptor(
    profile_id: &str,
    source: &str,
    metrics: &ProportionalTextMetrics,
    edge_label_max_width: Option<f64>,
) -> TextMetricsProfileDescriptor {
    TextMetricsProfileDescriptor {
        profile_id: profile_id.to_string(),
        source: source.to_string(),
        version: 1,
        default_text_style: TextMetricsStyleDescriptor {
            font_family: DEFAULT_GRAPH_FONT_FAMILY.to_string(),
            font_size: metrics.font_size,
            font_style: "normal".to_string(),
            font_weight: "400".to_string(),
            line_height: metrics.line_height,
        },
        layout_text: TextMetricsLayoutDescriptor {
            node_padding_x: metrics.node_padding_x,
            node_padding_y: metrics.node_padding_y,
            label_padding_x: metrics.label_padding_x,
            label_padding_y: metrics.label_padding_y,
            edge_label_max_width,
        },
    }
}

pub(crate) fn text_style_matches_descriptor(
    font_family: &str,
    font_size_px: f64,
    descriptor: &TextMetricsStyleDescriptor,
) -> Result<bool, String> {
    let requested_family = font_family_compare_key(font_family)?;
    let descriptor_family = font_family_compare_key(&descriptor.font_family)?;
    Ok(requested_family == descriptor_family
        && (font_size_px - descriptor.font_size).abs() <= f64::EPSILON)
}

pub(crate) fn font_family_compare_key(value: &str) -> Result<Vec<String>, String> {
    let tokens: Vec<String> = value
        .split(',')
        .map(normalize_font_family_token)
        .collect::<Result<_, _>>()?;

    if tokens.is_empty() {
        return Err("must contain at least one font family".to_string());
    }

    Ok(tokens)
}

fn normalize_font_family_token(token: &str) -> Result<String, String> {
    let token = token.trim();
    let token = strip_one_quote_layer(token).trim();
    let token = collapse_ascii_whitespace(token);
    if token.is_empty() {
        return Err("must not contain empty font family tokens".to_string());
    }
    Ok(token.to_ascii_lowercase())
}

fn strip_one_quote_layer(token: &str) -> &str {
    if token.len() >= 2 {
        let bytes = token.as_bytes();
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &token[1..token.len() - 1];
        }
    }
    token
}

fn collapse_ascii_whitespace(value: &str) -> String {
    let mut normalized = String::new();
    let mut previous_was_space = false;

    for ch in value.chars() {
        if ch.is_ascii_whitespace() {
            if !normalized.is_empty() && !previous_was_space {
                normalized.push(' ');
            }
            previous_was_space = true;
        } else {
            normalized.push(ch);
            previous_was_space = false;
        }
    }

    normalized.trim().to_string()
}

/// Greedy word-wrap that honors `max_width` in pixels using `metrics` for
/// per-character width estimates. `'\n'` in `text` is treated as a hard
/// break; each segment is wrapped independently. Falls back to per-character
/// splits when a single word exceeds `max_width`.
///
/// **Sequencing contract:** callers MUST normalize `<br>`/`<br/>`/`<br />`
/// variants to `'\n'` before calling this function. `wrap_lines` does not
/// inspect the raw Mermaid source.
pub fn wrap_lines(metrics: &ProportionalTextMetrics, text: &str, max_width: f64) -> Vec<String> {
    wrap_lines_with_provider(metrics, text, max_width)
}

pub(crate) fn measure_text_with_padding_for_provider(
    provider: &dyn TextMetricsProvider,
    text: &str,
    padding_x: f64,
    padding_y: f64,
) -> (f64, f64) {
    let style = GraphTextStyleKey::default_provider_style(provider);
    measure_text_with_padding_for_provider_and_style(provider, &style, text, padding_x, padding_y)
}

pub(crate) fn measure_text_with_padding_for_provider_and_style(
    provider: &dyn TextMetricsProvider,
    style: &GraphTextStyleKey,
    text: &str,
    padding_x: f64,
    padding_y: f64,
) -> (f64, f64) {
    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1) as f64;
    let max_width = lines
        .iter()
        .map(|line| provider.measure_line_width_for_style(style, line))
        .fold(0.0, f64::max);
    let width = max_width + padding_x * 2.0;
    let height = provider.line_height_for_style(style) * line_count + padding_y * 2.0;
    (width, height)
}

pub(crate) fn edge_label_dimensions_for_provider(
    provider: &dyn TextMetricsProvider,
    label: &str,
) -> (f64, f64) {
    let style = GraphTextStyleKey::default_provider_style(provider);
    edge_label_dimensions_for_provider_and_style(provider, &style, label)
}

pub(crate) fn edge_label_dimensions_for_provider_and_style(
    provider: &dyn TextMetricsProvider,
    style: &GraphTextStyleKey,
    label: &str,
) -> (f64, f64) {
    measure_text_with_padding_for_provider_and_style(
        provider,
        style,
        label,
        provider.label_padding_x(),
        provider.label_padding_y(),
    )
}

pub(crate) fn edge_label_dimensions_wrapped_for_provider(
    provider: &dyn TextMetricsProvider,
    lines: &[String],
) -> (f64, f64) {
    let style = GraphTextStyleKey::default_provider_style(provider);
    edge_label_dimensions_wrapped_for_provider_and_style(provider, &style, lines)
}

pub(crate) fn edge_label_dimensions_wrapped_for_provider_and_style(
    provider: &dyn TextMetricsProvider,
    style: &GraphTextStyleKey,
    lines: &[String],
) -> (f64, f64) {
    let line_count = lines.len().max(1) as f64;
    let max_width = lines
        .iter()
        .map(|line| provider.measure_line_width_for_style(style, line))
        .fold(0.0, f64::max);
    let width = max_width + provider.label_padding_x() * 2.0;
    let height =
        provider.line_height_for_style(style) * line_count + provider.label_padding_y() * 2.0;
    (width, height)
}

pub(crate) fn wrap_lines_with_provider(
    provider: &dyn TextMetricsProvider,
    text: &str,
    max_width: f64,
) -> Vec<String> {
    let style = GraphTextStyleKey::default_provider_style(provider);
    wrap_lines_with_provider_for_style(provider, &style, text, max_width)
}

pub(crate) fn wrap_lines_with_provider_for_style(
    provider: &dyn TextMetricsProvider,
    style: &GraphTextStyleKey,
    text: &str,
    max_width: f64,
) -> Vec<String> {
    let space_w = provider.measure_scalar_width_for_style(style, ' ');
    let mut out = Vec::new();
    for segment in text.split('\n') {
        let mut current = String::new();
        let mut current_w = 0.0_f64;
        for word in segment.split_whitespace() {
            let ww = provider.measure_line_width_for_style(style, word);
            if ww > max_width {
                // Oversized word: fall back to per-character splits regardless
                // of whether the word is first on the line. GPT-5.4 review of
                // PR #235 found the previous `&& current.is_empty()` guard let
                // oversized trailing words overflow `max_width`. Flush the
                // current line first so the char-split starts at column 0.
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    current_w = 0.0;
                }
                for ch in word.chars() {
                    let cw = provider.measure_scalar_width_for_style(style, ch);
                    if current_w + cw > max_width && !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                        current_w = 0.0;
                    }
                    current.push(ch);
                    current_w += cw;
                }
                continue;
            }
            let sep_w = if current.is_empty() { 0.0 } else { space_w };
            if current_w + sep_w + ww > max_width && !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_w = 0.0;
            }
            if !current.is_empty() {
                current.push(' ');
                current_w += space_w;
            }
            current.push_str(word);
            current_w += ww;
        }
        if !current.is_empty() {
            out.push(current);
        } else {
            out.push(String::new());
        }
    }
    out
}

/// Compatibility proportional metrics used by legacy engine-side callers.
///
/// This intentionally stays pinned to the heuristic profile even though direct
/// graph-family rendering now defaults to `mmdflux-sans-v1`; sequence and
/// internal compatibility paths consume this function directly.
pub fn default_proportional_text_metrics() -> ProportionalTextMetrics {
    ProportionalTextMetrics::new(
        DEFAULT_PROPORTIONAL_FONT_SIZE,
        DEFAULT_PROPORTIONAL_NODE_PADDING_X,
        DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
    )
}

#[cfg(test)]
pub(crate) fn default_proportional_text_metrics_provider() -> &'static ProportionalTextMetrics {
    static METRICS: std::sync::OnceLock<ProportionalTextMetrics> = std::sync::OnceLock::new();
    METRICS.get_or_init(default_proportional_text_metrics)
}

/// Calculate the grid dimensions needed to replay a node in the grid surface.
pub fn grid_node_dimensions(node: &Node, direction: Direction) -> (usize, usize) {
    let lines: Vec<&str> = node.label.split('\n').collect();
    let max_line_len = lines
        .iter()
        .filter(|l| **l != Node::SEPARATOR)
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);

    // TextBlock nodes are borderless labels (e.g. lollipop interface names).
    // Minimal dimensions keep the label tightly adjacent to the edge marker.
    if node.shape == Shape::TextBlock {
        let w = max_line_len.max(1);
        let h = lines.len().max(1);
        return (w, h);
    }

    let (w, h) = (max_line_len + 4, lines.len() + 2);

    if node.shape == Shape::ForkJoin
        && node.label.trim().is_empty()
        && matches!(direction, Direction::LeftRight | Direction::RightLeft)
    {
        return (h, w);
    }

    (w, h)
}

/// Grid edge-label dimensions used by layered measurement and grid replay.
pub fn grid_edge_label_dimensions(label: &str) -> (f64, f64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Grid edge-label dimensions for a pre-wrapped label. Mirrors
/// [`grid_edge_label_dimensions`] but consumes a persisted wrap artifact
/// instead of `'\n'`-splitting raw text, so wrap decisions made in pixel
/// units are honored by the Grid measurement path too.
pub fn grid_edge_label_dimensions_wrapped(lines: &[String]) -> (f64, f64) {
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

/// Proportional node dimensions used during layered measurement and float layout.
pub fn proportional_node_dimensions(
    metrics: &ProportionalTextMetrics,
    node: &Node,
    direction: Direction,
) -> (f64, f64) {
    proportional_node_dimensions_with_provider(metrics, node, direction)
}

pub(crate) fn proportional_node_dimensions_with_provider(
    provider: &dyn TextMetricsProvider,
    node: &Node,
    direction: Direction,
) -> (f64, f64) {
    let style = node_text_style_key(provider, node);
    let (label_w, label_h) =
        measure_text_with_padding_for_provider_and_style(provider, &style, &node.label, 0.0, 0.0);

    let (mut width, mut height) = match node.shape {
        Shape::Rectangle => (
            label_w + provider.node_padding_x() * 4.0,
            label_h + provider.node_padding_y() * 2.0,
        ),
        Shape::Diamond => {
            let w = label_w + provider.node_padding_x();
            let h = label_h + provider.node_padding_y();
            let size = w + h;
            (size, size)
        }
        Shape::Stadium => {
            let h = label_h + provider.node_padding_y() * 2.0;
            let radius = h / 2.0;
            (label_w + provider.node_padding_x() * 2.0 + radius, h)
        }
        Shape::Cylinder => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let rx = w / 2.0;
            let ry = rx / (2.5 + w / 50.0);
            (w, label_h + provider.node_padding_y() * 2.0 + ry)
        }
        Shape::Document => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w, h + h / 8.0)
        }
        Shape::Documents => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            let offset = 5.0;
            (w + 2.0 * offset, h + h / 4.0 + 2.0 * offset)
        }
        Shape::TaggedDocument => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 3.0;
            (w * 1.1, h + h / 4.0)
        }
        Shape::Card => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w + 12.0, h)
        }
        Shape::TaggedRect => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w + 0.2 * h, h)
        }
        Shape::Subroutine => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w + 20.0, h)
        }
        Shape::Hexagon => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w + h / 2.0, h)
        }
        Shape::Parallelogram | Shape::InvParallelogram | Shape::Trapezoid | Shape::InvTrapezoid => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w + h / 3.0, h)
        }
        Shape::Asymmetric => {
            let w = label_w + provider.node_padding_x() * 2.0;
            let h = label_h + provider.node_padding_y() * 2.0;
            (w + h / 3.0, h)
        }
        Shape::SmallCircle => (14.0, 14.0),
        Shape::FramedCircle => (28.0, 28.0),
        Shape::CrossedCircle => (60.0, 60.0),
        Shape::ForkJoin if node.label.trim().is_empty() => (70.0, 7.0),
        Shape::TextBlock => (label_w, label_h),
        _ => (
            label_w + provider.node_padding_x() * 2.0,
            label_h + provider.node_padding_y() * 2.0,
        ),
    };

    match node.shape {
        Shape::Hexagon | Shape::Trapezoid | Shape::InvTrapezoid | Shape::Asymmetric => {
            width *= 1.15;
        }
        Shape::Circle
        | Shape::DoubleCircle
        | Shape::SmallCircle
        | Shape::FramedCircle
        | Shape::CrossedCircle => {
            let size = width.max(height);
            width = size;
            height = size;
        }
        _ => {}
    }

    if node.shape == Shape::ForkJoin
        && node.label.trim().is_empty()
        && matches!(direction, Direction::LeftRight | Direction::RightLeft)
    {
        std::mem::swap(&mut width, &mut height);
    }

    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;
    use crate::internal_tests::stub_metrics::FixedWidthProvider;

    #[test]
    fn measure_text_uses_proportional_heuristic() {
        let metrics = ProportionalTextMetrics::new(16.0, 8.0, 6.4);
        let (w, h) = metrics.measure_text_with_padding("ABC", 0.0, 0.0);

        assert!(w > 16.0);
        assert!(h > 16.0);
    }

    #[test]
    fn text_metrics_default_profile_identity_is_explicit() {
        let resolved = resolve_text_metrics_profile(TextMetricsProfileConfig::default())
            .expect("default text metrics profile should resolve");

        assert_eq!(
            resolved.descriptor.profile_id,
            RECORDED_SANS_TEXT_METRICS_PROFILE_ID
        );
        assert_eq!(resolved.descriptor.source, "recorded");
        assert_eq!(resolved.descriptor.version, 1);
        assert_eq!(
            resolved.descriptor.default_text_style.font_family,
            DEFAULT_GRAPH_FONT_FAMILY
        );
        assert_eq!(resolved.descriptor.default_text_style.font_size, 16.0);
        assert_eq!(resolved.descriptor.default_text_style.font_style, "normal");
        assert_eq!(resolved.descriptor.default_text_style.font_weight, "400");
        assert_eq!(resolved.descriptor.default_text_style.line_height, 24.0);
        assert_eq!(resolved.descriptor.layout_text.node_padding_x, 15.0);
        assert_eq!(resolved.descriptor.layout_text.node_padding_y, 15.0);
        assert_eq!(resolved.descriptor.layout_text.label_padding_x, 4.0);
        assert_eq!(resolved.descriptor.layout_text.label_padding_y, 2.0);
        assert_eq!(
            resolved.descriptor.layout_text.edge_label_max_width,
            Some(200.0)
        );
    }

    #[test]
    fn proportional_text_metrics_implements_provider_contract() {
        let metrics = resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some(RECORDED_SANS_TEXT_METRICS_PROFILE_ID),
            node_padding_x: 11.0,
            node_padding_y: 7.0,
            edge_label_max_width: Some(123.0),
        })
        .unwrap()
        .metrics;

        let provider: &dyn TextMetricsProvider = &metrics;

        assert_eq!(provider.font_size(), metrics.font_size);
        assert_eq!(provider.line_height(), metrics.line_height);
        assert_eq!(provider.node_padding_x(), metrics.node_padding_x);
        assert_eq!(provider.node_padding_y(), metrics.node_padding_y);
        assert_eq!(provider.label_padding_x(), metrics.label_padding_x);
        assert_eq!(provider.label_padding_y(), metrics.label_padding_y);
        assert_eq!(
            provider.measure_line_width("Abc"),
            metrics.measure_line_width("Abc")
        );
        assert_eq!(
            provider.measure_space_width(),
            metrics.measure_space_width()
        );
    }

    #[test]
    fn graph_text_style_key_is_hashable_and_orderable_without_raw_f64_fields() {
        fn assert_key<T: Eq + Ord + std::hash::Hash>() {}

        assert_key::<GraphTextStyleKey>();
    }

    #[test]
    fn style_aware_provider_distinguishes_same_text_under_different_styles() {
        let provider = StyleStubProvider::new()
            .with_line_width("small", "Same", 20.0)
            .with_line_width("large", "Same", 80.0);

        let small = GraphTextStyleKey::test("small");
        let large = GraphTextStyleKey::test("large");

        assert_eq!(provider.measure_line_width_for_style(&small, "Same"), 20.0);
        assert_eq!(provider.measure_line_width_for_style(&large, "Same"), 80.0);
    }

    #[test]
    fn static_profile_default_style_matches_existing_widths() {
        let metrics = default_proportional_text_metrics();
        let default_style = GraphTextStyleKey::default_profile_style(&metrics);

        assert_eq!(
            metrics.measure_line_width_for_style(&default_style, "Alpha"),
            metrics.measure_line_width("Alpha")
        );
        assert_eq!(
            metrics.measure_scalar_width_for_style(&default_style, 'A'),
            metrics.measure_scalar_width('A')
        );
        assert_eq!(
            metrics.font_size_for_style(&default_style),
            metrics.font_size()
        );
        assert_eq!(
            metrics.line_height_for_style(&default_style),
            metrics.line_height()
        );
    }

    #[test]
    fn wrap_lines_can_use_stub_provider_widths() {
        let provider = FixedWidthProvider;

        let lines = wrap_lines_with_provider(&provider, "mmmm iiii", 130.0);

        assert_eq!(lines, vec!["mmmm", "iiii"]);
    }

    #[test]
    fn proportional_node_dimensions_can_use_provider_padding_and_widths() {
        let provider = FixedWidthProvider;
        let node = Node::new("A").with_label("mmmm");

        let (width, height) =
            proportional_node_dimensions_with_provider(&provider, &node, Direction::TopDown);

        assert_eq!(width, 30.0 * 4.0 + provider.node_padding_x() * 4.0);
        assert_eq!(
            height,
            provider.line_height() + provider.node_padding_y() * 2.0
        );
    }

    #[test]
    fn text_metrics_compatibility_profile_matches_existing_heuristic_exactly() {
        let direct = default_proportional_text_metrics();
        let resolved = resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID),
            ..TextMetricsProfileConfig::default()
        })
        .expect("compatibility text metrics profile should resolve");

        assert_eq!(resolved.metrics.font_size, direct.font_size);
        assert_eq!(resolved.metrics.line_height, direct.line_height);
        assert_eq!(resolved.metrics.node_padding_x, direct.node_padding_x);
        assert_eq!(resolved.metrics.node_padding_y, direct.node_padding_y);
        assert_eq!(resolved.metrics.label_padding_x, direct.label_padding_x);
        assert_eq!(resolved.metrics.label_padding_y, direct.label_padding_y);

        for sample in ["ABC", "minimum width", "millimeter WWW", "A labeled edge"] {
            assert_eq!(
                resolved.metrics.measure_line_width(sample),
                direct.measure_line_width(sample),
                "resolved metrics drifted for sample {sample:?}"
            );
            assert_eq!(
                resolved.metrics.measure_text_with_padding(sample, 3.0, 5.0),
                direct.measure_text_with_padding(sample, 3.0, 5.0),
                "resolved padded metrics drifted for sample {sample:?}"
            );
        }
    }

    #[test]
    fn compatibility_width_model_preserves_existing_scaled_widths() {
        let metrics = default_proportional_text_metrics();
        let expected = 16.0 * 1.16 * (0.48 + 0.46 + 0.46);
        let actual = metrics.measure_line_width("Abc");

        assert!(
            (actual - expected).abs() < 1e-9,
            "actual={actual} expected={expected}"
        );
    }

    #[test]
    fn wrap_lines_uses_profile_aware_width_helpers() {
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(
            &metrics,
            "Alpha Beta",
            metrics.measure_line_width("Alpha") + 1.0,
        );

        assert_eq!(lines, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn resolves_mmdflux_sans_v1_profile() {
        let resolved = resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some("mmdflux-sans-v1"),
            ..TextMetricsProfileConfig::default()
        })
        .expect("recorded profile resolves");

        assert_eq!(resolved.descriptor.profile_id, "mmdflux-sans-v1");
        assert_eq!(resolved.descriptor.source, "recorded");
        assert_eq!(resolved.metrics.line_height, 24.0);
    }

    #[test]
    fn compatibility_profile_source_is_heuristic() {
        let resolved = resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some(COMPATIBILITY_TEXT_METRICS_PROFILE_ID),
            ..TextMetricsProfileConfig::default()
        })
        .expect("compatibility profile resolves");

        assert_eq!(
            resolved.descriptor.profile_id,
            "mmdflux-heuristic-proportional-v1"
        );
        assert_eq!(resolved.descriptor.source, "heuristic");
    }

    #[test]
    fn recorded_profile_widths_are_static_and_scale_linearly() {
        let metrics_16 = resolve_recorded_for_test(16.0);
        let metrics_24 = resolve_recorded_for_test(24.0);

        assert!(metrics_16.measure_line_width("mmmm") > metrics_16.measure_line_width("iiii"));
        assert_approx_eq(
            metrics_24.measure_line_width("Alpha"),
            metrics_16.measure_line_width("Alpha") * 1.5,
        );
    }

    #[test]
    fn recorded_profile_fallback_buckets_are_deterministic() {
        let metrics = resolve_recorded_for_test(16.0);

        assert_approx_eq(
            metrics.measure_scalar_width('\t'),
            metrics.measure_space_width() * 4.0,
        );
        assert_approx_eq(metrics.measure_scalar_width('\u{0301}'), 0.0);
        assert_approx_eq(metrics.measure_scalar_width('😀'), 16.0);
        assert_approx_eq(metrics.measure_scalar_width('\u{E000}'), 16.0 * 0.56);
        assert_approx_eq(metrics.measure_scalar_width('\u{0001}'), 0.0);
    }

    #[test]
    fn text_metrics_unsupported_profile_is_explicit_error() {
        let err = resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some("mermaid-sans-v1"),
            ..TextMetricsProfileConfig::default()
        })
        .expect_err("unsupported profile must not silently fall back");

        assert_eq!(err.profile_id(), "mermaid-sans-v1");
        assert!(
            err.to_string()
                .contains("unsupported text metrics profile 'mermaid-sans-v1'"),
            "{err}"
        );
    }

    #[test]
    fn grid_node_dimensions_match_grid_box_model() {
        let node = Node::new("A").with_label("Hello");
        let (w, h) = grid_node_dimensions(&node, Direction::TopDown);
        assert_eq!((w, h), (9, 3));
    }

    // -- wrapped-dims measurement primitive --

    #[test]
    fn edge_label_dimensions_wrapped_measures_multi_line_height() {
        let metrics = default_proportional_text_metrics();
        let lines = vec!["short".to_string(), "another line".to_string()];
        let (w, h) = metrics.edge_label_dimensions_wrapped(&lines);
        let expected_min_h = 2.0 * metrics.line_height + 2.0 * metrics.label_padding_y - 0.001;
        assert!(
            h >= expected_min_h,
            "expected >= 2 line heights (~{expected_min_h}), got {h}"
        );
        assert!(
            w < 200.0,
            "width should be bounded by max line, got {w} for lines {lines:?}"
        );
    }

    #[test]
    fn edge_label_dimensions_wrapped_matches_unwrapped_single_line() {
        let metrics = default_proportional_text_metrics();
        let (w_single, h_single) = metrics.edge_label_dimensions("exactly the same");
        let (w_wrap, h_wrap) =
            metrics.edge_label_dimensions_wrapped(&["exactly the same".to_string()]);
        assert!((w_single - w_wrap).abs() < 0.001);
        assert!((h_single - h_wrap).abs() < 0.001);
    }

    // -- Label padding contract --

    #[test]
    fn proportional_text_metrics_default_label_padding_matches_svg_constants() {
        let metrics = ProportionalTextMetrics::new(16.0, 15.0, 15.0);
        assert_eq!(metrics.label_padding_x, DEFAULT_LABEL_PADDING_X);
        assert_eq!(metrics.label_padding_y, DEFAULT_LABEL_PADDING_Y);
        assert_eq!(DEFAULT_LABEL_PADDING_X, 4.0);
        assert_eq!(DEFAULT_LABEL_PADDING_Y, 2.0);
    }

    // -- wrap_lines greedy word-wrap --

    #[test]
    fn wrap_lines_greedy_breaks_on_word_boundaries() {
        let metrics = ProportionalTextMetrics::new(
            DEFAULT_PROPORTIONAL_FONT_SIZE,
            DEFAULT_PROPORTIONAL_NODE_PADDING_X,
            DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
        );
        // max_width 100 is wider than every word in the fixture so the
        // greedy path never hits the char-fallback — this test pins
        // whitespace-boundary wrap specifically (char fallback is covered
        // by `wrap_lines_char_fallback_triggers_for_mid_line_oversized_word`).
        let lines = wrap_lines(&metrics, "this is a deliberately long label", 100.0);
        assert!(lines.len() >= 2, "expected multi-line wrap, got {lines:?}");
        // Word boundaries preserved: no line begins or ends mid-word.
        for line in &lines {
            assert!(
                !line.starts_with(' ') && !line.ends_with(' '),
                "line has stray whitespace: {line:?}"
            );
        }
        // Round-tripping words preserves the original token order and content.
        let all: String = lines.join(" ");
        assert_eq!(all, "this is a deliberately long label");
    }

    #[test]
    fn wrap_lines_empty_returns_single_empty_line() {
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(&metrics, "", 200.0);
        assert_eq!(lines, vec![String::new()]);
    }

    #[test]
    fn wrap_lines_fits_when_max_width_is_large() {
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(&metrics, "single", 10_000.0);
        assert_eq!(lines, vec!["single".to_string()]);
    }

    // -- <br>-derived '\n' acts as a hard segment break --

    #[test]
    fn wrap_lines_preserves_br_hard_breaks_as_segment_boundaries() {
        let metrics = ProportionalTextMetrics::new(
            DEFAULT_PROPORTIONAL_FONT_SIZE,
            DEFAULT_PROPORTIONAL_NODE_PADDING_X,
            DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
        );
        // Simulate output of normalize_br_tags: <br> has already become '\n'.
        // max_width 100 keeps every word under the threshold so the test
        // focuses on segment-boundary behavior rather than char fallback.
        let input = "yes\nsome very long continuation";
        let lines = wrap_lines(&metrics, input, 100.0);

        // The first segment ("yes") stands alone on its own line — not merged with "some".
        assert_eq!(
            lines[0], "yes",
            "first segment must stand alone, got {lines:?}"
        );

        // The second segment's tokens are wrapped across the remaining lines, in order.
        let rest: String = lines[1..].join(" ");
        assert_eq!(rest, "some very long continuation");
    }

    // GPT-5.4 review of PR #235: oversized-word char-fallback must apply
    // even when the oversized word is NOT the first token on a line.
    #[test]
    fn wrap_lines_char_fallback_triggers_for_mid_line_oversized_word() {
        let metrics = default_proportional_text_metrics();
        let max_width = 200.0;
        let lines = wrap_lines(
            &metrics,
            "short supercalifragilisticexpialidocious",
            max_width,
        );
        for line in &lines {
            let w = metrics.measure_line_width(line);
            assert!(
                w <= max_width + 0.5,
                "line {line:?} is {w} px wide, exceeds max_width {max_width}"
            );
        }
    }

    #[test]
    fn wrap_lines_empty_middle_segment_preserved() {
        // Refactor invariant from Task 1.3: "a\n\nb" → ["a", "", "b"].
        let metrics = default_proportional_text_metrics();
        let lines = wrap_lines(&metrics, "a\n\nb", 1000.0);
        assert_eq!(lines, vec!["a".to_string(), String::new(), "b".to_string()]);
    }

    #[test]
    fn edge_label_dimensions_includes_padding() {
        let metrics = ProportionalTextMetrics::new(16.0, 15.0, 15.0);
        let (w_padded, h_padded) = metrics.edge_label_dimensions("foo");
        let (w_raw, h_raw) = metrics.measure_text_with_padding("foo", 0.0, 0.0);
        assert!(
            (w_padded - (w_raw + 2.0 * metrics.label_padding_x)).abs() < 0.01,
            "expected w_padded={} to equal w_raw={} + 2*pad_x={}",
            w_padded,
            w_raw,
            metrics.label_padding_x,
        );
        assert!(
            (h_padded - (h_raw + 2.0 * metrics.label_padding_y)).abs() < 0.01,
            "expected h_padded={} to equal h_raw={} + 2*pad_y={}",
            h_padded,
            h_raw,
            metrics.label_padding_y,
        );
    }

    fn resolve_recorded_for_test(font_size: f64) -> ProportionalTextMetrics {
        let mut metrics = resolve_text_metrics_profile(TextMetricsProfileConfig {
            profile_id: Some("mmdflux-sans-v1"),
            ..TextMetricsProfileConfig::default()
        })
        .expect("recorded profile resolves")
        .metrics;
        metrics.font_size = font_size;
        metrics.line_height = font_size * 1.5;
        metrics
    }

    fn assert_approx_eq(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "actual={actual} expected={expected}"
        );
    }

    #[derive(Default)]
    struct StyleStubProvider {
        line_widths: std::collections::BTreeMap<(GraphTextStyleKey, String), f64>,
    }

    impl StyleStubProvider {
        fn new() -> Self {
            Self::default()
        }

        fn with_line_width(mut self, style: &str, text: &str, width: f64) -> Self {
            self.line_widths
                .insert((GraphTextStyleKey::test(style), text.to_string()), width);
            self
        }
    }

    impl TextMetricsProvider for StyleStubProvider {
        fn measure_line_width(&self, text: &str) -> f64 {
            self.measure_line_width_for_style(&GraphTextStyleKey::test("default"), text)
        }

        fn measure_scalar_width(&self, _ch: char) -> f64 {
            8.0
        }

        fn measure_line_width_for_style(&self, style: &GraphTextStyleKey, text: &str) -> f64 {
            self.line_widths
                .get(&(style.clone(), text.to_string()))
                .copied()
                .unwrap_or(0.0)
        }

        fn font_size(&self) -> f64 {
            16.0
        }

        fn line_height(&self) -> f64 {
            24.0
        }

        fn node_padding_x(&self) -> f64 {
            15.0
        }

        fn node_padding_y(&self) -> f64 {
            15.0
        }

        fn label_padding_x(&self) -> f64 {
            DEFAULT_LABEL_PADDING_X
        }

        fn label_padding_y(&self) -> f64 {
            DEFAULT_LABEL_PADDING_Y
        }
    }
}
