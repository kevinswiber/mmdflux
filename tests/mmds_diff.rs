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
