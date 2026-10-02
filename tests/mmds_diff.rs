use mmdflux::mmds::Subject;
use mmdflux::mmds::diff::{ChangeKind, diff_documents};
use mmdflux::{RenderConfig, materialize_diagram};

fn materialize(source: &str) -> mmdflux::mmds::Document {
    materialize_diagram(source, &RenderConfig::default()).expect("diagram should materialize")
}

#[test]
fn public_mmds_diff_exposes_changes_and_geometry_levels() {
    let before = materialize(
        r#"
graph TD
    A[Alpha] --> B[Beta]
"#,
    );
    let after = materialize(
        r#"
graph TD
    A[Alpine] --> B[Beta]
"#,
    );

    let diff = diff_documents(&before, &after);

    assert_eq!(diff.before_geometry_level, before.geometry_level);
    assert_eq!(diff.after_geometry_level, after.geometry_level);
    assert!(diff.changes.iter().any(|event| {
        event.kind == ChangeKind::NodeLabelChanged
            && matches!(&event.subject, Subject::Node(id) if id == "A")
    }));
    assert!(!ChangeKind::NodeLabelChanged.is_geometry());
    assert!(ChangeKind::NodeLabelChanged.is_model());
}

#[test]
fn public_mmds_diff_docs_name_snapshot_diff_contract() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mmds/diff.rs"),
    )
    .expect("diff source should be readable");

    for required in [
        "snapshot diff",
        "diagnostic and not format-stable",
        "secondary semantic effects",
        "related_change_ids",
    ] {
        assert!(
            source.contains(required),
            "diff rustdoc should document: {required}"
        );
    }
}

fn materialize_routed(source: &str) -> mmdflux::mmds::Document {
    let config = RenderConfig {
        geometry_level: mmdflux::graph::GeometryLevel::Routed,
        ..RenderConfig::default()
    };
    materialize_diagram(source, &config).expect("diagram should materialize")
}

fn edge_ids(change: &mmdflux::mmds::diff::Change) -> (Option<&str>, Option<&str>) {
    let ids = change
        .edge_ids
        .as_ref()
        .expect("edge changes should carry before/after edge ids");
    (ids.before_id.as_deref(), ids.after_id.as_deref())
}

#[test]
fn edge_changes_identify_the_edge_in_both_documents() {
    let before = materialize("graph TD\nA-->B\nC-->D\n");
    let after = materialize("graph TD\nB-->C\nC-->D\n");
    let diff = diff_documents(&before, &after);

    let removed = diff
        .changes
        .iter()
        .find(|c| c.kind == ChangeKind::EdgeRemoved)
        .expect("A->B should be removed");
    let added = diff
        .changes
        .iter()
        .find(|c| c.kind == ChangeKind::EdgeAdded)
        .expect("B->C should be added");
    assert_eq!(edge_ids(removed), (Some("e0"), None));
    assert_eq!(edge_ids(added), (None, Some("e0")));

    for change in &diff.changes {
        assert_eq!(
            change.edge_ids.is_some(),
            matches!(change.subject, Subject::Edge(_)),
            "only edge changes carry edge ids: {change:?}"
        );
    }
}

#[test]
fn removed_edge_ids_name_the_before_edge_only() {
    let before = materialize(
        "flowchart LR\napi[API] --> auth[Auth]\napi --> billing[Billing]\nbilling --> ledger[(Ledger)]\nauth --> ledger\n",
    );
    let after = materialize("flowchart LR\napi[API] --> auth[Auth]\nauth --> ledger[(Ledger)]\n");
    let diff = diff_documents(&before, &after);

    let mut removed = diff
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::EdgeRemoved)
        .map(edge_ids)
        .collect::<Vec<_>>();
    removed.sort();
    assert_eq!(removed, vec![(Some("e1"), None), (Some("e2"), None)]);
}

#[test]
fn matched_edge_changes_carry_both_ids_when_they_differ() {
    let before = materialize("graph TD\nA-->B\nC-->D\n");
    let after = materialize("graph TD\nC-->|x|D\n");
    let diff = diff_documents(&before, &after);

    let relabel = diff
        .changes
        .iter()
        .find(|c| c.kind == ChangeKind::EdgeLabelChanged)
        .expect("C->D should be relabeled");
    assert_eq!(edge_ids(relabel), (Some("e1"), Some("e0")));
    assert!(matches!(&relabel.subject, Subject::Edge(id) if id == "e0"));
}

#[test]
fn related_geometry_links_only_changes_for_the_same_edge() {
    let before = materialize_routed(
        r#"graph TD
    client[Web Client] --> gw[API Gateway]
    gw --> auth[Auth Service]
    gw --> orders[Order Service]
    orders --> pay[Payment]
    pay --> stripe[Stripe Adapter]
    orders --> inv[Inventory]
    inv --> cache
    orders --> db
    auth --> db
    subgraph data[Data Layer]
        db[(Orders DB)]
        cache[(Redis Cache)]
    end
"#,
    );
    let after = materialize_routed(
        r#"graph TD
    client[Web Client] --> gw[Edge Gateway]
    gw --> auth[Auth Service]
    gw --> orders[Order Service]
    orders --> pay[Payment]
    orders --> inv[Inventory]
    inv --> db
    orders --> db
    auth --> db
    orders --> notify[Notification Service]
    subgraph data[Data Layer]
        db[(Orders DB)]
    end
    subgraph platform[Platform]
        cache[(Redis Cache)]
    end
    auth --> cache
"#,
    );
    let diff = diff_documents(&before, &after);

    assert!(
        diff.changes
            .iter()
            .any(|c| c.kind == ChangeKind::EdgeRemoved),
        "the pair should remove edges"
    );
    for change in &diff.changes {
        let Some(ids) = &change.edge_ids else {
            continue;
        };
        for &related in &change.related_change_ids {
            assert_eq!(
                diff.changes[related].edge_ids.as_ref(),
                Some(ids),
                "{:?} {ids:?} is linked to {:?}",
                change.kind,
                diff.changes[related]
            );
        }
        if change.kind == ChangeKind::EdgeRemoved {
            assert!(
                change.related_change_ids.is_empty(),
                "a removed edge has no after geometry: {change:?}"
            );
        }
    }
}

fn model_edge_changes(
    before: &str,
    after: &str,
) -> Vec<(ChangeKind, Option<String>, Option<String>)> {
    let diff = diff_documents(&materialize(before), &materialize(after));
    let mut changes = diff
        .changes
        .iter()
        .filter(|c| c.kind.is_model() && c.edge_ids.is_some())
        .map(|c| {
            let ids = c.edge_ids.as_ref().unwrap();
            (c.kind, ids.before_id.clone(), ids.after_id.clone())
        })
        .collect::<Vec<_>>();
    changes.sort_by_key(|(kind, b, a)| (format!("{kind:?}"), b.clone(), a.clone()));
    changes
}

fn ids(before: Option<&str>, after: Option<&str>) -> (Option<String>, Option<String>) {
    (before.map(str::to_string), after.map(str::to_string))
}

#[test]
fn reconnect_at_a_stable_index_is_reported() {
    let changes = model_edge_changes(
        "graph TD\nX-->A\nA-->B\nA-->C\nB-->C\n",
        "graph TD\nX-->A\nA-->B\nA-->C\nB-->X\n",
    );
    let (b, a) = ids(Some("e3"), Some("e3"));
    assert_eq!(changes, vec![(ChangeKind::EdgeReconnected, b, a)]);
}

#[test]
fn reconnect_survives_an_earlier_edge_deletion() {
    let changes = model_edge_changes(
        "graph TD\nX-->A\nA-->B\nA-->C\nB-->C\n",
        "graph TD\nX-->A\nA-->B\nB-->X\nC\n",
    );
    let (rb, ra) = ids(Some("e3"), Some("e2"));
    let (db, da) = ids(Some("e2"), None);
    assert_eq!(
        changes,
        vec![
            (ChangeKind::EdgeReconnected, rb, ra),
            (ChangeKind::EdgeRemoved, db, da),
        ]
    );

    let diff = diff_documents(
        &materialize("graph TD\nX-->A\nA-->B\nA-->C\nB-->C\n"),
        &materialize("graph TD\nX-->A\nA-->B\nB-->X\nC\n"),
    );
    let reconnect = diff
        .changes
        .iter()
        .find(|c| c.kind == ChangeKind::EdgeReconnected)
        .unwrap();
    assert!(
        reconnect
            .evidence
            .iter()
            .any(|e| e.contains("matched_by=endpoint_label")),
        "{reconnect:?}"
    );
}

#[test]
fn reconnect_between_edges_sharing_no_endpoint_is_suppressed() {
    let changes = model_edge_changes(
        "graph TD\nX-->A\nA-->B\nA-->C\n",
        "graph TD\nX-->A\nA-->B\nB-->X\nC\n",
    );
    let (rb, ra) = ids(Some("e2"), None);
    let (ab, aa) = ids(None, Some("e2"));
    assert_eq!(
        changes,
        vec![
            (ChangeKind::EdgeAdded, ab, aa),
            (ChangeKind::EdgeRemoved, rb, ra),
        ]
    );
}

#[test]
fn ambiguous_endpoint_reconnect_falls_back_to_remove_and_add() {
    // Both removed edges share source A with the added edge, so neither
    // pairing is unique.
    let changes = model_edge_changes(
        "graph TD\nA-->B\nA-->C\nB-->C\nX\n",
        "graph TD\nB-->C\nA-->X\n",
    );
    assert!(
        changes
            .iter()
            .all(|(kind, _, _)| *kind != ChangeKind::EdgeReconnected),
        "{changes:?}"
    );
}

/// Build a document whose nodes are `node_ids` and whose edges are `edges`,
/// cloning layout fields from a tiny materialized template so the diff sees
/// well-formed nodes and edges without running layout on a large graph.
fn synthetic_document(node_ids: &[String], edges: &[(String, String)]) -> mmdflux::mmds::Document {
    let mut doc = materialize("graph TD\nA-->B\n");
    let node_template = doc.nodes[0].clone();
    let edge_template = doc.edges[0].clone();
    doc.nodes = node_ids
        .iter()
        .map(|id| {
            let mut node = node_template.clone();
            node.id = id.clone();
            node.label = id.clone();
            node
        })
        .collect();
    doc.edges = edges
        .iter()
        .enumerate()
        .map(|(index, (source, target))| {
            let mut edge = edge_template.clone();
            edge.id = format!("e{index}");
            edge.source = source.clone();
            edge.target = target.clone();
            edge
        })
        .collect();
    doc
}

#[test]
fn many_ambiguous_leftover_edges_fall_back_quickly() {
    // Every removed edge shares source A with every added edge, so every
    // before/after pair is a reconnect candidate and none is unique.
    const N: usize = 200;
    let mut node_ids = vec!["A".to_string()];
    node_ids.extend((0..N).map(|i| format!("B{i}")));
    node_ids.extend((0..N).map(|i| format!("C{i}")));
    let before_edges = (0..N)
        .map(|i| ("A".to_string(), format!("B{i}")))
        .collect::<Vec<_>>();
    let after_edges = (0..N)
        .map(|i| ("A".to_string(), format!("C{i}")))
        .collect::<Vec<_>>();
    let before = synthetic_document(&node_ids, &before_edges);
    let after = synthetic_document(&node_ids, &after_edges);

    let started = std::time::Instant::now();
    let diff = diff_documents(&before, &after);
    let elapsed = started.elapsed();

    let count = |kind: ChangeKind| diff.changes.iter().filter(|c| c.kind == kind).count();
    assert_eq!(count(ChangeKind::EdgeReconnected), 0);
    assert_eq!(count(ChangeKind::EdgeRemoved), N);
    assert_eq!(count(ChangeKind::EdgeAdded), N);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "diff of {N} ambiguous leftover edges took {elapsed:?}"
    );
}

fn extension_changes(diff: &mmdflux::mmds::diff::Diff) -> Vec<Option<&str>> {
    diff.changes
        .iter()
        .filter(|c| c.kind == ChangeKind::ExtensionChanged)
        .map(|c| c.extension_namespace.as_deref())
        .collect()
}

#[test]
fn derived_render_projection_does_not_report_extension_changes() {
    for (before, after) in [
        (
            "graph TD\nA-->B\nB-->C\n",
            "graph TD\nA-->B\nB-->C\nA-->C\n",
        ),
        (
            "graph TD\nX-->A\nA-->B\nA-->C\nB-->C\n",
            "graph TD\nX-->A\nA-->B\nA-->C\nB-->X\n",
        ),
        ("graph TD\nA-->B\n", "graph TD\nA-->B\nB-->C\nC-->D\n"),
    ] {
        let diff = diff_documents(&materialize(before), &materialize(after));
        assert!(
            extension_changes(&diff).is_empty(),
            "{before:?} -> {after:?}: {:?}",
            diff.changes
        );
    }
}

#[test]
fn authored_extension_changes_name_their_namespace() {
    let before = materialize("graph TD\nA-->B\n");
    let after = materialize("graph TD\nA-->B\nstyle A fill:#f00\n");
    let diff = diff_documents(&before, &after);
    assert_eq!(
        extension_changes(&diff),
        vec![Some("org.mmdflux.node-style.v1")]
    );

    let mut custom = after.clone();
    custom
        .extensions
        .insert("example.b".to_string(), serde_json::Map::new());
    custom
        .extensions
        .insert("example.a".to_string(), serde_json::Map::new());
    let diff = diff_documents(&after, &custom);
    assert_eq!(
        extension_changes(&diff),
        vec![Some("example.a"), Some("example.b")]
    );
}
