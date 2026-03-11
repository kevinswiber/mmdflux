use std::collections::HashMap;
use std::str::FromStr;

use crate::errors::RenderError;
use crate::format::{Curve, EdgePreset, RoutingStyle, normalize_enum_token};

/// Ranking algorithm selection for the public layout config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ranker {
    #[default]
    NetworkSimplex,
    LongestPath,
}

impl From<Ranker> for crate::engines::graph::algorithms::layered::Ranker {
    fn from(value: Ranker) -> Self {
        match value {
            Ranker::NetworkSimplex => Self::NetworkSimplex,
            Ranker::LongestPath => Self::LongestPath,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::Ranker> for Ranker {
    fn from(value: crate::engines::graph::algorithms::layered::Ranker) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::Ranker::NetworkSimplex => {
                Ranker::NetworkSimplex
            }
            crate::engines::graph::algorithms::layered::Ranker::LongestPath => Ranker::LongestPath,
        }
    }
}

/// Direction for the public layout config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutDirection {
    #[default]
    TopBottom,
    BottomTop,
    LeftRight,
    RightLeft,
}

impl From<LayoutDirection> for crate::engines::graph::algorithms::layered::Direction {
    fn from(value: LayoutDirection) -> Self {
        match value {
            LayoutDirection::TopBottom => Self::TopBottom,
            LayoutDirection::BottomTop => Self::BottomTop,
            LayoutDirection::LeftRight => Self::LeftRight,
            LayoutDirection::RightLeft => Self::RightLeft,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::Direction> for LayoutDirection {
    fn from(value: crate::engines::graph::algorithms::layered::Direction) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::Direction::TopBottom => {
                LayoutDirection::TopBottom
            }
            crate::engines::graph::algorithms::layered::Direction::BottomTop => {
                LayoutDirection::BottomTop
            }
            crate::engines::graph::algorithms::layered::Direction::LeftRight => {
                LayoutDirection::LeftRight
            }
            crate::engines::graph::algorithms::layered::Direction::RightLeft => {
                LayoutDirection::RightLeft
            }
        }
    }
}

/// Strategy for placing edge-label dummies within long edge chains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelDummyStrategy {
    #[default]
    Midpoint,
    WidestLayer,
}

impl From<LabelDummyStrategy> for crate::engines::graph::algorithms::layered::LabelDummyStrategy {
    fn from(value: LabelDummyStrategy) -> Self {
        match value {
            LabelDummyStrategy::Midpoint => Self::Midpoint,
            LabelDummyStrategy::WidestLayer => Self::WidestLayer,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::LabelDummyStrategy> for LabelDummyStrategy {
    fn from(value: crate::engines::graph::algorithms::layered::LabelDummyStrategy) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::LabelDummyStrategy::Midpoint => {
                LabelDummyStrategy::Midpoint
            }
            crate::engines::graph::algorithms::layered::LabelDummyStrategy::WidestLayer => {
                LabelDummyStrategy::WidestLayer
            }
        }
    }
}

/// Canonical layout configuration type for the public API.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    pub direction: LayoutDirection,
    pub node_sep: f64,
    pub edge_sep: f64,
    pub rank_sep: f64,
    pub rank_sep_overrides: HashMap<i32, f64>,
    pub margin: f64,
    pub acyclic: bool,
    pub ranker: Ranker,
    pub greedy_switch: bool,
    pub model_order_tiebreak: bool,
    pub variable_rank_spacing: bool,
    pub always_compound_ordering: bool,
    pub track_reversed_chains: bool,
    pub per_edge_label_spacing: bool,
    pub label_side_selection: bool,
    pub label_dummy_strategy: LabelDummyStrategy,
    pub edge_label_spacing: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            direction: LayoutDirection::default(),
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 50.0,
            rank_sep_overrides: HashMap::new(),
            margin: 8.0,
            acyclic: true,
            ranker: Ranker::default(),
            greedy_switch: false,
            model_order_tiebreak: false,
            variable_rank_spacing: false,
            always_compound_ordering: false,
            track_reversed_chains: false,
            per_edge_label_spacing: false,
            label_side_selection: false,
            label_dummy_strategy: LabelDummyStrategy::default(),
            edge_label_spacing: 2.0,
        }
    }
}

impl LayoutConfig {
    pub fn rank_sep_for_gap(&self, rank: i32) -> f64 {
        self.rank_sep_overrides
            .get(&rank)
            .copied()
            .unwrap_or(self.rank_sep)
    }
}

impl From<LayoutConfig> for crate::engines::graph::algorithms::layered::LayoutConfig {
    fn from(value: LayoutConfig) -> Self {
        Self {
            direction: value.direction.into(),
            node_sep: value.node_sep,
            edge_sep: value.edge_sep,
            rank_sep: value.rank_sep,
            rank_sep_overrides: value.rank_sep_overrides,
            margin: value.margin,
            acyclic: value.acyclic,
            ranker: value.ranker.into(),
            greedy_switch: value.greedy_switch,
            model_order_tiebreak: value.model_order_tiebreak,
            variable_rank_spacing: value.variable_rank_spacing,
            always_compound_ordering: value.always_compound_ordering,
            track_reversed_chains: value.track_reversed_chains,
            per_edge_label_spacing: value.per_edge_label_spacing,
            label_side_selection: value.label_side_selection,
            label_dummy_strategy: value.label_dummy_strategy.into(),
            edge_label_spacing: value.edge_label_spacing,
        }
    }
}

impl From<&LayoutConfig> for crate::engines::graph::algorithms::layered::LayoutConfig {
    fn from(value: &LayoutConfig) -> Self {
        value.clone().into()
    }
}

impl From<crate::engines::graph::algorithms::layered::LayoutConfig> for LayoutConfig {
    fn from(value: crate::engines::graph::algorithms::layered::LayoutConfig) -> Self {
        Self {
            direction: value.direction.into(),
            node_sep: value.node_sep,
            edge_sep: value.edge_sep,
            rank_sep: value.rank_sep,
            rank_sep_overrides: value.rank_sep_overrides,
            margin: value.margin,
            acyclic: value.acyclic,
            ranker: value.ranker.into(),
            greedy_switch: value.greedy_switch,
            model_order_tiebreak: value.model_order_tiebreak,
            variable_rank_spacing: value.variable_rank_spacing,
            always_compound_ordering: value.always_compound_ordering,
            track_reversed_chains: value.track_reversed_chains,
            per_edge_label_spacing: value.per_edge_label_spacing,
            label_side_selection: value.label_side_selection,
            label_dummy_strategy: value.label_dummy_strategy.into(),
            edge_label_spacing: value.edge_label_spacing,
        }
    }
}

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
/// Values are always interpreted as `engine-algorithm` pairs:
/// - `flux-layered`
/// - `mermaid-layered`
/// - `elk-layered`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineAlgorithmId {
    engine: EngineId,
    algorithm: AlgorithmId,
}

impl EngineAlgorithmId {
    /// Create an explicit `engine + algorithm` selection.
    pub fn new(engine: EngineId, algorithm: AlgorithmId) -> Self {
        Self { engine, algorithm }
    }

    /// Return the engine half of the `engine-algorithm` identifier.
    pub fn engine(&self) -> EngineId {
        self.engine
    }

    /// Return the algorithm half of the `engine-algorithm` identifier.
    pub fn algorithm(&self) -> AlgorithmId {
        self.algorithm
    }

    /// Parse an `engine-algorithm` ID string (case-insensitive, trims whitespace).
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

    /// Check whether this `engine-algorithm` combination is available at runtime.
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

    /// Validate that the requested routing style is supported by this engine.
    pub fn check_routing_style(&self, config: &RenderConfig) -> Result<(), RenderError> {
        let effective = config
            .routing_style
            .or_else(|| config.edge_preset.map(|preset| preset.expand().0));
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

impl std::fmt::Display for EngineAlgorithmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.engine, self.algorithm) {
            (EngineId::Flux, AlgorithmId::Layered) => write!(f, "flux-layered"),
            (EngineId::Mermaid, AlgorithmId::Layered) => write!(f, "mermaid-layered"),
            (EngineId::Elk, AlgorithmId::Layered) => write!(f, "elk-layered"),
            (EngineId::Elk, AlgorithmId::MrTree) => write!(f, "elk-mrtree"),
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
    /// Engine+algo computes final routed paths natively.
    Native,
    /// Engine provides waypoint hints; compatibility router finalizes paths.
    HintDriven,
    /// External engine returns fully routed paths.
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
    pub supported_routing_styles: &'static [RoutingStyle],
}

/// Post-routing path simplification level for MMDS and SVG output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathSimplification {
    /// No simplification. All routed waypoints are retained.
    None,
    /// Lossless: remove redundant collinear and duplicate interior points.
    #[default]
    Lossless,
    /// Lossy: reduce to start, midpoint, and end (3 points max).
    Lossy,
    /// Minimal: start and end only (2 points max).
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

    /// Simplify a path according to the simplification level.
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

impl FromStr for PathSimplification {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PathSimplification::parse(s)
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

/// Configuration for rendering.
#[derive(Debug, Clone, Default)]
pub struct RenderConfig {
    /// Layout configuration.
    pub layout: LayoutConfig,
    /// Layout engine+algorithm selection.
    pub layout_engine: Option<EngineAlgorithmId>,
    /// Cluster (subgraph) rank separation override.
    pub cluster_ranksep: Option<f64>,
    /// Padding around content.
    pub padding: Option<usize>,
    /// Resolved text color mode for text/ascii output.
    pub text_color_mode: TextColorMode,
    /// SVG-specific: scale factor.
    pub svg_scale: Option<f64>,
    /// SVG edge style preset. Expands to routing + curve defaults.
    pub edge_preset: Option<EdgePreset>,
    /// SVG routing style override.
    pub routing_style: Option<RoutingStyle>,
    /// SVG curve override.
    pub curve: Option<Curve>,
    /// SVG-specific: corner arc radius (px).
    pub edge_radius: Option<f64>,
    /// SVG-specific: diagram padding (px).
    pub svg_diagram_padding: Option<f64>,
    /// SVG-specific: node padding on x-axis (px).
    pub svg_node_padding_x: Option<f64>,
    /// SVG-specific: node padding on y-axis (px).
    pub svg_node_padding_y: Option<f64>,
    /// Show node IDs alongside labels.
    pub show_ids: bool,
    /// MMDS geometry level for JSON output.
    pub geometry_level: GeometryLevel,
    /// Path simplification level for edge waypoints.
    pub path_simplification: PathSimplification,
}
