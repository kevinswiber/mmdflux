//! MMDS input detection and logical diagram type resolution.

use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::mmds::{MmdsOutput, MmdsParseError, parse_mmds_input};

pub const SUPPORTED_OUTPUT_FORMATS: &[OutputFormat] = &[
    OutputFormat::Text,
    OutputFormat::Ascii,
    OutputFormat::Svg,
    OutputFormat::Mmds,
    OutputFormat::Mermaid,
];

#[must_use]
pub fn supports_format(format: OutputFormat) -> bool {
    SUPPORTED_OUTPUT_FORMATS.contains(&format)
}

/// Detect if input appears to be MMDS JSON.
#[must_use]
pub fn is_mmds_input(input: &str) -> bool {
    let trimmed = input.trim_start();
    trimmed.starts_with('{')
        && contains_json_key(trimmed, "version")
        && contains_json_key(trimmed, "geometry_level")
        && contains_json_key(trimmed, "metadata")
        && contains_json_key(trimmed, "nodes")
        && contains_json_key(trimmed, "edges")
}

fn contains_json_key(input: &str, key: &str) -> bool {
    input.contains(&format!("\"{key}\""))
}

/// Resolve the logical diagram ID carried by an MMDS payload.
pub fn resolve_logical_diagram_id(output: &MmdsOutput) -> Result<&'static str, RenderError> {
    match output.metadata.diagram_type.as_str() {
        "flowchart" => Ok("flowchart"),
        "class" => Ok("class"),
        other => Err(RenderError {
            message: format!(
                "MMDS input currently supports flowchart/class logical diagrams, got {other}"
            ),
        }),
    }
}

/// Parse MMDS input and resolve the logical diagram ID it hydrates.
pub fn detect_diagram_type(input: &str) -> Result<&'static str, MmdsParseError> {
    let output = parse_mmds_input(input)?;
    resolve_logical_diagram_id(&output).map_err(|error| MmdsParseError::new(error.message))
}
