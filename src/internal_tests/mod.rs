//! Residual crate-local tests that intentionally remain cross-pipeline.

mod cross_pipeline;
mod graph_routing_pipeline;
mod grid_routing_regression;
mod layered_adapter_pipeline;
mod mmds_output_serialization;
mod mmds_roundtrip;
mod svg_render_pipeline;
mod text_render_pipeline;

mod direction_policy_cross;
mod float_router_direction;
mod graph_diagram_cross;
mod sequence_layout;
mod subgraph_render_fixture;
