//! Flux graph engine adapters.
//!
//! Flux is the native mmdflux graph engine. It reuses the shared layered
//! algorithm kernel, then applies Flux-specific profile selection and native
//! routing behavior.

use crate::engines::graph::algorithms::layered::{
    LabelDummyStrategy, LayoutConfig, MeasurementMode, build_float_layout_with_flags,
    layout_config_from_layered, run_layered_layout,
};
use crate::engines::graph::{
    EngineAlgorithmId, EngineConfig, GraphEngine, GraphGeometryContract, GraphSolveRequest,
    GraphSolveResult,
};
use crate::errors::RenderError;
use crate::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{GeometryLevel, Graph};

/// Flux-layered engine: native graph-family layout plus native routing.
pub struct FluxLayeredEngine;

/// Select the internal Flux profile for the layered algorithm.
///
/// The engine-algorithm ID is still `flux-layered`: Flux is the engine,
/// layered is the algorithm. Curve choice is intentionally excluded here so
/// render-only presets do not perturb node ordering.
pub(crate) fn flux_layout_profile(
    input_cfg: &LayoutConfig,
    _edge_routing: EdgeRouting,
) -> LayoutConfig {
    LayoutConfig {
        greedy_switch: true,
        model_order_tiebreak: input_cfg.model_order_tiebreak,
        variable_rank_spacing: true,
        always_compound_ordering: true,
        track_reversed_chains: true,
        per_edge_label_spacing: true,
        label_side_selection: true,
        label_dummy_strategy: LabelDummyStrategy::WidestLayer,
        ..input_cfg.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CrowdingScore {
    node_intrusions: usize,
    edge_crossings: usize,
}

impl CrowdingScore {
    fn is_clean(self) -> bool {
        self.node_intrusions == 0 && self.edge_crossings == 0
    }
}

fn strict_segment_interior_intersection(
    a1: (f64, f64),
    a2: (f64, f64),
    b1: (f64, f64),
    b2: (f64, f64),
) -> bool {
    const EPS: f64 = 1e-6;

    fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
        a.0 * b.1 - a.1 * b.0
    }

    let r = (a2.0 - a1.0, a2.1 - a1.1);
    let s = (b2.0 - b1.0, b2.1 - b1.1);
    let denom = cross(r, s);
    if denom.abs() <= EPS {
        return false;
    }

    let q_minus_p = (b1.0 - a1.0, b1.1 - a1.1);
    let t = cross(q_minus_p, s) / denom;
    let u = cross(q_minus_p, r) / denom;
    t > EPS && t < 1.0 - EPS && u > EPS && u < 1.0 - EPS
}

fn segment_crosses_rect_interior(
    a: (f64, f64),
    b: (f64, f64),
    rect: (f64, f64, f64, f64),
    margin: f64,
) -> bool {
    const EPS: f64 = 1e-6;

    fn axis_interval(a: f64, d: f64, min_v: f64, max_v: f64) -> Option<(f64, f64)> {
        const EPS: f64 = 1e-6;
        if d.abs() <= EPS {
            if a > min_v + EPS && a < max_v - EPS {
                Some((0.0, 1.0))
            } else {
                None
            }
        } else {
            let t1 = (min_v - a) / d;
            let t2 = (max_v - a) / d;
            let lo = t1.min(t2).max(0.0);
            let hi = t1.max(t2).min(1.0);
            if hi > lo + EPS { Some((lo, hi)) } else { None }
        }
    }

    let (x, y, width, height) = rect;
    let min_x = x + margin;
    let max_x = x + width - margin;
    let min_y = y + margin;
    let max_y = y + height - margin;
    if !(max_x > min_x && max_y > min_y) {
        return false;
    }

    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let Some((tx_lo, tx_hi)) = axis_interval(a.0, dx, min_x, max_x) else {
        return false;
    };
    let Some((ty_lo, ty_hi)) = axis_interval(a.1, dy, min_y, max_y) else {
        return false;
    };

    let lo = tx_lo.max(ty_lo);
    let hi = tx_hi.min(ty_hi);
    hi > lo + EPS
}

fn edge_crowding_score(
    diagram: &Graph,
    geometry: &GraphGeometry,
    edge_routing: EdgeRouting,
) -> CrowdingScore {
    let routed = route_graph_geometry(diagram, geometry, edge_routing);

    let mut node_intrusions = 0usize;
    for edge in &routed.edges {
        for (node_id, node) in &geometry.nodes {
            if node_id == &edge.from || node_id == &edge.to {
                continue;
            }
            let hit = edge.path.windows(2).any(|segment| {
                segment_crosses_rect_interior(
                    (segment[0].x, segment[0].y),
                    (segment[1].x, segment[1].y),
                    (node.rect.x, node.rect.y, node.rect.width, node.rect.height),
                    1.0,
                )
            });
            if hit {
                node_intrusions += 1;
            }
        }
    }

    let mut edge_crossings = 0usize;
    for i in 0..routed.edges.len() {
        for j in (i + 1)..routed.edges.len() {
            let crossed = routed.edges[i].path.windows(2).any(|a_seg| {
                routed.edges[j].path.windows(2).any(|b_seg| {
                    strict_segment_interior_intersection(
                        (a_seg[0].x, a_seg[0].y),
                        (a_seg[1].x, a_seg[1].y),
                        (b_seg[0].x, b_seg[0].y),
                        (b_seg[1].x, b_seg[1].y),
                    )
                })
            });
            if crossed {
                edge_crossings += 1;
            }
        }
    }

    CrowdingScore {
        node_intrusions,
        edge_crossings,
    }
}

pub(crate) fn adapt_flux_profile_for_reversed_chain_crowding(
    mode: &MeasurementMode,
    diagram: &Graph,
    edge_routing: EdgeRouting,
    profile: &LayoutConfig,
) -> Result<LayoutConfig, RenderError> {
    if !profile.track_reversed_chains {
        return Ok(profile.clone());
    }

    let baseline_cfg = EngineConfig::Layered(profile.clone());
    let baseline_geometry = run_layered_layout(mode, diagram, &baseline_cfg)?;
    if baseline_geometry.reversed_edges.is_empty() {
        return Ok(profile.clone());
    }

    let baseline_score = edge_crowding_score(diagram, &baseline_geometry, edge_routing);
    if baseline_score.is_clean() {
        return Ok(profile.clone());
    }

    let severe_crowding = baseline_score.node_intrusions >= 2 || baseline_score.edge_crossings >= 4;
    if !severe_crowding {
        return Ok(profile.clone());
    }

    let mut relaxed = profile.clone();
    relaxed.track_reversed_chains = false;
    let relaxed_cfg = EngineConfig::Layered(relaxed.clone());
    let relaxed_geometry = run_layered_layout(mode, diagram, &relaxed_cfg)?;
    let relaxed_score = edge_crowding_score(diagram, &relaxed_geometry, edge_routing);

    if relaxed_score < baseline_score {
        Ok(relaxed)
    } else {
        Ok(profile.clone())
    }
}

impl FluxLayeredEngine {
    /// Create the Flux graph engine adapter.
    pub fn text() -> Self {
        Self
    }
}

impl GraphEngine for FluxLayeredEngine {
    fn id(&self) -> EngineAlgorithmId {
        EngineAlgorithmId::FLUX_LAYERED
    }

    fn solve(
        &self,
        diagram: &Graph,
        config: &EngineConfig,
        request: &GraphSolveRequest,
    ) -> Result<GraphSolveResult, RenderError> {
        let mode = request.measurement_mode.clone();

        let EngineConfig::Layered(ref input_cfg) = *config;
        let edge_routing = self.id().edge_routing_for_style(request.routing_style);
        let enhanced_layout_cfg = flux_layout_profile(input_cfg, edge_routing);
        let should_adapt_reversed_chain_crowding =
            matches!(mode, MeasurementMode::Proportional(_)) && diagram.nodes.len() >= 10;
        let enhanced_layout_cfg = if should_adapt_reversed_chain_crowding {
            adapt_flux_profile_for_reversed_chain_crowding(
                &mode,
                diagram,
                edge_routing,
                &enhanced_layout_cfg,
            )?
        } else {
            enhanced_layout_cfg
        };
        let enhanced_config = EngineConfig::Layered(enhanced_layout_cfg);
        let config = &enhanced_config;

        if matches!(request.geometry_contract, GraphGeometryContract::Visual) {
            let MeasurementMode::Proportional(ref metrics) = mode else {
                return Err(RenderError {
                    message: "internal: visual geometry requires proportional measurement mode"
                        .to_string(),
                });
            };
            let EngineConfig::Layered(ref layered_cfg) = *config;
            let mut layout_config = layout_config_from_layered(layered_cfg, diagram);
            layout_config.cluster_rank_sep = 0.0;
            let geometry = build_float_layout_with_flags(
                diagram,
                &layout_config,
                metrics,
                edge_routing,
                false,
                Some(layered_cfg),
            );
            return Ok(GraphSolveResult {
                engine_id: self.id(),
                geometry,
                routed: None,
            });
        }

        let geometry = run_layered_layout(&mode, diagram, config)?;
        let routed: Option<RoutedGraphGeometry> =
            if matches!(request.geometry_level, GeometryLevel::Routed) {
                Some(route_graph_geometry(diagram, &geometry, edge_routing))
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
