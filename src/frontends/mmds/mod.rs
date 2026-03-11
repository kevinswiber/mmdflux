//! MMDS frontend boundary.
//!
//! MMDS is a source-format frontend, not a logical diagram type. The frontend
//! ingests MMDS JSON, resolves the logical diagram type from metadata, and
//! reuses the graph-family pipeline or hydrated geometry as needed.

mod detect;
mod hydrate;
mod parse;
mod render_input;

pub use detect::{
    SUPPORTED_OUTPUT_FORMATS, detect_diagram_type, is_mmds_input, resolve_logical_diagram_id,
    supports_format,
};
pub use hydrate::{
    MmdsHydrationError, from_mmds_output, from_mmds_str, hydrate_graph_geometry_from_mmds,
    hydrate_graph_geometry_from_output, hydrate_graph_geometry_from_output_with_diagram,
    hydrate_routed_geometry_from_mmds, hydrate_routed_geometry_from_output, stub_hydrate,
};
pub use parse::{parse_with_profiles, validate_input};
pub use render_input::{render_input, render_output};

pub use crate::mmds::{evaluate_mmds_profiles, parse_mmds_input};
