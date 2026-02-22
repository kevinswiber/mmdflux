//! Layered layout engine adapter.
//!
//! Provides layered layout via `run_layered_layout` for text and SVG measurement
//! modes, and implements `GraphEngine` for `FluxLayeredEngine` and `MermaidLayeredEngine`.

use std::collections::HashMap;

use super::geometry::GraphGeometry;
use super::render::layout::{
    build_layered_layout, center_override_subgraphs, expand_parent_bounds,
};
use super::render::svg::svg_node_dimensions;
use super::render::svg_metrics::SvgTextMetrics;
use crate::diagram::{
    AlgorithmId, EngineAlgorithmCapabilities, EngineAlgorithmId, EngineConfig, EngineId,
    GeometryLevel, GraphEngine, GraphSolveRequest, GraphSolveResult, OutputFormat, RenderConfig,
    RenderError, RouteOwnership, RoutingStyle,
};
use crate::diagrams::flowchart::geometry::RoutedGraphGeometry;
use crate::graph::Diagram;
use crate::render::SvgOptions;

/// Measurement mode controls whether layout uses text-grid character
/// dimensions or SVG pixel dimensions for node sizing.
#[derive(Debug, Clone)]
pub enum MeasurementMode {
    /// Text-grid character dimensions (for text/ascii rendering).
    Text,
    /// SVG pixel dimensions (for MMDS and SVG output).
    Svg(SvgTextMetrics),
}

impl MeasurementMode {
    /// Determine the measurement mode from the output format.
    pub fn for_format(format: OutputFormat, config: &RenderConfig) -> Self {
        match format {
            OutputFormat::Mmds | OutputFormat::Svg => {
                let defaults = SvgOptions::default();
                let font_size = defaults.font_size;
                let node_padding_x = config.svg_node_padding_x.unwrap_or(defaults.node_padding_x);
                let node_padding_y = config.svg_node_padding_y.unwrap_or(defaults.node_padding_y);
                let metrics = SvgTextMetrics::new(font_size, node_padding_x, node_padding_y);
                MeasurementMode::Svg(metrics)
            }
            _ => MeasurementMode::Text,
        }
    }
}

fn text_edge_label_dimensions(label: &str) -> (f64, f64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let height = lines.len().max(1);
    (width as f64 + 2.0, height as f64)
}

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
fn mermaid_subgraph_has_tainting_cross_boundary_edges(diagram: &Diagram, sg_id: &str) -> bool {
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

/// Mermaid dagre subgraph direction policy.
///
/// Effective behavior (default `inheritDir: false`):
/// - explicit dir + isolated: use explicit dir
/// - explicit dir + non-isolated: ignore explicit, inherit parent
/// - no explicit dir + isolated: use default alternating direction
/// - no explicit dir + non-isolated: inherit parent
///
/// We encode this by normalizing `subgraph.dir` in a transient diagram view.
fn apply_mermaid_subgraph_direction_policy(diagram: &Diagram) -> Option<Diagram> {
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

/// Run layered layout with a given measurement mode.
///
/// Shared by `FluxLayeredEngine` and `MermaidLayeredEngine` — both use
/// the same layered kernel; only routing behavior differs.
pub fn run_layered_layout(
    mode: &MeasurementMode,
    diagram: &Diagram,
    config: &EngineConfig,
) -> Result<GraphGeometry, RenderError> {
    use crate::diagrams::flowchart::geometry;

    let EngineConfig::Layered(layered_cfg) = config;
    let layout_config = layout_config_from_layered(layered_cfg, diagram);
    let direction = diagram.direction;
    let mut result = match mode {
        MeasurementMode::Text => build_layered_layout(
            diagram,
            &layout_config,
            |node| {
                let (w, h) = crate::render::node_dimensions(node, direction);
                (w as f64, h as f64)
            },
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| text_edge_label_dimensions(label))
            },
        ),
        MeasurementMode::Svg(metrics) => build_layered_layout(
            diagram,
            &layout_config,
            |node| svg_node_dimensions(metrics, node, direction),
            |edge| {
                edge.label
                    .as_ref()
                    .map(|label| metrics.edge_label_dimensions(label))
            },
        ),
    };

    // Apply subgraph fixups to match compute_layout_direct() behavior:
    // center direction-override subgraph predecessors and expand parent bounds.
    center_override_subgraphs(diagram, &mut result);
    expand_parent_bounds(diagram, &mut result, 0.0, 0.0);

    Ok(geometry::from_layered_layout(&result, diagram))
}

/// Flux-layered engine: Sugiyama framework layout + orthgonal routing natively.
///
/// Implements `GraphEngine::solve()` with `RouteOwnership::Native` —
/// layout and routing are performed together inside `solve()`.
pub struct FluxLayeredEngine {
    mode: MeasurementMode,
}

impl FluxLayeredEngine {
    /// Create with text-grid measurement mode.
    pub fn text() -> Self {
        Self {
            mode: MeasurementMode::Text,
        }
    }

    /// Create with the specified measurement mode.
    pub fn with_mode(mode: MeasurementMode) -> Self {
        Self { mode }
    }
}

impl GraphEngine for FluxLayeredEngine {
    fn id(&self) -> EngineAlgorithmId {
        EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered)
    }

    fn capabilities(&self) -> EngineAlgorithmCapabilities {
        EngineAlgorithmCapabilities {
            route_ownership: RouteOwnership::Native,
            supports_subgraphs: true,
            supported_routing_styles: &[
                RoutingStyle::Direct,
                RoutingStyle::Polyline,
                RoutingStyle::Orthogonal,
            ],
        }
    }

    fn solve(
        &self,
        diagram: &Diagram,
        config: &EngineConfig,
        request: &GraphSolveRequest,
    ) -> Result<GraphSolveResult, RenderError> {
        use crate::render::SvgOptions;

        // For SVG/MMDS output, pixel-accurate SVG measurement mode is required.
        // Use self.mode if already SVG (explicit override), else derive from format.
        let mode = match request.output_format {
            OutputFormat::Svg | OutputFormat::Mmds => match &self.mode {
                MeasurementMode::Svg(_) => self.mode.clone(),
                MeasurementMode::Text => {
                    let defaults = SvgOptions::default();
                    let metrics = super::render::svg_metrics::SvgTextMetrics::new(
                        defaults.font_size,
                        defaults.node_padding_x,
                        defaults.node_padding_y,
                    );
                    MeasurementMode::Svg(metrics)
                }
            },
            _ => self.mode.clone(),
        };

        // SVG output: use the full SVG layout pipeline (subgraph post-processing,
        // direction overrides, padding, edge rerouting). This is what makes
        // FluxLayeredEngine an independent algorithm — it owns the SVG geometry
        // production end-to-end, not just the raw layered layout step.
        if matches!(request.output_format, OutputFormat::Svg) {
            let MeasurementMode::Svg(ref metrics) = mode else {
                return Err(RenderError {
                    message: "internal: SVG output requires SVG measurement mode".to_string(),
                });
            };
            let EngineConfig::Layered(ref layered_cfg) = *config;
            let mut layout_config = layout_config_from_layered(layered_cfg, diagram);
            // SVG does not add extra rank separation for clusters (matches Mermaid).
            layout_config.cluster_rank_sep = 0.0;
            let edge_routing = self.id().edge_routing_for_style(request.routing_style);
            let geometry = super::render::svg::build_svg_layout(
                diagram,
                &layout_config,
                metrics,
                edge_routing,
                false, // flux-layered: always respect direction overrides
            );
            return Ok(GraphSolveResult {
                engine_id: self.id(),
                geometry,
                routed: None,
            });
        }

        let geometry = run_layered_layout(&mode, diagram, config)?;

        // Route when routed geometry is requested (Native ownership).
        // Routing style selects the algorithm via edge_routing_for_style().
        let routed: Option<RoutedGraphGeometry> =
            if matches!(request.geometry_level, GeometryLevel::Routed) {
                let edge_routing = self.id().edge_routing_for_style(request.routing_style);
                Some(super::routing::route_graph_geometry(
                    diagram,
                    &geometry,
                    edge_routing,
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

/// Mermaid-layered engine: layered layout with polyline routing.
///
/// Implements `GraphEngine::solve()` with `RouteOwnership::HintDriven` —
/// layout uses the same layered kernel as `FluxLayeredEngine`, but routing
/// uses the `PolylineRoute` path for Mermaid.js compatibility.
pub struct MermaidLayeredEngine {
    mode: MeasurementMode,
}

impl MermaidLayeredEngine {
    /// Create with text-grid measurement mode.
    pub fn text() -> Self {
        Self {
            mode: MeasurementMode::Text,
        }
    }

    /// Create with the specified measurement mode.
    pub fn with_mode(mode: MeasurementMode) -> Self {
        Self { mode }
    }
}

impl GraphEngine for MermaidLayeredEngine {
    fn id(&self) -> EngineAlgorithmId {
        EngineAlgorithmId::new(EngineId::Mermaid, AlgorithmId::Layered)
    }

    fn capabilities(&self) -> EngineAlgorithmCapabilities {
        EngineAlgorithmCapabilities {
            route_ownership: RouteOwnership::HintDriven,
            supports_subgraphs: true,
            supported_routing_styles: &[RoutingStyle::Polyline],
        }
    }

    fn solve(
        &self,
        diagram: &Diagram,
        config: &EngineConfig,
        request: &GraphSolveRequest,
    ) -> Result<GraphSolveResult, RenderError> {
        use crate::diagram::EdgeRouting;
        use crate::render::SvgOptions;

        // Build a transient Mermaid-compatible view that normalizes per-subgraph
        // direction semantics to match Mermaid dagre behavior.
        let compat_diagram = apply_mermaid_subgraph_direction_policy(diagram);
        let diagram = compat_diagram.as_ref().unwrap_or(diagram);

        // For SVG/MMDS output, pixel-accurate SVG measurement mode is required.
        // Use self.mode if already SVG (explicit override), else derive from format.
        let mode = match request.output_format {
            OutputFormat::Svg | OutputFormat::Mmds => match &self.mode {
                MeasurementMode::Svg(_) => self.mode.clone(),
                MeasurementMode::Text => {
                    let defaults = SvgOptions::default();
                    let metrics = super::render::svg_metrics::SvgTextMetrics::new(
                        defaults.font_size,
                        defaults.node_padding_x,
                        defaults.node_padding_y,
                    );
                    MeasurementMode::Svg(metrics)
                }
            },
            _ => self.mode.clone(),
        };

        // SVG/MMDS output: run the full SVG layout pipeline (subgraph post-processing,
        // direction overrides, padding, edge rerouting) via build_svg_layout().
        // MermaidLayeredEngine uses PolylineRoute routing (no orthogonal path
        // injection), preserving the legacy render_svg() behavior for this engine.
        if matches!(
            request.output_format,
            OutputFormat::Svg | OutputFormat::Mmds
        ) {
            let MeasurementMode::Svg(ref metrics) = mode else {
                return Err(RenderError {
                    message: "internal: SVG output requires SVG measurement mode".to_string(),
                });
            };
            let EngineConfig::Layered(ref layered_cfg) = *config;
            let mut layout_config = layout_config_from_layered(layered_cfg, diagram);
            layout_config.cluster_rank_sep = 0.0;
            let geometry = super::render::svg::build_svg_layout(
                diagram,
                &layout_config,
                metrics,
                EdgeRouting::PolylineRoute,
                true, // mermaid compat: skip tainting non-isolated sublayout extraction
            );
            let routed: Option<RoutedGraphGeometry> = if matches!(
                (request.output_format, request.geometry_level),
                (OutputFormat::Mmds, GeometryLevel::Routed)
            ) {
                Some(super::routing::route_graph_geometry(
                    diagram,
                    &geometry,
                    EdgeRouting::PolylineRoute,
                ))
            } else {
                None
            };
            return Ok(GraphSolveResult {
                engine_id: self.id(),
                geometry,
                routed,
            });
        }

        let geometry = run_layered_layout(&mode, diagram, config)?;

        // HintDriven: route via PolylineRoute path if routed level requested.
        let routed: Option<RoutedGraphGeometry> =
            if matches!(request.geometry_level, GeometryLevel::Routed) {
                Some(super::routing::route_graph_geometry(
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

/// Build a flowchart LayoutConfig from layered config parameters.
///
/// This bridges the engine's layered config back to the flowchart render
/// config that `build_layered_layout` expects.
fn layout_config_from_layered(
    layered_cfg: &crate::layered::types::LayoutConfig,
    diagram: &Diagram,
) -> crate::diagrams::flowchart::render::layout::LayoutConfig {
    use crate::diagrams::flowchart::render::layout::LayoutConfig as FlowchartLayoutConfig;

    let defaults = FlowchartLayoutConfig::default();
    let extra_padding = if diagram.has_subgraphs() {
        diagram
            .subgraphs
            .keys()
            .map(|id| diagram.subgraph_depth(id))
            .max()
            .unwrap_or(0)
            * 2
    } else {
        0
    };

    FlowchartLayoutConfig {
        node_sep: layered_cfg.node_sep,
        edge_sep: layered_cfg.edge_sep,
        rank_sep: layered_cfg.rank_sep,
        margin: layered_cfg.margin,
        ranker: Some(layered_cfg.ranker),
        padding: defaults.padding + extra_padding,
        ..defaults
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::EngineAlgorithmId;

    #[test]
    fn run_layered_layout_simple_graph() {
        let input = "graph TD\nA-->B";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let config = EngineConfig::Layered(crate::layered::types::LayoutConfig::default());
        let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();

        assert_eq!(geom.nodes.len(), 2);
        assert!(geom.nodes.contains_key("A"));
        assert!(geom.nodes.contains_key("B"));
        assert_eq!(geom.edges.len(), 1);
    }

    #[test]
    fn run_layered_layout_with_subgraphs() {
        let input = "graph TD\nsubgraph sg1[Group]\nA-->B\nend\nC-->A";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let config = EngineConfig::Layered(crate::layered::types::LayoutConfig::default());
        let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();

        assert!(geom.nodes.contains_key("A"));
        assert!(geom.nodes.contains_key("B"));
        assert!(geom.nodes.contains_key("C"));
        assert!(!geom.subgraphs.is_empty());
    }

    #[test]
    fn run_layered_layout_svg_mode_produces_larger_dimensions() {
        let input = "graph TD\nA-->B";
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let config = EngineConfig::Layered(crate::layered::types::LayoutConfig::default());
        let text_geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();
        let svg_geom = run_layered_layout(
            &MeasurementMode::Svg(SvgTextMetrics::new(16.0, 15.0, 15.0)),
            &diagram,
            &config,
        )
        .unwrap();

        // SVG dimensions should be significantly larger than text dimensions
        let text_w = text_geom.nodes["A"].rect.width;
        let svg_w = svg_geom.nodes["A"].rect.width;
        assert!(
            svg_w > text_w * 3.0,
            "SVG width ({svg_w}) should be much larger than text width ({text_w})"
        );
    }

    #[test]
    fn run_layered_layout_applies_subgraph_centering_and_expansion() {
        // direction_override.mmd: TD graph with LR subgraph containing A → B → C
        // plus external edges: Start → A, C → End.
        // After centering, "Start" should be positioned above the center of the
        // A/B/C cluster, not at the leftmost node.
        let input = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/flowchart/direction_override.mmd"
        ))
        .unwrap();
        let flowchart = crate::parser::parse_flowchart(&input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);

        let config = EngineConfig::Layered(crate::layered::types::LayoutConfig::default());
        let geom = run_layered_layout(&MeasurementMode::Text, &diagram, &config).unwrap();

        // Subgraph bounds should encompass all member nodes.
        let sg_bounds = geom.subgraphs.get("sg1").expect("sg1 should exist");
        for member in &["A", "B", "C"] {
            let node = geom
                .nodes
                .get(*member)
                .unwrap_or_else(|| panic!("{member} missing"));
            let nr = &node.rect;
            let sr = &sg_bounds.rect;
            assert!(
                nr.x >= sr.x
                    && nr.x + nr.width <= sr.x + sr.width
                    && nr.y >= sr.y
                    && nr.y + nr.height <= sr.y + sr.height,
                "Node {member} at {:?} should be within sg1 bounds {:?}",
                nr,
                sr,
            );
        }

        // "Start" should be roughly centered over the subgraph horizontally.
        let start = geom.nodes.get("Start").expect("Start should exist");
        let sg_center_x = sg_bounds.rect.x + sg_bounds.rect.width / 2.0;
        let start_center_x = start.rect.x + start.rect.width / 2.0;
        assert!(
            (start_center_x - sg_center_x).abs() < sg_bounds.rect.width * 0.4,
            "Start center ({start_center_x}) should be near sg1 center ({sg_center_x})"
        );
    }

    #[test]
    fn selected_engine_rejects_unknown_engine_at_parse_boundary() {
        let err = EngineAlgorithmId::parse("nonexistent").unwrap_err();
        assert!(
            err.message.contains("unknown engine"),
            "error should mention unknown: {}",
            err.message
        );
    }

    // =========================================================================
    // Subgraph direction override tests (plan-0085)
    // =========================================================================

    fn solve_svg(engine: &dyn GraphEngine, diagram: &Diagram) -> GraphSolveResult {
        use crate::diagram::PathSimplification;
        let config = EngineConfig::Layered(crate::layered::types::LayoutConfig::default());
        let request = GraphSolveRequest {
            output_format: OutputFormat::Svg,
            geometry_level: GeometryLevel::Layout,
            path_simplification: PathSimplification::None,
            routing_style: Some(RoutingStyle::Polyline),
        };
        engine.solve(diagram, &config, &request).unwrap()
    }

    fn solve_mmds_layout(engine: &dyn GraphEngine, diagram: &Diagram) -> GraphSolveResult {
        use crate::diagram::PathSimplification;
        let config = EngineConfig::Layered(crate::layered::types::LayoutConfig::default());
        let request = GraphSolveRequest {
            output_format: OutputFormat::Mmds,
            geometry_level: GeometryLevel::Layout,
            path_simplification: PathSimplification::None,
            routing_style: Some(RoutingStyle::Polyline),
        };
        engine.solve(diagram, &config, &request).unwrap()
    }

    #[test]
    fn subgraph_direction_isolated_both_engines_respect_override() {
        let input =
            include_str!("../../../tests/fixtures/flowchart/subgraph_direction_isolated.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);

        let flux = FluxLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
        let flux_result = solve_svg(&flux, &diagram);
        let a_flux = &flux_result.geometry.nodes["A"].rect;
        let b_flux = &flux_result.geometry.nodes["B"].rect;
        // LR override respected: A,B side-by-side (different X, similar Y)
        assert!(
            (a_flux.y - b_flux.y).abs() < 1.0,
            "flux: A.y={} B.y={} should be similar (LR override)",
            a_flux.y,
            b_flux.y
        );
        assert!(
            (a_flux.x - b_flux.x).abs() > 10.0,
            "flux: A.x={} B.x={} should differ (LR override)",
            a_flux.x,
            b_flux.x
        );

        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let mermaid_result = solve_svg(&mermaid, &diagram);
        let a_mermaid = &mermaid_result.geometry.nodes["A"].rect;
        let b_mermaid = &mermaid_result.geometry.nodes["B"].rect;
        // Isolated subgraph: mermaid also respects LR override
        assert!(
            (a_mermaid.y - b_mermaid.y).abs() < 1.0,
            "mermaid: A.y={} B.y={} should be similar (LR override respected for isolated sg)",
            a_mermaid.y,
            b_mermaid.y
        );
        assert!(
            (a_mermaid.x - b_mermaid.x).abs() > 10.0,
            "mermaid: A.x={} B.x={} should differ (LR override respected for isolated sg)",
            a_mermaid.x,
            b_mermaid.x
        );
    }

    #[test]
    fn subgraph_direction_cross_boundary_engines_diverge() {
        let input =
            include_str!("../../../tests/fixtures/flowchart/subgraph_direction_cross_boundary.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);

        // Flux: LR sublayout applied (A,B side-by-side with significant X spread)
        let flux = FluxLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
        let flux_result = solve_svg(&flux, &diagram);
        let a_flux = &flux_result.geometry.nodes["A"].rect;
        let b_flux = &flux_result.geometry.nodes["B"].rect;
        let flux_x_spread = (a_flux.x - b_flux.x).abs();
        assert!(
            (a_flux.y - b_flux.y).abs() < 1.0,
            "flux: A.y={} B.y={} should be similar (LR sublayout applied)",
            a_flux.y,
            b_flux.y
        );
        assert!(
            flux_x_spread > 10.0,
            "flux: A-B X spread={flux_x_spread} should be large (LR sublayout)",
        );

        // Mermaid: LR override ignored → sublayout uses parent TD direction
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let mermaid_result = solve_svg(&mermaid, &diagram);
        let a_mermaid = &mermaid_result.geometry.nodes["A"].rect;
        let b_mermaid = &mermaid_result.geometry.nodes["B"].rect;
        // TD sublayout: A and B should be stacked (different Y)
        assert!(
            (a_mermaid.y - b_mermaid.y).abs() > 10.0,
            "mermaid: A.y={} B.y={} should differ (TD sublayout, LR override ignored)",
            a_mermaid.y,
            b_mermaid.y
        );
    }

    #[test]
    fn subgraph_direction_nested_mixed_isolation() {
        let input =
            include_str!("../../../tests/fixtures/flowchart/subgraph_direction_nested_mixed.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);

        // Mermaid engine: outer LR skipped (cross-boundary from E-->C), inner BT respected
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
        let mermaid_result = solve_svg(&mermaid, &diagram);
        let a_mermaid = &mermaid_result.geometry.nodes["A"].rect;
        let b_mermaid = &mermaid_result.geometry.nodes["B"].rect;
        // Inner subgraph has BT direction and is isolated → B should be above A (lower Y)
        assert!(
            b_mermaid.y < a_mermaid.y,
            "mermaid: B.y={} should be less than A.y={} (BT override respected for isolated inner)",
            b_mermaid.y,
            a_mermaid.y
        );

        // Flux engine: both overrides respected
        let flux = FluxLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let flux_result = solve_svg(&flux, &diagram);
        let a_flux = &flux_result.geometry.nodes["A"].rect;
        let b_flux = &flux_result.geometry.nodes["B"].rect;
        // Inner BT respected here too
        assert!(
            b_flux.y < a_flux.y,
            "flux: B.y={} should be less than A.y={} (BT override respected)",
            b_flux.y,
            a_flux.y
        );
    }

    #[test]
    fn mermaid_non_isolated_override_matches_parent_flow_in_svg_and_mmds() {
        let input = include_str!("../../../tests/fixtures/flowchart/direction_override.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));

        let svg_result = solve_svg(&mermaid, &diagram);
        let start = svg_result.geometry.nodes["Start"].rect;
        let sg = svg_result.geometry.subgraphs["sg1"].rect;
        assert!(
            start.y + start.height <= sg.y + 0.001,
            "mermaid svg: Start should be above sg1 (no overlap): start_bottom={} sg_top={}",
            start.y + start.height,
            sg.y
        );

        let a_svg = svg_result.geometry.nodes["A"].rect;
        let b_svg = svg_result.geometry.nodes["B"].rect;
        let c_svg = svg_result.geometry.nodes["C"].rect;
        assert!(
            a_svg.y < b_svg.y && b_svg.y < c_svg.y,
            "mermaid svg: A/B/C should stack vertically when non-isolated override is ignored: A.y={} B.y={} C.y={}",
            a_svg.y,
            b_svg.y,
            c_svg.y
        );

        let mmds_result = solve_mmds_layout(&mermaid, &diagram);
        let a_mmds = mmds_result.geometry.nodes["A"].rect;
        let b_mmds = mmds_result.geometry.nodes["B"].rect;
        let c_mmds = mmds_result.geometry.nodes["C"].rect;
        assert!(
            a_mmds.y < b_mmds.y && b_mmds.y < c_mmds.y,
            "mermaid mmds: A/B/C should stack vertically when non-isolated override is ignored: A.y={} B.y={} C.y={}",
            a_mmds.y,
            b_mmds.y,
            c_mmds.y
        );
    }

    #[test]
    fn mermaid_default_direction_matches_nested_with_siblings_fixture() {
        let input = include_str!("../../../tests/fixtures/flowchart/nested_with_siblings.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));

        for (label, result) in [
            ("svg", solve_svg(&mermaid, &diagram)),
            ("mmds", solve_mmds_layout(&mermaid, &diagram)),
        ] {
            let a = result.geometry.nodes["A"].rect;
            let b = result.geometry.nodes["B"].rect;
            let c = result.geometry.nodes["C"].rect;
            let d = result.geometry.nodes["D"].rect;

            assert!(
                (a.x - b.x).abs() < 1.0 && (c.x - d.x).abs() < 1.0,
                "mermaid {label} nested_with_siblings: sibling subgraphs should stack A->B and C->D vertically (x aligned): A.x={} B.x={} C.x={} D.x={}",
                a.x,
                b.x,
                c.x,
                d.x
            );
            assert!(
                a.y < b.y && b.y < c.y && c.y < d.y,
                "mermaid {label} nested_with_siblings: expected vertical order A < B < C < D; got A.y={} B.y={} C.y={} D.y={}",
                a.y,
                b.y,
                c.y,
                d.y
            );
        }
    }

    #[test]
    fn mermaid_subgraph_as_node_edge_uses_isolated_default_direction() {
        let input = include_str!("../../../tests/fixtures/flowchart/subgraph_as_node_edge.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics.clone()));
        let svg_result = solve_svg(&mermaid, &diagram);

        let api_svg = svg_result.geometry.nodes["API"].rect;
        let db_svg = svg_result.geometry.nodes["DB"].rect;
        assert!(
            (api_svg.y - db_svg.y).abs() < 1.0 && (api_svg.x - db_svg.x).abs() > 10.0,
            "mermaid svg subgraph_as_node_edge: API and DB should be side-by-side (isolated default dir): API=({}, {}), DB=({}, {})",
            api_svg.x,
            api_svg.y,
            db_svg.x,
            db_svg.y
        );

        let mmds_result = solve_mmds_layout(&mermaid, &diagram);
        let api_mmds = mmds_result.geometry.nodes["API"].rect;
        let db_mmds = mmds_result.geometry.nodes["DB"].rect;
        assert!(
            (api_mmds.y - db_mmds.y).abs() < 1.0 && (api_mmds.x - db_mmds.x).abs() > 10.0,
            "mermaid mmds subgraph_as_node_edge: API and DB should be side-by-side (isolated default dir): API=({}, {}), DB=({}, {})",
            api_mmds.x,
            api_mmds.y,
            db_mmds.x,
            db_mmds.y
        );
    }

    #[test]
    fn mermaid_mmds_keeps_isolated_direction_override_layouted() {
        let input =
            include_str!("../../../tests/fixtures/flowchart/subgraph_direction_isolated.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let mmds_result = solve_mmds_layout(&mermaid, &diagram);

        let a = mmds_result.geometry.nodes["A"].rect;
        let b = mmds_result.geometry.nodes["B"].rect;
        let c = mmds_result.geometry.nodes["C"].rect;
        assert!(
            (a.y - b.y).abs() < 1.0 && (b.y - c.y).abs() < 1.0,
            "mermaid mmds subgraph_direction_isolated: A/B/C should share row in LR override; A.y={} B.y={} C.y={}",
            a.y,
            b.y,
            c.y
        );
        assert!(
            a.x < b.x && b.x < c.x,
            "mermaid mmds subgraph_direction_isolated: A/B/C should be ordered left-to-right; A.x={} B.x={} C.x={}",
            a.x,
            b.x,
            c.x
        );
    }

    #[test]
    fn mermaid_nested_subgraph_bounds_are_compact_after_policy_normalization() {
        let input = include_str!("../../../tests/fixtures/flowchart/nested_subgraph.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let result = solve_svg(&mermaid, &diagram);

        let outer = result.geometry.subgraphs["outer"].rect;
        let inner = result.geometry.subgraphs["inner"].rect;
        assert!(
            inner.height < 160.0,
            "mermaid nested_subgraph: inner height should stay compact; got {}",
            inner.height
        );
        assert!(
            outer.height < 220.0,
            "mermaid nested_subgraph: outer height should stay compact; got {}",
            outer.height
        );
    }

    #[test]
    fn mermaid_multi_subgraph_direction_override_bottom_cluster_is_compact_and_centered() {
        let input =
            include_str!("../../../tests/fixtures/flowchart/multi_subgraph_direction_override.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let result = solve_svg(&mermaid, &diagram);

        let g = result.geometry.subgraphs["G"].rect;
        let e = result.geometry.nodes["E"].rect;
        let f = result.geometry.nodes["F"].rect;
        let g_center_x = g.x + g.width / 2.0;
        let feed_center_x = ((e.x + e.width / 2.0) + (f.x + f.width / 2.0)) / 2.0;

        assert!(
            g.height < 180.0,
            "mermaid multi_subgraph_direction_override: G height should be compact; got {}",
            g.height
        );
        assert!(
            g.y > e.y,
            "mermaid multi_subgraph_direction_override: G should be below middle tier; G.y={} E.y={}",
            g.y,
            e.y
        );
        assert!(
            (g_center_x - feed_center_x).abs() < 120.0,
            "mermaid multi_subgraph_direction_override: G should stay centered under incoming feeds; G.cx={} feeds.cx={}",
            g_center_x,
            feed_center_x
        );
    }

    #[test]
    fn mermaid_nested_subgraph_edge_keeps_compact_subgraph_to_node_gap() {
        let input = include_str!("../../../tests/fixtures/flowchart/nested_subgraph_edge.mmd");
        let flowchart = crate::parser::parse_flowchart(input).unwrap();
        let diagram = crate::graph::build_diagram(&flowchart);
        let metrics = SvgTextMetrics::new(16.0, 15.0, 15.0);
        let mermaid = MermaidLayeredEngine::with_mode(MeasurementMode::Svg(metrics));
        let result = solve_svg(&mermaid, &diagram);

        let cloud = result.geometry.subgraphs["cloud"].rect;
        let monitoring = result.geometry.nodes["Monitoring"].rect;
        let gap = monitoring.y - (cloud.y + cloud.height);

        assert!(
            gap > 8.0,
            "mermaid nested_subgraph_edge: subgraph->node gap should remain visible; got {}",
            gap
        );
        assert!(
            gap < 90.0,
            "mermaid nested_subgraph_edge: subgraph->node gap should stay compact; got {}",
            gap
        );
    }
}
