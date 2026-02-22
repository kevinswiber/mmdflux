//! Flowchart diagram instance implementation.

use crate::diagram::{
    AlgorithmId, EdgeRouting, EngineAlgorithmId, EngineConfig, EngineId, GraphSolveRequest,
    OutputFormat, RenderConfig, RenderError,
};
use crate::diagrams::flowchart::render::svg::render_svg_from_geometry;
use crate::engines::graph::GraphEngineRegistry;
use crate::graph::{Diagram, build_diagram};
use crate::mmds::to_mmds_json;
use crate::parser::parse_flowchart;
use crate::registry::DiagramInstance;
use crate::render::{RenderOptions, layout_config_for_diagram, render_text_from_layout};

/// Flowchart diagram instance.
///
/// Wraps the existing flowchart parsing and rendering logic behind
/// the `DiagramInstance` trait.
pub struct FlowchartInstance {
    /// Built diagram model.
    diagram: Option<Diagram>,
}

impl FlowchartInstance {
    /// Create a new flowchart instance.
    pub fn new() -> Self {
        Self { diagram: None }
    }
}

impl Default for FlowchartInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for FlowchartInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let flowchart = parse_flowchart(input)?;
        self.diagram = Some(build_diagram(&flowchart));
        Ok(())
    }

    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError> {
        let diagram = self.diagram.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        let annotated;
        let diagram = if config.show_ids {
            annotated = annotate_node_ids(diagram);
            &annotated
        } else {
            diagram
        };

        // Resolve engine (default: flux-layered).
        let engine_id = config
            .layout_engine
            .unwrap_or_else(|| EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered));
        engine_id.check_available()?;
        engine_id.check_routing_style(config)?;

        let mut options: RenderOptions = config.into();
        options.output_format = format;

        match format {
            OutputFormat::Mmds => {
                // MMDS: use solve() to obtain geometry and optionally routed paths.
                let request = GraphSolveRequest::from_config(config, format);
                let registry = GraphEngineRegistry::default();
                let engine = registry.get_solver(engine_id).ok_or_else(|| RenderError {
                    message: format!("no engine registered for: {engine_id}"),
                })?;
                let result = engine.solve(
                    diagram,
                    &EngineConfig::Layered(config.layout.clone()),
                    &request,
                )?;
                to_mmds_json(
                    diagram,
                    &result.geometry,
                    result.routed.as_ref(),
                    config.geometry_level,
                    config.path_simplification,
                    Some(engine_id),
                )
            }
            OutputFormat::Svg => {
                // SVG: solve() produces GraphGeometry; render_svg_from_geometry() renders it.
                // This dispatches layout through the engine registry so --layout-engine
                // is respected. Edge routing mode is derived from engine capabilities +
                // the resolved routing style (already in options.edge_routing).
                let request = GraphSolveRequest::from_config(config, format);
                let registry = GraphEngineRegistry::default();
                let engine = registry.get_solver(engine_id).ok_or_else(|| RenderError {
                    message: format!("no engine registered for: {engine_id}"),
                })?;
                let result = engine.solve(
                    diagram,
                    &EngineConfig::Layered(config.layout.clone()),
                    &request,
                )?;
                let edge_routing = options.edge_routing.unwrap_or(EdgeRouting::OrthogonalRoute);
                Ok(render_svg_from_geometry(
                    diagram,
                    &options,
                    &result.geometry,
                    edge_routing,
                ))
            }
            _ => {
                // Text/Ascii: engine → text adapter → text renderer.
                let request = GraphSolveRequest::from_config(config, format);
                let registry = GraphEngineRegistry::default();
                let engine = registry.get_solver(engine_id).ok_or_else(|| RenderError {
                    message: format!("no engine registered for: {engine_id}"),
                })?;
                let result = engine.solve(
                    diagram,
                    &EngineConfig::Layered(config.layout.clone()),
                    &request,
                )?;
                let mut text_config = layout_config_for_diagram(diagram, &options);
                text_config.ranker = options.ranker;
                let layout = super::render::text_adapter::geometry_to_text_layout(
                    diagram,
                    &result.geometry,
                    &text_config,
                );
                Ok(render_text_from_layout(diagram, &layout, &options))
            }
        }
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        matches!(
            format,
            OutputFormat::Text | OutputFormat::Ascii | OutputFormat::Svg | OutputFormat::Mmds
        )
    }
}

/// Create a copy of the diagram with node labels annotated as "ID: Label".
/// Skips nodes where label == id (bare nodes).
fn annotate_node_ids(diagram: &Diagram) -> Diagram {
    let mut annotated = diagram.clone();
    for node in annotated.nodes.values_mut() {
        if node.label != node.id {
            node.label = format!("{}: {}", node.id, node.label);
        }
    }
    annotated
}
