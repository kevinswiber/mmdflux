//! Versioned JSON wire format for MMDS snapshot diffs.
//!
//! See the crate-level [Stability](crate#stability) section for the policy on the Rust
//! types in this module. The JSON shape itself is a separate, versioned contract:
//! [`to_json`] emits a document whose `schema` field is [`SCHEMA`] (`mmdflux.diff.v1`),
//! described by `docs/mmds-diff.schema.json` and documented in `docs/mmds.md`.
//!
//! The wire view is built from a [`Diff`] and the two documents it compares, so each
//! change can carry inline `before`/`after` values (labels, endpoints, parents) that let a
//! consumer describe it without loading either document.
//!
//! Within `mmdflux.diff.v1`, changes are additive: new kinds, categories and fields may
//! appear, and consumers should ignore what they don't recognize. Renaming or removing a
//! kind or field, or changing its meaning, needs a new schema version. Evidence strings
//! are only included on request ([`WireOptions::with_evidence`]) and stay diagnostic and
//! not format-stable.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use super::{Change, ChangeKind, Diff};
use crate::mmds::{Document, MmdsToken, Subject};

/// Schema identifier written to the `schema` field of every wire document.
pub const SCHEMA: &str = "mmdflux.diff.v1";

/// Which change layer a wire document includes.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WireLayer {
    /// Model and geometry changes.
    #[default]
    All,
    /// Model changes only ([`ChangeKind::is_model`]).
    Model,
    /// Geometry changes only ([`ChangeKind::is_geometry`]).
    Geometry,
}

/// Options for [`to_json`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WireOptions {
    /// Change layer to include. Defaults to [`WireLayer::All`].
    pub layer: WireLayer,
    /// Include the diagnostic, not format-stable evidence strings.
    pub evidence: bool,
}

impl WireOptions {
    /// Include only changes from `layer`.
    pub fn with_layer(mut self, layer: WireLayer) -> Self {
        self.layer = layer;
        self
    }

    /// Include each change's evidence strings.
    pub fn with_evidence(mut self, evidence: bool) -> Self {
        self.evidence = evidence;
        self
    }
}

/// Coarse grouping of change kinds for consumers that don't need every kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Category {
    Added,
    Removed,
    Changed,
    Moved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Layer {
    Model,
    Geometry,
}

#[derive(Serialize)]
struct WireDiff<'a> {
    schema: &'static str,
    before_geometry_level: &'static str,
    after_geometry_level: &'static str,
    summary: Summary,
    changes: Vec<WireChange<'a>>,
}

#[derive(Default, Serialize)]
struct Summary {
    added: usize,
    removed: usize,
    changed: usize,
    moved: usize,
}

#[derive(Serialize)]
struct WireChange<'a> {
    id: usize,
    kind: &'static str,
    category: Category,
    layer: Layer,
    subject: WireSubject<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    related: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence: Option<&'a [String]>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireSubject<'a> {
    Document {
        #[serde(skip_serializing_if = "Option::is_none")]
        extension_namespace: Option<&'a str>,
    },
    Node {
        id: &'a str,
    },
    Edge {
        before_id: Option<&'a str>,
        after_id: Option<&'a str>,
    },
    Subgraph {
        id: &'a str,
    },
}

/// Serialize `diff`, computed from `before` to `after`, as an `mmdflux.diff.v1` JSON value.
///
/// Change `id`s are indexes into [`Diff::changes`], so they stay stable when
/// [`WireOptions::layer`] filters changes out; `related` lists only ids that are present.
pub fn to_json(diff: &Diff, before: &Document, after: &Document, options: &WireOptions) -> Value {
    let included = diff
        .changes
        .iter()
        .enumerate()
        .filter(|(_, change)| layer_included(change.kind, options.layer))
        .map(|(index, _)| index)
        .collect::<BTreeSet<_>>();

    let mut summary = Summary::default();
    let changes = included
        .iter()
        .map(|&index| {
            let change = &diff.changes[index];
            let wire = wire_change(index, change, before, after, &included, options);
            match wire.category {
                Category::Added => summary.added += 1,
                Category::Removed => summary.removed += 1,
                Category::Changed => summary.changed += 1,
                Category::Moved => summary.moved += 1,
            }
            wire
        })
        .collect();

    serde_json::to_value(WireDiff {
        schema: SCHEMA,
        before_geometry_level: diff.before_geometry_level.as_mmds_str(),
        after_geometry_level: diff.after_geometry_level.as_mmds_str(),
        summary,
        changes,
    })
    .expect("diff wire view should serialize")
}

fn layer_included(kind: ChangeKind, layer: WireLayer) -> bool {
    match layer {
        WireLayer::All => true,
        WireLayer::Model => kind.is_model(),
        WireLayer::Geometry => kind.is_geometry(),
    }
}

fn wire_change<'a>(
    index: usize,
    change: &'a Change,
    before: &Document,
    after: &Document,
    included: &BTreeSet<usize>,
    options: &WireOptions,
) -> WireChange<'a> {
    let (subject, before_value, after_value) = match &change.subject {
        Subject::Document => (
            WireSubject::Document {
                extension_namespace: change.extension_namespace.as_deref(),
            },
            document_value(change.kind, before),
            document_value(change.kind, after),
        ),
        Subject::Node(id) => (
            WireSubject::Node { id },
            node_value(before, id),
            node_value(after, id),
        ),
        Subject::Edge(id) => {
            let (before_id, after_id) = match &change.edge_ids {
                Some(ids) => (ids.before_id.as_deref(), ids.after_id.as_deref()),
                None => (Some(id.as_str()), Some(id.as_str())),
            };
            (
                WireSubject::Edge {
                    before_id,
                    after_id,
                },
                before_id.and_then(|id| edge_value(before, id)),
                after_id.and_then(|id| edge_value(after, id)),
            )
        }
        Subject::Subgraph(id) => (
            WireSubject::Subgraph { id },
            subgraph_value(before, id),
            subgraph_value(after, id),
        ),
    };

    WireChange {
        id: index,
        kind: kind_name(change.kind),
        category: category(change.kind),
        layer: if change.kind.is_geometry() {
            Layer::Geometry
        } else {
            Layer::Model
        },
        subject,
        before: before_value,
        after: after_value,
        related: change
            .related_change_ids
            .iter()
            .copied()
            .filter(|id| included.contains(id))
            .collect(),
        evidence: options.evidence.then_some(change.evidence.as_slice()),
    }
}

fn document_value(kind: ChangeKind, document: &Document) -> Option<Value> {
    let value = match kind {
        ChangeKind::DiagramTypeChanged => {
            serde_json::json!({ "diagram_type": document.metadata.diagram_type })
        }
        ChangeKind::DirectionChanged => {
            serde_json::json!({ "direction": document.metadata.direction.as_mmds_str() })
        }
        ChangeKind::EngineChanged => serde_json::json!({ "engine": document.metadata.engine }),
        ChangeKind::GeometryLevelChanged => {
            serde_json::json!({ "geometry_level": document.geometry_level.as_mmds_str() })
        }
        _ => return None,
    };
    Some(value)
}

fn node_value(document: &Document, id: &str) -> Option<Value> {
    let node = document.nodes.iter().find(|node| node.id == id)?;
    Some(serde_json::json!({
        "label": node.label,
        "shape": node.shape.as_mmds_str(),
        "parent": node.parent,
    }))
}

fn edge_value(document: &Document, id: &str) -> Option<Value> {
    let edge = document.edges.iter().find(|edge| edge.id == id)?;
    Some(serde_json::json!({
        "source": edge.source,
        "target": edge.target,
        "label": edge.label,
    }))
}

fn subgraph_value(document: &Document, id: &str) -> Option<Value> {
    let subgraph = document
        .subgraphs
        .iter()
        .find(|subgraph| subgraph.id == id)?;
    Some(serde_json::json!({
        "title": subgraph.title,
        "parent": subgraph.parent,
    }))
}

fn category(kind: ChangeKind) -> Category {
    match kind {
        ChangeKind::NodeAdded | ChangeKind::EdgeAdded | ChangeKind::SubgraphAdded => {
            Category::Added
        }
        ChangeKind::NodeRemoved | ChangeKind::EdgeRemoved | ChangeKind::SubgraphRemoved => {
            Category::Removed
        }
        ChangeKind::NodeParentChanged
        | ChangeKind::SubgraphParentChanged
        | ChangeKind::NodeMoved
        | ChangeKind::LabelMoved => Category::Moved,
        ChangeKind::GeometryLevelChanged
        | ChangeKind::DiagramTypeChanged
        | ChangeKind::DirectionChanged
        | ChangeKind::EngineChanged
        | ChangeKind::NodeLabelChanged
        | ChangeKind::NodeShapeChanged
        | ChangeKind::NodeStyleChanged
        | ChangeKind::EdgeReconnected
        | ChangeKind::EdgeEndpointIntentChanged
        | ChangeKind::EdgeLabelChanged
        | ChangeKind::EdgeStyleChanged
        | ChangeKind::SubgraphTitleChanged
        | ChangeKind::SubgraphDirectionChanged
        | ChangeKind::SubgraphMembershipChanged
        | ChangeKind::SubgraphVisibilityChanged
        | ChangeKind::ProfileChanged
        | ChangeKind::ExtensionChanged
        | ChangeKind::NodeResized
        | ChangeKind::CanvasResized
        | ChangeKind::SubgraphBoundsChanged
        | ChangeKind::EdgeRerouted
        | ChangeKind::EndpointFaceChanged
        | ChangeKind::PortIntentChanged
        | ChangeKind::LabelResized
        | ChangeKind::LabelSideChanged
        | ChangeKind::PathPortDivergenceChanged
        | ChangeKind::GlobalReflowDetected => Category::Changed,
    }
}

/// Stable wire name for each kind. This match is exhaustive on purpose: a new
/// kind must be given a wire name before it can ship.
fn kind_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::GeometryLevelChanged => "GeometryLevelChanged",
        ChangeKind::DiagramTypeChanged => "DiagramTypeChanged",
        ChangeKind::DirectionChanged => "DirectionChanged",
        ChangeKind::EngineChanged => "EngineChanged",
        ChangeKind::NodeAdded => "NodeAdded",
        ChangeKind::NodeRemoved => "NodeRemoved",
        ChangeKind::EdgeAdded => "EdgeAdded",
        ChangeKind::EdgeRemoved => "EdgeRemoved",
        ChangeKind::SubgraphAdded => "SubgraphAdded",
        ChangeKind::SubgraphRemoved => "SubgraphRemoved",
        ChangeKind::NodeLabelChanged => "NodeLabelChanged",
        ChangeKind::NodeShapeChanged => "NodeShapeChanged",
        ChangeKind::NodeParentChanged => "NodeParentChanged",
        ChangeKind::NodeStyleChanged => "NodeStyleChanged",
        ChangeKind::EdgeReconnected => "EdgeReconnected",
        ChangeKind::EdgeEndpointIntentChanged => "EdgeEndpointIntentChanged",
        ChangeKind::EdgeLabelChanged => "EdgeLabelChanged",
        ChangeKind::EdgeStyleChanged => "EdgeStyleChanged",
        ChangeKind::SubgraphTitleChanged => "SubgraphTitleChanged",
        ChangeKind::SubgraphDirectionChanged => "SubgraphDirectionChanged",
        ChangeKind::SubgraphParentChanged => "SubgraphParentChanged",
        ChangeKind::SubgraphMembershipChanged => "SubgraphMembershipChanged",
        ChangeKind::SubgraphVisibilityChanged => "SubgraphVisibilityChanged",
        ChangeKind::ProfileChanged => "ProfileChanged",
        ChangeKind::ExtensionChanged => "ExtensionChanged",
        ChangeKind::NodeMoved => "NodeMoved",
        ChangeKind::NodeResized => "NodeResized",
        ChangeKind::CanvasResized => "CanvasResized",
        ChangeKind::SubgraphBoundsChanged => "SubgraphBoundsChanged",
        ChangeKind::EdgeRerouted => "EdgeRerouted",
        ChangeKind::EndpointFaceChanged => "EndpointFaceChanged",
        ChangeKind::PortIntentChanged => "PortIntentChanged",
        ChangeKind::LabelMoved => "LabelMoved",
        ChangeKind::LabelResized => "LabelResized",
        ChangeKind::LabelSideChanged => "LabelSideChanged",
        ChangeKind::PathPortDivergenceChanged => "PathPortDivergenceChanged",
        ChangeKind::GlobalReflowDetected => "GlobalReflowDetected",
    }
}
