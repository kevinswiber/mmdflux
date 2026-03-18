//! Engine-specific capability descriptors and routing resolution for
//! graph-family layout selection.
//!
//! Defines the vocabulary types [`EngineId`], [`AlgorithmId`], and
//! [`EngineAlgorithmId`], plus engine-specific capability descriptors
//! and routing resolution that depend on graph-family routing types.

use std::str::FromStr;

use crate::errors::RenderError;
use crate::format::{RoutingStyle, normalize_enum_token};
use crate::graph::routing::EdgeRouting;

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

/// Combined engine+algorithm identifier for explicit layout engine selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineAlgorithmId {
    engine: EngineId,
    algorithm: AlgorithmId,
}

impl EngineAlgorithmId {
    pub const FLUX_LAYERED: Self = Self::new(EngineId::Flux, AlgorithmId::Layered);
    pub const MERMAID_LAYERED: Self = Self::new(EngineId::Mermaid, AlgorithmId::Layered);
    pub const ELK_LAYERED: Self = Self::new(EngineId::Elk, AlgorithmId::Layered);
    pub const ELK_MRTREE: Self = Self::new(EngineId::Elk, AlgorithmId::MrTree);

    /// Create an explicit `engine + algorithm` selection.
    pub const fn new(engine: EngineId, algorithm: AlgorithmId) -> Self {
        Self { engine, algorithm }
    }

    /// Return the engine half of the `engine-algorithm` identifier.
    pub const fn engine(self) -> EngineId {
        self.engine
    }

    /// Return the algorithm half of the `engine-algorithm` identifier.
    pub const fn algorithm(self) -> AlgorithmId {
        self.algorithm
    }

    /// Parse an `engine-algorithm` ID string (case-insensitive, trims whitespace).
    pub fn parse(s: &str) -> Result<Self, RenderError> {
        match normalize_enum_token(s).as_str() {
            "flux-layered" => Ok(Self::FLUX_LAYERED),
            "mermaid-layered" => Ok(Self::MERMAID_LAYERED),
            #[cfg(feature = "engine-elk")]
            "elk-layered" => Ok(Self::ELK_LAYERED),
            #[cfg(feature = "engine-elk")]
            "elk-mrtree" => Ok(Self::ELK_MRTREE),
            #[cfg(not(feature = "engine-elk"))]
            "elk-layered" | "elk-mrtree" | "elk" => Err(RenderError {
                message: "ELK engines are not available in this build. \
                          Use \"flux-layered\" (recommended) or \"mermaid-layered\"."
                    .into(),
            }),
            other => Err(RenderError {
                message: format!(
                    "unknown engine: {other:?}. Valid options: \
                     flux-layered, mermaid-layered"
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
            (EngineId::Flux, AlgorithmId::MrTree) => write!(f, "flux-mrtree"),
            (EngineId::Mermaid, AlgorithmId::MrTree) => write!(f, "mermaid-mrtree"),
        }
    }
}

impl FromStr for EngineAlgorithmId {
    type Err = RenderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
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
    pub fn routes_edges(self) -> bool {
        matches!(self, Self::Native | Self::EngineProvided)
    }
}

/// Capabilities for a combined engine+algorithm pair.
#[derive(Debug, Clone, Copy)]
pub struct EngineAlgorithmCapabilities {
    pub route_ownership: RouteOwnership,
    pub supports_subgraphs: bool,
    pub supported_routing_styles: &'static [RoutingStyle],
}

impl EngineAlgorithmCapabilities {
    pub fn edge_routing_for_style(self, routing_style: Option<RoutingStyle>) -> EdgeRouting {
        match self.route_ownership {
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

/// Static engine descriptor owned by the graph engine layer.
#[derive(Debug, Clone, Copy)]
pub struct EngineAlgorithmDescriptor {
    pub capabilities: EngineAlgorithmCapabilities,
    pub required_feature: Option<&'static str>,
}

const FLUX_LAYERED_DESCRIPTOR: EngineAlgorithmDescriptor = EngineAlgorithmDescriptor {
    capabilities: EngineAlgorithmCapabilities {
        route_ownership: RouteOwnership::Native,
        supports_subgraphs: true,
        supported_routing_styles: &[
            RoutingStyle::Direct,
            RoutingStyle::Polyline,
            RoutingStyle::Orthogonal,
        ],
    },
    required_feature: None,
};

const MERMAID_LAYERED_DESCRIPTOR: EngineAlgorithmDescriptor = EngineAlgorithmDescriptor {
    capabilities: EngineAlgorithmCapabilities {
        route_ownership: RouteOwnership::HintDriven,
        supports_subgraphs: true,
        supported_routing_styles: &[RoutingStyle::Polyline],
    },
    required_feature: None,
};

const ELK_LAYERED_DESCRIPTOR: EngineAlgorithmDescriptor = EngineAlgorithmDescriptor {
    capabilities: EngineAlgorithmCapabilities {
        route_ownership: RouteOwnership::EngineProvided,
        supports_subgraphs: true,
        supported_routing_styles: &[RoutingStyle::Polyline, RoutingStyle::Orthogonal],
    },
    required_feature: Some("engine-elk"),
};

const ELK_MRTREE_DESCRIPTOR: EngineAlgorithmDescriptor = EngineAlgorithmDescriptor {
    capabilities: EngineAlgorithmCapabilities {
        route_ownership: RouteOwnership::EngineProvided,
        supports_subgraphs: false,
        supported_routing_styles: &[RoutingStyle::Polyline],
    },
    required_feature: Some("engine-elk"),
};

const UNKNOWN_DESCRIPTOR: EngineAlgorithmDescriptor = EngineAlgorithmDescriptor {
    capabilities: EngineAlgorithmCapabilities {
        route_ownership: RouteOwnership::HintDriven,
        supports_subgraphs: false,
        supported_routing_styles: &[RoutingStyle::Polyline],
    },
    required_feature: None,
};

impl EngineAlgorithmId {
    pub fn descriptor(self) -> &'static EngineAlgorithmDescriptor {
        match (self.engine(), self.algorithm()) {
            (EngineId::Flux, AlgorithmId::Layered) => &FLUX_LAYERED_DESCRIPTOR,
            (EngineId::Mermaid, AlgorithmId::Layered) => &MERMAID_LAYERED_DESCRIPTOR,
            (EngineId::Elk, AlgorithmId::Layered) => &ELK_LAYERED_DESCRIPTOR,
            (EngineId::Elk, AlgorithmId::MrTree) => &ELK_MRTREE_DESCRIPTOR,
            _ => &UNKNOWN_DESCRIPTOR,
        }
    }

    /// Static capability matrix for this engine+algorithm combination.
    pub fn capabilities(self) -> EngineAlgorithmCapabilities {
        self.descriptor().capabilities
    }

    /// Check whether this `engine-algorithm` combination is available at runtime.
    pub fn check_available(self) -> Result<(), RenderError> {
        match self.descriptor().required_feature {
            None => Ok(()),
            Some(_) => Err(RenderError {
                message: format!(
                    "{self} is not yet implemented. \
                     Use \"flux-layered\" (recommended) or \"mermaid-layered\"."
                ),
            }),
        }
    }

    /// Validate that the requested routing style is supported by this engine.
    pub fn check_routing_style(
        self,
        routing_style: Option<RoutingStyle>,
    ) -> Result<(), RenderError> {
        let Some(style) = routing_style else {
            return Ok(());
        };

        let caps = self.capabilities();
        if caps.supported_routing_styles.contains(&style) {
            Ok(())
        } else {
            Err(RenderError {
                message: format!(
                    "{} does not support {style} routing. Supported: {}",
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

    /// Resolve the concrete routing algorithm for a requested routing style.
    pub fn edge_routing_for_style(self, routing_style: Option<RoutingStyle>) -> EdgeRouting {
        self.capabilities().edge_routing_for_style(routing_style)
    }
}

#[cfg(test)]
mod tests {
    use super::RouteOwnership;

    #[test]
    fn route_ownership_routes_edges_matrix() {
        assert!(RouteOwnership::Native.routes_edges());
        assert!(!RouteOwnership::HintDriven.routes_edges());
        assert!(RouteOwnership::EngineProvided.routes_edges());
    }
}
