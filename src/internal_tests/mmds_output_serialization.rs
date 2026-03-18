//! MMDS output serialization tests that require engine-produced geometry.
//!
//! Moved from `mmds/output.rs` to respect the mmds→engines boundary:
//! mmds must not import from engines, even in tests.

use crate::engines::graph::EngineConfig;
use crate::engines::graph::algorithms::layered::{MeasurementMode, run_layered_layout};
use crate::graph::geometry::{GraphGeometry, RoutedGraphGeometry};
use crate::graph::routing::{EdgeRouting, route_graph_geometry};
use crate::graph::{GeometryLevel, Graph};
use crate::mmds::output::{Output, to_json, to_layout, to_routed};
use crate::simplification::PathSimplification;

fn layout_geometry(input: &str) -> (Graph, GraphGeometry) {
    use crate::diagrams::flowchart::compile_to_graph;
    use crate::mermaid::parse_flowchart;

    let fc = parse_flowchart(input).unwrap();
    let diagram = compile_to_graph(&fc);
    let config =
        EngineConfig::Layered(crate::engines::graph::algorithms::layered::LayoutConfig::default());
    let geom = run_layered_layout(&MeasurementMode::Grid, &diagram, &config).unwrap();
    (diagram, geom)
}

fn routed_geometry(diagram: &Graph, geometry: &GraphGeometry) -> RoutedGraphGeometry {
    route_graph_geometry(diagram, geometry, EdgeRouting::PolylineRoute)
}

#[test]
fn layout_json_has_version_and_level() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.version, 1);
    assert_eq!(output.geometry_level, "layout");
}

#[test]
fn layout_json_has_metadata() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.defaults.node.shape, "rectangle");
    assert_eq!(output.defaults.edge.stroke, "solid");
    assert_eq!(output.defaults.edge.arrow_start, "none");
    assert_eq!(output.defaults.edge.arrow_end, "normal");
    assert_eq!(output.defaults.edge.minlen, 1);
    assert_eq!(output.metadata.diagram_type, "flowchart");
    assert_eq!(output.metadata.direction, "TD");
    assert!(output.metadata.bounds.width > 0.0);
    assert!(output.metadata.bounds.height > 0.0);
}

#[test]
fn layout_json_has_nodes_with_positions() {
    let (diagram, geom) = layout_geometry("graph TD\nA[Start]-->B[End]");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();

    assert_eq!(output.nodes.len(), 2);
    let node_a = output.nodes.iter().find(|n| n.id == "A").unwrap();
    assert_eq!(node_a.label, "Start");
    assert_eq!(node_a.shape, "rectangle");
    assert!(node_a.size.width > 0.0);
    assert!(node_a.size.height > 0.0);
}

#[test]
fn layout_json_edges_have_no_paths() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let json = to_layout(&diagram, &geom);

    // Layout-level: no edge geometry fields
    assert!(!json.contains("\"path\""));
    assert!(!json.contains("\"label_position\""));
    assert!(!json.contains("\"is_backward\""));

    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.edges.len(), 1);
    assert_eq!(output.edges[0].source, "A");
    assert_eq!(output.edges[0].target, "B");
    assert!(output.edges[0].path.is_none());
}

#[test]
fn layout_json_edge_semantics() {
    let (diagram, geom) = layout_geometry("graph TD\nA-.label.->B");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();

    let edge = &output.edges[0];
    assert_eq!(edge.id, "e0");
    assert_eq!(edge.stroke, "dotted");
    assert_eq!(edge.label, Some("label".to_string()));
    assert_eq!(edge.arrow_end, "normal");
    assert_eq!(edge.minlen, 1);
}

#[test]
fn layout_omits_default_edge_fields() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let json = to_json(
        &diagram,
        &geom,
        None,
        GeometryLevel::Layout,
        PathSimplification::None,
        None,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edge = &value["edges"][0];
    assert!(edge.get("stroke").is_none());
    assert!(edge.get("arrow_start").is_none());
    assert!(edge.get("arrow_end").is_none());
    assert!(edge.get("minlen").is_none());
}

#[test]
fn layout_keeps_non_default_edge_fields() {
    let (diagram, geom) = layout_geometry("graph TD\nA -.-> B\nC --x D\nE ----> F");
    let json = to_json(
        &diagram,
        &geom,
        None,
        GeometryLevel::Layout,
        PathSimplification::None,
        None,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let edges = value["edges"].as_array().unwrap();
    assert_eq!(edges[0]["stroke"], "dotted");
    assert_eq!(edges[1]["arrow_end"], "cross");
    assert!(edges[2]["minlen"].as_i64().unwrap() > 1);
}

#[test]
fn layout_omits_default_node_shape() {
    let (diagram, geom) = layout_geometry("graph TD\nA[Rect]\nB(Round)");
    let json = to_json(
        &diagram,
        &geom,
        None,
        GeometryLevel::Layout,
        PathSimplification::None,
        None,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = value["nodes"].as_array().unwrap();
    assert!(nodes[0].get("shape").is_none());
    assert_eq!(nodes[1]["shape"], "round");
}

#[test]
fn layout_omits_empty_subgraphs_key() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let json = to_json(
        &diagram,
        &geom,
        None,
        GeometryLevel::Layout,
        PathSimplification::None,
        None,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("subgraphs").is_none());
}

#[test]
fn layout_deserializes_with_defaults() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let json = to_json(
        &diagram,
        &geom,
        None,
        GeometryLevel::Layout,
        PathSimplification::None,
        None,
    )
    .unwrap();
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.nodes[0].shape, "rectangle");
    assert_eq!(output.edges[0].stroke, "solid");
    assert_eq!(output.edges[0].arrow_start, "none");
    assert_eq!(output.edges[0].arrow_end, "normal");
    assert_eq!(output.edges[0].minlen, 1);
    assert!(output.subgraphs.is_empty());
}

#[test]
fn layout_json_subgraphs() {
    let (diagram, geom) = layout_geometry("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();

    assert_eq!(output.subgraphs.len(), 1);
    assert_eq!(output.subgraphs[0].id, "sg1");
    assert_eq!(output.subgraphs[0].title, "Group");
    assert_eq!(output.subgraphs[0].direction, None);
    assert!(output.subgraphs[0].bounds.is_none());
}

#[test]
fn layout_json_subgraph_direction_override() {
    let (diagram, geom) =
        layout_geometry("graph TD\nsubgraph sg1[Group]\ndirection LR\nA-->B\nend");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.subgraphs[0].direction.as_deref(), Some("LR"));
}

#[test]
fn layout_json_nodes_sorted_by_id() {
    let (diagram, geom) = layout_geometry("graph TD\nC-->B\nB-->A");
    let json = to_layout(&diagram, &geom);
    let output: Output = serde_json::from_str(&json).unwrap();

    let ids: Vec<&str> = output.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, vec!["A", "B", "C"]);
}

#[test]
fn layout_json_direction_variants() {
    for (dir_str, expected) in [("TD", "TD"), ("LR", "LR"), ("BT", "BT"), ("RL", "RL")] {
        let input = format!("graph {dir_str}\nA-->B");
        let (diagram, geom) = layout_geometry(&input);
        let json = to_layout(&diagram, &geom);
        let output: Output = serde_json::from_str(&json).unwrap();
        assert_eq!(output.metadata.direction, expected);
    }
}

#[test]
fn routed_json_has_version_and_level() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let routed = routed_geometry(&diagram, &geom);
    let json = to_routed(&diagram, &geom, &routed);
    let output: Output = serde_json::from_str(&json).unwrap();
    assert_eq!(output.version, 1);
    assert_eq!(output.geometry_level, "routed");
}

#[test]
fn routed_json_includes_edge_paths() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let routed = routed_geometry(&diagram, &geom);
    let json = to_routed(&diagram, &geom, &routed);

    assert!(json.contains("\"path\""));

    let output: Output = serde_json::from_str(&json).unwrap();
    let edge = &output.edges[0];
    assert!(edge.path.is_some());
    assert!(edge.path.as_ref().unwrap().len() >= 2);
    assert!(edge.is_backward.is_some());
}

#[test]
fn routed_json_includes_metadata_bounds() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let routed = routed_geometry(&diagram, &geom);
    let json = to_routed(&diagram, &geom, &routed);
    let output: Output = serde_json::from_str(&json).unwrap();

    let bounds = &output.metadata.bounds;
    assert!(bounds.width > 0.0);
    assert!(bounds.height > 0.0);
}

#[test]
fn routed_json_subgraph_bounds() {
    let (diagram, geom) = layout_geometry("graph TD\nsubgraph sg1[Group]\nA-->B\nend");
    let routed = routed_geometry(&diagram, &geom);
    let json = to_routed(&diagram, &geom, &routed);
    let output: Output = serde_json::from_str(&json).unwrap();

    let sg = &output.subgraphs[0];
    assert!(sg.bounds.is_some());
    let bounds = sg.bounds.as_ref().unwrap();
    assert!(bounds.width > 0.0);
}

#[test]
fn to_mmds_json_dispatches_by_level() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let routed = routed_geometry(&diagram, &geom);

    let layout_json = to_json(
        &diagram,
        &geom,
        Some(&routed),
        GeometryLevel::Layout,
        PathSimplification::None,
        None,
    )
    .unwrap();
    assert!(!layout_json.contains("\"path\""));

    let routed_json = to_json(
        &diagram,
        &geom,
        Some(&routed),
        GeometryLevel::Routed,
        PathSimplification::None,
        None,
    )
    .unwrap();
    assert!(routed_json.contains("\"path\""));
}

#[test]
fn routed_json_includes_port_metadata() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let routed = routed_geometry(&diagram, &geom);
    let json = to_routed(&diagram, &geom, &routed);
    let output: Output = serde_json::from_str(&json).unwrap();
    let edge = &output.edges[0];
    // A simple TD A-->B should have port metadata at routed level
    assert!(edge.source_port.is_some());
    assert!(edge.target_port.is_some());
    let sp = edge.source_port.as_ref().unwrap();
    let tp = edge.target_port.as_ref().unwrap();
    assert_eq!(sp.face, "bottom");
    assert_eq!(tp.face, "top");
    assert_eq!(sp.group_size, 1);
    assert_eq!(tp.group_size, 1);
}

#[test]
fn to_mmds_json_routed_requires_routed_geometry() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B");
    let err = to_json(
        &diagram,
        &geom,
        None,
        GeometryLevel::Routed,
        PathSimplification::None,
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("routed MMDS output requested"));
}

#[test]
fn routed_mmds_metadata_uses_routed_bounds_not_layout_bounds() {
    let (diagram, geom) = layout_geometry("graph TD\nA-->B\nB-->C\nC-->A");
    let routed = routed_geometry(&diagram, &geom);

    let routed_output: Output = serde_json::from_str(&to_routed(&diagram, &geom, &routed)).unwrap();

    // The MMDS routed bounds must always match the routed geometry bounds.
    assert!(
        (routed_output.metadata.bounds.width - routed.bounds.width).abs() < 0.001,
        "routed MMDS metadata.bounds.width should match routed geometry bounds.width; \
         mmds={:.2}, routed_geom={:.2}",
        routed_output.metadata.bounds.width,
        routed.bounds.width
    );
    assert!(
        (routed_output.metadata.bounds.height - routed.bounds.height).abs() < 0.001,
        "routed MMDS metadata.bounds.height should match routed geometry bounds.height; \
         mmds={:.2}, routed_geom={:.2}",
        routed_output.metadata.bounds.height,
        routed.bounds.height
    );
}
