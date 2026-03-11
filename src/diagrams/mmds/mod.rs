//! MMDS input diagram implementation.

mod hydrate;
mod instance;

pub use hydrate::{
    MmdsHydrationError, from_mmds_output, from_mmds_str, hydrate_graph_geometry_from_mmds,
    hydrate_graph_geometry_from_output, hydrate_graph_geometry_from_output_with_diagram,
    hydrate_routed_geometry_from_mmds, hydrate_routed_geometry_from_output, stub_hydrate,
};
pub use instance::MmdsInstance;

use crate::diagram::{DiagramFamily, OutputFormat};
pub use crate::mmds::{
    MmdsParseError, MmdsProfileNegotiation, evaluate_mmds_profiles,
    evaluate_mmds_profiles_for_output, parse_mmds_input,
};
use crate::registry::{DiagramDefinition, DiagramDetector};

/// Detect if input appears to be MMDS JSON.
///
/// This detector is intentionally conservative:
/// - Input must look like JSON object text
/// - Required MMDS marker fields must be present
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
    let marker = format!("\"{key}\"");
    input.contains(&marker)
}

/// MMDS diagram definition for registry.
pub fn definition() -> DiagramDefinition {
    DiagramDefinition {
        id: "mmds",
        family: DiagramFamily::Graph,
        detector: is_mmds_input as DiagramDetector,
        factory: || Box::new(MmdsInstance::default()),
        supported_formats: &[
            OutputFormat::Text,
            OutputFormat::Ascii,
            OutputFormat::Svg,
            OutputFormat::Mmds,
            OutputFormat::Mermaid,
        ],
    }
}
