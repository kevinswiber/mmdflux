//! Flowchart diagram instance implementation.

use crate::diagram::{
    AlgorithmId, EngineAlgorithmId, EngineId, OutputFormat, RenderConfig, RenderError,
};
use crate::engines::graph::solve_graph_family;
use crate::graph::{Diagram, build_diagram};
use crate::parser::parse_flowchart;
use crate::registry::DiagramInstance;
use crate::render::RenderOptions;

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

        // Solve layout through the engine registry.
        let result = solve_graph_family(diagram, engine_id, config, format)?;

        let mut options: RenderOptions = config.into();
        options.output_format = format;

        // Dispatch to format-owned emitters.
        match format {
            OutputFormat::Mmds => crate::formats::mmds::render_mmds_full(
                "flowchart",
                diagram,
                &result,
                config.geometry_level,
                config.path_simplification,
            ),
            OutputFormat::Svg => Ok(crate::formats::svg::render_svg_with_options(
                diagram, &result, &options,
            )),
            _ => Ok(crate::formats::text::render_text_with_options(
                diagram, &result, &options,
            )),
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
