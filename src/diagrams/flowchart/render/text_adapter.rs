//! Text adapter: converts engine `GraphGeometry` to the integer `Layout`
//! struct consumed by the text renderer.
//!
//! This is the bridge between the engine pipeline (which produces float
//! coordinates via `MeasurementMode::Text`) and text rendering (which
//! operates on character-grid integer coordinates).

use super::layout::{Layout, LayoutConfig, compute_layout_direct};
use crate::diagrams::flowchart::geometry::GraphGeometry;
use crate::graph::Diagram;

/// Convert engine-produced `GraphGeometry` (with text-scale node dimensions)
/// to the integer-coordinate `Layout` struct consumed by the text renderer.
///
/// # Delegating stub
///
/// Currently delegates to `compute_layout_direct()` — the geometry parameter
/// is unused. Subsequent tasks (1.4, 2.1, 2.2, 3.1-3.3) incrementally
/// replace the delegation with direct geometry conversion.
pub fn geometry_to_text_layout(
    diagram: &Diagram,
    _geometry: &GraphGeometry,
    config: &LayoutConfig,
) -> Layout {
    compute_layout_direct(diagram, config)
}
