//! MMDS frontend compatibility namespace.
//!
//! MMDS is a source-format frontend, not a logical diagram type. The MMDS
//! contract, hydration, and replay implementation live under [`crate::mmds`];
//! this module preserves the frontend-oriented access path for callers that
//! work in terms of input source formats.

pub use crate::mmds::detect::{
    SUPPORTED_OUTPUT_FORMATS, detect_diagram_type, is_mmds_input, resolve_logical_diagram_id,
    supports_format,
};
pub use crate::mmds::hydrate::{
    MmdsHydrationError, from_mmds_output, from_mmds_str, hydrate_graph_geometry_from_mmds,
    hydrate_graph_geometry_from_output, hydrate_graph_geometry_from_output_with_diagram,
    hydrate_routed_geometry_from_mmds, hydrate_routed_geometry_from_output, stub_hydrate,
};
pub use crate::mmds::parse::{parse_with_profiles, validate_input};
pub use crate::mmds::replay::{render_input, render_output};
pub use crate::mmds::{evaluate_mmds_profiles, parse_mmds_input};
