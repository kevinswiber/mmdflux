//! Internal layered graph engine decomposition.

#[path = "../../../layered/acyclic.rs"]
pub(crate) mod acyclic;
#[path = "../../../layered/bk.rs"]
pub(crate) mod bk;
#[path = "../../../layered/border.rs"]
pub(crate) mod border;
pub mod debug;
#[path = "../../../layered/graph.rs"]
pub(crate) mod graph;
#[path = "../../../layered/nesting.rs"]
pub(crate) mod nesting;
#[path = "../../../layered/network_simplex.rs"]
pub(crate) mod network_simplex;
#[path = "../../../layered/normalize.rs"]
pub mod normalize;
#[path = "../../../layered/order.rs"]
pub(crate) mod order;
#[path = "../../../layered/parent_dummy_chains.rs"]
pub(crate) mod parent_dummy_chains;
pub mod pipeline;
#[path = "../../../layered/position.rs"]
pub(crate) mod position;
#[path = "../../../layered/rank.rs"]
pub(crate) mod rank;
pub mod support;
#[path = "../../../layered/types.rs"]
pub mod types;

pub use graph::DiGraph;
pub use pipeline::{layout, layout_with_labels, run_layered_layout};
pub(crate) use support::{
    assign_node_intersects, compute_rank_sep_overrides, count_forward_edges_per_gap,
    extract_self_edges, insert_self_edge_dummies, label_dummy_gap_ranks,
    make_space_for_edge_labels, make_space_for_labeled_edges, position_self_edges,
    select_label_sides, switch_label_dummies, translate_layout_result,
};
pub use types::{
    Direction, EdgeLayout, LabelDummyStrategy, LayoutConfig, LayoutResult, NodeId, Point, Ranker,
    Rect, SelfEdge, SelfEdgeLayout,
};
