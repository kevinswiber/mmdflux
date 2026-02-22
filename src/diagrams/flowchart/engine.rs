//! Layered layout engine adapter.
//!
//! Provides layered layout via `run_layered_layout` for text and SVG measurement
//! modes, and implements `GraphEngine` for `FluxLayeredEngine` and `MermaidLayeredEngine`.

use super::geometry::GraphGeometry;
use super::render::layout::build_layered_layout;
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
    let result = match mode {
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

        // SVG output: run the full SVG layout pipeline (subgraph post-processing,
        // direction overrides, padding, edge rerouting) via build_svg_layout().
        // MermaidLayeredEngine uses PolylineRoute routing (no orthogonal path
        // injection), preserving the legacy render_svg() behavior for this engine.
        if matches!(request.output_format, OutputFormat::Svg) {
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
                true, // mermaid-layered: skip overrides for non-isolated subgraphs
            );
            return Ok(GraphSolveResult {
                engine_id: self.id(),
                geometry,
                routed: None,
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
}
