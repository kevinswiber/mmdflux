//! Output format, curve, edge preset, and routing style definitions.

use std::str::FromStr;

/// Normalize an enum token: trim, lowercase, replace underscores with hyphens.
pub fn normalize_enum_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}
use crate::errors::RenderError;

/// Output format for rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Unicode text output (default).
    #[default]
    Text,
    /// ASCII-only text output.
    Ascii,
    /// SVG vector graphics.
    Svg,
    /// MMDS structured JSON output.
    Mmds,
    /// Mermaid syntax output (from MMDS input).
    Mermaid,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Ascii => write!(f, "ascii"),
            OutputFormat::Svg => write!(f, "svg"),
            OutputFormat::Mmds => write!(f, "mmds"),
            OutputFormat::Mermaid => write!(f, "mermaid"),
        }
    }
}

impl OutputFormat {
    /// Parse output format from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "text" => Ok(OutputFormat::Text),
            "ascii" => Ok(OutputFormat::Ascii),
            "svg" => Ok(OutputFormat::Svg),
            "mmds" | "json" => Ok(OutputFormat::Mmds),
            "mermaid" => Ok(OutputFormat::Mermaid),
            _ => Err(RenderError {
                message: format!("unknown output format: {s:?}"),
            }),
        }
    }
}

impl FromStr for OutputFormat {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        OutputFormat::parse(s)
    }
}

/// Path routing topology for SVG edge generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoutingStyle {
    /// Direct routing: single segment from source to target, bypassing waypoints.
    Direct,
    /// Polyline routing: engine computes waypoints; SVG connects them with line segments.
    Polyline,
    /// Orthogonal routing: engine enforces axis-aligned path segments.
    Orthogonal,
}

impl std::fmt::Display for RoutingStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingStyle::Direct => write!(f, "direct"),
            RoutingStyle::Polyline => write!(f, "polyline"),
            RoutingStyle::Orthogonal => write!(f, "orthogonal"),
        }
    }
}

impl RoutingStyle {
    /// Parse routing style from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "direct" => Ok(RoutingStyle::Direct),
            "polyline" => Ok(RoutingStyle::Polyline),
            "orthogonal" => Ok(RoutingStyle::Orthogonal),
            _ => Err(RenderError {
                message: format!(
                    "unknown routing style: {s:?} (expected one of: direct, polyline, orthogonal)"
                ),
            }),
        }
    }
}

impl FromStr for RoutingStyle {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RoutingStyle::parse(s)
    }
}

/// Corner arc treatment for SVG edge rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CornerStyle {
    /// Hard corners at waypoints (no arc rounding).
    Sharp,
    /// Rounded arc corners at waypoints.
    Rounded,
}

impl std::fmt::Display for CornerStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CornerStyle::Sharp => write!(f, "sharp"),
            CornerStyle::Rounded => write!(f, "rounded"),
        }
    }
}

impl CornerStyle {
    /// Parse corner style from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "sharp" => Ok(CornerStyle::Sharp),
            "rounded" => Ok(CornerStyle::Rounded),
            _ => Err(RenderError {
                message: format!("unknown corner style: {s:?} (expected one of: sharp, rounded)"),
            }),
        }
    }
}

impl FromStr for CornerStyle {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CornerStyle::parse(s)
    }
}

/// Path curve treatment for SVG edge rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Curve {
    /// Linear segments between waypoints, with corner treatment.
    Linear(CornerStyle),
    /// Cubic basis spline interpolation between waypoints.
    Basis,
}

impl std::fmt::Display for Curve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Curve::Linear(CornerStyle::Sharp) => write!(f, "linear"),
            Curve::Linear(CornerStyle::Rounded) => write!(f, "linear-rounded"),
            Curve::Basis => write!(f, "basis"),
        }
    }
}

impl Curve {
    /// Parse curve style from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "basis" => Ok(Curve::Basis),
            "linear" | "linear-sharp" => Ok(Curve::Linear(CornerStyle::Sharp)),
            "linear-rounded" => Ok(Curve::Linear(CornerStyle::Rounded)),
            "catmull-rom" | "catmullrom" => Err(RenderError {
                message: "\"catmull-rom\" curve is recognized but not yet implemented. \
                          Use \"basis\", \"linear\", \"linear-sharp\", or \"linear-rounded\"."
                    .into(),
            }),
            _ => Err(RenderError {
                message: format!(
                    "unknown curve: {s:?} (expected one of: basis, linear, linear-sharp, linear-rounded)"
                ),
            }),
        }
    }
}

impl FromStr for Curve {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Curve::parse(s)
    }
}

/// User-facing edge style preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgePreset {
    /// Direct straight segment with sharp corners.
    Straight,
    /// Multi-segment polyline with sharp corners.
    Polyline,
    /// Orthogonal (right-angle) path with sharp corners.
    Step,
    /// Orthogonal path with rounded arc corners.
    SmoothStep,
    /// Orthogonal path rendered with basis interpolation.
    CurvedStep,
    /// Polyline with basis curve rendering.
    Basis,
}

impl EdgePreset {
    /// Expand this preset into `(RoutingStyle, Curve)`.
    pub fn expand(self) -> (RoutingStyle, Curve) {
        match self {
            EdgePreset::Straight => (RoutingStyle::Direct, Curve::Linear(CornerStyle::Sharp)),
            EdgePreset::Polyline => (RoutingStyle::Polyline, Curve::Linear(CornerStyle::Sharp)),
            EdgePreset::Step => (RoutingStyle::Orthogonal, Curve::Linear(CornerStyle::Sharp)),
            EdgePreset::SmoothStep => (
                RoutingStyle::Orthogonal,
                Curve::Linear(CornerStyle::Rounded),
            ),
            EdgePreset::CurvedStep => (RoutingStyle::Orthogonal, Curve::Basis),
            EdgePreset::Basis => (RoutingStyle::Polyline, Curve::Basis),
        }
    }

    /// Parse edge preset from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "straight" => Ok(EdgePreset::Straight),
            "polyline" => Ok(EdgePreset::Polyline),
            "step" => Ok(EdgePreset::Step),
            "smooth-step" | "smoothstep" => Ok(EdgePreset::SmoothStep),
            "curved-step" | "curvedstep" => Ok(EdgePreset::CurvedStep),
            "basis" => Ok(EdgePreset::Basis),
            "direct" => Err(RenderError {
                message: "\"direct\" is a routing style, not an edge preset. \
                          Use --routing-style direct or --edge-preset straight."
                    .into(),
            }),
            _ => Err(RenderError {
                message: format!(
                    "unknown edge preset: {s:?} (expected one of: straight, polyline, step, smooth-step, curved-step, basis)"
                ),
            }),
        }
    }
}

impl std::fmt::Display for EdgePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgePreset::Straight => write!(f, "straight"),
            EdgePreset::Polyline => write!(f, "polyline"),
            EdgePreset::Step => write!(f, "step"),
            EdgePreset::SmoothStep => write!(f, "smooth-step"),
            EdgePreset::CurvedStep => write!(f, "curved-step"),
            EdgePreset::Basis => write!(f, "basis"),
        }
    }
}

impl FromStr for EdgePreset {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EdgePreset::parse(s)
    }
}

/// Caller-facing text color policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorWhen {
    /// Never emit ANSI color.
    Off,
    /// Emit ANSI color only when the output sink supports it.
    #[default]
    Auto,
    /// Always emit ANSI color.
    Always,
}

impl ColorWhen {
    /// Parse text color policy from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "off" => Ok(ColorWhen::Off),
            "auto" => Ok(ColorWhen::Auto),
            "always" => Ok(ColorWhen::Always),
            _ => Err(RenderError {
                message: format!(
                    "unknown color policy: {s:?} (expected one of: off, auto, always)"
                ),
            }),
        }
    }

    /// Resolve the caller-facing policy into a renderer-facing mode.
    pub fn resolve(self, output_is_terminal: bool) -> TextColorMode {
        match self {
            ColorWhen::Off => TextColorMode::Plain,
            ColorWhen::Auto => {
                if output_is_terminal {
                    TextColorMode::Ansi
                } else {
                    TextColorMode::Plain
                }
            }
            ColorWhen::Always => TextColorMode::Ansi,
        }
    }
}

impl std::fmt::Display for ColorWhen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorWhen::Off => write!(f, "off"),
            ColorWhen::Auto => write!(f, "auto"),
            ColorWhen::Always => write!(f, "always"),
        }
    }
}

impl FromStr for ColorWhen {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ColorWhen::parse(s)
    }
}

/// Resolved renderer-facing text color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextColorMode {
    /// Plain text output with no ANSI color.
    #[default]
    Plain,
    /// ANSI-capable text output.
    Ansi,
}

impl TextColorMode {
    pub fn uses_ansi(self) -> bool {
        matches!(self, TextColorMode::Ansi)
    }
}
