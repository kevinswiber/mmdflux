#[allow(unused_imports)]
pub use mmdflux::engines::graph::algorithms::layered::{MeasurementMode, run_layered_layout};
#[allow(unused_imports)]
pub use mmdflux::engines::graph::contracts::{
    EngineConfig, GraphGeometryContract, GraphSolveRequest,
};
use mmdflux::graph::measure::default_proportional_text_metrics;
use mmdflux::{GeometryLevel, RoutingStyle};

#[allow(dead_code)]
pub fn default_grid_request(
    level: GeometryLevel,
    routing_style: Option<RoutingStyle>,
) -> GraphSolveRequest {
    GraphSolveRequest::new(
        MeasurementMode::Grid,
        GraphGeometryContract::Canonical,
        level,
        routing_style,
    )
}

#[allow(dead_code)]
pub fn default_proportional_mode() -> MeasurementMode {
    MeasurementMode::Proportional(default_proportional_text_metrics())
}

#[allow(dead_code)]
pub fn default_proportional_request(
    geometry_contract: GraphGeometryContract,
    level: GeometryLevel,
    routing_style: Option<RoutingStyle>,
) -> GraphSolveRequest {
    GraphSolveRequest::new(
        default_proportional_mode(),
        geometry_contract,
        level,
        routing_style,
    )
}
