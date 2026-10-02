use mmdflux::graph::{GeometryLevel, Stroke};
use mmdflux::mmds::diff::union::{
    ADDED_COLOR, CHANGED_COLOR, EXTENSION_NAMESPACE, REMOVED_COLOR, UnionOptions, union_document,
};
use mmdflux::mmds::{Document, Edge, NODE_STYLE_EXTENSION_NAMESPACE};
use mmdflux::{OutputFormat, RenderConfig, materialize_diagram, render_document_with_relayout};
use serde_json::{Value, json};

fn materialize(source: &str) -> Document {
    materialize_diagram(source, &RenderConfig::default()).expect("diagram should materialize")
}

fn materialize_routed(source: &str) -> Document {
    let config = RenderConfig {
        geometry_level: GeometryLevel::Routed,
        ..RenderConfig::default()
    };
    materialize_diagram(source, &config).expect("diagram should materialize")
}

fn union_of(before: &str, after: &str) -> Document {
    union_document(
        &materialize(before),
        &materialize(after),
        &UnionOptions::default(),
    )
}

fn tags(union: &Document) -> Value {
    Value::Object(union.extensions[EXTENSION_NAMESPACE].clone())
}

fn styles(union: &Document) -> Value {
    Value::Object(union.extensions[NODE_STYLE_EXTENSION_NAMESPACE].clone())
}

fn node_ids(union: &Document) -> Vec<&str> {
    union.nodes.iter().map(|node| node.id.as_str()).collect()
}

fn edge<'a>(union: &'a Document, id: &str) -> &'a Edge {
    union
        .edges
        .iter()
        .find(|edge| edge.id == id)
        .unwrap_or_else(|| panic!("union should have edge {id}"))
}

fn label_of<'a>(union: &'a Document, id: &str) -> &'a str {
    &union.nodes.iter().find(|node| node.id == id).unwrap().label
}

const BEFORE: &str = "graph TD\nA-->B\nB-->C[Old]\nX[Gone]-->A\n";
const AFTER: &str = "graph TD\nA-->B\nB-->C[New]\nB-->D\n";

#[test]
fn union_holds_after_items_plus_removed_ghosts_with_tags() {
    let union = union_of(BEFORE, AFTER);

    assert_eq!(node_ids(&union), ["A", "B", "C", "D", "X"]);
    assert_eq!(label_of(&union, "X"), "Gone");
    assert_eq!(
        tags(&union)["nodes"],
        json!({"C": "changed", "D": "added", "X": "removed"})
    );
    assert_eq!(union.geometry_level, GeometryLevel::Layout);
}

#[test]
fn union_edges_get_collision_free_ids_with_correspondence() {
    let union = union_of(BEFORE, AFTER);

    let ids: Vec<&str> = union.edges.iter().map(|edge| edge.id.as_str()).collect();
    assert_eq!(ids, ["e0", "e1", "e2", "e3"]);
    assert_eq!(
        tags(&union)["edges"],
        json!({
            "e0": {"before_id": "e0", "after_id": "e0"},
            "e1": {"before_id": "e1", "after_id": "e1"},
            "e2": {"status": "added", "before_id": null, "after_id": "e2"},
            "e3": {"status": "removed", "before_id": "e2", "after_id": null},
        })
    );
    let ghost = edge(&union, "e3");
    assert_eq!((ghost.source.as_str(), ghost.target.as_str()), ("X", "A"));
    assert_eq!(ghost.stroke, Stroke::Dotted);
}

#[test]
fn union_keeps_removed_and_added_edges_that_share_a_positional_id_apart() {
    let union = union_of("graph TD\nA-->B\n", "graph TD\nC-->D\n");

    assert_eq!(
        tags(&union)["edges"],
        json!({
            "e0": {"status": "added", "before_id": null, "after_id": "e0"},
            "e1": {"status": "removed", "before_id": "e0", "after_id": null},
        })
    );
    assert_eq!(edge(&union, "e0").source, "C");
    assert_eq!(edge(&union, "e1").source, "A");
}

#[test]
fn union_tags_changed_edges() {
    let union = union_of("graph TD\nA-->|go|B\n", "graph TD\nA-->|stop|B\n");

    assert_eq!(
        tags(&union)["edges"]["e0"],
        json!({"status": "changed", "before_id": "e0", "after_id": "e0"})
    );
}

#[test]
fn removed_items_keep_parents_that_are_in_the_union() {
    let before = "graph TD\nsubgraph S[Kept]\nA\nG[Ghost]\nend\nsubgraph R[Dropped]\nH[Old]\nend\n";
    let after = "graph TD\nsubgraph S[Kept]\nA\nend\n";
    let union = union_of(before, after);

    let parent_of = |id: &str| {
        union
            .nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap()
            .parent
            .clone()
    };
    assert_eq!(parent_of("G").as_deref(), Some("S"));
    // The removed subgraph comes back as a ghost, so its member keeps it.
    assert_eq!(parent_of("H").as_deref(), Some("R"));

    let subgraph = |id: &str| union.subgraphs.iter().find(|sg| sg.id == id).unwrap();
    assert_eq!(subgraph("S").children, ["A", "G"]);
    assert_eq!(subgraph("R").children, ["H"]);
    assert_eq!(subgraph("R").title, "Dropped");
    assert_eq!(
        tags(&union)["subgraphs"],
        json!({"R": "removed", "S": "changed"})
    );
}

#[test]
fn removed_node_moves_to_top_level_when_its_parent_is_gone() {
    let before = "graph TD\nsubgraph S\nG[Ghost]\nend\nA\n";
    let after = "graph TD\nA\n";
    let mut before_doc = materialize(before);
    // Drop S from `before` while keeping G's parent link, as a hand-edited
    // document might.
    before_doc.subgraphs.clear();
    let union = union_document(&before_doc, &materialize(after), &UnionOptions::default());

    let ghost = union.nodes.iter().find(|node| node.id == "G").unwrap();
    assert_eq!(ghost.parent, None);
}

#[test]
fn union_styles_tagged_items_without_fills() {
    let union = union_of(BEFORE, AFTER);
    let styles = styles(&union);

    assert_eq!(
        styles["nodes"]["D"],
        json!({"stroke": ADDED_COLOR, "color": ADDED_COLOR})
    );
    assert_eq!(
        styles["nodes"]["C"],
        json!({"stroke": CHANGED_COLOR, "color": CHANGED_COLOR})
    );
    assert_eq!(styles["nodes"]["X"]["stroke"], REMOVED_COLOR);
    assert_eq!(styles["nodes"]["X"]["stroke-dasharray"], "5 5");
    assert_eq!(styles["edges"]["e2"], json!({"stroke": ADDED_COLOR}));
    assert_eq!(styles["edges"]["e3"], json!({"stroke": REMOVED_COLOR}));
    assert!(styles["edges"].get("e0").is_none());
    assert!(
        union
            .profiles
            .iter()
            .any(|profile| profile == "mmdflux-node-style-v1")
    );
}

#[test]
fn union_without_style_only_tags() {
    let union = union_document(
        &materialize(BEFORE),
        &materialize(AFTER),
        &UnionOptions::default().with_style(false),
    );

    assert!(
        !union
            .extensions
            .contains_key(NODE_STYLE_EXTENSION_NAMESPACE)
    );
    assert_eq!(tags(&union)["nodes"]["D"], "added");
}

#[test]
fn union_markers_prefix_tagged_labels() {
    let union = union_document(
        &materialize("graph TD\nA-->|go|B\nsubgraph S[Box]\nX[Gone]\nend\n"),
        &materialize("graph TD\nA-->|stop|B\nN[New]\n"),
        &UnionOptions::default().with_markers(true),
    );

    assert_eq!(label_of(&union, "N"), "+ New");
    assert_eq!(label_of(&union, "X"), "- Gone");
    assert_eq!(label_of(&union, "A"), "A");
    assert_eq!(edge(&union, "e0").label.as_deref(), Some("~ stop"));
    assert_eq!(union.subgraphs[0].title, "- Box");
}

#[test]
fn union_of_routed_documents_drops_each_sides_routes() {
    let union = union_document(
        &materialize_routed(BEFORE),
        &materialize_routed(AFTER),
        &UnionOptions::default(),
    );

    assert_eq!(union.geometry_level, GeometryLevel::Layout);
    assert!(union.edges.iter().all(|edge| edge.path.is_none()));
}

#[test]
fn union_renders_after_relayout_with_ghosts_and_colors() {
    let union = union_of(BEFORE, AFTER);

    let svg = render_document_with_relayout(&union, OutputFormat::Svg, &RenderConfig::default())
        .expect("union should render as SVG");
    assert!(svg.contains(ADDED_COLOR), "added color missing:\n{svg}");
    assert!(svg.contains(REMOVED_COLOR), "removed color missing:\n{svg}");
    assert!(svg.contains("Gone"));

    let text = render_document_with_relayout(&union, OutputFormat::Text, &RenderConfig::default())
        .expect("union should render as text");
    for label in ["Gone", "New", "D"] {
        assert!(text.contains(label), "{label} missing:\n{text}");
    }

    let mmds = render_document_with_relayout(&union, OutputFormat::Mmds, &RenderConfig::default())
        .expect("union should relayout as MMDS");
    let relaid: Value = serde_json::from_str(&mmds).unwrap();
    assert_eq!(relaid["nodes"].as_array().unwrap().len(), 5);
    assert_eq!(
        relaid["extensions"][NODE_STYLE_EXTENSION_NAMESPACE]["edges"]["e3"]["stroke"],
        REMOVED_COLOR
    );
}

fn subgraph<'a>(union: &'a Document, id: &str) -> &'a mmdflux::mmds::Subgraph {
    union
        .subgraphs
        .iter()
        .find(|sg| sg.id == id)
        .unwrap_or_else(|| panic!("union should have subgraph {id}"))
}

fn render_all_formats(union: &Document) -> String {
    let config = RenderConfig::default();
    for format in [OutputFormat::Svg, OutputFormat::Mmds] {
        render_document_with_relayout(union, format, &config)
            .unwrap_or_else(|error| panic!("union should render as {format:?}: {error}"));
    }
    render_document_with_relayout(union, OutputFormat::Text, &config)
        .unwrap_or_else(|error| panic!("union should render as text: {error}"))
}

const NESTED: &str = "graph TD\nsubgraph Outer\nsubgraph Inner\nA\nend\nend\n";

#[test]
fn union_of_unchanged_nested_subgraphs_lists_only_nodes_as_children() {
    let union = union_of(NESTED, NESTED);

    assert!(subgraph(&union, "Outer").children.is_empty());
    assert_eq!(subgraph(&union, "Inner").children, ["A"]);
    assert_eq!(subgraph(&union, "Inner").parent.as_deref(), Some("Outer"));
    let text = render_all_formats(&union);
    for title in ["Outer", "Inner"] {
        assert!(text.contains(title), "{title} missing:\n{text}");
    }
}

#[test]
fn union_keeps_removed_nested_subgraph_ghosts_inside_their_parent() {
    let before = "graph TD\nsubgraph Outer\nsubgraph Inner[Gone]\nA\nend\nB\nend\n";
    let after = "graph TD\nsubgraph Outer\nB\nend\n";
    let union = union_of(before, after);

    assert_eq!(subgraph(&union, "Outer").children, ["B"]);
    assert_eq!(subgraph(&union, "Inner").parent.as_deref(), Some("Outer"));
    assert_eq!(subgraph(&union, "Inner").children, ["A"]);
    assert_eq!(tags(&union)["subgraphs"]["Inner"], "removed");
    let text = render_all_formats(&union);
    assert!(text.contains("Gone"), "removed subgraph missing:\n{text}");
}

#[test]
fn union_carries_removed_concurrent_regions_into_their_surviving_parent() {
    let before = "stateDiagram-v2\nstate P {\n[*] --> A\n--\n[*] --> B\n}\n";
    let after = "stateDiagram-v2\nstate P {\n[*] --> A\n}\n";
    let before_doc = materialize(before);
    let after_doc = materialize(after);
    let before_regions = subgraph(&before_doc, "P").concurrent_regions.clone();
    assert_eq!(before_regions.len(), 2, "fixture should have two regions");

    let union = union_document(&before_doc, &after_doc, &UnionOptions::default());

    let union_subgraph_ids: Vec<&str> = union.subgraphs.iter().map(|sg| sg.id.as_str()).collect();
    for region in &subgraph(&union, "P").concurrent_regions {
        assert!(union_subgraph_ids.contains(&region.as_str()));
        assert_eq!(subgraph(&union, region).parent.as_deref(), Some("P"));
    }
    for region in &before_regions {
        assert!(
            subgraph(&union, "P").concurrent_regions.contains(region),
            "region {region} should stay a concurrent region of P"
        );
    }
}

fn with_edge_id(mut document: Document, id: &str) -> Document {
    assert_eq!(document.edges.len(), 1);
    document.edges[0].id = id.to_string();
    document
}

#[test]
fn union_renumbers_sparse_imported_edge_ids_densely() {
    let before = with_edge_id(materialize("graph TD\nA-->|old|B\n"), "e7");
    let after = with_edge_id(materialize("graph TD\nA-->|new|B\n"), "e7");

    let union = union_document(&before, &after, &UnionOptions::default());

    let ids: Vec<&str> = union.edges.iter().map(|edge| edge.id.as_str()).collect();
    assert_eq!(ids, ["e0"]);
    assert_eq!(
        tags(&union)["edges"],
        json!({"e0": {"status": "changed", "before_id": "e7", "after_id": "e7"}})
    );
    assert_eq!(styles(&union)["edges"]["e0"]["stroke"], CHANGED_COLOR);
    assert!(styles(&union)["edges"].get("e7").is_none());
}

#[test]
fn union_keeps_removed_ghost_apart_from_imported_after_edge_with_its_id() {
    let before = materialize("graph TD\nX-->A\n");
    let after = with_edge_id(materialize("graph TD\nA-->B\n"), "e1");

    let union = union_document(&before, &after, &UnionOptions::default());

    assert_eq!(
        tags(&union)["edges"],
        json!({
            "e0": {"status": "added", "before_id": null, "after_id": "e1"},
            "e1": {"status": "removed", "before_id": "e0", "after_id": null},
        })
    );
    assert_eq!(edge(&union, "e0").source, "A");
    assert_eq!(edge(&union, "e1").source, "X");
    assert_eq!(styles(&union)["edges"]["e0"]["stroke"], ADDED_COLOR);
    assert_eq!(styles(&union)["edges"]["e1"]["stroke"], REMOVED_COLOR);
}

#[test]
fn union_rekeys_after_edge_styles_to_union_ids() {
    let mut after = with_edge_id(
        materialize("graph TD\nA-->B\nlinkStyle 0 stroke:#ff0000\n"),
        "e4",
    );
    let styles_section = after
        .extensions
        .get_mut(NODE_STYLE_EXTENSION_NAMESPACE)
        .and_then(|extension| extension.get_mut("edges"))
        .and_then(Value::as_object_mut)
        .expect("after should carry an edge style");
    let style = styles_section.remove("e0").expect("style keyed by e0");
    styles_section.insert("e4".to_string(), style);
    let before = after.clone();

    let union = union_document(&before, &after, &UnionOptions::default());

    assert_eq!(styles(&union)["edges"]["e0"]["stroke"], "#ff0000");
    assert!(styles(&union)["edges"].get("e4").is_none());
}
