use std::fs;
use std::path::Path;

use super::hydrate::{hydrate_graph_geometry_from_mmds, hydrate_routed_geometry_from_mmds};
use super::{MmdsHydrationError, evaluate_mmds_profiles, from_mmds_str};
use crate::graph::{Direction, Shape};

#[test]
fn hydration_applies_defaults_to_omitted_node_and_edge_fields() {
    let payload = mmds_fixture("defaults-minimal.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    assert_eq!(diagram.nodes["A"].shape, Shape::Round);
    assert_eq!(diagram.edges[0].minlen, 2);
}

#[test]
fn hydration_maps_direction_subgraphs_and_minlen() {
    let payload = mmds_fixture("layout-with-subgraphs.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    assert_eq!(diagram.direction, Direction::LeftRight);
    assert_eq!(diagram.edges[0].minlen, 2);
    assert_eq!(diagram.edges[0].label.as_deref(), Some("go"));
    assert!(diagram.subgraphs.contains_key("sg1"));
    assert!(diagram.subgraphs.contains_key("sg2"));
    assert_eq!(diagram.subgraphs["sg1"].dir, Some(Direction::BottomTop));
    assert_eq!(diagram.subgraphs["sg2"].parent.as_deref(), Some("sg1"));
}

#[test]
fn hydration_reconstructs_compound_membership_for_nested_subgraphs() {
    let payload = mmds_fixture("layout-with-subgraphs.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    assert_eq!(
        diagram.subgraphs["sg1"].nodes,
        vec!["B".to_string(), "C".to_string()]
    );
    assert_eq!(diagram.subgraphs["sg2"].nodes, vec!["C".to_string()]);
}

#[test]
fn hydration_compound_membership_order_is_deterministic() {
    let payload = mmds_fixture("layout-with-subgraphs.json");
    let first = from_mmds_str(&payload).expect("valid hydration");
    let second = from_mmds_str(&payload).expect("valid hydration");

    assert_eq!(first.subgraphs["sg1"].nodes, second.subgraphs["sg1"].nodes);
    assert_eq!(first.subgraphs["sg2"].nodes, second.subgraphs["sg2"].nodes);
}

#[test]
fn hydration_rejects_dangling_edge_reference() {
    let payload = invalid_fixture("dangling-edge-target.json");
    let err = from_mmds_str(&payload).unwrap_err();

    assert!(matches!(err, MmdsHydrationError::DanglingEdgeTarget { .. }));
}

#[test]
fn hydration_rejects_dangling_subgraph_endpoint_intent_reference() {
    let payload = invalid_fixture("dangling-endpoint-intent-subgraph.json");
    let err = from_mmds_str(&payload).unwrap_err();

    assert!(matches!(
        err,
        MmdsHydrationError::DanglingEdgeToSubgraphIntent { .. }
    ));
}

#[test]
fn hydration_rejects_missing_required_id() {
    let payload = invalid_fixture("missing-node-id.json");
    let err = from_mmds_str(&payload).unwrap_err();

    assert!(matches!(err, MmdsHydrationError::MissingNodeId { .. }));
}

#[test]
fn hydration_rejects_invalid_enum_value() {
    let payload = invalid_fixture("invalid-shape.json");
    let err = from_mmds_str(&payload).unwrap_err();

    assert!(matches!(err, MmdsHydrationError::InvalidShape { .. }));
}

#[test]
fn hydration_rejects_cyclic_subgraph_parent_chain() {
    let payload = invalid_fixture("cyclic-subgraph-parent.json");
    let err = from_mmds_str(&payload).unwrap_err();

    assert!(matches!(
        err,
        MmdsHydrationError::CyclicSubgraphParentChain { .. }
    ));
}

#[test]
fn hydration_rejects_unsupported_mmds_core_version() {
    let payload = invalid_fixture("unsupported-version.json");
    let err = from_mmds_str(&payload).unwrap_err();

    assert!(matches!(err, MmdsHydrationError::UnsupportedVersion { .. }));
}

#[test]
fn hydration_preserves_deterministic_edge_order_by_edge_id() {
    let payload = mmds_fixture("layout-unsorted-edges.json");
    let diagram1 = from_mmds_str(&payload).unwrap();
    let diagram2 = from_mmds_str(&payload).unwrap();

    assert_eq!(diagram1.edges, diagram2.edges);
    let edge_pairs: Vec<(&str, &str)> = diagram1
        .edges
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    assert_eq!(edge_pairs, vec![("A", "B"), ("C", "A"), ("A", "C")]);
}

#[test]
fn hydration_ignores_unknown_extension_namespace() {
    let payload = mmds_fixture("layout-with-unknown-extension.json");
    assert!(from_mmds_str(&payload).is_ok());
}

#[test]
fn routed_mmds_hydrates_node_sizes_paths_and_label_positions() {
    let payload = positioned_fixture("routed-basic.json");
    let geom = hydrate_routed_geometry_from_mmds(&payload).expect("routed geometry should hydrate");

    assert_eq!(geom.nodes["A"].rect.width, 120.0);
    assert_eq!(geom.edges[0].path.len(), 3);
    assert_eq!(geom.edges[0].label_position.unwrap().x, 80.0);
    assert_eq!(geom.subgraphs["sg1"].rect.width, 180.0);
}

#[test]
fn layout_geometry_level_builds_graph_geometry_without_edge_paths() {
    let payload = positioned_fixture("layout-basic.json");
    let geom = hydrate_graph_geometry_from_mmds(&payload).expect("layout geometry should hydrate");

    assert_eq!(geom.nodes["A"].rect.width, 120.0);
    assert!(geom.edges[0].layout_path_hint.is_none());
    assert!(geom.edges[0].label_position.is_none());
    assert!(geom.subgraphs.contains_key("sg1"));
}

#[test]
fn hydration_populates_edge_subgraph_endpoint_intent_when_present() {
    let payload = mmds_fixture("subgraph-endpoint-intent-present.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    let into_subgraph = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Client" && edge.to == "API")
        .expect("client -> api edge should exist");
    assert_eq!(into_subgraph.to_subgraph.as_deref(), Some("sg1"));
    assert!(into_subgraph.from_subgraph.is_none());

    let from_subgraph = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "DB" && edge.to == "Logs")
        .expect("db -> logs edge should exist");
    assert_eq!(from_subgraph.from_subgraph.as_deref(), Some("sg1"));
    assert!(from_subgraph.to_subgraph.is_none());
}

#[test]
fn hydration_preserves_subgraph_endpoint_fallback_when_intent_is_omitted() {
    let payload = mmds_fixture("subgraph-endpoint-intent-missing.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    assert!(
        diagram
            .edges
            .iter()
            .all(|edge| edge.from_subgraph.is_none() && edge.to_subgraph.is_none())
    );
}

#[test]
fn endpoint_intent_absent_payload_uses_documented_fallback_behavior() {
    let payload = mmds_fixture("subgraph-endpoint-intent-missing.json");
    let diagram = from_mmds_str(&payload).expect("valid hydration");

    let into_backend = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "Client" && edge.to == "API")
        .expect("client -> api edge should exist");
    let from_backend = diagram
        .edges
        .iter()
        .find(|edge| edge.from == "DB" && edge.to == "Logs")
        .expect("db -> logs edge should exist");

    assert!(into_backend.to_subgraph.is_none());
    assert!(from_backend.from_subgraph.is_none());
}

#[test]
fn hydration_accepts_unknown_extension_namespace_profiles_fixture() {
    let payload = profile_fixture("unknown-extension.json");
    assert!(from_mmds_str(&payload).is_ok());
}

#[test]
fn hydration_rejects_unknown_core_version_even_with_known_profiles() {
    let payload = profile_fixture("unknown-core-version.json");
    let err = from_mmds_str(&payload).unwrap_err();
    assert!(matches!(err, MmdsHydrationError::UnsupportedVersion { .. }));
}

#[test]
fn profile_negotiation_reports_supported_and_unknown_profiles() {
    let payload = profile_fixture("mixed-known-unknown.json");
    let result = evaluate_mmds_profiles(&payload).unwrap();

    assert!(result.supported.contains(&"mmds-core-v1".to_string()));
    assert!(result.supported.contains(&"mmdflux-svg-v1".to_string()));
    assert!(result.supported.contains(&"mmdflux-text-v1".to_string()));
    assert!(
        result
            .unknown
            .contains(&"vendor.experimental-v9".to_string())
    );
}

#[test]
fn mmds_fixture_matrix_covers_valid_and_invalid_payloads() {
    let cases = [
        ("layout-valid-flowchart.json", true),
        ("layout-valid-class.json", true),
        ("subgraph-endpoint-intent-present.json", true),
        ("subgraph-endpoint-intent-missing.json", true),
        ("subgraph-endpoint-subgraph-to-subgraph-present.json", true),
        ("subgraph-endpoint-subgraph-to-subgraph-missing.json", true),
        ("profiles/unknown-extension.json", true),
        ("invalid/dangling-edge-target.json", false),
        ("invalid/dangling-endpoint-intent-subgraph.json", false),
        ("invalid/dangling-subgraph-parent.json", false),
        ("invalid/invalid-shape.json", false),
        ("invalid/unsupported-version.json", false),
        ("profiles/unknown-core-version.json", false),
    ];

    for (path, should_pass) in cases {
        let payload = mmds_fixture(path);
        assert_eq!(
            from_mmds_str(&payload).is_ok(),
            should_pass,
            "fixture {} expected pass={}",
            path,
            should_pass
        );
    }
}

#[test]
fn dangling_edge_error_message_matches_docs_example() {
    let payload = invalid_fixture("dangling-edge-target.json");
    let err = from_mmds_str(&payload).unwrap_err();
    assert_eq!(
        err.to_string(),
        "MMDS validation error: edge e0 target 'X' not found"
    );
}

fn mmds_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mmds")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn invalid_fixture(name: &str) -> String {
    mmds_fixture(&format!("invalid/{name}"))
}

fn positioned_fixture(name: &str) -> String {
    mmds_fixture(&format!("positioned/{name}"))
}

fn profile_fixture(name: &str) -> String {
    mmds_fixture(&format!("profiles/{name}"))
}
