//! MMDS diagram instance.

use super::{
    MmdsProfileNegotiation, evaluate_mmds_profiles_for_output, from_mmds_output,
    hydrate_graph_geometry_from_output_with_diagram, parse_mmds_input,
};
use crate::engines::graph::{EdgeRouting, GeometryLevel, OutputFormat, RenderConfig, RenderError};
use crate::mmds::{MmdsOutput, generate_mermaid_from_mmds};
use crate::registry::DiagramInstance;
use crate::render::{RenderOptions, render, render_svg_from_geometry};

/// MMDS diagram instance.
#[derive(Default)]
pub struct MmdsInstance {
    parsed_payload: Option<MmdsOutput>,
    profile_negotiation: Option<MmdsProfileNegotiation>,
}

impl MmdsInstance {
    /// Returns true when the instance has a parsed MMDS payload.
    pub fn has_parsed_payload(&self) -> bool {
        self.parsed_payload.is_some()
    }

    /// Access the parsed MMDS payload.
    pub fn parsed_payload(&self) -> Option<&MmdsOutput> {
        self.parsed_payload.as_ref()
    }

    /// Access profile negotiation for the last parsed payload.
    pub fn profile_negotiation(&self) -> Option<&MmdsProfileNegotiation> {
        self.profile_negotiation.as_ref()
    }
}

impl DiagramInstance for MmdsInstance {
    fn parse(&mut self, input: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = parse_mmds_input(input)?;
        self.profile_negotiation = Some(evaluate_mmds_profiles_for_output(&payload));
        self.parsed_payload = Some(payload);
        Ok(())
    }

    fn render(&self, format: OutputFormat, config: &RenderConfig) -> Result<String, RenderError> {
        let payload = self.parsed_payload.as_ref().ok_or_else(|| RenderError {
            message: "No diagram parsed. Call parse() first.".to_string(),
        })?;

        if !matches!(payload.geometry_level.as_str(), "layout" | "routed") {
            return Err(RenderError {
                message: format!(
                    "MMDS validation error: invalid geometry_level '{}'",
                    payload.geometry_level
                ),
            });
        }

        if matches!(format, OutputFormat::Mmds) {
            let output = if payload.geometry_level == "routed"
                && config.geometry_level == GeometryLevel::Layout
            {
                strip_routed_fields(payload)
            } else {
                payload.clone()
            };
            let json = serde_json::to_string_pretty(&output).map_err(|err| RenderError {
                message: format!("MMDS serialization error: {err}"),
            })?;
            return Ok(json);
        }

        if matches!(format, OutputFormat::Mermaid) {
            return generate_mermaid_from_mmds(payload).map_err(|err| RenderError {
                message: err.to_string(),
            });
        }

        let diagram = from_mmds_output(payload).map_err(|err| RenderError {
            message: err.to_string(),
        })?;

        let mut options: RenderOptions = config.into();
        options.output_format = format;

        if payload.geometry_level == "routed" && matches!(format, OutputFormat::Svg) {
            let geometry = hydrate_graph_geometry_from_output_with_diagram(payload, &diagram)
                .map_err(|err| RenderError {
                    message: err.to_string(),
                })?;
            return Ok(render_svg_from_geometry(
                &diagram,
                &options,
                &geometry,
                EdgeRouting::EngineProvided,
            ));
        }

        Ok(render(&diagram, &options))
    }

    fn supports_format(&self, format: OutputFormat) -> bool {
        matches!(
            format,
            OutputFormat::Text
                | OutputFormat::Ascii
                | OutputFormat::Svg
                | OutputFormat::Mmds
                | OutputFormat::Mermaid
        )
    }
}

/// Produce a layout-level copy of a routed MMDS payload by stripping
/// edge paths, label positions, backward flags, and subgraph bounds.
fn strip_routed_fields(payload: &MmdsOutput) -> MmdsOutput {
    let mut output = payload.clone();
    output.geometry_level = "layout".to_string();
    for edge in &mut output.edges {
        edge.path = None;
        edge.label_position = None;
        edge.is_backward = None;
    }
    for sg in &mut output.subgraphs {
        sg.bounds = None;
    }
    output
}
