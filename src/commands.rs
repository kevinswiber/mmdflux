//! Command vocabulary and synchronous MMDS command application.
//!
//! See the crate-level [Stability](crate#stability) section for the
//! variant-addition and field-addition policy on the public types in this module.
//!
//! This module applies one [`Command`] to a fully materialized MMDS [`Document`] and
//! returns model events, not snapshot diff changes. Model events describe accepted
//! model mutations. They are intentionally different from the snapshot diff returned by
//! [`crate::mmds::diff::diff_documents`], which compares two document states and can
//! collapse, reorder, or expand command headlines.
//!
//! Use [`apply`] when default relayout configuration is acceptable. Use
//! [`apply_with_config`] when layout-affecting commands should preserve caller-owned
//! [`RenderConfig`] fields. In both cases, document-owned fields remain authoritative:
//! the document `geometry_level` replaces the effective relayout config, and
//! `metadata.engine` overrides the caller engine when it is present.
//!
//! Edge commands can use [`EdgeSelector::Id`] for exact edge identity or
//! [`EdgeSelector::Semantic`] for best-effort matching by endpoint and optional edge
//! fields. Semantic selectors can be ambiguous for parallel edges and return
//! [`CommandApplyError::EdgeSelectorAmbiguous`].
//!
//! `AddEdge.id` accepts `None` to allocate the next dense `e{index}` identifier. An
//! explicit ID must be that next dense ID; existing IDs and arbitrary non-next IDs are
//! rejected with structured errors. [`Command::SetExtension`] wraps non-object JSON
//! values under a `"value"` key because [`Document::extensions`] stores object payloads.
//! Finite MMDS vocabularies such as node shapes, layout directions, strokes, arrows,
//! and geometry levels use typed Rust enums; callers with string input should parse
//! graph tokens through [`crate::mmds::MmdsToken::parse_mmds`] before constructing a
//! command. Engine IDs remain string-backed MMDS metadata in this slice because the
//! engine selector type lives behind the runtime facade. `Command` itself does not
//! define a wire serialization contract yet.
//!
//! [`CommandApplyError`] implements [`std::error::Error`] and keeps its variants public
//! so callers can either bubble errors through trait objects or match precise failure
//! policies.

use serde_json::Value;
use thiserror::Error;

use crate::format::OutputFormat;
use crate::graph::{Arrow, Direction, GeometryLevel, Shape, Stroke};
#[cfg(test)]
use crate::mmds::diff::ChangeKind;
use crate::mmds::events::{ModelEvent, ModelEventKind};
use crate::mmds::{
    Document, Edge, NODE_STYLE_EXTENSION_NAMESPACE, Node, Position, Size, Subgraph, Subject,
};
use crate::runtime::config::RenderConfig;

#[cfg(test)]
const MODEL_EVENT_KINDS: &[ModelEventKind] = &[
    ModelEventKind::GeometryLevelChanged,
    ModelEventKind::DirectionChanged,
    ModelEventKind::EngineChanged,
    ModelEventKind::NodeAdded,
    ModelEventKind::NodeRemoved,
    ModelEventKind::EdgeAdded,
    ModelEventKind::EdgeRemoved,
    ModelEventKind::SubgraphAdded,
    ModelEventKind::SubgraphRemoved,
    ModelEventKind::NodeLabelChanged,
    ModelEventKind::NodeShapeChanged,
    ModelEventKind::NodeParentChanged,
    ModelEventKind::NodeStyleChanged,
    ModelEventKind::EdgeReconnected,
    ModelEventKind::EdgeEndpointIntentChanged,
    ModelEventKind::EdgeLabelChanged,
    ModelEventKind::EdgeStyleChanged,
    ModelEventKind::SubgraphTitleChanged,
    ModelEventKind::SubgraphDirectionChanged,
    ModelEventKind::SubgraphParentChanged,
    ModelEventKind::SubgraphMembershipChanged,
    ModelEventKind::SubgraphVisibilityChanged,
    ModelEventKind::ProfileChanged,
    ModelEventKind::ExtensionChanged,
];

#[cfg(test)]
const GEOMETRY_CHANGE_KINDS: &[ChangeKind] = &[
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
];

/// Command vocabulary for crate-owned MMDS document edits.
///
/// Commands describe intent, not transport. Applying a command can update semantic
/// fields directly or run a full relayout depending on the variant, but the returned
/// events remain model events rather than snapshot diff changes.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    SetGeometryLevel {
        level: GeometryLevel,
    },
    SetDirection {
        direction: Direction,
    },
    SetEngine {
        engine: Option<String>,
    },
    AddNode {
        id: String,
        label: String,
        shape: Shape,
        parent: Option<String>,
    },
    RemoveNode {
        id: String,
    },
    ChangeNodeLabel {
        node: String,
        label: String,
    },
    ChangeNodeShape {
        node: String,
        shape: Shape,
    },
    SetNodeParent {
        node: String,
        parent: Option<String>,
    },
    SetNodeStyleExtension {
        node: String,
        value: Value,
    },
    SetProfiles {
        profiles: Vec<String>,
    },
    SetExtension {
        namespace: String,
        value: Value,
    },
    AddEdge {
        id: Option<String>,
        source: String,
        target: String,
        from_subgraph: Option<String>,
        to_subgraph: Option<String>,
        label: Option<String>,
        stroke: Stroke,
        arrow_start: Arrow,
        arrow_end: Arrow,
        minlen: i32,
    },
    RemoveEdge {
        edge: EdgeSelector,
    },
    ReconnectEdge {
        edge: EdgeSelector,
        source: String,
        target: String,
    },
    SetEdgeEndpointIntent {
        edge: EdgeSelector,
        from_subgraph: Option<String>,
        to_subgraph: Option<String>,
    },
    ChangeEdgeLabel {
        edge: EdgeSelector,
        label: Option<String>,
    },
    ChangeEdgeStyle {
        edge: EdgeSelector,
        stroke: Option<Stroke>,
        arrow_start: Option<Arrow>,
        arrow_end: Option<Arrow>,
        minlen: Option<i32>,
    },
    AddSubgraph {
        id: String,
        title: Option<String>,
        parent: Option<String>,
        direction: Option<Direction>,
        children: Vec<String>,
        concurrent_regions: Vec<String>,
        invisible: bool,
    },
    RemoveSubgraph {
        id: String,
    },
    ChangeSubgraphTitle {
        subgraph: String,
        title: Option<String>,
    },
    SetSubgraphDirection {
        subgraph: String,
        direction: Option<Direction>,
    },
    SetSubgraphParent {
        subgraph: String,
        parent: Option<String>,
    },
    ChangeSubgraphMembership {
        subgraph: String,
        added_children: Vec<String>,
        removed_children: Vec<String>,
        added_concurrent_regions: Vec<String>,
        removed_concurrent_regions: Vec<String>,
    },
    SetSubgraphVisibility {
        subgraph: String,
        invisible: bool,
    },
}

/// Selector used by commands that operate on an existing edge.
///
/// Prefer [`EdgeSelector::Id`] when the caller has a stable MMDS edge ID. Use
/// [`EdgeSelector::Semantic`] when selecting by source, target, and optional edge
/// attributes is acceptable. Semantic selectors intentionally return ambiguity errors
/// instead of guessing when multiple parallel edges match.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeSelector {
    Id(String),
    Semantic {
        source: String,
        target: String,
        label: Option<String>,
        stroke: Option<Stroke>,
        arrow_start: Option<Arrow>,
        arrow_end: Option<Arrow>,
        minlen: Option<i32>,
    },
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeKindLayer {
    Model,
    Geometry,
}

/// Structured failures produced while applying an MMDS command.
///
/// Variants are part of the public command contract and can be matched directly.
/// The type also implements [`std::error::Error`] through `thiserror`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandApplyError {
    #[error("node not found: {id}")]
    NodeNotFound { id: String },
    #[error("subgraph not found: {id}")]
    SubgraphNotFound { id: String },
    #[error("subject already exists: {id}")]
    SubjectAlreadyExists { id: String },
    #[error("edge selector did not match any edge: {selector:?}")]
    EdgeSelectorNoMatch { selector: Box<EdgeSelector> },
    #[error("edge selector matched multiple edges {matches:?}: {selector:?}")]
    EdgeSelectorAmbiguous {
        selector: Box<EdgeSelector>,
        matches: Vec<String>,
    },
    #[error("edge id already exists: {id}")]
    AddEdgeIdCollision { id: String },
    #[error("unsupported edge id {id}; expected {expected}")]
    AddEdgeIdUnsupported { id: String, expected: String },
    #[error("relayout failed during {stage}: {message}")]
    RelayoutFailed { stage: String, message: String },
}

#[cfg(test)]
pub(crate) fn model_event_kinds() -> &'static [ModelEventKind] {
    MODEL_EVENT_KINDS
}

#[cfg(test)]
pub(crate) fn geometry_change_kinds() -> &'static [ChangeKind] {
    GEOMETRY_CHANGE_KINDS
}

#[cfg(test)]
pub(crate) fn change_kind_layer(kind: ChangeKind) -> ChangeKindLayer {
    let role = match kind {
        ChangeKind::GeometryLevelChanged
        | ChangeKind::DirectionChanged
        | ChangeKind::EngineChanged
        | ChangeKind::NodeAdded
        | ChangeKind::NodeRemoved
        | ChangeKind::EdgeAdded
        | ChangeKind::EdgeRemoved
        | ChangeKind::SubgraphAdded
        | ChangeKind::SubgraphRemoved
        | ChangeKind::NodeLabelChanged
        | ChangeKind::NodeShapeChanged
        | ChangeKind::NodeParentChanged
        | ChangeKind::NodeStyleChanged
        | ChangeKind::EdgeReconnected
        | ChangeKind::EdgeEndpointIntentChanged
        | ChangeKind::EdgeLabelChanged
        | ChangeKind::EdgeStyleChanged
        | ChangeKind::SubgraphTitleChanged
        | ChangeKind::SubgraphDirectionChanged
        | ChangeKind::SubgraphParentChanged
        | ChangeKind::SubgraphMembershipChanged
        | ChangeKind::SubgraphVisibilityChanged
        | ChangeKind::ProfileChanged
        | ChangeKind::ExtensionChanged
        | ChangeKind::DiagramTypeChanged => ChangeKindLayer::Model,
        ChangeKind::NodeMoved
        | ChangeKind::NodeResized
        | ChangeKind::CanvasResized
        | ChangeKind::SubgraphBoundsChanged
        | ChangeKind::EdgeRerouted
        | ChangeKind::EndpointFaceChanged
        | ChangeKind::PortIntentChanged
        | ChangeKind::LabelMoved
        | ChangeKind::LabelResized
        | ChangeKind::LabelSideChanged
        | ChangeKind::PathPortDivergenceChanged
        | ChangeKind::GlobalReflowDetected => ChangeKindLayer::Geometry,
    };

    debug_assert_eq!(kind.is_geometry(), role == ChangeKindLayer::Geometry);
    role
}

#[cfg(test)]
pub(crate) fn model_event_kind_for_command(command: &Command) -> ModelEventKind {
    match command {
        Command::SetGeometryLevel { .. } => ModelEventKind::GeometryLevelChanged,
        Command::SetDirection { .. } => ModelEventKind::DirectionChanged,
        Command::SetEngine { .. } => ModelEventKind::EngineChanged,
        Command::AddNode { .. } => ModelEventKind::NodeAdded,
        Command::RemoveNode { .. } => ModelEventKind::NodeRemoved,
        Command::ChangeNodeLabel { .. } => ModelEventKind::NodeLabelChanged,
        Command::ChangeNodeShape { .. } => ModelEventKind::NodeShapeChanged,
        Command::SetNodeParent { .. } => ModelEventKind::NodeParentChanged,
        Command::SetNodeStyleExtension { .. } => ModelEventKind::NodeStyleChanged,
        Command::SetProfiles { .. } => ModelEventKind::ProfileChanged,
        Command::SetExtension { .. } => ModelEventKind::ExtensionChanged,
        Command::AddEdge { .. } => ModelEventKind::EdgeAdded,
        Command::RemoveEdge { .. } => ModelEventKind::EdgeRemoved,
        Command::ReconnectEdge { .. } => ModelEventKind::EdgeReconnected,
        Command::SetEdgeEndpointIntent { .. } => ModelEventKind::EdgeEndpointIntentChanged,
        Command::ChangeEdgeLabel { .. } => ModelEventKind::EdgeLabelChanged,
        Command::ChangeEdgeStyle { .. } => ModelEventKind::EdgeStyleChanged,
        Command::AddSubgraph { .. } => ModelEventKind::SubgraphAdded,
        Command::RemoveSubgraph { .. } => ModelEventKind::SubgraphRemoved,
        Command::ChangeSubgraphTitle { .. } => ModelEventKind::SubgraphTitleChanged,
        Command::SetSubgraphDirection { .. } => ModelEventKind::SubgraphDirectionChanged,
        Command::SetSubgraphParent { .. } => ModelEventKind::SubgraphParentChanged,
        Command::ChangeSubgraphMembership { .. } => ModelEventKind::SubgraphMembershipChanged,
        Command::SetSubgraphVisibility { .. } => ModelEventKind::SubgraphVisibilityChanged,
    }
}

/// Apply a command using [`RenderConfig::default`] for any required relayout.
///
/// The returned events describe the accepted model mutation. To compare the final
/// document against an earlier snapshot, use [`crate::mmds::diff::diff_documents`] and
/// inspect the resulting snapshot diff.
pub fn apply(
    command: &Command,
    output: &mut Document,
) -> Result<Vec<ModelEvent>, CommandApplyError> {
    apply_with_config(command, output, &RenderConfig::default())
}

/// Apply a command using caller-provided relayout configuration.
///
/// The supplied [`RenderConfig`] is used as the base configuration for layout-affecting
/// commands. Document-owned fields still win: `Document::geometry_level` replaces the
/// caller geometry level, and `Document::metadata.engine` replaces the caller engine
/// when the document records one. Caller-owned fields such as spacing, routing, padding,
/// and simplification settings flow through.
///
/// Like [`apply`], this returns model events rather than snapshot diff changes.
pub fn apply_with_config(
    command: &Command,
    output: &mut Document,
    config: &RenderConfig,
) -> Result<Vec<ModelEvent>, CommandApplyError> {
    match command {
        Command::RemoveNode { id } => {
            node_index(output, id)?;
            apply_with_relayout_event(
                output,
                config,
                node_event(ModelEventKind::NodeRemoved, id.clone()),
                |candidate| {
                    candidate.nodes.retain(|node| node.id != *id);
                    candidate
                        .edges
                        .retain(|edge| edge.source != *id && edge.target != *id);
                    for subgraph in &mut candidate.subgraphs {
                        subgraph.children.retain(|child| child != id);
                        subgraph.concurrent_regions.retain(|region| region != id);
                    }
                    Ok(())
                },
            )
        }
        Command::ChangeEdgeLabel { edge, label } => {
            let edge_index = resolve_edge_index(output, edge)?;
            let edge_id = output.edges[edge_index].id.clone();
            apply_with_relayout_event(
                output,
                config,
                edge_event(ModelEventKind::EdgeLabelChanged, edge_id),
                |candidate| {
                    let edge_index = resolve_edge_index(candidate, edge)?;
                    candidate.edges[edge_index].label = label.clone();
                    Ok(())
                },
            )
        }
        Command::AddEdge {
            id,
            source,
            target,
            from_subgraph,
            to_subgraph,
            label,
            stroke,
            arrow_start,
            arrow_end,
            minlen,
        } => {
            let edge_id = resolve_new_edge_id(output, id.as_deref())?;
            node_index(output, source)?;
            node_index(output, target)?;
            if let Some(from_subgraph) = from_subgraph {
                subgraph_index(output, from_subgraph)?;
            }
            if let Some(to_subgraph) = to_subgraph {
                subgraph_index(output, to_subgraph)?;
            }
            apply_with_relayout_event(
                output,
                config,
                edge_event(ModelEventKind::EdgeAdded, edge_id.clone()),
                |candidate| {
                    candidate.edges.push(Edge {
                        id: edge_id,
                        source: source.clone(),
                        target: target.clone(),
                        from_subgraph: from_subgraph.clone(),
                        to_subgraph: to_subgraph.clone(),
                        label: label.clone(),
                        stroke: *stroke,
                        arrow_start: *arrow_start,
                        arrow_end: *arrow_end,
                        minlen: *minlen,
                        path: None,
                        label_position: None,
                        is_backward: None,
                        source_port: None,
                        target_port: None,
                        label_side: None,
                        label_rect: None,
                    });
                    Ok(())
                },
            )
        }
        Command::RemoveEdge { edge } => {
            let edge_index = resolve_edge_index(output, edge)?;
            let edge_id = output.edges[edge_index].id.clone();
            apply_with_relayout_event(
                output,
                config,
                edge_event(ModelEventKind::EdgeRemoved, edge_id),
                |candidate| {
                    let edge_index = resolve_edge_index(candidate, edge)?;
                    candidate.edges.remove(edge_index);
                    Ok(())
                },
            )
        }
        Command::ReconnectEdge {
            edge,
            source,
            target,
        } => {
            let edge_index = resolve_edge_index(output, edge)?;
            let edge_id = output.edges[edge_index].id.clone();
            node_index(output, source)?;
            node_index(output, target)?;
            apply_with_relayout_event(
                output,
                config,
                edge_event(ModelEventKind::EdgeReconnected, edge_id),
                |candidate| {
                    let edge_index = resolve_edge_index(candidate, edge)?;
                    candidate.edges[edge_index].source = source.clone();
                    candidate.edges[edge_index].target = target.clone();
                    Ok(())
                },
            )
        }
        Command::SetEdgeEndpointIntent {
            edge,
            from_subgraph,
            to_subgraph,
        } => {
            let edge_index = resolve_edge_index(output, edge)?;
            let edge_id = output.edges[edge_index].id.clone();
            if let Some(from_subgraph) = from_subgraph {
                subgraph_index(output, from_subgraph)?;
            }
            if let Some(to_subgraph) = to_subgraph {
                subgraph_index(output, to_subgraph)?;
            }
            apply_with_relayout_event(
                output,
                config,
                edge_event(ModelEventKind::EdgeEndpointIntentChanged, edge_id),
                |candidate| {
                    let edge_index = resolve_edge_index(candidate, edge)?;
                    candidate.edges[edge_index].from_subgraph = from_subgraph.clone();
                    candidate.edges[edge_index].to_subgraph = to_subgraph.clone();
                    Ok(())
                },
            )
        }
        Command::ChangeEdgeStyle {
            edge,
            stroke,
            arrow_start,
            arrow_end,
            minlen,
        } => {
            let edge_index = resolve_edge_index(output, edge)?;
            let edge_id = output.edges[edge_index].id.clone();
            apply_with_relayout_event(
                output,
                config,
                edge_event(ModelEventKind::EdgeStyleChanged, edge_id),
                |candidate| {
                    let edge_index = resolve_edge_index(candidate, edge)?;
                    if let Some(stroke) = stroke {
                        candidate.edges[edge_index].stroke = *stroke;
                    }
                    if let Some(arrow_start) = arrow_start {
                        candidate.edges[edge_index].arrow_start = *arrow_start;
                    }
                    if let Some(arrow_end) = arrow_end {
                        candidate.edges[edge_index].arrow_end = *arrow_end;
                    }
                    if let Some(minlen) = minlen {
                        candidate.edges[edge_index].minlen = *minlen;
                    }
                    Ok(())
                },
            )
        }
        Command::SetGeometryLevel { level } => apply_with_relayout(
            output,
            config,
            ModelEventKind::GeometryLevelChanged,
            |candidate| {
                candidate.geometry_level = *level;
                Ok(())
            },
        ),
        Command::SetDirection { direction } => apply_with_relayout(
            output,
            config,
            ModelEventKind::DirectionChanged,
            |candidate| {
                candidate.metadata.direction = *direction;
                Ok(())
            },
        ),
        Command::SetEngine { engine } => {
            apply_with_relayout(output, config, ModelEventKind::EngineChanged, |candidate| {
                candidate.metadata.engine = engine.clone();
                Ok(())
            })
        }
        Command::AddNode {
            id,
            label,
            shape,
            parent,
        } => {
            if output.nodes.iter().any(|node| node.id == *id) {
                return Err(CommandApplyError::SubjectAlreadyExists { id: id.clone() });
            }
            if let Some(parent) = parent {
                subgraph_index(output, parent)?;
            }
            apply_with_relayout_event(
                output,
                config,
                node_event(ModelEventKind::NodeAdded, id.clone()),
                |candidate| {
                    candidate.nodes.push(Node {
                        id: id.clone(),
                        label: label.clone(),
                        shape: *shape,
                        parent: parent.clone(),
                        position: Position { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                    });
                    Ok(())
                },
            )
        }
        Command::ChangeNodeLabel { node, label } => {
            node_index(output, node)?;
            apply_with_relayout_event(
                output,
                config,
                node_event(ModelEventKind::NodeLabelChanged, node.clone()),
                |candidate| {
                    let index = node_index(candidate, node)?;
                    candidate.nodes[index].label = label.clone();
                    Ok(())
                },
            )
        }
        Command::ChangeNodeShape { node, shape } => {
            node_index(output, node)?;
            apply_with_relayout_event(
                output,
                config,
                node_event(ModelEventKind::NodeShapeChanged, node.clone()),
                |candidate| {
                    let index = node_index(candidate, node)?;
                    candidate.nodes[index].shape = *shape;
                    Ok(())
                },
            )
        }
        Command::SetNodeParent { node, parent } => {
            node_index(output, node)?;
            if let Some(parent) = parent {
                subgraph_index(output, parent)?;
            }
            apply_with_relayout_event(
                output,
                config,
                node_event(ModelEventKind::NodeParentChanged, node.clone()),
                |candidate| {
                    let index = node_index(candidate, node)?;
                    candidate.nodes[index].parent = parent.clone();
                    Ok(())
                },
            )
        }
        Command::SetProfiles { profiles } => {
            output.profiles = profiles.clone();
            Ok(vec![document_event(ModelEventKind::ProfileChanged)])
        }
        Command::SetExtension { namespace, value } => {
            output.extensions.insert(
                namespace.clone(),
                value.as_object().cloned().unwrap_or_else(|| {
                    let mut payload = serde_json::Map::new();
                    payload.insert("value".to_string(), value.clone());
                    payload
                }),
            );
            Ok(vec![document_event(ModelEventKind::ExtensionChanged)])
        }
        Command::SetNodeStyleExtension { node, value } => {
            if !output.nodes.iter().any(|candidate| candidate.id == *node) {
                return Err(CommandApplyError::NodeNotFound { id: node.clone() });
            }
            set_node_style_extension(output, node, value.clone());
            Ok(vec![node_event(
                ModelEventKind::NodeStyleChanged,
                node.clone(),
            )])
        }
        Command::ChangeSubgraphTitle { subgraph, title } => {
            let subgraph_index = output
                .subgraphs
                .iter()
                .position(|candidate| candidate.id == *subgraph)
                .ok_or_else(|| CommandApplyError::SubgraphNotFound {
                    id: subgraph.clone(),
                })?;
            output.subgraphs[subgraph_index].title = title
                .clone()
                .unwrap_or_else(|| output.subgraphs[subgraph_index].id.clone());
            Ok(vec![subgraph_event(
                ModelEventKind::SubgraphTitleChanged,
                subgraph.clone(),
            )])
        }
        Command::AddSubgraph {
            id,
            title,
            parent,
            direction,
            children,
            concurrent_regions,
            invisible,
        } => {
            if output.subgraphs.iter().any(|subgraph| subgraph.id == *id) {
                return Err(CommandApplyError::SubjectAlreadyExists { id: id.clone() });
            }
            if let Some(parent) = parent {
                subgraph_index(output, parent)?;
            }
            for child in children {
                node_index(output, child)?;
            }
            for region in concurrent_regions {
                subgraph_index(output, region)?;
            }
            apply_with_relayout_event(
                output,
                config,
                subgraph_event(ModelEventKind::SubgraphAdded, id.clone()),
                |candidate| {
                    sync_subgraph_children_from_node_parents(candidate);
                    candidate.subgraphs.push(Subgraph {
                        id: id.clone(),
                        title: title.clone().unwrap_or_else(|| id.clone()),
                        children: Vec::new(),
                        parent: parent.clone(),
                        direction: *direction,
                        bounds: None,
                        invisible: *invisible,
                        concurrent_regions: concurrent_regions.clone(),
                    });
                    for child in children {
                        let node_index = node_index(candidate, child)?;
                        candidate.nodes[node_index].parent = Some(id.clone());
                    }
                    for region in concurrent_regions {
                        let region_index = subgraph_index(candidate, region)?;
                        candidate.subgraphs[region_index].parent = Some(id.clone());
                    }
                    sync_subgraph_children_from_node_parents(candidate);
                    Ok(())
                },
            )
        }
        Command::RemoveSubgraph { id } => {
            subgraph_index(output, id)?;
            apply_with_relayout_event(
                output,
                config,
                subgraph_event(ModelEventKind::SubgraphRemoved, id.clone()),
                |candidate| {
                    candidate.subgraphs.retain(|subgraph| subgraph.id != *id);
                    for node in &mut candidate.nodes {
                        if node.parent.as_deref() == Some(id.as_str()) {
                            node.parent = None;
                        }
                    }
                    for subgraph in &mut candidate.subgraphs {
                        if subgraph.parent.as_deref() == Some(id.as_str()) {
                            subgraph.parent = None;
                        }
                        subgraph.concurrent_regions.retain(|region| region != id);
                    }
                    sync_subgraph_children_from_node_parents(candidate);
                    Ok(())
                },
            )
        }
        Command::SetSubgraphDirection {
            subgraph,
            direction,
        } => {
            subgraph_index(output, subgraph)?;
            apply_with_relayout_event(
                output,
                config,
                subgraph_event(ModelEventKind::SubgraphDirectionChanged, subgraph.clone()),
                |candidate| {
                    let subgraph_index = subgraph_index(candidate, subgraph)?;
                    candidate.subgraphs[subgraph_index].direction = *direction;
                    Ok(())
                },
            )
        }
        Command::SetSubgraphParent { subgraph, parent } => {
            subgraph_index(output, subgraph)?;
            if let Some(parent) = parent {
                subgraph_index(output, parent)?;
            }
            apply_with_relayout_event(
                output,
                config,
                subgraph_event(ModelEventKind::SubgraphParentChanged, subgraph.clone()),
                |candidate| {
                    let subgraph_index = subgraph_index(candidate, subgraph)?;
                    candidate.subgraphs[subgraph_index].parent = parent.clone();
                    Ok(())
                },
            )
        }
        Command::ChangeSubgraphMembership {
            subgraph,
            added_children,
            removed_children,
            added_concurrent_regions,
            removed_concurrent_regions,
        } => {
            subgraph_index(output, subgraph)?;
            for child in added_children.iter().chain(removed_children.iter()) {
                node_index(output, child)?;
            }
            for region in added_concurrent_regions
                .iter()
                .chain(removed_concurrent_regions.iter())
            {
                subgraph_index(output, region)?;
            }
            apply_with_relayout_event(
                output,
                config,
                subgraph_event(ModelEventKind::SubgraphMembershipChanged, subgraph.clone()),
                |candidate| {
                    sync_subgraph_children_from_node_parents(candidate);
                    for child in removed_children {
                        let node_index = node_index(candidate, child)?;
                        if candidate.nodes[node_index].parent.as_deref() == Some(subgraph.as_str())
                        {
                            candidate.nodes[node_index].parent = None;
                        }
                    }
                    for child in added_children {
                        let node_index = node_index(candidate, child)?;
                        candidate.nodes[node_index].parent = Some(subgraph.clone());
                    }
                    let target_subgraph_index = subgraph_index(candidate, subgraph)?;
                    for region in removed_concurrent_regions {
                        candidate.subgraphs[target_subgraph_index]
                            .concurrent_regions
                            .retain(|candidate| candidate != region);
                        let region_index = subgraph_index(candidate, region)?;
                        if candidate.subgraphs[region_index].parent.as_deref()
                            == Some(subgraph.as_str())
                        {
                            candidate.subgraphs[region_index].parent = None;
                        }
                    }
                    for region in added_concurrent_regions {
                        if !candidate.subgraphs[target_subgraph_index]
                            .concurrent_regions
                            .contains(region)
                        {
                            candidate.subgraphs[target_subgraph_index]
                                .concurrent_regions
                                .push(region.clone());
                        }
                        let region_index = subgraph_index(candidate, region)?;
                        candidate.subgraphs[region_index].parent = Some(subgraph.clone());
                    }
                    sync_subgraph_children_from_node_parents(candidate);
                    Ok(())
                },
            )
        }
        Command::SetSubgraphVisibility {
            subgraph,
            invisible,
        } => {
            subgraph_index(output, subgraph)?;
            apply_with_relayout_event(
                output,
                config,
                subgraph_event(ModelEventKind::SubgraphVisibilityChanged, subgraph.clone()),
                |candidate| {
                    let subgraph_index = subgraph_index(candidate, subgraph)?;
                    candidate.subgraphs[subgraph_index].invisible = *invisible;
                    Ok(())
                },
            )
        }
    }
}

fn apply_with_relayout(
    output: &mut Document,
    config: &RenderConfig,
    kind: ModelEventKind,
    mutate: impl FnOnce(&mut Document) -> Result<(), CommandApplyError>,
) -> Result<Vec<ModelEvent>, CommandApplyError> {
    apply_with_relayout_event(output, config, document_event(kind), mutate)
}

fn apply_with_relayout_event(
    output: &mut Document,
    config: &RenderConfig,
    event: ModelEvent,
    mutate: impl FnOnce(&mut Document) -> Result<(), CommandApplyError>,
) -> Result<Vec<ModelEvent>, CommandApplyError> {
    let mut candidate = output.clone();
    mutate(&mut candidate)?;
    let relaid = relayout_output_for_command_apply_with_config(&candidate, config)?;
    *output = relaid;
    Ok(vec![event])
}

fn set_node_style_extension(output: &mut Document, node: &str, value: Value) {
    let extension = output
        .extensions
        .entry(NODE_STYLE_EXTENSION_NAMESPACE.to_string())
        .or_default();
    let nodes = extension
        .entry("nodes".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !nodes.is_object() {
        *nodes = Value::Object(serde_json::Map::new());
    }
    nodes
        .as_object_mut()
        .expect("nodes style extension should be an object")
        .insert(node.to_string(), value);
}

fn document_event(kind: ModelEventKind) -> ModelEvent {
    ModelEvent {
        kind,
        subject: Subject::Document,
    }
}

fn node_event(kind: ModelEventKind, id: String) -> ModelEvent {
    ModelEvent {
        kind,
        subject: Subject::Node(id),
    }
}

fn edge_event(kind: ModelEventKind, id: String) -> ModelEvent {
    ModelEvent {
        kind,
        subject: Subject::Edge(id),
    }
}

fn subgraph_event(kind: ModelEventKind, id: String) -> ModelEvent {
    ModelEvent {
        kind,
        subject: Subject::Subgraph(id),
    }
}

fn node_index(output: &Document, id: &str) -> Result<usize, CommandApplyError> {
    output
        .nodes
        .iter()
        .position(|node| node.id == id)
        .ok_or_else(|| CommandApplyError::NodeNotFound { id: id.to_string() })
}

fn subgraph_index(output: &Document, id: &str) -> Result<usize, CommandApplyError> {
    output
        .subgraphs
        .iter()
        .position(|subgraph| subgraph.id == id)
        .ok_or_else(|| CommandApplyError::SubgraphNotFound { id: id.to_string() })
}

fn sync_subgraph_children_from_node_parents(output: &mut Document) {
    for subgraph in &mut output.subgraphs {
        subgraph.children.clear();
    }

    let memberships: Vec<(String, String)> = output
        .nodes
        .iter()
        .filter_map(|node| {
            node.parent
                .as_ref()
                .map(|parent| (node.id.clone(), parent.clone()))
        })
        .collect();

    for (node, parent) in memberships {
        if let Some(subgraph) = output
            .subgraphs
            .iter_mut()
            .find(|subgraph| subgraph.id == parent)
        {
            subgraph.children.push(node);
        }
    }
}

fn resolve_new_edge_id(
    output: &Document,
    requested: Option<&str>,
) -> Result<String, CommandApplyError> {
    let expected = format!("e{}", output.edges.len());
    match requested {
        None => Ok(expected),
        Some(id) if output.edges.iter().any(|edge| edge.id == id) => {
            Err(CommandApplyError::AddEdgeIdCollision { id: id.to_string() })
        }
        Some(id) if id == expected => Ok(expected),
        Some(id) => Err(CommandApplyError::AddEdgeIdUnsupported {
            id: id.to_string(),
            expected,
        }),
    }
}

fn resolve_edge_index(
    output: &Document,
    selector: &EdgeSelector,
) -> Result<usize, CommandApplyError> {
    match selector {
        EdgeSelector::Id(id) => output
            .edges
            .iter()
            .position(|edge| edge.id == *id)
            .ok_or_else(|| CommandApplyError::EdgeSelectorNoMatch {
                selector: Box::new(selector.clone()),
            }),
        EdgeSelector::Semantic { .. } => {
            let matches: Vec<usize> = output
                .edges
                .iter()
                .enumerate()
                .filter_map(|(index, edge)| {
                    semantic_selector_matches(edge, selector).then_some(index)
                })
                .collect();

            match matches.as_slice() {
                [] => Err(CommandApplyError::EdgeSelectorNoMatch {
                    selector: Box::new(selector.clone()),
                }),
                [index] => Ok(*index),
                _ => Err(CommandApplyError::EdgeSelectorAmbiguous {
                    selector: Box::new(selector.clone()),
                    matches: matches
                        .into_iter()
                        .map(|index| output.edges[index].id.clone())
                        .collect(),
                }),
            }
        }
    }
}

fn semantic_selector_matches(edge: &Edge, selector: &EdgeSelector) -> bool {
    let EdgeSelector::Semantic {
        source,
        target,
        label,
        stroke,
        arrow_start,
        arrow_end,
        minlen,
    } = selector
    else {
        return false;
    };

    edge.source == *source
        && edge.target == *target
        && label
            .as_ref()
            .is_none_or(|expected| edge.label.as_ref() == Some(expected))
        && stroke
            .as_ref()
            .is_none_or(|expected| edge.stroke == *expected)
        && arrow_start
            .as_ref()
            .is_none_or(|expected| edge.arrow_start == *expected)
        && arrow_end
            .as_ref()
            .is_none_or(|expected| edge.arrow_end == *expected)
        && minlen.is_none_or(|expected| edge.minlen == expected)
}

#[cfg(test)]
pub(crate) fn relayout_output_for_command_apply(
    output: &Document,
) -> Result<Document, CommandApplyError> {
    relayout_output_for_command_apply_with_config(output, &RenderConfig::default())
}

fn relayout_output_for_command_apply_with_config(
    output: &Document,
    config: &RenderConfig,
) -> Result<Document, CommandApplyError> {
    let mut diagram = crate::mmds::from_document(output)
        .map_err(|err| relayout_failed("hydrate", err.to_string()))?;
    let mut config = config.clone();
    config.geometry_level = output.geometry_level;
    if let Some(engine) = output.metadata.engine.as_deref() {
        config.layout_engine =
            Some(engine.parse().map_err(|err: crate::errors::RenderError| {
                relayout_failed("config", err.to_string())
            })?);
    }
    let json = crate::runtime::graph_family::render_graph_family(
        &output.metadata.diagram_type,
        &mut diagram,
        OutputFormat::Mmds,
        &config,
    )
    .map_err(|err| relayout_failed("solve-render", err.to_string()))?;

    crate::mmds::parse_input(&json).map_err(|err| relayout_failed("parse", err.to_string()))
}

fn relayout_failed(stage: &str, source: String) -> CommandApplyError {
    CommandApplyError::RelayoutFailed {
        stage: stage.to_string(),
        message: source,
    }
}
