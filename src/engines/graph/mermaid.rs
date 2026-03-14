//! Mermaid-compatible graph engine adapters.
//!
//! This engine borrows the shared layered algorithm from Flux, but applies
//! Mermaid.js and dagre-compatible policy differences so proportional
//! float-geometry solves match Mermaid behavior more closely.

use std::collections::HashMap;

use crate::engines::graph::algorithms::layered::{
    LayoutConfig, MeasurementMode, build_float_layout_with_flags, layout_config_from_layered,
};
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest,
    GraphSolveResult,
};
use crate::errors::RenderError;
use crate::graph::geometry::RoutedGraphGeometry;
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{GeometryLevel, Graph};

/// Mermaid dagre default for isolated subgraphs without explicit direction:
/// alternate axis from parent (horizontal <-> vertical).
fn mermaid_default_subgraph_direction(parent: crate::graph::Direction) -> crate::graph::Direction {
    use crate::graph::Direction;
    match parent {
        Direction::TopDown | Direction::BottomTop => Direction::LeftRight,
        Direction::LeftRight | Direction::RightLeft => Direction::TopDown,
    }
}

/// Mermaid compatibility isolation check.
///
/// Treat edges that target or source the subgraph itself (`to_subgraph` /
/// `from_subgraph`) as cluster-endpoint edges, not node-level cross-boundary
/// links for direction-tainting purposes.
fn mermaid_subgraph_has_tainting_cross_boundary_edges(diagram: &Graph, sg_id: &str) -> bool {
    let Some(sg) = diagram.subgraphs.get(sg_id) else {
        return false;
    };
    let sg_nodes: std::collections::HashSet<&str> = sg.nodes.iter().map(|s| s.as_str()).collect();
    diagram.edges.iter().any(|edge| {
        let from_in = sg_nodes.contains(edge.from.as_str());
        let to_in = sg_nodes.contains(edge.to.as_str());
        if from_in == to_in {
            return false;
        }

        let via_sg_endpoint = edge.to_subgraph.as_deref() == Some(sg_id)
            || edge.from_subgraph.as_deref() == Some(sg_id);
        !via_sg_endpoint
    })
}

/// Normalize subgraph direction semantics to Mermaid dagre behavior.
fn apply_mermaid_subgraph_direction_policy(diagram: &Graph) -> Option<Graph> {
    let mut adjusted = diagram.clone();
    let mut changed = false;

    let mut sg_ids: Vec<&String> = diagram.subgraphs.keys().collect();
    sg_ids.sort_by(|a, b| {
        diagram
            .subgraph_depth(a)
            .cmp(&diagram.subgraph_depth(b))
            .then_with(|| a.cmp(b))
    });

    let mut effective_dirs: HashMap<String, crate::graph::Direction> = HashMap::new();

    for sg_id in sg_ids {
        let sg = &diagram.subgraphs[sg_id];
        let parent_effective = sg
            .parent
            .as_ref()
            .and_then(|parent| effective_dirs.get(parent))
            .copied()
            .unwrap_or(diagram.direction);
        let isolated = !mermaid_subgraph_has_tainting_cross_boundary_edges(diagram, sg_id);

        let normalized_dir = match sg.dir {
            Some(explicit) if isolated => Some(explicit),
            Some(_) => Some(parent_effective),
            None if isolated => Some(mermaid_default_subgraph_direction(parent_effective)),
            None => None,
        };

        let effective = normalized_dir.unwrap_or(parent_effective);
        effective_dirs.insert(sg_id.clone(), effective);

        if normalized_dir != sg.dir {
            changed = true;
            if let Some(sg_mut) = adjusted.subgraphs.get_mut(sg_id) {
                sg_mut.dir = normalized_dir;
            }
        }
    }

    changed.then_some(adjusted)
}

/// Mermaid-layered engine: shared layered layout with Mermaid-compatible policy.
pub struct MermaidLayeredEngine;

impl Default for MermaidLayeredEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MermaidLayeredEngine {
    /// Create the Mermaid-compatible graph engine adapter.
    pub fn new() -> Self {
        Self
    }
}

impl GraphEngine for MermaidLayeredEngine {
    fn id(&self) -> EngineAlgorithmId {
        EngineAlgorithmId::MERMAID_LAYERED
    }

    fn solve(
        &self,
        diagram: &Graph,
        config: &EngineConfig,
        request: &GraphSolveRequest,
    ) -> Result<GraphSolveResult, RenderError> {
        if matches!(request.measurement_mode, MeasurementMode::Grid) {
            return Err(RenderError {
                message:
                    "mermaid-layered does not support grid measurement solves; use flux-layered instead"
                        .to_string(),
            });
        }

        let compat_diagram = apply_mermaid_subgraph_direction_policy(diagram);
        let diagram = compat_diagram.as_ref().unwrap_or(diagram);

        let mode = request.measurement_mode.clone();

        let MeasurementMode::Proportional(ref metrics) = mode else {
            return Err(RenderError {
                message: "internal: Mermaid float geometry requires proportional measurement mode"
                    .to_string(),
            });
        };
        let EngineConfig::Layered(ref layered_cfg) = *config;
        let mut layout_config = layout_config_from_layered(layered_cfg, diagram);
        layout_config.cluster_rank_sep = 0.0;
        let mermaid_flags = LayoutConfig {
            always_compound_ordering: true,
            ..Default::default()
        };
        let geometry = build_float_layout_with_flags(
            diagram,
            &layout_config,
            metrics,
            EdgeRouting::PolylineRoute,
            true,
            Some(&mermaid_flags),
        );
        let routed: Option<RoutedGraphGeometry> = if matches!(
            (request.geometry_contract, request.geometry_level),
            (GraphGeometryContract::Canonical, GeometryLevel::Routed)
        ) {
            Some(route_graph_geometry(
                diagram,
                &geometry,
                EdgeRouting::PolylineRoute,
            ))
        } else {
            None
        };

        Ok(GraphSolveResult {
            engine_id: self.id(),
            geometry,
            routed,
        })
    }
}
