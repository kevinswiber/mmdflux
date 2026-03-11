//! Class diagram instance implementation.

use super::compiler;
use super::parser::parse_class_diagram;
use crate::diagram::{
    AlgorithmId, EngineAlgorithmId, EngineId, OutputFormat, RenderConfig, RenderError,
};
use crate::engines::graph::solve_graph_family;
use crate::graph::Diagram;
use crate::registry::DiagramInstance;
use crate::render::RenderOptions;

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

        // Solve layout through the shared graph-family pipeline.
        let result = solve_graph_family(diagram, engine_id, config, format)?;

        let mut options: RenderOptions = config.into();
        options.output_format = format;

        // Dispatch to format-owned emitters.
        match format {
            OutputFormat::Mmds => crate::formats::mmds::render_mmds_full(
                "class",
                diagram,
                &result,
                config.geometry_level,
                config.path_simplification,
            ),
            OutputFormat::Svg => Ok(crate::formats::svg::render_svg_with_options(
                diagram, &result, &options,
            )),
            OutputFormat::Text | OutputFormat::Ascii => Ok(
                crate::formats::text::render_text_with_options(diagram, &result, &options),
            ),
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
