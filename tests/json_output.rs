use mmdflux::graph::{Edge, Node, Shape};

#[test]
fn test_node_serializes_to_json() {
    let node = Node::new("A").with_label("Start");
    let json = serde_json::to_string(&node).unwrap();
    assert!(json.contains("\"id\":\"A\""));
    assert!(json.contains("\"label\":\"Start\""));
}

#[test]
fn test_shape_serializes_as_snake_case() {
    let json = serde_json::to_string(&Shape::Rectangle).unwrap();
    assert_eq!(json, "\"rectangle\"");

    let json = serde_json::to_string(&Shape::DoubleCircle).unwrap();
    assert_eq!(json, "\"double_circle\"");
}

#[test]
fn test_edge_serializes_to_json() {
    let edge = Edge::new("A", "B").with_label("yes");
    let json = serde_json::to_string(&edge).unwrap();
    assert!(json.contains("\"from\":\"A\""));
    assert!(json.contains("\"to\":\"B\""));
    assert!(json.contains("\"yes\""));
}
