//! Residual crate-local tests that intentionally remain cross-pipeline.

mod composition_probe;
mod cross_pipeline;
mod edge_endpoint_invariant;
mod event_change_mapping;
mod graph_routing_pipeline;
mod grid_routing_regression;
mod label_bend_endpoint_invariants;
mod label_node_overlap;
mod layered_adapter_pipeline;
mod layout_stability;
mod mmds_commands;
mod mmds_diff;
mod mmds_document_serialization;
mod mmds_roundtrip;
mod port_attachment_observation;
mod singleton_centering_observation;
pub(crate) mod stub_metrics;
mod svg_render_pipeline;
mod text_render_pipeline;

mod direction_policy_cross;
mod float_router_direction;
mod graph_diagram_cross;
mod layered_kernel_bend;
mod render_time_placer;
mod sequence_layout;
mod subgraph_render_fixture;
mod wrap_pipeline;
