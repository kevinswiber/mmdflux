//! Low-level graph-family engine contracts.
//!
//! Consumer-facing config, format, family, and error definitions live in the
//! crate's flat top-level public contract modules. This module keeps the
//! engine-side solve contracts and provides a focused import surface for
//! callers that manage graph-family solves directly.

use super::{EngineAlgorithmCapabilities, EngineAlgorithmId, LayoutConfig};
use crate::engines::graph::algorithms::layered::MeasurementMode;
use crate::errors::RenderError;
use crate::format::RoutingStyle;
use crate::graph::GeometryLevel;

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
    /// Measurement model used for node and edge label sizing.
    pub measurement_mode: MeasurementMode,
    /// Float-geometry contract requested by the caller.
    pub geometry_contract: GraphGeometryContract,
    /// Geometry detail level requested by the caller.
    pub geometry_level: GeometryLevel,
    /// Routing style requested by the caller (after preset resolution).
    pub routing_style: Option<RoutingStyle>,
}

/// Float-geometry contract requested from the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphGeometryContract {
    /// Plain float geometry for downstream routing/export.
    Canonical,
    /// Float geometry tuned for direct visual emission.
    Visual,
}

impl GraphSolveRequest {
    /// Build a solve request from explicit engine-owned solve instructions.
    pub fn new(
        measurement_mode: MeasurementMode,
        geometry_contract: GraphGeometryContract,
        geometry_level: GeometryLevel,
        routing_style: Option<RoutingStyle>,
    ) -> Self {
        Self {
            measurement_mode,
            geometry_contract,
            geometry_level,
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
    #[allow(dead_code)]
    fn capabilities(&self) -> EngineAlgorithmCapabilities {
        self.id().capabilities()
    }

    /// Solve: layout and optionally route the diagram.
    fn solve(
        &self,
        diagram: &crate::graph::Graph,
        config: &EngineConfig,
        request: &GraphSolveRequest,
    ) -> Result<GraphSolveResult, RenderError>;
}
