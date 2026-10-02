use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::event_change_mapping::model_event_kind_to_change_kind;
use super::layout_stability::mutations;
use crate::commands::{
    ChangeKindLayer, Command, CommandApplyError, EdgeSelector, apply, change_kind_layer,
    geometry_change_kinds, model_event_kind_for_command, model_event_kinds,
    relayout_output_for_command_apply,
};
use crate::graph::{Arrow, Direction, GeometryLevel, Shape, Stroke};
use crate::mmds::diff::ChangeKind;
use crate::mmds::events::{ModelEvent, ModelEventKind};
use crate::mmds::{NODE_STYLE_EXTENSION_NAMESPACE, Subject, TEXT_EXTENSION_NAMESPACE};
use crate::{OutputFormat, RenderConfig};

#[test]
fn mmds_command_apply_returns_result_error_surface() {
    let mut output = parse_routed_for_command_apply("graph TD; A --> B");

    let result = apply(
        &Command::RemoveNode {
            id: "Missing".to_string(),
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::NodeNotFound { id }) if id == "Missing"
    ));
}

#[test]
fn mmds_command_apply_target_relayout_preserves_semantic_output() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    output
        .nodes
        .iter_mut()
        .find(|node| node.id == "A")
        .expect("node A should exist")
        .label = "Alpha".into();

    let relaid = relayout_output_for_command_apply(&output).expect("relayout should succeed");

    assert_eq!(node_label(&relaid, "A"), "Alpha");
    assert_eq!(relaid.geometry_level, output.geometry_level);
    assert!(relaid.metadata.bounds.width > 0.0);
}

#[test]
fn mmds_command_apply_target_relayout_preserves_existing_dense_edge_ids() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    B --> C\n");
    let before_ids = edge_ids(&output);
    output
        .nodes
        .iter_mut()
        .find(|node| node.id == "A")
        .expect("node A should exist")
        .label = "Alpha".into();

    let relaid = relayout_output_for_command_apply(&output).expect("relayout should succeed");

    assert_eq!(edge_ids(&relaid), before_ids);
}

#[test]
fn mmds_command_apply_target_relayout_documents_profile_and_extension_policy() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    output.profiles = vec!["custom-profile".to_string()];
    output.extensions.insert(
        TEXT_EXTENSION_NAMESPACE.to_string(),
        map_from_pairs([("projection", json!({ "note": "caller-owned" }))]),
    );
    output.extensions.insert(
        NODE_STYLE_EXTENSION_NAMESPACE.to_string(),
        map_from_pairs([(
            "nodes",
            json!({
                "A": {
                    "fill": "#abcdef",
                    "color": "#123456"
                }
            }),
        )]),
    );

    let relaid = relayout_output_for_command_apply(&output).expect("relayout should succeed");

    assert!(!relaid.profiles.contains(&"custom-profile".to_string()));
    assert!(relaid.extensions.contains_key(TEXT_EXTENSION_NAMESPACE));
    assert!(
        relaid.extensions[TEXT_EXTENSION_NAMESPACE]["projection"]
            .get("note")
            .is_none()
    );
    assert_eq!(
        relaid.extensions[NODE_STYLE_EXTENSION_NAMESPACE]["nodes"]["A"]["fill"],
        "#abcdef"
    );
    assert_eq!(
        relaid.extensions[NODE_STYLE_EXTENSION_NAMESPACE]["nodes"]["A"]["color"],
        "#123456"
    );
}

#[test]
fn mmds_command_apply_target_reconnect_diff_is_edge_reconnected() {
    let before = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");
    let mut changed = before.clone();
    changed
        .edges
        .iter_mut()
        .find(|edge| edge.id == "e0")
        .expect("edge e0 should exist")
        .target = "C".into();

    let relaid = relayout_output_for_command_apply(&changed).expect("relayout should succeed");
    let diff = crate::mmds::diff::diff_documents(&before, &relaid);

    assert!(diff.has_change(ChangeKind::EdgeReconnected, "e0"));
    assert!(diff.changes.iter().any(|event| {
        event.kind == ChangeKind::EdgeReconnected
            && event.evidence_mentions("matched_by=id_reconnected")
    }));
}

#[test]
fn mmds_command_apply_edge_selector_id_matches_exact_edge() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A -->|old| B\n");

    let events = apply(
        &Command::ChangeEdgeLabel {
            edge: EdgeSelector::Id("e0".to_string()),
            label: Some("new".to_string()),
        },
        &mut output,
    )
    .expect("edge selector should resolve");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, ModelEventKind::EdgeLabelChanged);
    assert!(matches!(
        &events[0].subject,
        crate::mmds::Subject::Edge(id) if id == "e0"
    ));
}

#[test]
fn mmds_command_apply_edge_selector_semantic_ambiguous_errors() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    A --> B\n");

    let result = apply(
        &Command::ChangeEdgeLabel {
            edge: EdgeSelector::Semantic {
                source: "A".to_string(),
                target: "B".to_string(),
                label: None,
                stroke: None,
                arrow_start: None,
                arrow_end: None,
                minlen: None,
            },
            label: Some("new".to_string()),
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::EdgeSelectorAmbiguous { matches, .. })
            if matches == vec!["e0".to_string(), "e1".to_string()]
    ));
}

#[test]
fn mmds_command_apply_edge_selector_semantic_no_match_errors() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");

    let result = apply(
        &Command::ChangeEdgeLabel {
            edge: EdgeSelector::Semantic {
                source: "A".to_string(),
                target: "C".to_string(),
                label: None,
                stroke: None,
                arrow_start: None,
                arrow_end: None,
                minlen: None,
            },
            label: Some("new".to_string()),
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::EdgeSelectorNoMatch { .. })
    ));
}

#[test]
fn mmds_command_apply_add_edge_id_none_uses_next_dense_id() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");

    let events = apply(&add_edge_command(None), &mut output).expect("add edge should apply");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, ModelEventKind::EdgeAdded);
    assert!(matches!(
        &events[0].subject,
        crate::mmds::Subject::Edge(id) if id == "e1"
    ));
    assert!(output.edges.iter().any(|edge| edge.id == "e1"));
}

#[test]
fn mmds_command_apply_add_edge_id_accepts_next_dense_id() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");

    let events = apply(&add_edge_command(Some("e1")), &mut output).expect("add edge should apply");

    assert_eq!(events[0].kind, ModelEventKind::EdgeAdded);
    assert!(output.edges.iter().any(|edge| edge.id == "e1"));
}

#[test]
fn mmds_command_apply_add_edge_id_rejects_collision() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");

    let result = apply(&add_edge_command(Some("e0")), &mut output);

    assert!(matches!(
        result,
        Err(CommandApplyError::AddEdgeIdCollision { id }) if id == "e0"
    ));
}

#[test]
fn mmds_command_apply_add_edge_id_rejects_custom_id() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");

    let result = apply(&add_edge_command(Some("edge-stable")), &mut output);

    assert!(matches!(
        result,
        Err(CommandApplyError::AddEdgeIdUnsupported { id, expected })
            if id == "edge-stable" && expected == "e1"
    ));
}

#[test]
fn mmds_command_apply_in_place_sets_profiles_without_geometry_change() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::SetProfiles {
            profiles: vec!["custom-profile".to_string()],
        },
        &mut output,
    )
    .expect("set profiles should apply");

    assert_eq!(output.profiles, vec!["custom-profile".to_string()]);
    assert_primary_event(&events, ModelEventKind::ProfileChanged, "");
    assert_in_place_diff(&before, &output, ChangeKind::ProfileChanged, "");
}

#[test]
fn mmds_command_apply_in_place_sets_extension_without_geometry_change() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::SetExtension {
            namespace: "example.extension".to_string(),
            value: json!({ "enabled": true }),
        },
        &mut output,
    )
    .expect("set extension should apply");

    assert_eq!(output.extensions["example.extension"]["enabled"], true);
    assert_primary_event(&events, ModelEventKind::ExtensionChanged, "");
    assert_in_place_diff(&before, &output, ChangeKind::ExtensionChanged, "");
}

#[test]
fn mmds_command_apply_in_place_sets_node_style_extension() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::SetNodeStyleExtension {
            node: "A".to_string(),
            value: json!({ "fill": "#abcdef" }),
        },
        &mut output,
    )
    .expect("set node style extension should apply");

    assert_eq!(
        output.extensions[NODE_STYLE_EXTENSION_NAMESPACE]["nodes"]["A"]["fill"],
        "#abcdef"
    );
    assert_primary_event(&events, ModelEventKind::NodeStyleChanged, "A");
    assert_in_place_diff(&before, &output, ChangeKind::NodeStyleChanged, "A");
}

#[test]
fn mmds_command_apply_in_place_sets_node_style_extension_missing_node_errors() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");

    let result = apply(
        &Command::SetNodeStyleExtension {
            node: "Missing".to_string(),
            value: json!({ "fill": "#abcdef" }),
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::NodeNotFound { id }) if id == "Missing"
    ));
}

#[test]
fn mmds_command_apply_in_place_changes_subgraph_title() {
    let mut output =
        parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A --> B\n    end\n");
    let before = output.clone();

    let events = apply(
        &Command::ChangeSubgraphTitle {
            subgraph: "sg1".to_string(),
            title: Some("Pipeline".to_string()),
        },
        &mut output,
    )
    .expect("change subgraph title should apply");

    assert_eq!(subgraph_title(&output, "sg1"), "Pipeline");
    assert_primary_event(&events, ModelEventKind::SubgraphTitleChanged, "sg1");
    assert_in_place_diff(&before, &output, ChangeKind::SubgraphTitleChanged, "sg1");
}

#[test]
fn mmds_command_apply_in_place_changes_subgraph_title_missing_subgraph_errors() {
    let mut output =
        parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A --> B\n    end\n");

    let result = apply(
        &Command::ChangeSubgraphTitle {
            subgraph: "missing".to_string(),
            title: Some("Pipeline".to_string()),
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::SubgraphNotFound { id }) if id == "missing"
    ));
}

#[test]
fn mmds_command_apply_relayout_sets_geometry_level() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::SetGeometryLevel {
            level: GeometryLevel::Layout,
        },
        &mut output,
    )
    .expect("set geometry level should apply");

    assert_eq!(output.geometry_level, GeometryLevel::Layout);
    assert_primary_event(&events, ModelEventKind::GeometryLevelChanged, "");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::GeometryLevelChanged, "")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_direction() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::SetDirection {
            direction: Direction::LeftRight,
        },
        &mut output,
    )
    .expect("set direction should apply");

    assert_eq!(output.metadata.direction, Direction::LeftRight);
    assert_primary_event(&events, ModelEventKind::DirectionChanged, "");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::DirectionChanged, "")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_engine() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::SetEngine {
            engine: Some("mermaid-layered".to_string()),
        },
        &mut output,
    )
    .expect("set engine should apply");

    assert_eq!(output.metadata.engine.as_deref(), Some("mermaid-layered"));
    assert_primary_event(&events, ModelEventKind::EngineChanged, "");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::EngineChanged, "")
    );
}

#[test]
fn mmds_command_apply_relayout_invalid_document_config_rolls_back() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    output.metadata.engine = Some("not-a-real-engine".to_string());
    let before = output.clone();

    let result = apply(
        &Command::AddNode {
            id: "C".to_string(),
            label: "Gamma".to_string(),
            shape: Shape::Rectangle,
            parent: None,
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::RelayoutFailed { .. })
    ));
    assert_output_eq(&output, &before);
}

#[test]
fn mmds_command_apply_relayout_invalid_engine_config_rolls_back() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    output.metadata.engine = Some("not-a-real-engine".to_string());
    assert_apply_error_preserves_output(
        &mut output,
        Command::AddNode {
            id: "C".to_string(),
            label: "Gamma".to_string(),
            shape: Shape::Rectangle,
            parent: None,
        },
        |error| matches!(error, CommandApplyError::RelayoutFailed { stage, .. } if stage == "config"),
    );
}

#[test]
fn mmds_command_apply_relayout_adds_node() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::AddNode {
            id: "C".to_string(),
            label: "Gamma".to_string(),
            shape: Shape::Rectangle,
            parent: None,
        },
        &mut output,
    )
    .expect("add node should apply");

    assert_eq!(node_label(&output, "C"), "Gamma");
    assert_primary_event(&events, ModelEventKind::NodeAdded, "C");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output).has_change(ChangeKind::NodeAdded, "C")
    );
}

#[test]
fn mmds_command_apply_relayout_removes_node() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");
    let before = output.clone();

    let events = apply(
        &Command::RemoveNode {
            id: "C".to_string(),
        },
        &mut output,
    )
    .expect("remove node should apply");

    assert!(!output.nodes.iter().any(|node| node.id == "C"));
    assert_primary_event(&events, ModelEventKind::NodeRemoved, "C");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::NodeRemoved, "C")
    );
}

#[test]
fn mmds_command_apply_relayout_changes_node_label() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A[Old] --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::ChangeNodeLabel {
            node: "A".to_string(),
            label: "Alpha".to_string(),
        },
        &mut output,
    )
    .expect("change node label should apply");

    assert_eq!(node_label(&output, "A"), "Alpha");
    assert_primary_event(&events, ModelEventKind::NodeLabelChanged, "A");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::NodeLabelChanged, "A")
    );
}

#[test]
fn mmds_command_apply_relayout_changes_node_shape() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A[Old] --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::ChangeNodeShape {
            node: "A".to_string(),
            shape: Shape::Stadium,
        },
        &mut output,
    )
    .expect("change node shape should apply");

    assert_eq!(node_shape(&output, "A"), Shape::Stadium);
    assert_primary_event(&events, ModelEventKind::NodeShapeChanged, "A");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::NodeShapeChanged, "A")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_node_parent() {
    let mut output = parse_routed_for_command_apply(
        "graph TD\n    subgraph sg1[Group]\n    B\n    end\n    A[Alpha]\n",
    );
    let before = output.clone();

    let events = apply(
        &Command::SetNodeParent {
            node: "A".to_string(),
            parent: Some("sg1".to_string()),
        },
        &mut output,
    )
    .expect("set node parent should apply");

    assert_eq!(node_parent(&output, "A").as_deref(), Some("sg1"));
    assert_primary_event(&events, ModelEventKind::NodeParentChanged, "A");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::NodeParentChanged, "A")
    );
}

#[test]
fn mmds_command_apply_relayout_add_node_duplicate_errors_without_mutation() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let result = apply(
        &Command::AddNode {
            id: "A".to_string(),
            label: "Duplicate".to_string(),
            shape: Shape::Rectangle,
            parent: None,
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::SubjectAlreadyExists { id }) if id == "A"
    ));
    assert_output_eq(&output, &before);
}

#[test]
fn mmds_command_apply_relayout_set_node_parent_missing_subgraph_errors_without_mutation() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let result = apply(
        &Command::SetNodeParent {
            node: "A".to_string(),
            parent: Some("missing".to_string()),
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::SubgraphNotFound { id }) if id == "missing"
    ));
    assert_output_eq(&output, &before);
}

#[test]
fn mmds_command_apply_relayout_adds_edge() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");
    let before = output.clone();

    let events = apply(&add_edge_command(None), &mut output).expect("add edge should apply");

    let edge = edge_by_id(&output, "e1");
    assert_eq!(edge.source, "A");
    assert_eq!(edge.target, "C");
    assert!(edge.path.is_some(), "added edge should be routed");
    assert_primary_event(&events, ModelEventKind::EdgeAdded, "e1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output).has_change(ChangeKind::EdgeAdded, "e1")
    );
}

#[test]
fn mmds_command_apply_relayout_removes_edge() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    B --> C\n");
    let before = output.clone();

    let events = apply(
        &Command::RemoveEdge {
            edge: EdgeSelector::Id("e1".to_string()),
        },
        &mut output,
    )
    .expect("remove edge should apply");

    assert!(!output.edges.iter().any(|edge| edge.id == "e1"));
    assert_primary_event(&events, ModelEventKind::EdgeRemoved, "e1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::EdgeRemoved, "e1")
    );
}

#[test]
fn mmds_command_apply_relayout_reconnects_edge() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");
    let before = output.clone();

    let events = apply(
        &Command::ReconnectEdge {
            edge: EdgeSelector::Id("e0".to_string()),
            source: "A".to_string(),
            target: "C".to_string(),
        },
        &mut output,
    )
    .expect("reconnect edge should apply");

    let edge = edge_by_id(&output, "e0");
    assert_eq!(edge.source, "A");
    assert_eq!(edge.target, "C");
    assert_primary_event(&events, ModelEventKind::EdgeReconnected, "e0");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::EdgeReconnected, "e0")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_edge_endpoint_intent() {
    let mut output = parse_routed_for_command_apply(
        "graph TD\n    subgraph sg1[Source]\n    A\n    end\n    subgraph sg2[Target]\n    B\n    end\n    A --> B\n",
    );
    let before = output.clone();

    let events = apply(
        &Command::SetEdgeEndpointIntent {
            edge: EdgeSelector::Id("e0".to_string()),
            from_subgraph: Some("sg1".to_string()),
            to_subgraph: Some("sg2".to_string()),
        },
        &mut output,
    )
    .expect("set endpoint intent should apply");

    let edge = edge_by_id(&output, "e0");
    assert_eq!(edge.from_subgraph.as_deref(), Some("sg1"));
    assert_eq!(edge.to_subgraph.as_deref(), Some("sg2"));
    assert_primary_event(&events, ModelEventKind::EdgeEndpointIntentChanged, "e0");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::EdgeEndpointIntentChanged, "e0")
    );
}

#[test]
fn mmds_command_apply_relayout_changes_edge_label() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A -->|x| B\n");
    let before = output.clone();

    let events = apply(
        &Command::ChangeEdgeLabel {
            edge: EdgeSelector::Id("e0".to_string()),
            label: Some("much longer label".to_string()),
        },
        &mut output,
    )
    .expect("change edge label should apply");

    let edge = edge_by_id(&output, "e0");
    assert_eq!(edge.label.as_deref(), Some("much longer label"));
    assert_ne!(
        before.edges[0].label_rect.as_ref().map(|rect| rect.width),
        edge.label_rect.as_ref().map(|rect| rect.width)
    );
    assert_primary_event(&events, ModelEventKind::EdgeLabelChanged, "e0");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::EdgeLabelChanged, "e0")
    );
}

#[test]
fn mmds_command_apply_relayout_changes_edge_style() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::ChangeEdgeStyle {
            edge: EdgeSelector::Id("e0".to_string()),
            stroke: Some(Stroke::Dotted),
            arrow_start: Some(Arrow::Circle),
            arrow_end: Some(Arrow::Diamond),
            minlen: Some(2),
        },
        &mut output,
    )
    .expect("change edge style should apply");

    let edge = edge_by_id(&output, "e0");
    assert_eq!(edge.stroke, Stroke::Dotted);
    assert_eq!(edge.arrow_start, Arrow::Circle);
    assert_eq!(edge.arrow_end, Arrow::Diamond);
    assert_eq!(edge.minlen, 2);
    assert_primary_event(&events, ModelEventKind::EdgeStyleChanged, "e0");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::EdgeStyleChanged, "e0")
    );
}

#[test]
fn mmds_command_apply_relayout_ambiguous_selector_errors_without_mutation() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    A --> B\n");
    let before = output.clone();

    let result = apply(
        &Command::RemoveEdge {
            edge: EdgeSelector::Semantic {
                source: "A".to_string(),
                target: "B".to_string(),
                label: None,
                stroke: None,
                arrow_start: None,
                arrow_end: None,
                minlen: None,
            },
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::EdgeSelectorAmbiguous { .. })
    ));
    assert_output_eq(&output, &before);
}

#[test]
fn mmds_command_apply_relayout_reconnect_preserves_edge_fields() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A -->|calls| B\n    C[Gamma]\n");
    {
        let edge = output
            .edges
            .iter_mut()
            .find(|edge| edge.id == "e0")
            .expect("edge e0 should exist");
        edge.stroke = Stroke::Dashed;
        edge.arrow_start = Arrow::Circle;
        edge.arrow_end = Arrow::Diamond;
        edge.minlen = 2;
    }

    apply(
        &Command::ReconnectEdge {
            edge: EdgeSelector::Id("e0".to_string()),
            source: "A".to_string(),
            target: "C".to_string(),
        },
        &mut output,
    )
    .expect("reconnect edge should apply");

    let edge = edge_by_id(&output, "e0");
    assert_eq!(edge.source, "A");
    assert_eq!(edge.target, "C");
    assert_eq!(edge.label.as_deref(), Some("calls"));
    assert_eq!(edge.stroke, Stroke::Dashed);
    assert_eq!(edge.arrow_start, Arrow::Circle);
    assert_eq!(edge.arrow_end, Arrow::Diamond);
    assert_eq!(edge.minlen, 2);
}

#[test]
fn mmds_command_apply_relayout_adds_subgraph() {
    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n");
    let before = output.clone();

    let events = apply(
        &Command::AddSubgraph {
            id: "sg1".to_string(),
            title: Some("Group".to_string()),
            parent: None,
            direction: Some(Direction::LeftRight),
            children: vec!["A".to_string()],
            concurrent_regions: Vec::new(),
            invisible: false,
        },
        &mut output,
    )
    .expect("add subgraph should apply");

    assert_eq!(subgraph_title(&output, "sg1"), "Group");
    assert_eq!(node_parent(&output, "A").as_deref(), Some("sg1"));
    assert_primary_event(&events, ModelEventKind::SubgraphAdded, "sg1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphAdded, "sg1")
    );
}

#[test]
fn mmds_command_apply_relayout_removes_subgraph() {
    let mut output =
        parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n");
    let before = output.clone();

    let events = apply(
        &Command::RemoveSubgraph {
            id: "sg1".to_string(),
        },
        &mut output,
    )
    .expect("remove subgraph should apply");

    assert!(!output.subgraphs.iter().any(|subgraph| subgraph.id == "sg1"));
    assert_eq!(node_parent(&output, "A"), None);
    assert_primary_event(&events, ModelEventKind::SubgraphRemoved, "sg1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphRemoved, "sg1")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_subgraph_direction() {
    let mut output =
        parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n");
    let before = output.clone();

    let events = apply(
        &Command::SetSubgraphDirection {
            subgraph: "sg1".to_string(),
            direction: Some(Direction::LeftRight),
        },
        &mut output,
    )
    .expect("set subgraph direction should apply");

    assert_eq!(
        subgraph_by_id(&output, "sg1").direction,
        Some(Direction::LeftRight)
    );
    assert_primary_event(&events, ModelEventKind::SubgraphDirectionChanged, "sg1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphDirectionChanged, "sg1")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_subgraph_parent() {
    let mut output = parse_routed_for_command_apply(
        "graph TD\n    subgraph outer[Outer]\n    X\n    end\n    subgraph sg1[Group]\n    A\n    end\n",
    );
    let before = output.clone();

    let events = apply(
        &Command::SetSubgraphParent {
            subgraph: "sg1".to_string(),
            parent: Some("outer".to_string()),
        },
        &mut output,
    )
    .expect("set subgraph parent should apply");

    assert_eq!(
        subgraph_by_id(&output, "sg1").parent.as_deref(),
        Some("outer")
    );
    assert_primary_event(&events, ModelEventKind::SubgraphParentChanged, "sg1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphParentChanged, "sg1")
    );
}

#[test]
fn mmds_command_apply_relayout_changes_subgraph_membership() {
    let mut output = parse_routed_for_command_apply(
        "graph TD\n    A[Alpha]\n    subgraph sg1[Group]\n    B\n    end\n",
    );
    let before = output.clone();

    let events = apply(
        &Command::ChangeSubgraphMembership {
            subgraph: "sg1".to_string(),
            added_children: vec!["A".to_string()],
            removed_children: vec!["B".to_string()],
            added_concurrent_regions: Vec::new(),
            removed_concurrent_regions: Vec::new(),
        },
        &mut output,
    )
    .expect("change subgraph membership should apply");

    assert_eq!(node_parent(&output, "A").as_deref(), Some("sg1"));
    assert_eq!(node_parent(&output, "B"), None);
    assert!(
        subgraph_by_id(&output, "sg1")
            .children
            .contains(&"A".to_string())
    );
    assert!(
        !subgraph_by_id(&output, "sg1")
            .children
            .contains(&"B".to_string())
    );
    assert_primary_event(&events, ModelEventKind::SubgraphMembershipChanged, "sg1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphMembershipChanged, "sg1")
    );
}

#[test]
fn mmds_command_apply_relayout_changes_subgraph_membership_concurrent_regions_only() {
    let mut output = parse_routed_for_command_apply(
        "graph TD\n    subgraph parent[Parent]\n    A\n    end\n    subgraph r0[R0]\n    B\n    end\n    subgraph r1[R1]\n    C\n    end\n",
    );
    output
        .subgraphs
        .iter_mut()
        .find(|subgraph| subgraph.id == "parent")
        .expect("parent subgraph should exist")
        .concurrent_regions = vec!["r0".to_string()];
    output
        .subgraphs
        .iter_mut()
        .find(|subgraph| subgraph.id == "r0")
        .expect("r0 should exist")
        .parent = Some("parent".to_string());
    let before = output.clone();

    let events = apply(
        &Command::ChangeSubgraphMembership {
            subgraph: "parent".to_string(),
            added_children: Vec::new(),
            removed_children: Vec::new(),
            added_concurrent_regions: vec!["r1".to_string()],
            removed_concurrent_regions: vec!["r0".to_string()],
        },
        &mut output,
    )
    .expect("change concurrent membership should apply");

    let parent = subgraph_by_id(&output, "parent");
    assert!(parent.concurrent_regions.contains(&"r1".to_string()));
    assert!(!parent.concurrent_regions.contains(&"r0".to_string()));
    assert_eq!(
        subgraph_by_id(&output, "r1").parent.as_deref(),
        Some("parent")
    );
    assert_ne!(
        subgraph_by_id(&output, "r0").parent.as_deref(),
        Some("parent")
    );
    assert_primary_event(&events, ModelEventKind::SubgraphMembershipChanged, "parent");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphMembershipChanged, "parent")
    );
}

#[test]
fn mmds_command_apply_relayout_sets_subgraph_visibility() {
    let mut output =
        parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n");
    let before = output.clone();

    let events = apply(
        &Command::SetSubgraphVisibility {
            subgraph: "sg1".to_string(),
            invisible: true,
        },
        &mut output,
    )
    .expect("set subgraph visibility should apply");

    assert!(subgraph_by_id(&output, "sg1").invisible);
    assert_primary_event(&events, ModelEventKind::SubgraphVisibilityChanged, "sg1");
    assert!(
        crate::mmds::diff::diff_documents(&before, &output)
            .has_change(ChangeKind::SubgraphVisibilityChanged, "sg1")
    );
}

#[test]
fn mmds_command_apply_relayout_add_subgraph_duplicate_errors_without_mutation() {
    let mut output =
        parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n");
    let before = output.clone();

    let result = apply(
        &Command::AddSubgraph {
            id: "sg1".to_string(),
            title: Some("Duplicate".to_string()),
            parent: None,
            direction: None,
            children: Vec::new(),
            concurrent_regions: Vec::new(),
            invisible: false,
        },
        &mut output,
    );

    assert!(matches!(
        result,
        Err(CommandApplyError::SubjectAlreadyExists { id }) if id == "sg1"
    ));
    assert_output_eq(&output, &before);
}

#[test]
fn mmds_command_apply_round_trip_primary_event_matches_diff() {
    let cases = apply_round_trip_cases();
    for expected_kind in model_event_kinds() {
        assert!(
            cases
                .iter()
                .any(|case| case.expected_kind == model_event_kind_to_change_kind(*expected_kind)),
            "missing round-trip case for {expected_kind:?}"
        );
    }

    for case in cases {
        let mut after = case.before.clone();
        let apply_events = apply(&case.command, &mut after).unwrap_or_else(|err| {
            panic!("{} should apply successfully: {err:?}", case.name);
        });
        let diff = crate::mmds::diff::diff_documents(&case.before, &after);

        assert_has_model_event_change_pair(
            &apply_events,
            case.expected_kind,
            &case.expected_subject,
        );
        assert_has_change_pair(&diff.changes, case.expected_kind, &case.expected_subject);

        if case.expect_no_geometry_effects {
            assert!(
                !diff.changes.iter().any(|event| event.kind.is_geometry()),
                "{} produced geometry effects: {diff:#?}",
                case.name
            );
        }
    }
}

#[test]
fn mmds_command_apply_failure_modes_are_structured() {
    let mut ambiguous = parse_routed_for_command_apply("graph TD\n    A --> B\n    A --> B\n");
    assert_apply_error_preserves_output(
        &mut ambiguous,
        Command::RemoveEdge {
            edge: EdgeSelector::Semantic {
                source: "A".to_string(),
                target: "B".to_string(),
                label: None,
                stroke: None,
                arrow_start: None,
                arrow_end: None,
                minlen: None,
            },
        },
        |error| matches!(error, CommandApplyError::EdgeSelectorAmbiguous { .. }),
    );

    let mut output = parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n");
    assert_apply_error_preserves_output(
        &mut output,
        Command::RemoveEdge {
            edge: EdgeSelector::Id("missing".to_string()),
        },
        |error| matches!(error, CommandApplyError::EdgeSelectorNoMatch { .. }),
    );
    assert_apply_error_preserves_output(
        &mut output,
        add_edge_command(Some("e0")),
        |error| matches!(error, CommandApplyError::AddEdgeIdCollision { id } if id == "e0"),
    );
    assert_apply_error_preserves_output(
        &mut output,
        add_edge_command(Some("edge-stable")),
        |error| matches!(error, CommandApplyError::AddEdgeIdUnsupported { id, expected } if id == "edge-stable" && expected == "e1"),
    );
    assert_apply_error_preserves_output(
        &mut output,
        Command::RemoveNode {
            id: "Missing".to_string(),
        },
        |error| matches!(error, CommandApplyError::NodeNotFound { id } if id == "Missing"),
    );
    assert_apply_error_preserves_output(
        &mut output,
        Command::SetSubgraphDirection {
            subgraph: "missing".to_string(),
            direction: Some(Direction::LeftRight),
        },
        |error| matches!(error, CommandApplyError::SubgraphNotFound { id } if id == "missing"),
    );
    assert_apply_error_preserves_output(
        &mut output,
        Command::AddNode {
            id: "A".to_string(),
            label: "Duplicate".to_string(),
            shape: Shape::Rectangle,
            parent: None,
        },
        |error| matches!(error, CommandApplyError::SubjectAlreadyExists { id } if id == "A"),
    );
    output.metadata.engine = Some("not-a-real-engine".to_string());
    assert_apply_error_preserves_output(
        &mut output,
        Command::ChangeNodeLabel {
            node: "A".to_string(),
            label: "Alpha".to_string(),
        },
        |error| matches!(error, CommandApplyError::RelayoutFailed { stage, .. } if stage == "config"),
    );
}

#[test]
fn mmds_command_apply_tier_a_representative_canaries_hold() {
    for case in tier_a_representative_command_cases() {
        let (before, after) = render_tier_a_pair(case.pair_id);
        let canary_diff = crate::mmds::diff::diff_documents(&before, &after);

        assert_has_change_pair(
            &canary_diff.changes,
            case.expected_kind,
            &case.expected_subject,
        );
        if case.pair_id == "M07" {
            assert!(
                !canary_diff.has_change_kind(ChangeKind::EdgeRerouted),
                "M07 should remain style-only with no edge reroute: {canary_diff:#?}"
            );
        }

        let mut applied = before.clone();
        let apply_events = apply(&case.command, &mut applied).unwrap_or_else(|error| {
            panic!(
                "{} representative command should apply: {error:?}",
                case.pair_id
            )
        });

        assert_has_model_event_change_pair(
            &apply_events,
            case.expected_kind,
            &case.expected_subject,
        );
        let applied_diff = crate::mmds::diff::diff_documents(&before, &applied);
        assert_has_change_pair(
            &applied_diff.changes,
            case.expected_kind,
            &case.expected_subject,
        );
        if case.pair_id == "M07" {
            assert!(
                !applied_diff.has_change_kind(ChangeKind::EdgeRerouted),
                "M07 apply should remain style-only with no edge reroute: {applied_diff:#?}"
            );
        }
    }
}

#[test]
fn mmds_command_vocabulary_classifies_all_diff_kinds() {
    let cases = [
        (ChangeKind::GeometryLevelChanged, ChangeKindLayer::Model),
        (ChangeKind::DirectionChanged, ChangeKindLayer::Model),
        (ChangeKind::EngineChanged, ChangeKindLayer::Model),
        (ChangeKind::NodeAdded, ChangeKindLayer::Model),
        (ChangeKind::NodeRemoved, ChangeKindLayer::Model),
        (ChangeKind::EdgeAdded, ChangeKindLayer::Model),
        (ChangeKind::EdgeRemoved, ChangeKindLayer::Model),
        (ChangeKind::SubgraphAdded, ChangeKindLayer::Model),
        (ChangeKind::SubgraphRemoved, ChangeKindLayer::Model),
        (ChangeKind::NodeLabelChanged, ChangeKindLayer::Model),
        (ChangeKind::NodeShapeChanged, ChangeKindLayer::Model),
        (ChangeKind::NodeParentChanged, ChangeKindLayer::Model),
        (ChangeKind::NodeStyleChanged, ChangeKindLayer::Model),
        (ChangeKind::EdgeReconnected, ChangeKindLayer::Model),
        (
            ChangeKind::EdgeEndpointIntentChanged,
            ChangeKindLayer::Model,
        ),
        (ChangeKind::EdgeLabelChanged, ChangeKindLayer::Model),
        (ChangeKind::EdgeStyleChanged, ChangeKindLayer::Model),
        (ChangeKind::SubgraphTitleChanged, ChangeKindLayer::Model),
        (ChangeKind::SubgraphDirectionChanged, ChangeKindLayer::Model),
        (ChangeKind::SubgraphParentChanged, ChangeKindLayer::Model),
        (
            ChangeKind::SubgraphMembershipChanged,
            ChangeKindLayer::Model,
        ),
        (
            ChangeKind::SubgraphVisibilityChanged,
            ChangeKindLayer::Model,
        ),
        (ChangeKind::ProfileChanged, ChangeKindLayer::Model),
        (ChangeKind::ExtensionChanged, ChangeKindLayer::Model),
        (ChangeKind::DiagramTypeChanged, ChangeKindLayer::Model),
        (ChangeKind::NodeMoved, ChangeKindLayer::Geometry),
        (ChangeKind::NodeResized, ChangeKindLayer::Geometry),
        (ChangeKind::CanvasResized, ChangeKindLayer::Geometry),
        (ChangeKind::SubgraphBoundsChanged, ChangeKindLayer::Geometry),
        (ChangeKind::EdgeRerouted, ChangeKindLayer::Geometry),
        (ChangeKind::EndpointFaceChanged, ChangeKindLayer::Geometry),
        (ChangeKind::PortIntentChanged, ChangeKindLayer::Geometry),
        (ChangeKind::LabelMoved, ChangeKindLayer::Geometry),
        (ChangeKind::LabelResized, ChangeKindLayer::Geometry),
        (ChangeKind::LabelSideChanged, ChangeKindLayer::Geometry),
        (
            ChangeKind::PathPortDivergenceChanged,
            ChangeKindLayer::Geometry,
        ),
        (ChangeKind::GlobalReflowDetected, ChangeKindLayer::Geometry),
    ];

    // The exhaustive matches in `commands` are the compile-time drift
    // guard. This count keeps the test fixture's explicit variant list honest.
    assert_eq!(cases.len(), 37);

    for (kind, expected_role) in cases {
        assert_eq!(change_kind_layer(kind), expected_role, "{kind:?}");
        assert_eq!(
            kind.is_geometry(),
            expected_role == ChangeKindLayer::Geometry,
            "{kind:?}"
        );
    }
}

#[test]
fn mmds_command_vocabulary_document_node_examples_map_to_diff_kinds() {
    for (command, expected_kind) in document_node_command_examples() {
        assert_eq!(
            model_event_kind_to_change_kind(model_event_kind_for_command(&command)),
            expected_kind,
            "{command:?}"
        );
    }
}

#[test]
fn mmds_command_vocabulary_document_node_commands_are_semantic_only() {
    for (command, _) in document_node_command_examples() {
        match command {
            Command::SetGeometryLevel { level } => assert_eq!(level, GeometryLevel::Routed),
            Command::SetDirection { direction } => assert_eq!(direction, Direction::LeftRight),
            Command::SetEngine { engine } => {
                assert_eq!(engine.as_deref(), Some("flux-layered"))
            }
            Command::AddNode {
                id,
                label,
                shape,
                parent,
            } => {
                assert_eq!(id, "A");
                assert_eq!(label, "Alpha");
                assert_eq!(shape, Shape::Stadium);
                assert_eq!(parent.as_deref(), Some("cluster_0"));
            }
            Command::RemoveNode { id } => assert_eq!(id, "A"),
            Command::ChangeNodeLabel { node, label } => {
                assert_eq!(node, "A");
                assert_eq!(label, "Alpha");
            }
            Command::ChangeNodeShape { node, shape } => {
                assert_eq!(node, "A");
                assert_eq!(shape, Shape::Stadium);
            }
            Command::SetNodeParent { node, parent } => {
                assert_eq!(node, "A");
                assert_eq!(parent.as_deref(), Some("cluster_0"));
            }
            Command::SetNodeStyleExtension { node, value } => {
                assert_eq!(node, "A");
                assert_eq!(value["fill"], "#fff");
            }
            Command::SetProfiles { profiles } => {
                assert_eq!(profiles, vec!["mmds-core-v1".to_string()]);
            }
            Command::SetExtension { namespace, value } => {
                assert_eq!(namespace, "example.extension");
                assert_eq!(value["enabled"], true);
            }
            other => panic!("unexpected non-document/node command: {other:?}"),
        }
    }
}

fn document_node_command_examples() -> Vec<(Command, ChangeKind)> {
    vec![
        (
            Command::SetGeometryLevel {
                level: GeometryLevel::Routed,
            },
            ChangeKind::GeometryLevelChanged,
        ),
        (
            Command::SetDirection {
                direction: Direction::LeftRight,
            },
            ChangeKind::DirectionChanged,
        ),
        (
            Command::SetEngine {
                engine: Some("flux-layered".to_string()),
            },
            ChangeKind::EngineChanged,
        ),
        (
            Command::AddNode {
                id: "A".to_string(),
                label: "Alpha".to_string(),
                shape: Shape::Stadium,
                parent: Some("cluster_0".to_string()),
            },
            ChangeKind::NodeAdded,
        ),
        (
            Command::RemoveNode {
                id: "A".to_string(),
            },
            ChangeKind::NodeRemoved,
        ),
        (
            Command::ChangeNodeLabel {
                node: "A".to_string(),
                label: "Alpha".to_string(),
            },
            ChangeKind::NodeLabelChanged,
        ),
        (
            Command::ChangeNodeShape {
                node: "A".to_string(),
                shape: Shape::Stadium,
            },
            ChangeKind::NodeShapeChanged,
        ),
        (
            Command::SetNodeParent {
                node: "A".to_string(),
                parent: Some("cluster_0".to_string()),
            },
            ChangeKind::NodeParentChanged,
        ),
        (
            Command::SetNodeStyleExtension {
                node: "A".to_string(),
                value: json!({ "fill": "#fff" }),
            },
            ChangeKind::NodeStyleChanged,
        ),
        (
            Command::SetProfiles {
                profiles: vec!["mmds-core-v1".to_string()],
            },
            ChangeKind::ProfileChanged,
        ),
        (
            Command::SetExtension {
                namespace: "example.extension".to_string(),
                value: json!({ "enabled": true }),
            },
            ChangeKind::ExtensionChanged,
        ),
    ]
}

#[test]
fn mmds_command_vocabulary_edge_examples_map_to_diff_kinds() {
    for (command, expected_kind) in edge_command_examples() {
        assert_eq!(
            model_event_kind_to_change_kind(model_event_kind_for_command(&command)),
            expected_kind,
            "{command:?}"
        );
    }
}

#[test]
fn mmds_command_vocabulary_edge_selector_names_identity_without_layout() {
    let selector = EdgeSelector::Semantic {
        source: "A".to_string(),
        target: "B".to_string(),
        label: Some("calls".to_string()),
        stroke: Some(Stroke::Dotted),
        arrow_start: Some(Arrow::None),
        arrow_end: Some(Arrow::Normal),
        minlen: Some(2),
    };

    match selector {
        EdgeSelector::Semantic {
            source,
            target,
            label,
            stroke,
            arrow_start,
            arrow_end,
            minlen,
        } => {
            assert_eq!(source, "A");
            assert_eq!(target, "B");
            assert_eq!(label.as_deref(), Some("calls"));
            assert_eq!(stroke, Some(Stroke::Dotted));
            assert_eq!(arrow_start, Some(Arrow::None));
            assert_eq!(arrow_end, Some(Arrow::Normal));
            assert_eq!(minlen, Some(2));
        }
        EdgeSelector::Id(_) => panic!("expected semantic selector"),
    }
}

fn edge_command_examples() -> Vec<(Command, ChangeKind)> {
    let selector = EdgeSelector::Id("e0".to_string());

    vec![
        (
            Command::AddEdge {
                id: Some("e0".to_string()),
                source: "A".to_string(),
                target: "B".to_string(),
                from_subgraph: None,
                to_subgraph: None,
                label: Some("calls".to_string()),
                stroke: Stroke::Solid,
                arrow_start: Arrow::None,
                arrow_end: Arrow::Normal,
                minlen: 1,
            },
            ChangeKind::EdgeAdded,
        ),
        (
            Command::RemoveEdge {
                edge: selector.clone(),
            },
            ChangeKind::EdgeRemoved,
        ),
        (
            Command::ReconnectEdge {
                edge: selector.clone(),
                source: "A".to_string(),
                target: "C".to_string(),
            },
            ChangeKind::EdgeReconnected,
        ),
        (
            Command::SetEdgeEndpointIntent {
                edge: selector.clone(),
                from_subgraph: Some("cluster_a".to_string()),
                to_subgraph: Some("cluster_b".to_string()),
            },
            ChangeKind::EdgeEndpointIntentChanged,
        ),
        (
            Command::ChangeEdgeLabel {
                edge: selector.clone(),
                label: Some("calls".to_string()),
            },
            ChangeKind::EdgeLabelChanged,
        ),
        (
            Command::ChangeEdgeStyle {
                edge: selector,
                stroke: Some(Stroke::Dotted),
                arrow_start: Some(Arrow::None),
                arrow_end: Some(Arrow::Normal),
                minlen: Some(2),
            },
            ChangeKind::EdgeStyleChanged,
        ),
    ]
}

#[test]
fn mmds_command_vocabulary_subgraph_examples_map_to_diff_kinds() {
    for (command, expected_kind) in subgraph_command_examples() {
        assert_eq!(
            model_event_kind_to_change_kind(model_event_kind_for_command(&command)),
            expected_kind,
            "{command:?}"
        );
    }
}

#[test]
fn mmds_command_vocabulary_subgraph_membership_is_canonical() {
    let command = Command::ChangeSubgraphMembership {
        subgraph: "cluster_0".to_string(),
        added_children: vec!["A".to_string()],
        removed_children: vec!["B".to_string()],
        added_concurrent_regions: vec!["region_1".to_string()],
        removed_concurrent_regions: vec!["region_0".to_string()],
    };

    match command {
        Command::ChangeSubgraphMembership {
            subgraph,
            added_children,
            removed_children,
            added_concurrent_regions,
            removed_concurrent_regions,
        } => {
            assert_eq!(subgraph, "cluster_0");
            assert_eq!(added_children, vec!["A".to_string()]);
            assert_eq!(removed_children, vec!["B".to_string()]);
            assert_eq!(added_concurrent_regions, vec!["region_1".to_string()]);
            assert_eq!(removed_concurrent_regions, vec!["region_0".to_string()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

fn subgraph_command_examples() -> Vec<(Command, ChangeKind)> {
    vec![
        (
            Command::AddSubgraph {
                id: "cluster_0".to_string(),
                title: Some("Cluster".to_string()),
                parent: None,
                direction: Some(Direction::LeftRight),
                children: vec!["A".to_string()],
                concurrent_regions: vec!["region_0".to_string()],
                invisible: false,
            },
            ChangeKind::SubgraphAdded,
        ),
        (
            Command::RemoveSubgraph {
                id: "cluster_0".to_string(),
            },
            ChangeKind::SubgraphRemoved,
        ),
        (
            Command::ChangeSubgraphTitle {
                subgraph: "cluster_0".to_string(),
                title: Some("Cluster".to_string()),
            },
            ChangeKind::SubgraphTitleChanged,
        ),
        (
            Command::SetSubgraphDirection {
                subgraph: "cluster_0".to_string(),
                direction: Some(Direction::LeftRight),
            },
            ChangeKind::SubgraphDirectionChanged,
        ),
        (
            Command::SetSubgraphParent {
                subgraph: "cluster_0".to_string(),
                parent: Some("root".to_string()),
            },
            ChangeKind::SubgraphParentChanged,
        ),
        (
            Command::ChangeSubgraphMembership {
                subgraph: "cluster_0".to_string(),
                added_children: vec!["A".to_string()],
                removed_children: vec!["B".to_string()],
                added_concurrent_regions: vec!["region_1".to_string()],
                removed_concurrent_regions: vec!["region_0".to_string()],
            },
            ChangeKind::SubgraphMembershipChanged,
        ),
        (
            Command::SetSubgraphVisibility {
                subgraph: "cluster_0".to_string(),
                invisible: true,
            },
            ChangeKind::SubgraphVisibilityChanged,
        ),
    ]
}

#[test]
fn mmds_command_vocabulary_is_symmetric_with_diff_kinds() {
    let command_examples = all_command_examples();
    let command_kinds: Vec<ChangeKind> = command_examples
        .iter()
        .map(|(command, _)| model_event_kind_to_change_kind(model_event_kind_for_command(command)))
        .collect();

    assert_same_kinds(
        &command_kinds,
        &model_event_kinds()
            .iter()
            .map(|kind| model_event_kind_to_change_kind(*kind))
            .collect::<Vec<_>>(),
    );
    assert_same_kinds(geometry_change_kinds(), &geometry_effect_kinds());

    for kind in all_diff_kinds() {
        let is_geometry = contains_kind(geometry_change_kinds(), kind);
        let is_model = model_event_kinds()
            .iter()
            .any(|command_kind| model_event_kind_to_change_kind(*command_kind) == kind)
            || contains_kind(&snapshot_only_model_kinds(), kind);

        assert_ne!(is_geometry, is_model, "{kind:?}");
        assert_eq!(kind.is_geometry(), is_geometry, "{kind:?}");
    }
}

fn all_command_examples() -> Vec<(Command, ChangeKind)> {
    let mut examples = document_node_command_examples();
    examples.extend(edge_command_examples());
    examples.extend(subgraph_command_examples());
    examples
}

struct ApplyRoundTripCase {
    name: &'static str,
    before: crate::mmds::Document,
    command: Command,
    expected_kind: ChangeKind,
    expected_subject: Subject,
    expect_no_geometry_effects: bool,
}

struct TierARepresentativeCommandCase {
    pair_id: &'static str,
    command: Command,
    expected_kind: ChangeKind,
    expected_subject: Subject,
}

fn apply_round_trip_cases() -> Vec<ApplyRoundTripCase> {
    vec![
        round_trip_case(
            "set geometry level",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::SetGeometryLevel {
                level: GeometryLevel::Layout,
            },
            ChangeKind::GeometryLevelChanged,
            Subject::Document,
        ),
        round_trip_case(
            "set direction",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::SetDirection {
                direction: Direction::LeftRight,
            },
            ChangeKind::DirectionChanged,
            Subject::Document,
        ),
        round_trip_case(
            "set engine some",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::SetEngine {
                engine: Some("mermaid-layered".to_string()),
            },
            ChangeKind::EngineChanged,
            Subject::Document,
        ),
        round_trip_case(
            "set engine none",
            parse_routed_with_engine("graph TD\n    A --> B\n", "mermaid-layered"),
            Command::SetEngine { engine: None },
            ChangeKind::EngineChanged,
            Subject::Document,
        ),
        round_trip_case(
            "add node",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::AddNode {
                id: "C".to_string(),
                label: "Gamma".to_string(),
                shape: Shape::Rectangle,
                parent: None,
            },
            ChangeKind::NodeAdded,
            Subject::Node("C".to_string()),
        ),
        round_trip_case(
            "remove node",
            parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n"),
            Command::RemoveNode {
                id: "C".to_string(),
            },
            ChangeKind::NodeRemoved,
            Subject::Node("C".to_string()),
        ),
        round_trip_case(
            "change node label",
            parse_routed_for_command_apply("graph TD\n    A[Old] --> B\n"),
            Command::ChangeNodeLabel {
                node: "A".to_string(),
                label: "Alpha".to_string(),
            },
            ChangeKind::NodeLabelChanged,
            Subject::Node("A".to_string()),
        ),
        round_trip_case(
            "change node shape",
            parse_routed_for_command_apply("graph TD\n    A[Old] --> B\n"),
            Command::ChangeNodeShape {
                node: "A".to_string(),
                shape: Shape::Stadium,
            },
            ChangeKind::NodeShapeChanged,
            Subject::Node("A".to_string()),
        ),
        round_trip_case(
            "set node parent",
            parse_routed_for_command_apply(
                "graph TD\n    subgraph sg1[Group]\n    B\n    end\n    A[Alpha]\n",
            ),
            Command::SetNodeParent {
                node: "A".to_string(),
                parent: Some("sg1".to_string()),
            },
            ChangeKind::NodeParentChanged,
            Subject::Node("A".to_string()),
        ),
        round_trip_case_no_geometry(
            "set node style extension insert",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::SetNodeStyleExtension {
                node: "A".to_string(),
                value: json!({ "fill": "#abcdef" }),
            },
            ChangeKind::NodeStyleChanged,
            Subject::Node("A".to_string()),
        ),
        round_trip_case_no_geometry(
            "set node style extension update",
            output_with_node_style("graph TD\n    A --> B\n", "A", json!({ "fill": "#111111" })),
            Command::SetNodeStyleExtension {
                node: "A".to_string(),
                value: json!({ "fill": "#abcdef" }),
            },
            ChangeKind::NodeStyleChanged,
            Subject::Node("A".to_string()),
        ),
        round_trip_case_no_geometry(
            "set profiles",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::SetProfiles {
                profiles: vec!["custom-profile".to_string()],
            },
            ChangeKind::ProfileChanged,
            Subject::Document,
        ),
        round_trip_case_no_geometry(
            "set extension insert",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::SetExtension {
                namespace: "example.extension".to_string(),
                value: json!({ "enabled": true }),
            },
            ChangeKind::ExtensionChanged,
            Subject::Document,
        ),
        round_trip_case_no_geometry(
            "set extension update",
            output_with_extension(
                "graph TD\n    A --> B\n",
                "example.extension",
                json!({ "enabled": false }),
            ),
            Command::SetExtension {
                namespace: "example.extension".to_string(),
                value: json!({ "enabled": true }),
            },
            ChangeKind::ExtensionChanged,
            Subject::Document,
        ),
        round_trip_case(
            "add edge",
            parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n"),
            add_edge_command(None),
            ChangeKind::EdgeAdded,
            Subject::Edge("e1".to_string()),
        ),
        round_trip_case(
            "remove edge",
            parse_routed_for_command_apply("graph TD\n    A --> B\n    B --> C\n"),
            Command::RemoveEdge {
                edge: EdgeSelector::Id("e1".to_string()),
            },
            ChangeKind::EdgeRemoved,
            Subject::Edge("e1".to_string()),
        ),
        round_trip_case(
            "reconnect edge",
            parse_routed_for_command_apply("graph TD\n    A --> B\n    C[Gamma]\n"),
            Command::ReconnectEdge {
                edge: EdgeSelector::Id("e0".to_string()),
                source: "A".to_string(),
                target: "C".to_string(),
            },
            ChangeKind::EdgeReconnected,
            Subject::Edge("e0".to_string()),
        ),
        round_trip_case(
            "set edge endpoint intent",
            parse_routed_for_command_apply(
                "graph TD\n    subgraph sg1[Source]\n    A\n    end\n    subgraph sg2[Target]\n    B\n    end\n    A --> B\n",
            ),
            Command::SetEdgeEndpointIntent {
                edge: EdgeSelector::Id("e0".to_string()),
                from_subgraph: Some("sg1".to_string()),
                to_subgraph: Some("sg2".to_string()),
            },
            ChangeKind::EdgeEndpointIntentChanged,
            Subject::Edge("e0".to_string()),
        ),
        round_trip_case(
            "change edge label",
            parse_routed_for_command_apply("graph TD\n    A -->|x| B\n"),
            Command::ChangeEdgeLabel {
                edge: EdgeSelector::Id("e0".to_string()),
                label: Some("much longer label".to_string()),
            },
            ChangeKind::EdgeLabelChanged,
            Subject::Edge("e0".to_string()),
        ),
        round_trip_case(
            "change edge style",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::ChangeEdgeStyle {
                edge: EdgeSelector::Id("e0".to_string()),
                stroke: Some(Stroke::Dotted),
                arrow_start: Some(Arrow::Circle),
                arrow_end: Some(Arrow::Diamond),
                minlen: Some(2),
            },
            ChangeKind::EdgeStyleChanged,
            Subject::Edge("e0".to_string()),
        ),
        round_trip_case(
            "add subgraph",
            parse_routed_for_command_apply("graph TD\n    A --> B\n"),
            Command::AddSubgraph {
                id: "sg1".to_string(),
                title: Some("Group".to_string()),
                parent: None,
                direction: Some(Direction::LeftRight),
                children: vec!["A".to_string()],
                concurrent_regions: Vec::new(),
                invisible: false,
            },
            ChangeKind::SubgraphAdded,
            Subject::Subgraph("sg1".to_string()),
        ),
        round_trip_case(
            "remove subgraph",
            parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n"),
            Command::RemoveSubgraph {
                id: "sg1".to_string(),
            },
            ChangeKind::SubgraphRemoved,
            Subject::Subgraph("sg1".to_string()),
        ),
        round_trip_case_no_geometry(
            "change subgraph title",
            parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n"),
            Command::ChangeSubgraphTitle {
                subgraph: "sg1".to_string(),
                title: Some("Pipeline".to_string()),
            },
            ChangeKind::SubgraphTitleChanged,
            Subject::Subgraph("sg1".to_string()),
        ),
        round_trip_case(
            "set subgraph direction",
            parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n"),
            Command::SetSubgraphDirection {
                subgraph: "sg1".to_string(),
                direction: Some(Direction::LeftRight),
            },
            ChangeKind::SubgraphDirectionChanged,
            Subject::Subgraph("sg1".to_string()),
        ),
        round_trip_case(
            "set subgraph parent",
            parse_routed_for_command_apply(
                "graph TD\n    subgraph outer[Outer]\n    X\n    end\n    subgraph sg1[Group]\n    A\n    end\n",
            ),
            Command::SetSubgraphParent {
                subgraph: "sg1".to_string(),
                parent: Some("outer".to_string()),
            },
            ChangeKind::SubgraphParentChanged,
            Subject::Subgraph("sg1".to_string()),
        ),
        round_trip_case(
            "change subgraph membership",
            parse_routed_for_command_apply(
                "graph TD\n    A[Alpha]\n    subgraph sg1[Group]\n    B\n    end\n",
            ),
            Command::ChangeSubgraphMembership {
                subgraph: "sg1".to_string(),
                added_children: vec!["A".to_string()],
                removed_children: vec!["B".to_string()],
                added_concurrent_regions: Vec::new(),
                removed_concurrent_regions: Vec::new(),
            },
            ChangeKind::SubgraphMembershipChanged,
            Subject::Subgraph("sg1".to_string()),
        ),
        round_trip_case(
            "set subgraph visibility",
            parse_routed_for_command_apply("graph TD\n    subgraph sg1[Group]\n    A\n    end\n"),
            Command::SetSubgraphVisibility {
                subgraph: "sg1".to_string(),
                invisible: true,
            },
            ChangeKind::SubgraphVisibilityChanged,
            Subject::Subgraph("sg1".to_string()),
        ),
    ]
}

fn round_trip_case(
    name: &'static str,
    before: crate::mmds::Document,
    command: Command,
    expected_kind: ChangeKind,
    expected_subject: Subject,
) -> ApplyRoundTripCase {
    ApplyRoundTripCase {
        name,
        before,
        command,
        expected_kind,
        expected_subject,
        expect_no_geometry_effects: false,
    }
}

fn round_trip_case_no_geometry(
    name: &'static str,
    before: crate::mmds::Document,
    command: Command,
    expected_kind: ChangeKind,
    expected_subject: Subject,
) -> ApplyRoundTripCase {
    ApplyRoundTripCase {
        name,
        before,
        command,
        expected_kind,
        expected_subject,
        expect_no_geometry_effects: true,
    }
}

fn tier_a_representative_command_cases() -> Vec<TierARepresentativeCommandCase> {
    vec![
        TierARepresentativeCommandCase {
            pair_id: "M01",
            command: Command::AddNode {
                id: "X".to_string(),
                label: "Inserted".to_string(),
                shape: Shape::Rectangle,
                parent: None,
            },
            expected_kind: ChangeKind::NodeAdded,
            expected_subject: Subject::Node("X".to_string()),
        },
        TierARepresentativeCommandCase {
            pair_id: "M05",
            command: Command::ChangeNodeLabel {
                node: "Lint".to_string(),
                label: "Static Analysis".to_string(),
            },
            expected_kind: ChangeKind::NodeLabelChanged,
            expected_subject: Subject::Node("Lint".to_string()),
        },
        TierARepresentativeCommandCase {
            pair_id: "M07",
            command: Command::ChangeEdgeStyle {
                edge: EdgeSelector::Id("e0".to_string()),
                stroke: Some(Stroke::Dotted),
                arrow_start: None,
                arrow_end: None,
                minlen: None,
            },
            expected_kind: ChangeKind::EdgeStyleChanged,
            expected_subject: Subject::Edge("e0".to_string()),
        },
        TierARepresentativeCommandCase {
            pair_id: "M10",
            command: Command::SetSubgraphDirection {
                subgraph: "sg1".to_string(),
                direction: Some(Direction::TopDown),
            },
            expected_kind: ChangeKind::SubgraphDirectionChanged,
            expected_subject: Subject::Subgraph("sg1".to_string()),
        },
    ]
}

fn all_diff_kinds() -> [ChangeKind; 37] {
    [
        ChangeKind::GeometryLevelChanged,
        ChangeKind::DirectionChanged,
        ChangeKind::EngineChanged,
        ChangeKind::NodeAdded,
        ChangeKind::NodeRemoved,
        ChangeKind::EdgeAdded,
        ChangeKind::EdgeRemoved,
        ChangeKind::SubgraphAdded,
        ChangeKind::SubgraphRemoved,
        ChangeKind::NodeLabelChanged,
        ChangeKind::NodeShapeChanged,
        ChangeKind::NodeParentChanged,
        ChangeKind::NodeStyleChanged,
        ChangeKind::EdgeReconnected,
        ChangeKind::EdgeEndpointIntentChanged,
        ChangeKind::EdgeLabelChanged,
        ChangeKind::EdgeStyleChanged,
        ChangeKind::SubgraphTitleChanged,
        ChangeKind::SubgraphDirectionChanged,
        ChangeKind::SubgraphParentChanged,
        ChangeKind::SubgraphMembershipChanged,
        ChangeKind::SubgraphVisibilityChanged,
        ChangeKind::ProfileChanged,
        ChangeKind::ExtensionChanged,
        ChangeKind::DiagramTypeChanged,
        ChangeKind::NodeMoved,
        ChangeKind::NodeResized,
        ChangeKind::CanvasResized,
        ChangeKind::SubgraphBoundsChanged,
        ChangeKind::EdgeRerouted,
        ChangeKind::EndpointFaceChanged,
        ChangeKind::PortIntentChanged,
        ChangeKind::LabelMoved,
        ChangeKind::LabelResized,
        ChangeKind::LabelSideChanged,
        ChangeKind::PathPortDivergenceChanged,
        ChangeKind::GlobalReflowDetected,
    ]
}

/// Model changes a snapshot diff can observe that no command produces: a
/// document's diagram type is fixed when it is materialized.
fn snapshot_only_model_kinds() -> [ChangeKind; 1] {
    [ChangeKind::DiagramTypeChanged]
}

fn geometry_effect_kinds() -> [ChangeKind; 12] {
    [
        ChangeKind::NodeMoved,
        ChangeKind::NodeResized,
        ChangeKind::CanvasResized,
        ChangeKind::SubgraphBoundsChanged,
        ChangeKind::EdgeRerouted,
        ChangeKind::EndpointFaceChanged,
        ChangeKind::PortIntentChanged,
        ChangeKind::LabelMoved,
        ChangeKind::LabelResized,
        ChangeKind::LabelSideChanged,
        ChangeKind::PathPortDivergenceChanged,
        ChangeKind::GlobalReflowDetected,
    ]
}

fn assert_same_kinds(left: &[ChangeKind], right: &[ChangeKind]) {
    assert_eq!(left.len(), right.len());
    for kind in left {
        assert!(contains_kind(right, *kind), "missing {kind:?}");
    }
    for kind in right {
        assert!(contains_kind(left, *kind), "unexpected {kind:?}");
    }
}

fn contains_kind(kinds: &[ChangeKind], needle: ChangeKind) -> bool {
    kinds.contains(&needle)
}

fn node_label(output: &crate::mmds::Document, id: &str) -> String {
    output
        .nodes
        .iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("node {id} should exist"))
        .label
        .clone()
}

fn node_shape(output: &crate::mmds::Document, id: &str) -> Shape {
    output
        .nodes
        .iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("node {id} should exist"))
        .shape
}

fn node_parent(output: &crate::mmds::Document, id: &str) -> Option<String> {
    output
        .nodes
        .iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("node {id} should exist"))
        .parent
        .clone()
}

fn edge_ids(output: &crate::mmds::Document) -> Vec<String> {
    output.edges.iter().map(|edge| edge.id.clone()).collect()
}

fn edge_by_id<'a>(output: &'a crate::mmds::Document, id: &str) -> &'a crate::mmds::Edge {
    output
        .edges
        .iter()
        .find(|edge| edge.id == id)
        .unwrap_or_else(|| panic!("edge {id} should exist"))
}

fn subgraph_title(output: &crate::mmds::Document, id: &str) -> String {
    subgraph_by_id(output, id).title.clone()
}

fn subgraph_by_id<'a>(output: &'a crate::mmds::Document, id: &str) -> &'a crate::mmds::Subgraph {
    output
        .subgraphs
        .iter()
        .find(|subgraph| subgraph.id == id)
        .unwrap_or_else(|| panic!("subgraph {id} should exist"))
}

fn map_from_pairs<const N: usize>(
    pairs: [(&str, serde_json::Value); N],
) -> Map<String, serde_json::Value> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn assert_primary_event(events: &[ModelEvent], kind: ModelEventKind, subject_id: &str) {
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, kind);
    assert!(subject_matches(&events[0].subject, subject_id));
}

fn assert_has_change_pair(
    events: &[crate::mmds::diff::Change],
    kind: ChangeKind,
    subject: &Subject,
) {
    assert!(
        events
            .iter()
            .any(|event| event.kind == kind && event.subject == *subject),
        "missing ({kind:?}, {subject:?}) in {events:#?}"
    );
}

fn assert_has_model_event_change_pair(events: &[ModelEvent], kind: ChangeKind, subject: &Subject) {
    assert!(
        events
            .iter()
            .any(|event| model_event_kind_to_change_kind(event.kind) == kind
                && event.subject == *subject),
        "missing ({kind:?}, {subject:?}) in {events:#?}"
    );
}

fn assert_in_place_diff(
    before: &crate::mmds::Document,
    after: &crate::mmds::Document,
    kind: ChangeKind,
    subject_id: &str,
) {
    assert_geometry_unchanged(before, after);
    let diff = crate::mmds::diff::diff_documents(before, after);
    assert!(diff.has_change(kind, subject_id), "{diff:#?}");
    assert!(
        !diff.changes.iter().any(|event| event.kind.is_geometry()),
        "{diff:#?}"
    );
}

fn assert_geometry_unchanged(before: &crate::mmds::Document, after: &crate::mmds::Document) {
    assert_eq!(
        serde_json::to_value(&before.metadata.bounds).expect("bounds should serialize"),
        serde_json::to_value(&after.metadata.bounds).expect("bounds should serialize")
    );
    assert_eq!(
        serde_json::to_value(&before.nodes).expect("nodes should serialize"),
        serde_json::to_value(&after.nodes).expect("nodes should serialize")
    );
    assert_eq!(
        serde_json::to_value(&before.edges).expect("edges should serialize"),
        serde_json::to_value(&after.edges).expect("edges should serialize")
    );
    assert_eq!(
        subgraph_geometry(before),
        subgraph_geometry(after),
        "subgraph geometry changed"
    );
}

fn assert_output_eq(left: &crate::mmds::Document, right: &crate::mmds::Document) {
    assert_eq!(
        serde_json::to_value(left).expect("left output should serialize"),
        serde_json::to_value(right).expect("right output should serialize")
    );
}

fn assert_apply_error_preserves_output(
    output: &mut crate::mmds::Document,
    command: Command,
    matches_error: impl FnOnce(&CommandApplyError) -> bool,
) {
    let before = output.clone();
    let error = match apply(&command, output) {
        Ok(events) => panic!("{command:?} should return an error, got {events:#?}"),
        Err(error) => error,
    };

    assert!(matches_error(&error), "unexpected error: {error:?}");
    assert_output_eq(output, &before);
}

fn subgraph_geometry(output: &crate::mmds::Document) -> Vec<serde_json::Value> {
    output
        .subgraphs
        .iter()
        .map(|subgraph| {
            json!({
                "id": subgraph.id,
                "bounds": subgraph.bounds,
            })
        })
        .collect()
}

fn subject_matches(subject: &crate::mmds::Subject, subject_id: &str) -> bool {
    match subject {
        crate::mmds::Subject::Document => subject_id.is_empty(),
        crate::mmds::Subject::Node(id)
        | crate::mmds::Subject::Edge(id)
        | crate::mmds::Subject::Subgraph(id) => id == subject_id,
    }
}

fn output_with_extension(source: &str, namespace: &str, value: Value) -> crate::mmds::Document {
    let mut output = parse_routed_for_command_apply(source);
    output
        .extensions
        .insert(namespace.to_string(), object_payload(value));
    output
}

fn output_with_node_style(source: &str, node: &str, value: Value) -> crate::mmds::Document {
    let mut output = parse_routed_for_command_apply(source);
    let mut nodes = Map::new();
    nodes.insert(node.to_string(), value);
    let mut extension = Map::new();
    extension.insert("nodes".to_string(), Value::Object(nodes));
    output
        .extensions
        .insert(NODE_STYLE_EXTENSION_NAMESPACE.to_string(), extension);
    output
}

fn object_payload(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_else(|| {
        let mut payload = Map::new();
        payload.insert("value".to_string(), value);
        payload
    })
}

fn render_tier_a_pair(pair_id: &'static str) -> (crate::mmds::Document, crate::mmds::Document) {
    let pair = mutations::pair_by_id(pair_id)
        .unwrap_or_else(|| panic!("Tier A pair {pair_id} should exist"));
    (
        parse_routed_for_command_apply(&mutation_input_source(pair_id, "before", pair.base)),
        parse_routed_for_command_apply(&mutation_input_source(pair_id, "after", pair.mutated)),
    )
}

fn mutation_input_source(
    pair_id: &'static str,
    side: &'static str,
    input: mutations::MutationInput,
) -> String {
    match input {
        mutations::MutationInput::Inline(source) => source.to_string(),
        mutations::MutationInput::Fixture { family, name } => {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(family)
                .join(name);
            fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "failed to read {side} fixture for {pair_id} from {}: {error}",
                    path.display()
                )
            })
        }
    }
}

fn add_edge_command(id: Option<&str>) -> Command {
    Command::AddEdge {
        id: id.map(str::to_string),
        source: "A".to_string(),
        target: "C".to_string(),
        from_subgraph: None,
        to_subgraph: None,
        label: Some("new".to_string()),
        stroke: Stroke::Solid,
        arrow_start: Arrow::None,
        arrow_end: Arrow::Normal,
        minlen: 1,
    }
}

fn parse_routed_for_command_apply(source: &str) -> crate::mmds::Document {
    parse_routed_with_config(
        source,
        RenderConfig {
            geometry_level: GeometryLevel::Routed,
            ..RenderConfig::default()
        },
    )
}

fn parse_routed_with_engine(source: &str, engine: &str) -> crate::mmds::Document {
    parse_routed_with_config(
        source,
        RenderConfig {
            geometry_level: GeometryLevel::Routed,
            layout_engine: Some(
                crate::engines::graph::EngineAlgorithmId::parse(engine)
                    .expect("engine id should parse"),
            ),
            ..RenderConfig::default()
        },
    )
}

fn parse_routed_with_config(source: &str, config: RenderConfig) -> crate::mmds::Document {
    let json = crate::render_diagram(source, OutputFormat::Mmds, &config)
        .expect("routed MMDS should render");
    crate::mmds::parse_input(&json).expect("rendered MMDS should parse")
}
