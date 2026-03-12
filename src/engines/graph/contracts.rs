//! Low-level graph-family engine contracts.
//!
//! Consumer-facing config, format, family, and error definitions live in the
//! crate's flat top-level public contract modules. This module keeps the
//! engine-side solve contracts and provides a focused import surface for
//! callers that manage graph-family solves directly.

use crate::config::{
    EngineAlgorithmCapabilities, EngineAlgorithmId, GeometryLevel, LayoutConfig,
    PathSimplification, RenderConfig, RouteOwnership,
};
use crate::errors::RenderError;
use crate::format::{OutputFormat, RoutingStyle};
use crate::graph::routing::EdgeRouting;

impl EngineAlgorithmId {
    /// Resolve the edge routing algorithm for a given routing style.
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
}

/// Engine-specific configuration envelope.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EngineConfig {
    /// Layered (Sugiyama) layout engine configuration.
    Layered(crate::engines::graph::algorithms::layered::LayoutConfig),
}

impl From<LayoutConfig> for EngineConfig {
    fn from(config: LayoutConfig) -> Self {
        EngineConfig::Layered(config.into())
    }
}

/// Request parameters for a `GraphEngine::solve()` call.
#[derive(Debug, Clone)]
pub struct GraphSolveRequest {
    /// Target output format (affects node measurement: text-grid vs pixel).
    pub output_format: OutputFormat,
    /// Geometry detail level requested by the caller.
    pub geometry_level: GeometryLevel,
    /// Edge path simplification level for routed geometry.
    pub path_simplification: PathSimplification,
    /// Routing style requested by the caller (after preset resolution).
    pub routing_style: Option<RoutingStyle>,
}

impl GraphSolveRequest {
    /// Build a solve request from a render config and output format.
    pub fn from_config(config: &RenderConfig, output_format: OutputFormat) -> Self {
        let routing_style = config
            .routing_style
            .or_else(|| config.edge_preset.map(|preset| preset.expand().0));
        Self {
            output_format,
            geometry_level: config.geometry_level,
            path_simplification: config.path_simplification,
            routing_style,
        }
    }
}

/// Result of a `GraphEngine::solve()` call.
pub struct GraphSolveResult {
    /// Which engine+algorithm produced this result.
    pub engine_id: EngineAlgorithmId,
    /// Positioned node and edge geometry.
    pub geometry: crate::graph::geometry::GraphGeometry,
    /// Routed edge paths (present when engine routes natively and routed level requested).
    pub routed: Option<crate::graph::geometry::RoutedGraphGeometry>,
}

/// Unified graph engine trait combining layout and optional routing.
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
