use crate::mmds::hydrate::hydrate_graph_geometry_from_mmds;
use crate::{OutputFormat, RenderConfig};

#[test]
fn hydration_restores_grid_projection_from_generated_mmds() {
    let json = crate::render_diagram(
        "graph TD\nA-->B",
        OutputFormat::Mmds,
        &RenderConfig::default(),
    )
    .expect("MMDS render should succeed");

    let geom = hydrate_graph_geometry_from_mmds(&json).expect("layout geometry should hydrate");
    let projection = geom
        .grid_projection
        .expect("grid projection should hydrate");
    assert!(projection.node_ranks.contains_key("A"));
    assert!(projection.node_ranks.contains_key("B"));
}
