use std::collections::{BTreeMap, BTreeSet};

use mmdflux::graph::{Arrow, Direction, GeometryLevel, Shape, Stroke};
use mmdflux::mmds::{
    Bounds, Defaults, Document, Edge, Metadata, Node, Position, Size, Subgraph,
    TEXT_EXTENSION_NAMESPACE,
};
use mmdflux::views::{
    AnchorRef, ElisionReason, LayoutMode, NodePredicate, Selector, TraversalDirection,
    VIEW_EXTENSION_NAMESPACE, ViewError, ViewEvent, ViewSpec, ViewStatement, project,
};
use mmdflux::{OutputFormat, RenderConfig, materialize_diagram, render_document};
use serde_json::{Map, Value, json};

const LEGACY_TEXT_EXTENSION_NAMESPACE: &str = "org.mmdflux.text.v1.projection";
const CANARY_SOURCE: &str = r#"graph TD
service_a[Service A] --> service_b[Service B]
external[External] --> service_a
service_b --> service_c[Service C]
service_c --> database[Database]
service_a --> audit[Audit]
"#;

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn output(nodes: Vec<Node>, edges: Vec<Edge>, subgraphs: Vec<Subgraph>) -> Document {
    Document {
        version: 1,
        profiles: Vec::new(),
        extensions: BTreeMap::new(),
        defaults: Defaults::default(),
        geometry_level: GeometryLevel::Layout,
        metadata: Metadata {
            diagram_type: "flowchart".to_string(),
            direction: Direction::TopDown,
            bounds: Bounds {
                width: 200.0,
                height: 200.0,
            },
            engine: None,
            diagnostics: None,
        },
        nodes,
        edges,
        subgraphs,
    }
}

fn node(id: &str, shape: Shape, parent: Option<&str>) -> Node {
    Node {
        id: id.to_string(),
        label: id.to_string(),
        shape,
        parent: parent.map(str::to_string),
        position: Position { x: 0.0, y: 0.0 },
        size: Size {
            width: 40.0,
            height: 20.0,
        },
    }
}

fn edge(id: &str, source: &str, target: &str) -> Edge {
    Edge {
        id: id.to_string(),
        source: source.to_string(),
        target: target.to_string(),
        from_subgraph: None,
        to_subgraph: None,
        label: None,
        stroke: Stroke::Solid,
        arrow_start: Arrow::None,
        arrow_end: Arrow::Normal,
        minlen: 1,
        path: None,
        label_position: None,
        is_backward: None,
        source_port: None,
        target_port: None,
        label_side: None,
        label_rect: None,
    }
}

fn edge_with_label(id: &str, source: &str, target: &str, label: &str) -> Edge {
    Edge {
        label: Some(label.to_string()),
        ..edge(id, source, target)
    }
}

fn subgraph(id: &str, children: &[&str], parent: Option<&str>) -> Subgraph {
    Subgraph {
        id: id.to_string(),
        title: id.to_string(),
        children: children.iter().map(|child| (*child).to_string()).collect(),
        parent: parent.map(str::to_string),
        direction: None,
        bounds: None,
        invisible: false,
        concurrent_regions: Vec::new(),
    }
}

fn view(statements: Vec<ViewStatement>) -> ViewSpec {
    ViewSpec::new(statements)
}

fn text_projection_extension() -> Map<String, Value> {
    let mut extension = Map::new();
    extension.insert(
        "projection".to_string(),
        json!({
            "node_ranks": {
                "gateway": 0,
                "auth": 2,
                "db": 4,
                "internal": 6
            },
            "edge_waypoints": {
                "0": [{"x": 12.0, "y": 20.0, "rank": 0}],
                "1": [{"x": 48.0, "y": 60.0, "rank": 2}]
            },
            "label_positions": {
                "0": {"x": 18.0, "y": 24.0, "rank": 0},
                "1": {"x": 54.0, "y": 66.0, "rank": 2}
            }
        }),
    );
    extension
}

fn projection<'a>(payload: &'a Document, namespace: &str) -> &'a Map<String, Value> {
    payload
        .extensions
        .get(namespace)
        .and_then(|extension| extension.get("projection"))
        .and_then(Value::as_object)
        .expect("projection extension should be present")
}

fn view_replay_payloads() -> (Document, Document, Document) {
    let mut payload = output(
        vec![
            Node {
                position: Position { x: 0.0, y: 0.0 },
                ..node("gateway", Shape::Rectangle, None)
            },
            Node {
                position: Position { x: 20.0, y: 10.0 },
                ..node("auth", Shape::Rectangle, None)
            },
        ],
        vec![edge("e0", "gateway", "auth")],
        vec![],
    );
    payload.extensions.insert(
        TEXT_EXTENSION_NAMESPACE.to_string(),
        text_projection_extension(),
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 1,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");
    let mut plain_payload = view_payload.clone();
    plain_payload.extensions.remove(VIEW_EXTENSION_NAMESPACE);
    let mut no_projection_payload = plain_payload.clone();
    no_projection_payload
        .extensions
        .remove(TEXT_EXTENSION_NAMESPACE);

    (view_payload, plain_payload, no_projection_payload)
}

fn canary_view_replay_payloads() -> (Document, Document) {
    let view_payload = canary_view_payload();
    let mut plain_payload = view_payload.clone();
    plain_payload.extensions.remove(VIEW_EXTENSION_NAMESPACE);
    (view_payload, plain_payload)
}

fn render_mmds_payload(payload: &Document, format: OutputFormat) -> String {
    render_document(payload, format, &RenderConfig::default()).expect("payload should render")
}

fn canary_canonical_output() -> Document {
    materialize_diagram(CANARY_SOURCE, &RenderConfig::default())
        .expect("canary source should materialize to MMDS")
}

fn canary_view_spec() -> ViewSpec {
    view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("service_a".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 2,
    })])
}

fn canary_view_payload() -> Document {
    let canonical = canary_canonical_output();
    project(&canonical, &canary_view_spec())
        .expect("canary view should materialize")
        .0
}

fn render_canary_view(format: OutputFormat) -> String {
    render_mmds_payload(&canary_view_payload(), format)
}

fn edge_ids(payload: &Document) -> Vec<&str> {
    payload.edges.iter().map(|edge| edge.id.as_str()).collect()
}

fn node_ids(payload: &Document) -> BTreeSet<String> {
    payload.nodes.iter().map(|node| node.id.clone()).collect()
}

fn subgraph_ids(payload: &Document) -> BTreeSet<String> {
    payload
        .subgraphs
        .iter()
        .map(|subgraph| subgraph.id.clone())
        .collect()
}

#[test]
fn view_selector_downstream_hops_include_anchor() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("db", Shape::Cylinder, None),
            node("monitor", Shape::Rectangle, None),
        ],
        vec![
            edge("e0", "gateway", "auth"),
            edge("e1", "auth", "db"),
            edge("e2", "monitor", "gateway"),
        ],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 2,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert_eq!(node_ids(&view_payload), ids(&["gateway", "auth", "db"]));
    assert!(view_payload.subgraphs.is_empty());
}

#[test]
fn view_selector_upstream_hops_follow_incoming_edges() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("client", Shape::Rectangle, None),
            node("monitor", Shape::Rectangle, None),
        ],
        vec![
            edge("e0", "client", "gateway"),
            edge("e1", "gateway", "auth"),
            edge("e2", "monitor", "gateway"),
        ],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Upstream,
        hops: 1,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert_eq!(
        node_ids(&view_payload),
        ids(&["client", "gateway", "monitor"])
    );
}

#[test]
fn view_selector_neighbors_combines_directions() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("client", Shape::Rectangle, None),
            node("monitor", Shape::Rectangle, None),
        ],
        vec![
            edge("e0", "client", "gateway"),
            edge("e1", "gateway", "auth"),
            edge("e2", "monitor", "gateway"),
        ],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Neighbors,
        hops: 1,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert_eq!(
        node_ids(&view_payload),
        ids(&["auth", "client", "gateway", "monitor"])
    );
}

#[test]
fn view_selector_subgraph_descendants_recurse() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, Some("root")),
            node("auth", Shape::Rectangle, Some("payments")),
            node("charge", Shape::Rectangle, Some("payments")),
            node("unrelated", Shape::Rectangle, None),
        ],
        vec![],
        vec![
            subgraph("root", &["gateway"], None),
            subgraph("payments", &["auth", "charge"], Some("root")),
        ],
    );
    let spec = view(vec![ViewStatement::Include(Selector::SubgraphDescendants(
        "root".to_string(),
    ))]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert_eq!(node_ids(&view_payload), ids(&["auth", "charge", "gateway"]));
    assert_eq!(subgraph_ids(&view_payload), ids(&["payments", "root"]));
}

#[test]
fn view_selector_subgraph_anchor_keeps_container_without_descendants() {
    let payload = output(
        vec![
            node("auth", Shape::Rectangle, Some("payments")),
            node("charge", Shape::Rectangle, Some("payments")),
        ],
        vec![],
        vec![subgraph("payments", &["auth", "charge"], None)],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Subgraph("payments".to_string()),
    ))]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert!(view_payload.nodes.is_empty());
    assert_eq!(view_payload.subgraphs.len(), 1);
    assert_eq!(view_payload.subgraphs[0].id, "payments");
    assert!(view_payload.subgraphs[0].children.is_empty());
}

#[test]
fn view_selector_parent_and_shape_predicates_filter_nodes() {
    let payload = output(
        vec![
            node("auth", Shape::Rectangle, Some("payments")),
            node("db", Shape::Cylinder, Some("payments")),
            node("cache", Shape::Cylinder, Some("internal")),
        ],
        vec![],
        vec![
            subgraph("payments", &["auth", "db"], None),
            subgraph("internal", &["cache"], None),
        ],
    );
    let spec = view(vec![
        ViewStatement::Include(Selector::Predicate(NodePredicate::Parent(
            "payments".to_string(),
        ))),
        ViewStatement::Exclude(Selector::Predicate(NodePredicate::Shape(Shape::Rectangle))),
    ]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert_eq!(node_ids(&view_payload), ids(&["db"]));
}

#[test]
fn view_shape_predicate_serializes_as_graph_shape_token() {
    let spec = view(vec![ViewStatement::Include(Selector::Predicate(
        NodePredicate::Shape(Shape::Cylinder),
    ))]);
    let json = serde_json::to_value(&spec).expect("view spec should serialize");

    assert_eq!(
        json["statements"][0]["Include"]["Predicate"]["Shape"],
        "cylinder"
    );

    let round_trip: ViewSpec = serde_json::from_value(json).expect("view spec should deserialize");
    assert_eq!(round_trip, spec);
}

#[test]
fn view_selector_unknown_anchor_returns_error() {
    let payload = output(
        vec![node("gateway", Shape::Rectangle, None)],
        vec![],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("missing".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 1,
    })]);

    let error = project(&payload, &spec).expect_err("missing anchor should fail");

    assert_eq!(
        error,
        ViewError::UnknownAnchor {
            id: "missing".to_string()
        }
    );
}

#[test]
fn view_apply_keeps_node_positions_and_canonical_bounds() {
    let payload = output(
        vec![
            Node {
                position: Position { x: 20.0, y: 30.0 },
                size: Size {
                    width: 44.0,
                    height: 22.0,
                },
                ..node("gateway", Shape::Rectangle, None)
            },
            node("internal", Shape::Rectangle, None),
        ],
        vec![],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Node("gateway".to_string()),
    ))]);

    let (view_payload, events) = project(&payload, &spec).expect("view should materialize");

    assert_eq!(
        view_payload.metadata.bounds.width,
        payload.metadata.bounds.width
    );
    assert_eq!(
        view_payload.metadata.bounds.height,
        payload.metadata.bounds.height
    );
    assert_eq!(view_payload.nodes.len(), 1);
    assert_eq!(view_payload.nodes[0].id, "gateway");
    assert_eq!(view_payload.nodes[0].position.x, 20.0);
    assert_eq!(view_payload.nodes[0].position.y, 30.0);
    assert_eq!(view_payload.nodes[0].size.width, 44.0);
    assert_eq!(view_payload.nodes[0].size.height, 22.0);
    assert!(events.iter().any(|event| matches!(
        event,
        ViewEvent::NodeLeftView { id, .. } if id == "internal"
    )));
}

#[test]
fn view_apply_preserves_sparse_surviving_edge_ids() {
    let payload = output(
        vec![
            node("external", Shape::Rectangle, None),
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("db", Shape::Cylinder, None),
        ],
        vec![
            edge("e0", "external", "gateway"),
            edge("e1", "gateway", "auth"),
            edge("e2", "auth", "db"),
        ],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 2,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    let edge_ids: Vec<&str> = view_payload
        .edges
        .iter()
        .map(|edge| edge.id.as_str())
        .collect();
    assert_eq!(edge_ids, vec!["e1", "e2"]);
}

#[test]
fn view_apply_drops_edges_with_elided_endpoints() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("db", Shape::Cylinder, None),
        ],
        vec![edge("e0", "gateway", "auth"), edge("e1", "auth", "db")],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Node("gateway".to_string()),
    ))]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert!(view_payload.edges.is_empty());
}

#[test]
fn view_apply_emits_node_and_edge_elision_events() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("db", Shape::Cylinder, None),
        ],
        vec![
            edge_with_label("e0", "gateway", "auth", "first"),
            edge_with_label("e1", "gateway", "auth", "second"),
            edge("e2", "auth", "db"),
        ],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Node("gateway".to_string()),
    ))]);

    let (_, events) = project(&payload, &spec).expect("view should materialize");

    assert!(events.contains(&ViewEvent::NodeLeftView {
        id: "auth".to_string(),
        reason: ElisionReason::Excluded,
    }));
    assert!(events.contains(&ViewEvent::NodeLeftView {
        id: "db".to_string(),
        reason: ElisionReason::Excluded,
    }));
    assert!(events.contains(&ViewEvent::EdgeElided {
        source: "gateway".to_string(),
        target: "auth".to_string(),
        ordinal: 0,
        label: Some("first".to_string()),
        reason: ElisionReason::EndpointOutsideView,
    }));
    assert!(events.contains(&ViewEvent::EdgeElided {
        source: "gateway".to_string(),
        target: "auth".to_string(),
        ordinal: 1,
        label: Some("second".to_string()),
        reason: ElisionReason::EndpointOutsideView,
    }));
}

#[test]
fn view_apply_preserves_ancestor_subgraph_containers() {
    let payload = output(
        vec![
            node("gateway", Shape::Rectangle, Some("root")),
            node("auth", Shape::Rectangle, Some("payments")),
            node("charge", Shape::Rectangle, Some("payments")),
        ],
        vec![],
        vec![
            subgraph("root", &["gateway"], None),
            subgraph("payments", &["auth", "charge"], Some("root")),
        ],
    );
    let spec = view(vec![
        ViewStatement::Include(Selector::Predicate(NodePredicate::Parent(
            "payments".to_string(),
        ))),
        ViewStatement::Exclude(Selector::Anchor(AnchorRef::Node("charge".to_string()))),
    ]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    let node_ids: Vec<&str> = view_payload
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    let subgraph_ids: Vec<&str> = view_payload
        .subgraphs
        .iter()
        .map(|subgraph| subgraph.id.as_str())
        .collect();

    assert_eq!(node_ids, vec!["auth"]);
    assert_eq!(subgraph_ids, vec!["root", "payments"]);
    assert_eq!(view_payload.subgraphs[0].children, Vec::<String>::new());
    assert_eq!(view_payload.subgraphs[1].children, vec!["auth".to_string()]);
}

#[test]
fn view_apply_rejects_unimplemented_layout_modes() {
    let payload = output(
        vec![node("gateway", Shape::Rectangle, None)],
        vec![],
        vec![],
    );
    let mut spec = ViewSpec::new(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Node("gateway".to_string()),
    ))]);
    spec.layout = LayoutMode::Compact;

    let error = project(&payload, &spec).expect_err("compact mode is deferred");

    assert_eq!(
        error,
        ViewError::NotImplementedYet {
            feature: "non-shared-coordinate layout modes".to_string()
        }
    );
}

#[test]
fn view_apply_marks_shared_coordinates_view_payload() {
    let payload = output(
        vec![node("gateway", Shape::Rectangle, None)],
        vec![],
        vec![],
    );
    let spec = view(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Node("gateway".to_string()),
    ))]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    let marker = view_payload
        .extensions
        .get(VIEW_EXTENSION_NAMESPACE)
        .expect("view payload should carry a view extension marker");
    assert_eq!(
        marker.get("layout_mode").and_then(Value::as_str),
        Some("shared_coordinates")
    );
    assert_eq!(
        marker.get("boundary_policy").and_then(Value::as_str),
        Some("omit")
    );
}

#[test]
fn view_apply_preserves_text_projection_under_render_namespace() {
    let mut payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
        ],
        vec![edge("e0", "gateway", "auth")],
        vec![],
    );
    payload.extensions.insert(
        TEXT_EXTENSION_NAMESPACE.to_string(),
        text_projection_extension(),
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 1,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert!(
        view_payload
            .extensions
            .contains_key(TEXT_EXTENSION_NAMESPACE)
    );
    let projection = projection(&view_payload, TEXT_EXTENSION_NAMESPACE);
    assert!(projection.contains_key("node_ranks"));
}

#[test]
fn view_apply_prunes_node_ranks_to_retained_nodes() {
    let mut payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("db", Shape::Cylinder, None),
            node("internal", Shape::Rectangle, None),
        ],
        vec![edge("e0", "gateway", "auth"), edge("e1", "auth", "db")],
        vec![],
    );
    payload.extensions.insert(
        TEXT_EXTENSION_NAMESPACE.to_string(),
        text_projection_extension(),
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 1,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    let node_ranks = projection(&view_payload, TEXT_EXTENSION_NAMESPACE)
        .get("node_ranks")
        .and_then(Value::as_object)
        .expect("node ranks should remain present");
    let retained: BTreeSet<&str> = node_ranks.keys().map(String::as_str).collect();
    assert_eq!(retained, BTreeSet::from(["auth", "gateway"]));
    assert_eq!(node_ranks.get("gateway"), Some(&json!(0)));
    assert_eq!(node_ranks.get("auth"), Some(&json!(2)));
}

#[test]
fn view_apply_does_not_emit_legacy_text_namespace() {
    let mut payload = output(
        vec![node("gateway", Shape::Rectangle, None)],
        vec![],
        vec![],
    );
    payload.extensions.insert(
        TEXT_EXTENSION_NAMESPACE.to_string(),
        text_projection_extension(),
    );
    let spec = view(vec![ViewStatement::Include(Selector::Anchor(
        AnchorRef::Node("gateway".to_string()),
    ))]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    assert!(
        !view_payload
            .extensions
            .contains_key(LEGACY_TEXT_EXTENSION_NAMESPACE)
    );
}

#[test]
fn view_apply_drops_edge_indexed_text_projection_keys() {
    let mut payload = output(
        vec![
            node("gateway", Shape::Rectangle, None),
            node("auth", Shape::Rectangle, None),
            node("db", Shape::Cylinder, None),
        ],
        vec![edge("e0", "gateway", "auth"), edge("e1", "auth", "db")],
        vec![],
    );
    payload.extensions.insert(
        TEXT_EXTENSION_NAMESPACE.to_string(),
        text_projection_extension(),
    );
    let spec = view(vec![ViewStatement::Include(Selector::Traversal {
        anchor: AnchorRef::Node("gateway".to_string()),
        direction: TraversalDirection::Downstream,
        hops: 1,
    })]);

    let (view_payload, _) = project(&payload, &spec).expect("view should materialize");

    let projection = projection(&view_payload, TEXT_EXTENSION_NAMESPACE);
    assert!(!projection.contains_key("edge_waypoints"));
    assert!(!projection.contains_key("label_positions"));
}

#[test]
fn view_mmds_replay_enables_pinned_text_ranks_for_marked_payload() {
    let (view_payload, plain_payload, _) = view_replay_payloads();

    let marked_text = render_mmds_payload(&view_payload, OutputFormat::Text);
    let plain_text = render_mmds_payload(&plain_payload, OutputFormat::Text);

    assert!(
        marked_text.lines().count() > plain_text.lines().count(),
        "marked view replay should preserve sparse pinned ranks\nmarked:\n{marked_text}\nplain:\n{plain_text}"
    );
}

#[test]
fn plain_mmds_replay_keeps_pinned_text_ranks_disabled() {
    let (_, plain_payload, no_projection_payload) = view_replay_payloads();

    let plain_text = render_mmds_payload(&plain_payload, OutputFormat::Text);
    let no_projection_text = render_mmds_payload(&no_projection_payload, OutputFormat::Text);

    assert_eq!(plain_text, no_projection_text);
}

#[test]
fn view_mmds_replay_leaves_svg_and_mmds_paths_schema_clean() {
    let (view_payload, _, _) = view_replay_payloads();

    let svg = render_mmds_payload(&view_payload, OutputFormat::Svg);
    let mmds = render_mmds_payload(&view_payload, OutputFormat::Mmds);
    let replayed: Document =
        serde_json::from_str(&mmds).expect("MMDS replay should stay valid JSON");

    assert!(svg.starts_with("<svg"));
    assert!(!svg.contains(VIEW_EXTENSION_NAMESPACE));
    assert!(replayed.extensions.contains_key(VIEW_EXTENSION_NAMESPACE));
    assert!(replayed.extensions.contains_key(TEXT_EXTENSION_NAMESPACE));
}

#[test]
fn view_canary_mmds_output_preserves_sparse_edge_ids() {
    let replayed: Document = serde_json::from_str(&render_canary_view(OutputFormat::Mmds))
        .expect("view replay should produce valid MMDS");

    assert_eq!(edge_ids(&replayed), vec!["e0", "e2", "e4"]);
}

#[test]
fn view_canary_sparse_edge_ids_render_across_all_surfaces() {
    let mmds = render_canary_view(OutputFormat::Mmds);
    let svg = render_canary_view(OutputFormat::Svg);
    let text = render_canary_view(OutputFormat::Text);
    let replayed: Document = serde_json::from_str(&mmds).expect("view MMDS should parse");

    assert_eq!(edge_ids(&replayed), vec!["e0", "e2", "e4"]);
    assert!(svg.starts_with("<svg"));
    assert!(text.contains("Service A"));
}

#[test]
fn view_canary_svg_renders_filtered_payload() {
    let svg = render_canary_view(OutputFormat::Svg);

    assert!(svg.contains("Service A"));
    assert!(svg.contains("Service B"));
    assert!(svg.contains("Service C"));
    assert!(svg.contains("Audit"));
    assert!(!svg.contains("External"));
    assert!(!svg.contains("Database"));
}

#[test]
fn view_replay_text_renders_with_pinned_shared_coordinates() {
    let (view_payload, plain_payload, _) = view_replay_payloads();

    let marked_text = render_mmds_payload(&view_payload, OutputFormat::Text);
    let plain_text = render_mmds_payload(&plain_payload, OutputFormat::Text);

    assert!(marked_text.contains("gateway"));
    assert!(marked_text.contains("auth"));
    assert!(marked_text.lines().count() > plain_text.lines().count());
}

#[test]
fn view_canary_text_carries_producer_generated_pinned_ranks() {
    let (view_payload, plain_payload) = canary_view_replay_payloads();

    let node_ids: BTreeSet<&str> = view_payload
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    let node_ranks = projection(&view_payload, TEXT_EXTENSION_NAMESPACE)
        .get("node_ranks")
        .and_then(Value::as_object)
        .expect("producer-generated node ranks should be retained");
    let rank_ids: BTreeSet<&str> = node_ranks.keys().map(String::as_str).collect();

    assert!(
        view_payload
            .extensions
            .contains_key(VIEW_EXTENSION_NAMESPACE)
    );
    assert!(
        !plain_payload
            .extensions
            .contains_key(VIEW_EXTENSION_NAMESPACE)
    );
    assert_eq!(edge_ids(&view_payload), vec!["e0", "e2", "e4"]);
    assert_eq!(
        node_ids,
        BTreeSet::from(["audit", "service_a", "service_b", "service_c"])
    );
    assert_eq!(rank_ids, node_ids);
    assert!(!node_ranks.contains_key("external"));
    assert!(!node_ranks.contains_key("database"));
    assert!(node_ranks.values().all(Value::is_number));

    let marked_text = render_mmds_payload(&view_payload, OutputFormat::Text);
    let plain_text = render_mmds_payload(&plain_payload, OutputFormat::Text);

    assert!(marked_text.contains("Service A"));
    assert!(marked_text.contains("Service B"));
    assert!(marked_text.contains("Service C"));
    assert!(marked_text.contains("Audit"));
    assert!(!marked_text.contains("External"));
    assert!(!marked_text.contains("Database"));
    assert!(plain_text.contains("Service A"));
}

#[test]
fn view_canary_deferred_primitives_return_not_implemented() {
    let payload = canary_canonical_output();
    let cases = [
        (
            {
                let mut spec = canary_view_spec();
                spec.boundary = mmdflux::views::BoundaryPolicy::Stub {
                    aggregate_threshold: 3,
                };
                spec
            },
            "boundary stubs",
        ),
        (
            view(vec![ViewStatement::Include(Selector::Predicate(
                NodePredicate::Tag("critical".to_string()),
            ))]),
            "tag predicates",
        ),
        (
            {
                let mut spec = canary_view_spec();
                spec.layout = LayoutMode::Compact;
                spec
            },
            "non-shared-coordinate layout modes",
        ),
        (
            {
                let mut spec = canary_view_spec();
                spec.compound = mmdflux::views::CompoundPolicy::Flatten;
                spec
            },
            "compound flattening",
        ),
    ];

    for (spec, feature) in cases {
        let error = project(&payload, &spec).expect_err("deferred feature should fail");
        assert_eq!(
            error,
            ViewError::NotImplementedYet {
                feature: feature.to_string()
            }
        );
    }
}
