//! Class diagram instance implementation.

use super::compiler;
use super::parser::parse_class_diagram;
use crate::diagram::{
    AlgorithmId, EngineAlgorithmId, EngineConfig, EngineId, GeometryLevel, GraphSolveRequest,
    OutputFormat, RenderConfig, RenderError,
};
use crate::diagrams::flowchart::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::engines::graph::GraphEngineRegistry;
use crate::graph::Diagram;
use crate::mmds::to_mmds_json_typed;
use crate::registry::DiagramInstance;
use crate::render::{RenderOptions, render, render_svg_from_geometry};

/// Class diagram instance.
///
/// Parses class diagram syntax, compiles to `graph::Diagram`, then
/// renders through the shared graph-family pipeline.
pub struct ClassInstance {
    diagram: Option<Diagram>,
}

impl ClassInstance {
    /// Create a new class diagram instance.
    pub fn new() -> Self {
        Self { diagram: None }
    }
}

impl Default for ClassInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramInstance for ClassInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let model = parse_class_diagram(input)?;
        self.diagram = Some(compiler::compile(&model));
        Ok(())
    }

    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError> {
        let diagram = self.diagram.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        // Resolve engine (default: flux-layered, same as flowchart).
        let engine_id = config
            .layout_engine
            .unwrap_or_else(|| EngineAlgorithmId::new(EngineId::Flux, AlgorithmId::Layered));
        engine_id.check_available()?;
        engine_id.check_routing_style(config)?;

        let mut options: RenderOptions = config.into();
        options.output_format = format;

        match format {
            OutputFormat::Mmds => {
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
                to_mmds_json_typed(
                    "class",
                    diagram,
                    &result.geometry,
                    result.routed.as_ref(),
                    config.geometry_level,
                    config.path_simplification,
                    Some(engine_id),
                )
            }
            OutputFormat::Svg => {
                // SVG always needs routed paths for render_svg_from_geometry.
                let request = GraphSolveRequest {
                    geometry_level: GeometryLevel::Routed,
                    ..GraphSolveRequest::from_config(config, format)
                };
                let registry = GraphEngineRegistry::default();
                let engine = registry.get_solver(engine_id).ok_or_else(|| RenderError {
                    message: format!("no engine registered for: {engine_id}"),
                })?;
                let result = engine.solve(
                    diagram,
                    &EngineConfig::Layered(config.layout.clone()),
                    &request,
                )?;
                // Routing style selects the algorithm via edge_routing_for_style().
                let resolved_routing = config
                    .routing_style
                    .or_else(|| config.edge_preset.map(|p| p.expand().0));
                let edge_routing = engine_id.edge_routing_for_style(resolved_routing);
                let geom = if let Some(ref routed) = result.routed {
                    inject_routed_paths(&result.geometry, routed)
                } else {
                    result.geometry.clone()
                };
                Ok(render_svg_from_geometry(
                    diagram,
                    &options,
                    &geom,
                    edge_routing,
                ))
            }
            // Text/Ascii: use character-grid layout pipeline.
            OutputFormat::Text | OutputFormat::Ascii => Ok(render(diagram, &options)),
            _ => Err(RenderError {
                message: format!("{format} output is not supported for class diagrams"),
            }),
        }
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        matches!(
            format,
            OutputFormat::Text | OutputFormat::Ascii | OutputFormat::Svg | OutputFormat::Mmds
        )
    }
}

fn inject_routed_paths(geom: &GraphGeometry, routed: &RoutedGraphGeometry) -> GraphGeometry {
    let mut result = geom.clone();
    for routed_edge in &routed.edges {
        if let Some(layout_edge) = result
            .edges
            .iter_mut()
            .find(|e| e.index == routed_edge.index)
        {
            layout_edge.layout_path_hint = Some(routed_edge.path.clone());
            layout_edge.label_position = routed_edge.label_position;
        }
    }
    result
}
