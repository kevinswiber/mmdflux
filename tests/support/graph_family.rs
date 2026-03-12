#![allow(dead_code)]

pub use mmdflux::engines::graph::algorithms::layered::MeasurementMode;
pub use mmdflux::engines::graph::contracts::EngineConfig;
use mmdflux::graph::geometry::GraphGeometry;
use mmdflux::{Diagram, RenderError};

pub fn run_layered_layout(
    mode: &MeasurementMode,
    diagram: &Diagram,
    config: &EngineConfig,
) -> Result<GraphGeometry, RenderError> {
    mmdflux::engines::graph::algorithms::layered::run_layered_layout(mode, diagram, config)
}
