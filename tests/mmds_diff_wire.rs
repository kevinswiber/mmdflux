use std::path::Path;

use mmdflux::graph::GeometryLevel;
use mmdflux::mmds::Document;
use mmdflux::mmds::diff::diff_documents;
use mmdflux::mmds::diff::wire::{SCHEMA, WireLayer, WireOptions, to_json};
use mmdflux::{RenderConfig, materialize_diagram};
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

fn wire_of(before: &Document, after: &Document, options: WireOptions) -> Value {
    to_json(&diff_documents(before, after), before, after, &options)
}

fn changes_of_kind<'a>(wire: &'a Value, kind: &str) -> Vec<&'a Value> {
    wire["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|change| change["kind"] == kind)
        .collect()
}

fn assert_schema_valid(payload: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("mmds-diff.schema.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read schema {}: {err}", path.display()));
    let schema: Value = serde_json::from_str(&raw).expect("schema should be JSON");
    let validator = jsonschema::validator_for(&schema).expect("schema should compile");
    let errors: Vec<String> = validator
        .iter_errors(payload)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema errors: {errors:?}\n{payload:#}");
}

#[test]
fn wire_document_names_its_schema_and_geometry_levels() {
    let before = materialize("graph TD\nA-->B\n");
    let after = materialize_routed("graph TD\nA-->B\n");
    let wire = wire_of(&before, &after, WireOptions::default());

    assert_eq!(SCHEMA, "mmdflux.diff.v1");
    assert_eq!(wire["schema"], SCHEMA);
    assert_eq!(wire["before_geometry_level"], "layout");
    assert_eq!(wire["after_geometry_level"], "routed");
    assert_schema_valid(&wire);
}

#[test]
fn wire_edge_subjects_carry_both_ids_and_inline_endpoints() {
    let before = materialize("graph TD\nA-->B\nC-->D\n");
    let after = materialize("graph TD\nB-->|go|C\nC-->D\n");
    let wire = wire_of(
        &before,
        &after,
        WireOptions::default().with_layer(WireLayer::Model),
    );

    let removed = changes_of_kind(&wire, "EdgeRemoved");
    assert_eq!(removed.len(), 1, "{wire:#}");
    assert_eq!(removed[0]["category"], "removed");
    assert_eq!(removed[0]["layer"], "model");
    assert_eq!(
        removed[0]["subject"],
        json!({ "type": "edge", "before_id": "e0", "after_id": null })
    );
    assert_eq!(
        removed[0]["before"],
        json!({ "source": "A", "target": "B", "label": null })
    );
    assert!(removed[0].get("after").is_none());

    let added = changes_of_kind(&wire, "EdgeAdded");
    assert_eq!(added.len(), 1, "{wire:#}");
    assert_eq!(added[0]["category"], "added");
    assert_eq!(
        added[0]["subject"],
        json!({ "type": "edge", "before_id": null, "after_id": "e0" })
    );
    assert_eq!(
        added[0]["after"],
        json!({ "source": "B", "target": "C", "label": "go" })
    );
    assert_schema_valid(&wire);
}

#[test]
fn wire_node_changes_carry_inline_values_and_summary_counts_categories() {
    let before = materialize(
        "graph TD\nsubgraph pay[Payments]\nstripe[Stripe Adapter]\nend\ninv[Inventory] --> db[(DB)]\n",
    );
    let after = materialize("graph TD\ninv[Stock] --> db[(DB)]\nsubgraph pay[Payments]\nend\n");
    let wire = wire_of(
        &before,
        &after,
        WireOptions::default().with_layer(WireLayer::Model),
    );

    let removed = changes_of_kind(&wire, "NodeRemoved");
    assert_eq!(
        removed[0]["subject"],
        json!({ "type": "node", "id": "stripe" })
    );
    assert_eq!(
        removed[0]["before"],
        json!({ "label": "Stripe Adapter", "shape": "rectangle", "parent": "pay" })
    );

    let relabeled = changes_of_kind(&wire, "NodeLabelChanged");
    assert_eq!(relabeled[0]["category"], "changed");
    assert_eq!(relabeled[0]["before"]["label"], "Inventory");
    assert_eq!(relabeled[0]["after"]["label"], "Stock");

    let changes = wire["changes"].as_array().unwrap();
    let count = |category: &str| {
        changes
            .iter()
            .filter(|change| change["category"] == category)
            .count()
    };
    assert_eq!(
        wire["summary"],
        json!({
            "added": count("added"),
            "removed": count("removed"),
            "changed": count("changed"),
            "moved": count("moved"),
        })
    );
    assert_schema_valid(&wire);
}

#[test]
fn wire_document_changes_name_extension_namespace_and_diagram_type() {
    let before = materialize("flowchart LR\napi[API] --> auth[Auth]\n");
    let after = materialize("classDiagram\nclass api\nclass auth\napi --> auth\n");
    let wire = wire_of(&before, &after, WireOptions::default());
    let changed = changes_of_kind(&wire, "DiagramTypeChanged");
    assert_eq!(changed[0]["subject"], json!({ "type": "document" }));
    assert_eq!(changed[0]["before"], json!({ "diagram_type": "flowchart" }));
    assert_eq!(changed[0]["after"], json!({ "diagram_type": "class" }));

    let before = materialize("graph TD\nA-->B\n");
    let after = materialize("graph TD\nA-->B\nstyle A fill:#f00\n");
    let wire = wire_of(&before, &after, WireOptions::default());
    let extension = changes_of_kind(&wire, "ExtensionChanged");
    assert_eq!(
        extension[0]["subject"],
        json!({ "type": "document", "extension_namespace": "org.mmdflux.node-style.v1" })
    );
    assert_schema_valid(&wire);
}

#[test]
fn wire_layer_filter_keeps_change_ids_and_drops_dangling_related_ids() {
    let before = materialize_routed("graph TD\nA-->B\nB-->C\n");
    let after = materialize_routed("graph TD\nA[Alpha with a much longer label]-->B\nB-->C\n");
    let diff = diff_documents(&before, &after);
    let all = to_json(&diff, &before, &after, &WireOptions::default());
    let model = to_json(
        &diff,
        &before,
        &after,
        &WireOptions::default().with_layer(WireLayer::Model),
    );
    let geometry = to_json(
        &diff,
        &before,
        &after,
        &WireOptions::default().with_layer(WireLayer::Geometry),
    );

    let ids = |wire: &Value| {
        wire["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["id"].as_u64().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&all),
        (0..diff.changes.len() as u64).collect::<Vec<_>>()
    );
    assert!(
        !ids(&geometry).is_empty(),
        "the relabel should move geometry"
    );
    let mut merged = ids(&model);
    merged.extend(ids(&geometry));
    merged.sort();
    assert_eq!(merged, ids(&all));

    for change in model["changes"].as_array().unwrap() {
        assert_eq!(change["layer"], "model");
        assert!(
            change.get("related").is_none(),
            "related geometry is filtered out of a model-only view: {change}"
        );
    }
    assert!(
        all["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change.get("related").is_some()),
        "the full view should link geometry to the relabel: {all:#}"
    );
    assert_schema_valid(&all);
    assert_schema_valid(&model);
    assert_schema_valid(&geometry);
}

#[test]
fn wire_evidence_is_opt_in() {
    let before = materialize("graph TD\nX-->A\nA-->B\nA-->C\nB-->C\n");
    let after = materialize("graph TD\nX-->A\nA-->B\nB-->X\nC\n");
    let diff = diff_documents(&before, &after);

    let plain = to_json(&diff, &before, &after, &WireOptions::default());
    assert!(
        plain["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change.get("evidence").is_none())
    );

    let with_evidence = to_json(
        &diff,
        &before,
        &after,
        &WireOptions::default().with_evidence(true),
    );
    let reconnect = changes_of_kind(&with_evidence, "EdgeReconnected");
    assert!(
        reconnect[0]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("matched_by=endpoint_label")),
        "{with_evidence:#}"
    );
    assert_schema_valid(&with_evidence);
}
