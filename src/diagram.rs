//! Diagram abstraction traits for multi-diagram support.
//!
//! This module defines the core traits that allow mmdflux to support
//! multiple diagram types with different parsers, layout engines, and renderers.

use std::error::Error;
use std::str::FromStr;

/// Diagram family classification.
///
/// Families group diagram types by their layout and rendering strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramFamily {
    /// Node-edge graphs (flowchart, state, class, ER).
    Graph,
    /// Timeline-based (sequence, gantt, gitgraph).
    Timeline,
    /// Chart/visualization (pie, radar, xy).
    Chart,
    /// Tabular layout (packet, kanban).
    Table,
}

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

/// Path routing topology for SVG edge generation.
///
/// Controls how edge paths are computed between waypoints.
///
/// Engine constraints:
/// - `flux-layered` supports `Direct`, `Polyline`, and `Orthogonal`.
/// - `mermaid-layered` supports `Polyline` only.
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
    ///
    /// Accepts: `direct`, `polyline`, `orthogonal`.
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
///
/// Used as the corner subtype for `Curve::Linear`.
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
    ///
    /// Accepts: `sharp`, `rounded`.
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
///
/// Controls how segments between waypoints are drawn.
/// `catmull-rom` is recognized but not yet implemented.
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
    ///
    /// Accepts: `basis`, `linear`, `linear-sharp`, `linear-rounded`.
    /// `catmull-rom` is recognized but returns a "not yet implemented" error.
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
///
/// Expands deterministically to `(RoutingStyle, Curve)`:
/// - `Straight` → `Direct + Linear(Sharp)`
/// - `Polyline` → `Polyline + Linear(Sharp)`
/// - `Step` → `Orthogonal + Linear(Sharp)`
/// - `SmoothStep` → `Orthogonal + Linear(Rounded)`
/// - `CurvedStep` → `Orthogonal + Basis`
/// - `Basis` → `Polyline + Basis`
///
/// Precedence: explicit low-level fields > preset defaults > engine defaults.
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
    ///
    /// Accepts: `straight`, `polyline`, `step`, `smooth-step`, `curved-step`, `basis`.
    ///
    /// Legacy aliases accepted for compatibility: `smoothstep` (for `smooth-step`).
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
    ///
    /// Accepts `json` as an alias for `mmds`.
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

/// Metadata common to all diagram types.
///
/// Every diagram model must implement this trait to provide
/// basic metadata and lifecycle operations.
pub trait DiagramModel: Send + Sync {
    /// Clear/reset the model state.
    fn clear(&mut self);

    /// Get the diagram title, if set.
    fn title(&self) -> Option<&str>;

    /// Get the accessibility title, if set.
    fn acc_title(&self) -> Option<&str>;

    /// Get the accessibility description, if set.
    fn acc_description(&self) -> Option<&str>;
}

/// Parser that converts text input into a diagram model.
///
/// Each diagram type provides its own parser implementation.
pub trait DiagramParser: Send + Sync {
    /// The model type this parser produces.
    type Model: DiagramModel;
    /// Error type for parse failures.
    type Error: Error + Send + Sync + 'static;

    /// Parse input text into a diagram model.
    fn parse(&self, input: &str) -> Result<Self::Model, Self::Error>;
}

/// Renderer that produces output from a diagram model.
///
/// Renderers convert a parsed diagram model into a specific output format.
pub trait DiagramRenderer: Send + Sync {
    /// The model type this renderer consumes.
    type Model: DiagramModel;

    /// Render the model to a string in the specified format.
    fn render(
        &self,
        model: &Self::Model,
        format: OutputFormat,
        config: &RenderConfig,
    ) -> Result<String, RenderError>;

    /// Check if this renderer supports the given output format.
    fn supports_format(&self, format: OutputFormat) -> bool;
}

/// Configuration for layout computation.
///
/// This is a re-export of `layered::types::LayoutConfig` to provide a single
/// canonical layout configuration type across the crate.
pub type LayoutConfig = crate::layered::types::LayoutConfig;

/// Engine family identifier used in the combined engine+algorithm taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineId {
    /// mmdflux-native Sugiyama implementation (recommended default).
    Flux,
    /// Mermaid-compatible Sugiyama with dagre.js parity semantics.
    Mermaid,
    /// Eclipse Layout Kernel — requires `engine-elk` feature.
    Elk,
}

/// Algorithm identifier used in the combined engine+algorithm taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlgorithmId {
    /// Sugiyama layered hierarchical layout.
    Layered,
    /// ELK Mr. Tree algorithm.
    MrTree,
}

/// Combined engine+algorithm identifier for public selection.
///
/// Replaces the legacy `LayoutEngineId` in the public API. Accepted tokens:
/// `flux-layered`, `mermaid-layered`, `elk-layered`, `elk-mrtree`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineAlgorithmId {
    engine: EngineId,
    algorithm: AlgorithmId,
}

impl EngineAlgorithmId {
    pub fn new(engine: EngineId, algorithm: AlgorithmId) -> Self {
        Self { engine, algorithm }
    }

    pub fn engine(&self) -> EngineId {
        self.engine
    }

    pub fn algorithm(&self) -> AlgorithmId {
        self.algorithm
    }

    /// Parse a combined engine+algorithm ID string (case-insensitive, trims whitespace).
    ///
    /// Accepts: `flux-layered`, `mermaid-layered`, `elk-layered`, `elk-mrtree`.
    /// Legacy tokens (`dagre`, `elk`, `cose`) produce actionable migration errors.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "flux-layered" => Ok(Self::new(EngineId::Flux, AlgorithmId::Layered)),
            "mermaid-layered" => Ok(Self::new(EngineId::Mermaid, AlgorithmId::Layered)),
            "elk-layered" => Ok(Self::new(EngineId::Elk, AlgorithmId::Layered)),
            "elk-mrtree" => Ok(Self::new(EngineId::Elk, AlgorithmId::MrTree)),
            "dagre" => Err(RenderError {
                message: "\"dagre\" is no longer a valid engine ID. \
                          Use \"flux-layered\" (recommended) or \"mermaid-layered\"."
                    .into(),
            }),
            "elk" => Err(RenderError {
                message: "\"elk\" is no longer a valid engine ID. \
                          Use \"elk-layered\" or \"elk-mrtree\"."
                    .into(),
            }),
            "cose" | "cose-bilkent" => Err(RenderError {
                message: "\"cose\" is no longer supported. Use \"flux-layered\".".into(),
            }),
            other => Err(RenderError {
                message: format!(
                    "unknown engine: {other:?}. Valid options: \
                     flux-layered, mermaid-layered, elk-layered, elk-mrtree"
                ),
            }),
        }
    }
}

impl std::fmt::Display for EngineAlgorithmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.engine, self.algorithm) {
            (EngineId::Flux, AlgorithmId::Layered) => write!(f, "flux-layered"),
            (EngineId::Mermaid, AlgorithmId::Layered) => write!(f, "mermaid-layered"),
            (EngineId::Elk, AlgorithmId::Layered) => write!(f, "elk-layered"),
            (EngineId::Elk, AlgorithmId::MrTree) => write!(f, "elk-mrtree"),
            // Future variants — use engine-algorithm form as fallback
            (EngineId::Flux, AlgorithmId::MrTree) => write!(f, "flux-mrtree"),
            (EngineId::Mermaid, AlgorithmId::MrTree) => write!(f, "mermaid-mrtree"),
        }
    }
}

impl FromStr for EngineAlgorithmId {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EngineAlgorithmId::parse(s)
    }
}

/// How edge routing is owned for a given engine+algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOwnership {
    /// Engine+algo computes final routed paths natively (e.g., flux-layered).
    Native,
    /// Engine provides waypoint hints; compatibility router finalizes paths (e.g., mermaid-layered).
    HintDriven,
    /// External engine returns fully routed paths (e.g., elk-layered, elk-mrtree).
    EngineProvided,
}

impl RouteOwnership {
    /// Whether this ownership model produces routed edge paths.
    pub fn routes_edges(&self) -> bool {
        matches!(
            self,
            RouteOwnership::Native | RouteOwnership::EngineProvided
        )
    }
}

/// Capabilities for a combined engine+algorithm pair.
#[derive(Debug, Clone)]
pub struct EngineAlgorithmCapabilities {
    pub route_ownership: RouteOwnership,
    pub supports_subgraphs: bool,
    /// Routing styles this engine supports.
    ///
    /// `None` routing in a solve request means "use engine default". Engines
    /// validate explicit routing style requests against this slice and return
    /// an actionable error for unsupported combinations.
    pub supported_routing_styles: &'static [RoutingStyle],
}

impl EngineAlgorithmId {
    /// Check whether this engine+algorithm is available at runtime.
    ///
    /// Returns `Ok(())` if available, or an actionable error naming the required feature flag.
    pub fn check_available(&self) -> Result<(), RenderError> {
        match self.engine {
            EngineId::Flux | EngineId::Mermaid => Ok(()),
            EngineId::Elk => {
                #[cfg(feature = "engine-elk")]
                {
                    Ok(())
                }
                #[cfg(not(feature = "engine-elk"))]
                {
                    Err(RenderError {
                        message: format!(
                            "{} is not available; rebuild with the `engine-elk` feature flag enabled",
                            self
                        ),
                    })
                }
            }
        }
    }

    /// Static capability matrix for this engine+algorithm combination.
    pub fn capabilities(&self) -> EngineAlgorithmCapabilities {
        match (self.engine, self.algorithm) {
            (EngineId::Flux, AlgorithmId::Layered) => EngineAlgorithmCapabilities {
                route_ownership: RouteOwnership::Native,
                supports_subgraphs: true,
                supported_routing_styles: &[
                    RoutingStyle::Direct,
                    RoutingStyle::Polyline,
                    RoutingStyle::Orthogonal,
                ],
            },
            (EngineId::Mermaid, AlgorithmId::Layered) => EngineAlgorithmCapabilities {
                route_ownership: RouteOwnership::HintDriven,
                supports_subgraphs: true,
                supported_routing_styles: &[RoutingStyle::Polyline],
            },
            (EngineId::Elk, AlgorithmId::Layered) => EngineAlgorithmCapabilities {
                route_ownership: RouteOwnership::EngineProvided,
                supports_subgraphs: true,
                supported_routing_styles: &[RoutingStyle::Polyline, RoutingStyle::Orthogonal],
            },
            (EngineId::Elk, AlgorithmId::MrTree) => EngineAlgorithmCapabilities {
                route_ownership: RouteOwnership::EngineProvided,
                supports_subgraphs: false,
                supported_routing_styles: &[RoutingStyle::Polyline],
            },
            _ => EngineAlgorithmCapabilities {
                route_ownership: RouteOwnership::HintDriven,
                supports_subgraphs: false,
                supported_routing_styles: &[RoutingStyle::Polyline],
            },
        }
    }

    /// Resolve the edge routing algorithm for a given routing style.
    ///
    /// Maps `(route_ownership, routing_style)` to the concrete `EdgeRouting` variant.
    /// Used by the render pipeline and class instance SVG path to select the routing algorithm.
    ///
    /// - `None` routing style means "use engine default" → resolves to `Orthogonal` for Native.
    pub fn edge_routing_for_style(&self, routing_style: Option<RoutingStyle>) -> EdgeRouting {
        match self.capabilities().route_ownership {
            RouteOwnership::Native => match routing_style {
                Some(RoutingStyle::Direct) => EdgeRouting::DirectRoute,
                Some(RoutingStyle::Polyline) => EdgeRouting::PolylineRoute,
                _ => EdgeRouting::OrthogonalRoute,
            },
            RouteOwnership::HintDriven => EdgeRouting::PolylineRoute,
            RouteOwnership::EngineProvided => EdgeRouting::EngineProvided,
        }
    }

    /// Validate that the requested routing style is supported by this engine.
    ///
    /// Resolves the effective routing style from the config (explicit > preset > none),
    /// then checks it against `capabilities().supported_routing_styles`.
    /// Returns `Ok(())` if supported or if no routing is explicitly requested.
    pub fn check_routing_style(&self, config: &RenderConfig) -> Result<(), RenderError> {
        // Resolve effective routing style (explicit beats preset; None means engine default).
        let effective = config
            .routing_style
            .or_else(|| config.edge_preset.map(|p| p.expand().0));
        let Some(style) = effective else {
            return Ok(());
        };
        let caps = self.capabilities();
        if caps.supported_routing_styles.contains(&style) {
            Ok(())
        } else {
            Err(RenderError {
                message: format!(
                    "{} does not support {style} routing. \
                     Supported: {}",
                    self,
                    caps.supported_routing_styles
                        .iter()
                        .map(|s| format!("{s}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
        }
    }
}

/// Engine-specific configuration envelope.
///
/// Wraps engine-specific layout parameters. Currently supports Layered
/// (Sugiyama/dagre) only; future engines (ELK, COSE) will add variants here.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EngineConfig {
    /// Layered (Sugiyama) layout engine configuration.
    Layered(crate::layered::types::LayoutConfig),
}

impl From<LayoutConfig> for EngineConfig {
    fn from(config: LayoutConfig) -> Self {
        EngineConfig::Layered(config)
    }
}

/// Edge routing determined by engine capabilities.
///
/// Controls how the rendering pipeline processes edge paths after layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeRouting {
    /// Engine provides node positions; emit direct source→target paths.
    DirectRoute,
    /// Engine provides only node positions; run full edge routing.
    PolylineRoute,
    /// Engine provides routed edge paths; apply clipping and spacing only.
    EngineProvided,
    /// Orthogonal routing: engine produces axis-aligned (right-angle) edge paths.
    OrthogonalRoute,
}

/// Request parameters for a `GraphEngine::solve()` call.
///
/// Engines use this to determine measurement mode, routing strategy, and output detail level.
///
/// ## Style model vocabulary (Phase 7 taxonomy)
///
/// Graph-level:
/// - `RoutingStyle` (`Direct`, `Polyline`, `Orthogonal`) — path topology requested by caller.
///
/// Render-level (applied after routing, does not affect path topology):
/// - `Curve` (`Basis`, `Linear(Sharp|Rounded)`) — segment drawing treatment.
///   `catmull-rom` parsing is recognized but deferred.
///
/// User-facing presets (expand to routing + render defaults):
/// - `Straight` → `Direct + Linear(Sharp)`
/// - `Polyline` → `Polyline + Linear(Sharp)`
/// - `Step` → `Orthogonal + Linear(Sharp)`
/// - `SmoothStep` → `Orthogonal + Linear(Rounded)`
/// - `CurvedStep` → `Orthogonal + Basis`
/// - `Basis` → `Polyline + Basis`
///
/// Precedence: explicit low-level fields > preset defaults > engine defaults.
#[derive(Debug, Clone)]
pub struct GraphSolveRequest {
    /// Target output format (affects node measurement: text-grid vs pixel).
    pub output_format: OutputFormat,
    /// Geometry detail level requested by the caller.
    pub geometry_level: GeometryLevel,
    /// Edge path simplification level for routed geometry.
    pub path_simplification: PathSimplification,
    /// Routing style requested by the caller (after preset resolution).
    ///
    /// `None` means use the engine's default routing for the selected algorithm.
    pub routing_style: Option<RoutingStyle>,
}

impl GraphSolveRequest {
    /// Build a solve request from a render config and output format.
    pub fn from_config(config: &RenderConfig, output_format: OutputFormat) -> Self {
        // Resolve routing style: explicit overrides preset; preset overrides engine default.
        let routing_style = config.routing_style.or_else(|| {
            config
                .edge_preset
                .map(|p| p.expand().0 /* routing component */)
        });
        Self {
            output_format,
            geometry_level: config.geometry_level,
            path_simplification: config.path_simplification,
            routing_style,
        }
    }
}

/// Result of a `GraphEngine::solve()` call.
///
/// Always contains positioned layout geometry. Optionally contains
/// routed edge paths when the engine owns routing and `geometry_level`
/// is `Routed`.
pub struct GraphSolveResult {
    /// Which engine+algorithm produced this result.
    pub engine_id: EngineAlgorithmId,
    /// Positioned node and edge geometry.
    pub geometry: crate::diagrams::flowchart::geometry::GraphGeometry,
    /// Routed edge paths (present when engine routes natively and routed level requested).
    pub routed: Option<crate::diagrams::flowchart::geometry::RoutedGraphGeometry>,
}

/// Unified graph engine trait combining layout and optional routing.
///
/// Engines compute layout and optionally route edges in a single `solve()` call.
/// The routing strategy is engine-owned, determined by `capabilities().route_ownership`.
pub trait GraphEngine: Send + Sync {
    /// Combined engine+algorithm identifier.
    fn id(&self) -> EngineAlgorithmId;

    /// Capabilities this engine+algorithm provides.
    fn capabilities(&self) -> EngineAlgorithmCapabilities;

    /// Solve: layout and optionally route the diagram.
    fn solve(
        &self,
        diagram: &crate::graph::Diagram,
        config: &EngineConfig,
        request: &GraphSolveRequest,
    ) -> Result<GraphSolveResult, RenderError>;
}

/// Post-routing path simplification level for MMDS and SVG output.
///
/// Controls how many anchor points are retained after the routing engine
/// produces fully-routed edge geometry. Higher simplification levels
/// reduce point counts at the cost of path fidelity.
///
/// Ignored for text/ASCII output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathSimplification {
    /// No simplification. All routed waypoints are retained.
    None,
    /// Lossless: remove redundant collinear and duplicate interior points.
    /// Path shape is preserved exactly.
    #[default]
    Lossless,
    /// Lossy: reduce to start, midpoint, and end (3 points max).
    /// Path shape may change significantly.
    Lossy,
    /// Minimal: start and end only (2 points).
    /// Maximum simplification.
    Minimal,
}

impl std::fmt::Display for PathSimplification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathSimplification::None => write!(f, "none"),
            PathSimplification::Lossless => write!(f, "lossless"),
            PathSimplification::Lossy => write!(f, "lossy"),
            PathSimplification::Minimal => write!(f, "minimal"),
        }
    }
}

impl PathSimplification {
    /// Parse path simplification level from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "none" => Ok(PathSimplification::None),
            "lossless" => Ok(PathSimplification::Lossless),
            "lossy" => Ok(PathSimplification::Lossy),
            "minimal" => Ok(PathSimplification::Minimal),
            _ => Err(RenderError {
                message: format!("unknown path simplification: {s:?}"),
            }),
        }
    }
}

impl FromStr for PathSimplification {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PathSimplification::parse(s)
    }
}

impl PathSimplification {
    /// Simplify a path according to the simplification level.
    ///
    /// Returns a new vec with the appropriate number of points:
    /// - `None` — all points unchanged
    /// - `Lossy` — first, middle, last (3 points max)
    /// - `Minimal` — first and last only (2 points max)
    pub fn simplify<T: Clone>(&self, points: &[T]) -> Vec<T> {
        match self {
            PathSimplification::None => points.to_vec(),
            PathSimplification::Lossless => points.to_vec(),
            PathSimplification::Lossy if points.len() > 3 => {
                let mid = points.len() / 2;
                vec![
                    points[0].clone(),
                    points[mid].clone(),
                    points[points.len() - 1].clone(),
                ]
            }
            PathSimplification::Minimal if points.len() > 2 => {
                vec![points[0].clone(), points[points.len() - 1].clone()]
            }
            _ => points.to_vec(),
        }
    }

    /// Simplify path points with coordinate-aware compacting.
    ///
    /// - `Lossless` removes consecutive duplicates and strictly collinear
    ///   interior points while preserving overall shape.
    /// - Other variants behave the same as `simplify`.
    pub fn simplify_with_coords<T: Clone>(
        &self,
        points: &[T],
        coords: impl Fn(&T) -> (f64, f64),
    ) -> Vec<T> {
        match self {
            PathSimplification::Lossless => compact_points(points, coords),
            _ => self.simplify(points),
        }
    }
}

fn compact_points<T: Clone>(points: &[T], coords: impl Fn(&T) -> (f64, f64)) -> Vec<T> {
    const EPS: f64 = 1e-6;

    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut deduped = Vec::with_capacity(points.len());
    for point in points {
        let keep = deduped.last().is_none_or(|prev: &T| {
            let (px, py) = coords(prev);
            let (x, y) = coords(point);
            (px - x).abs() > EPS || (py - y).abs() > EPS
        });
        if keep {
            deduped.push(point.clone());
        }
    }

    if deduped.len() <= 2 {
        return deduped;
    }

    let mut result = Vec::with_capacity(deduped.len());
    result.push(deduped[0].clone());
    for idx in 1..(deduped.len() - 1) {
        let prev = result.last().expect("result has first element");
        let curr = &deduped[idx];
        let next = &deduped[idx + 1];

        let (px, py) = coords(prev);
        let (cx, cy) = coords(curr);
        let (nx, ny) = coords(next);

        let dx1 = cx - px;
        let dy1 = cy - py;
        let dx2 = nx - cx;
        let dy2 = ny - cy;
        let cross = dx1 * dy2 - dy1 * dx2;
        let dot = dx1 * dx2 + dy1 * dy2;
        let collinear_same_direction = cross.abs() <= EPS && dot >= -EPS;

        if !collinear_same_direction {
            result.push(curr.clone());
        }
    }
    result.push(deduped[deduped.len() - 1].clone());
    result
}

/// MMDS geometry level for JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeometryLevel {
    /// Node geometry + edge topology only (no edge paths).
    #[default]
    Layout,
    /// Full geometry including routed edge paths.
    Routed,
}

impl std::fmt::Display for GeometryLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeometryLevel::Layout => write!(f, "layout"),
            GeometryLevel::Routed => write!(f, "routed"),
        }
    }
}

impl GeometryLevel {
    /// Parse MMDS geometry level from user-provided text.
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "layout" => Ok(GeometryLevel::Layout),
            "routed" => Ok(GeometryLevel::Routed),
            _ => Err(RenderError {
                message: format!("unknown geometry level: {s:?}"),
            }),
        }
    }
}

impl FromStr for GeometryLevel {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GeometryLevel::parse(s)
    }
}

/// Configuration for rendering.
#[derive(Debug, Clone, Default)]
pub struct RenderConfig {
    /// Layout configuration.
    pub layout: LayoutConfig,
    /// Layout engine+algorithm selection.
    ///
    /// - `None` => default (`flux-layered`)
    /// - `Some(EngineAlgorithmId::...)` => explicit engine+algorithm pair
    ///
    /// Edge routing is derived from the engine's capabilities; it cannot
    /// be overridden independently.
    pub layout_engine: Option<EngineAlgorithmId>,
    /// Cluster (subgraph) rank separation override.
    pub cluster_ranksep: Option<f64>,
    /// Padding around content.
    pub padding: Option<usize>,
    /// SVG-specific: scale factor.
    pub svg_scale: Option<f64>,
    /// SVG edge style preset. Expands to routing + curve defaults.
    ///
    /// Precedence: explicit low-level fields > preset defaults > engine defaults.
    pub edge_preset: Option<EdgePreset>,
    /// SVG routing style override (polyline or orthogonal).
    ///
    /// When set, takes precedence over the preset's routing component.
    pub routing_style: Option<RoutingStyle>,
    /// SVG curve override (basis, linear, linear-rounded).
    ///
    /// When set, takes precedence over the preset's curve component.
    pub curve: Option<Curve>,
    /// SVG-specific: corner arc radius (px) for `Curve::Linear(CornerStyle::Rounded)`.
    pub edge_radius: Option<f64>,
    /// SVG-specific: diagram padding (px).
    pub svg_diagram_padding: Option<f64>,
    /// SVG-specific: node padding on x-axis (px).
    pub svg_node_padding_x: Option<f64>,
    /// SVG-specific: node padding on y-axis (px).
    pub svg_node_padding_y: Option<f64>,
    /// Show node IDs alongside labels (e.g., "A: Start").
    pub show_ids: bool,
    /// MMDS geometry level for JSON output.
    pub geometry_level: GeometryLevel,
    /// Path simplification level for edge waypoints (MMDS and SVG).
    pub path_simplification: PathSimplification,
}

/// Error type for rendering failures.
#[derive(Debug, Clone)]
pub struct RenderError {
    pub message: String,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for RenderError {}

impl From<String> for RenderError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for RenderError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

fn normalize_enum_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}
