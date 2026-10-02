//! Snapshot diff for MMDS documents.
//!
//! See the crate-level [Stability](crate#stability) section for the
//! variant-addition and field-addition policy on the public types in this module.
//!
//! [`diff_documents`] compares two fully materialized MMDS [`Document`] values and returns a
//! snapshot diff: the changes describe what differs between the two states, regardless of
//! the command sequence or editing path that produced them.
//!
//! This is intentionally different from the model events returned by
//! `mmdflux::commands::apply` and `mmdflux::commands::apply_with_config`. Model events
//! describe accepted model mutations; a snapshot diff may collapse, reorder, or expand
//! those headlines based only on the compared document states.
//!
//! The secondary semantic effects are returned as ordinary flat changes in [`Diff::changes`].
//! For example, changing subgraph membership can also change a node's parent, and changing
//! node style extensions can also change the document extension map. Geometry changes are
//! not filtered out; callers can use [`ChangeKind::is_model`] and
//! [`ChangeKind::is_geometry`] to separate model changes from geometry changes.
//!
//! Edge ids in MMDS are positional (`e{declaration_index}`), so one id can name different
//! edges in the two documents. Every edge change therefore carries [`Change::edge_ids`],
//! the edge's id in the `before` document (none when added) and in the `after` document
//! (none when removed). [`Subject::Edge`] keeps a single id: the `before` id for a removed
//! edge and the `after` id otherwise.
//!
//! A [`ChangeKind::EdgeReconnected`] change pairs a removed and an added edge that keep
//! exactly one endpoint in the same role. It is reported only when that pairing is
//! unambiguous, either by a shared id or by a shared label and style; otherwise the diff
//! reports a remove plus an add rather than guess.
//!
//! [`Change::related_change_ids`] links geometry effects back to related semantic
//! changes by index within the same [`Diff::changes`] vector. For edges, only changes
//! about the same before/after edge pair are linked. [`Change::evidence`]
//! contains diagnostic and not format-stable strings intended for debugging and test output,
//! not for long-lived parsing contracts.
//!
//! The diff is an output comparison. It can say that a route moved, a label moved, or a
//! fallback edge match was used; it does not provide causal route attribution.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;

use super::document::NODE_STYLE_EXTENSION_NAMESPACE;
use super::{Bounds, Document, Edge, Node, Port, Position, Rect, Subgraph, Subject};
use crate::graph::GeometryLevel;
use crate::mmds::MmdsToken;

const COORD_EPS: f64 = 0.01;
const DISPLAY_EPS: f64 = 1.0;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    /// Geometry level recorded on the `before` document.
    pub before_geometry_level: GeometryLevel,
    /// Geometry level recorded on the `after` document.
    pub after_geometry_level: GeometryLevel,
    /// Flat change list describing differences between the two documents.
    pub changes: Vec<Change>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Change classification.
    pub kind: ChangeKind,
    /// Entity the change is about.
    pub subject: Subject,
    /// Diagnostic, not format-stable evidence strings.
    pub evidence: Vec<String>,
    /// Change indexes in the same `Diff::changes` vector that are related to this change.
    pub related_change_ids: Vec<usize>,
    /// For an edge change, the edge's id in each document. `None` for other subjects.
    pub edge_ids: Option<EdgeIds>,
}

/// An edge's id in the `before` and `after` documents of a snapshot diff.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeIds {
    /// Id in the `before` document, or `None` when the edge was added.
    pub before_id: Option<String>,
    /// Id in the `after` document, or `None` when the edge was removed.
    pub after_id: Option<String>,
}

/// Kind of change observed in an MMDS snapshot diff.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    GeometryLevelChanged,
    DirectionChanged,
    EngineChanged,
    NodeAdded,
    NodeRemoved,
    EdgeAdded,
    EdgeRemoved,
    SubgraphAdded,
    SubgraphRemoved,
    NodeLabelChanged,
    NodeShapeChanged,
    NodeParentChanged,
    NodeStyleChanged,
    EdgeReconnected,
    EdgeEndpointIntentChanged,
    EdgeLabelChanged,
    EdgeStyleChanged,
    SubgraphTitleChanged,
    SubgraphDirectionChanged,
    SubgraphParentChanged,
    SubgraphMembershipChanged,
    SubgraphVisibilityChanged,
    ProfileChanged,
    ExtensionChanged,
    NodeMoved,
    NodeResized,
    CanvasResized,
    SubgraphBoundsChanged,
    EdgeRerouted,
    EndpointFaceChanged,
    PortIntentChanged,
    LabelMoved,
    LabelResized,
    LabelSideChanged,
    PathPortDivergenceChanged,
    GlobalReflowDetected,
}

impl ChangeKind {
    /// Returns true when this change describes geometry or routed output.
    pub fn is_geometry(self) -> bool {
        matches!(
            self,
            Self::NodeMoved
                | Self::NodeResized
                | Self::CanvasResized
                | Self::SubgraphBoundsChanged
                | Self::EdgeRerouted
                | Self::EndpointFaceChanged
                | Self::PortIntentChanged
                | Self::LabelMoved
                | Self::LabelResized
                | Self::LabelSideChanged
                | Self::PathPortDivergenceChanged
                | Self::GlobalReflowDetected
        )
    }

    /// Returns true when this change describes model state rather than geometry.
    pub fn is_model(self) -> bool {
        !self.is_geometry()
    }

    fn can_have_related_geometry(self) -> bool {
        !self.is_geometry()
    }
}

pub fn diff_documents(before: &Document, after: &Document) -> Diff {
    let mut changes = Vec::new();

    if before.geometry_level != after.geometry_level {
        changes.push(document_change(ChangeKind::GeometryLevelChanged));
    }
    if before.metadata.direction != after.metadata.direction {
        changes.push(document_change(ChangeKind::DirectionChanged));
    }
    if before.metadata.engine != after.metadata.engine {
        changes.push(document_change(ChangeKind::EngineChanged));
    }
    if bounds_changed(&before.metadata.bounds, &after.metadata.bounds) {
        changes.push(document_change_with_evidence(
            ChangeKind::CanvasResized,
            vec![bounds_evidence(
                "canvas",
                &before.metadata.bounds,
                &after.metadata.bounds,
            )],
        ));
    }
    if before.profiles != after.profiles {
        changes.push(document_change(ChangeKind::ProfileChanged));
    }
    if before.extensions != after.extensions {
        changes.push(document_change(ChangeKind::ExtensionChanged));
    }

    let before_nodes = nodes_by_id(before);
    let after_nodes = nodes_by_id(after);
    push_removed_added(
        &mut changes,
        &before_nodes,
        &after_nodes,
        ChangeKind::NodeRemoved,
        ChangeKind::NodeAdded,
        Subject::Node,
    );
    push_node_semantic_events(&mut changes, before, after, &before_nodes, &after_nodes);
    push_node_geometry_events(&mut changes, &before_nodes, &after_nodes);
    push_global_reflow_event(&mut changes, before, after, &before_nodes, &after_nodes);

    let before_edges = edges_by_id(before);
    let after_edges = edges_by_id(after);
    let edge_matches =
        edge_correspondences(&before_edges, &after_edges, &before_nodes, &after_nodes);
    push_edge_removed_added(&mut changes, &edge_matches);
    push_edge_semantic_events(&mut changes, &edge_matches);
    push_edge_geometry_events(&mut changes, &edge_matches, &before_nodes, &after_nodes);

    let before_subgraphs = subgraphs_by_id(before);
    let after_subgraphs = subgraphs_by_id(after);
    push_removed_added(
        &mut changes,
        &before_subgraphs,
        &after_subgraphs,
        ChangeKind::SubgraphRemoved,
        ChangeKind::SubgraphAdded,
        Subject::Subgraph,
    );
    push_subgraph_semantic_events(&mut changes, &before_subgraphs, &after_subgraphs);
    push_subgraph_geometry_events(&mut changes, &before_subgraphs, &after_subgraphs);

    link_related_geometry(&mut changes);

    Diff {
        before_geometry_level: before.geometry_level,
        after_geometry_level: after.geometry_level,
        changes,
    }
}

#[cfg(test)]
impl Diff {
    pub(crate) fn has_change(&self, kind: ChangeKind, subject_id: &str) -> bool {
        self.changes
            .iter()
            .any(|change| change.kind == kind && change.subject.matches_id(subject_id))
    }

    pub(crate) fn has_change_kind(&self, kind: ChangeKind) -> bool {
        self.changes.iter().any(|change| change.kind == kind)
    }

    pub(crate) fn has_related_geometry_for(&self, subject_id: &str) -> bool {
        self.changes.iter().any(|change| {
            change.subject.matches_id(subject_id)
                && (change.kind.is_geometry() || !change.related_change_ids.is_empty())
        })
    }
}

#[cfg(test)]
impl Change {
    pub(crate) fn evidence_mentions(&self, needle: &str) -> bool {
        self.evidence
            .iter()
            .any(|evidence| evidence.contains(needle))
    }
}

fn document_change(kind: ChangeKind) -> Change {
    document_change_with_evidence(kind, Vec::new())
}

fn document_change_with_evidence(kind: ChangeKind, evidence: Vec<String>) -> Change {
    Change {
        kind,
        subject: Subject::Document,
        evidence,
        related_change_ids: Vec::new(),
        edge_ids: None,
    }
}

fn nodes_by_id(output: &Document) -> BTreeMap<String, &Node> {
    output
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect()
}

fn edges_by_id(output: &Document) -> BTreeMap<String, &Edge> {
    output
        .edges
        .iter()
        .map(|edge| (edge.id.clone(), edge))
        .collect()
}

fn subgraphs_by_id(output: &Document) -> BTreeMap<String, &Subgraph> {
    output
        .subgraphs
        .iter()
        .map(|subgraph| (subgraph.id.clone(), subgraph))
        .collect()
}

fn push_removed_added<T>(
    events: &mut Vec<Change>,
    before: &BTreeMap<String, T>,
    after: &BTreeMap<String, T>,
    removed_kind: ChangeKind,
    added_kind: ChangeKind,
    subject: fn(String) -> Subject,
) {
    for id in before.keys().filter(|id| !after.contains_key(*id)) {
        events.push(Change {
            kind: removed_kind,
            subject: subject(id.clone()),
            evidence: Vec::new(),
            related_change_ids: Vec::new(),
            edge_ids: None,
        });
    }

    for id in after.keys().filter(|id| !before.contains_key(*id)) {
        events.push(Change {
            kind: added_kind,
            subject: subject(id.clone()),
            evidence: Vec::new(),
            related_change_ids: Vec::new(),
            edge_ids: None,
        });
    }
}

struct EdgeCorrespondences<'a> {
    matches: Vec<EdgeMatch<'a>>,
    removed: Vec<(String, &'a Edge)>,
    added: Vec<(String, &'a Edge)>,
}

struct EdgeMatch<'a> {
    before_id: String,
    after_id: String,
    before: &'a Edge,
    after: &'a Edge,
    method: EdgeMatchMethod,
}

#[derive(Clone, Copy)]
enum EdgeMatchMethod {
    Id,
    IdReconnected,
    EndpointLabel,
    Fallback {
        rule: &'static str,
        candidate_count: usize,
    },
}

fn edge_correspondences<'a>(
    before: &BTreeMap<String, &'a Edge>,
    after: &BTreeMap<String, &'a Edge>,
    before_nodes: &BTreeMap<String, &Node>,
    after_nodes: &BTreeMap<String, &Node>,
) -> EdgeCorrespondences<'a> {
    let mut matches = Vec::new();
    let mut before_unmatched = before.keys().cloned().collect::<BTreeSet<_>>();
    let mut after_unmatched = after.keys().cloned().collect::<BTreeSet<_>>();

    for id in before.keys().filter(|id| after.contains_key(*id)) {
        let before_edge = before
            .get(id)
            .copied()
            .expect("before edge ID should exist");
        let after_edge = after.get(id).copied().expect("after edge ID should exist");
        if same_edge_endpoints(before_edge, after_edge)
            && endpoint_group_count(before, before_edge) == 1
            && endpoint_group_count(after, after_edge) == 1
        {
            matches.push(EdgeMatch {
                before_id: id.clone(),
                after_id: id.clone(),
                before: before_edge,
                after: after_edge,
                method: EdgeMatchMethod::Id,
            });
            before_unmatched.remove(id);
            after_unmatched.remove(id);
        }
    }

    let endpoint_groups = before_unmatched
        .iter()
        .map(|before_id| {
            let edge = before
                .get(before_id)
                .copied()
                .expect("unmatched before edge should exist");
            (edge.source.clone(), edge.target.clone())
        })
        .collect::<BTreeSet<_>>();

    for (source, target) in endpoint_groups {
        let mut before_group = before_unmatched
            .iter()
            .filter(|before_id| {
                let edge = before
                    .get(*before_id)
                    .copied()
                    .expect("unmatched before edge should exist");
                edge.source == source && edge.target == target
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut after_group = after_unmatched
            .iter()
            .filter(|after_id| {
                let edge = after
                    .get(*after_id)
                    .copied()
                    .expect("unmatched after edge should exist");
                edge.source == source && edge.target == target
            })
            .cloned()
            .collect::<BTreeSet<_>>();

        if after_group.is_empty() {
            continue;
        }

        let mut matcher = EdgeFallbackMatcher {
            matches: &mut matches,
            before_unmatched: &mut before_unmatched,
            after_unmatched: &mut after_unmatched,
            before,
            after,
        };
        matcher.pair_by_key(
            &mut before_group,
            &mut after_group,
            "label_style",
            edge_label_style_key,
        );
        matcher.pair_by_key(&mut before_group, &mut after_group, "label", edge_label_key);
        matcher.pair_by_key(&mut before_group, &mut after_group, "style", edge_style_key);
        matcher.pair_by_declaration_order(&mut before_group, &mut after_group);
    }

    // Reconnects. Edge ids are positional, so a shared id alone does not show
    // that two leftover edges are the same edge: deleting an earlier edge
    // shifts every later id. A reconnect needs a shared endpoint and an
    // unambiguous pairing; anything less is reported as a remove plus an add.
    let reconnect_candidates = |before_unmatched: &BTreeSet<String>,
                                after_unmatched: &BTreeSet<String>,
                                same_key: bool| {
        let mut pairs = Vec::new();
        for before_id in before_unmatched {
            let before_edge = before[before_id];
            for after_id in after_unmatched {
                let after_edge = after[after_id];
                if shares_one_endpoint(before_edge, after_edge)
                    && edge_endpoints_exist_in_both_outputs(
                        before_edge,
                        after_edge,
                        before_nodes,
                        after_nodes,
                    )
                    && (!same_key
                        || edge_label_style_key(before_edge) == edge_label_style_key(after_edge))
                {
                    pairs.push((before_id.clone(), after_id.clone()));
                }
            }
        }
        pairs
    };

    let candidates = reconnect_candidates(&before_unmatched, &after_unmatched, false);
    for (before_id, after_id) in unique_pairs(&candidates) {
        if before_id != after_id {
            continue;
        }
        matches.push(EdgeMatch {
            before_id: before_id.clone(),
            after_id: after_id.clone(),
            before: before[&before_id],
            after: after[&after_id],
            method: EdgeMatchMethod::IdReconnected,
        });
        before_unmatched.remove(&before_id);
        after_unmatched.remove(&after_id);
    }

    let candidates = reconnect_candidates(&before_unmatched, &after_unmatched, true);
    for (before_id, after_id) in unique_pairs(&candidates) {
        matches.push(EdgeMatch {
            before_id: before_id.clone(),
            after_id: after_id.clone(),
            before: before[&before_id],
            after: after[&after_id],
            method: EdgeMatchMethod::EndpointLabel,
        });
        before_unmatched.remove(&before_id);
        after_unmatched.remove(&after_id);
    }

    let removed = before_unmatched
        .into_iter()
        .map(|id| {
            let edge = before.get(&id).copied().expect("removed edge should exist");
            (id, edge)
        })
        .collect();
    let added = after_unmatched
        .into_iter()
        .map(|id| {
            let edge = after.get(&id).copied().expect("added edge should exist");
            (id, edge)
        })
        .collect();

    EdgeCorrespondences {
        matches,
        removed,
        added,
    }
}

/// True when the edges keep exactly one endpoint in the same role.
fn shares_one_endpoint(before: &Edge, after: &Edge) -> bool {
    (before.source == after.source) != (before.target == after.target)
}

/// Pairs whose before and after ids each appear in exactly one candidate pair.
///
/// Counts each side once up front so the check stays linear in the number of
/// candidates; ambiguous leftovers can produce a quadratic number of them.
fn unique_pairs(candidates: &[(String, String)]) -> Vec<(String, String)> {
    let mut before_counts: HashMap<&str, usize> = HashMap::new();
    let mut after_counts: HashMap<&str, usize> = HashMap::new();
    for (before_id, after_id) in candidates {
        *before_counts.entry(before_id).or_default() += 1;
        *after_counts.entry(after_id).or_default() += 1;
    }
    candidates
        .iter()
        .filter(|(before_id, after_id)| {
            before_counts[before_id.as_str()] == 1 && after_counts[after_id.as_str()] == 1
        })
        .cloned()
        .collect()
}

fn same_edge_endpoints(before: &Edge, after: &Edge) -> bool {
    before.source == after.source && before.target == after.target
}

fn endpoint_group_count(edges: &BTreeMap<String, &Edge>, edge: &Edge) -> usize {
    edges
        .values()
        .filter(|candidate| same_edge_endpoints(edge, candidate))
        .count()
}

struct EdgeFallbackMatcher<'m, 'a> {
    matches: &'m mut Vec<EdgeMatch<'a>>,
    before_unmatched: &'m mut BTreeSet<String>,
    after_unmatched: &'m mut BTreeSet<String>,
    before: &'m BTreeMap<String, &'a Edge>,
    after: &'m BTreeMap<String, &'a Edge>,
}

impl<'a> EdgeFallbackMatcher<'_, 'a> {
    fn pair_by_key(
        &mut self,
        before_group: &mut BTreeSet<String>,
        after_group: &mut BTreeSet<String>,
        rule: &'static str,
        key: fn(&Edge) -> String,
    ) {
        for before_id in before_group.iter().cloned().collect::<Vec<_>>() {
            let before_edge = self
                .before
                .get(&before_id)
                .copied()
                .expect("fallback before edge should exist");
            let before_key = key(before_edge);
            let before_key_count = before_group
                .iter()
                .filter(|candidate_id| {
                    let candidate = self
                        .before
                        .get(*candidate_id)
                        .copied()
                        .expect("fallback before edge should exist");
                    key(candidate) == before_key
                })
                .count();
            if before_key_count != 1 {
                continue;
            }

            let candidates = after_group
                .iter()
                .filter(|after_id| {
                    let after_edge = self
                        .after
                        .get(*after_id)
                        .copied()
                        .expect("fallback after edge should exist");
                    key(after_edge) == before_key
                })
                .cloned()
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                continue;
            }

            let after_id = candidates
                .first()
                .expect("one fallback candidate should exist")
                .clone();
            self.push_match(&before_id, &after_id, rule, candidates.len());
            before_group.remove(&before_id);
            after_group.remove(&after_id);
        }
    }

    fn pair_by_declaration_order(
        &mut self,
        before_group: &mut BTreeSet<String>,
        after_group: &mut BTreeSet<String>,
    ) {
        while let (Some(before_id), Some(after_id)) = (
            before_group.iter().next().cloned(),
            after_group.iter().next().cloned(),
        ) {
            self.push_match(
                &before_id,
                &after_id,
                "declaration_order",
                after_group.len(),
            );
            before_group.remove(&before_id);
            after_group.remove(&after_id);
        }
    }

    fn push_match(
        &mut self,
        before_id: &str,
        after_id: &str,
        rule: &'static str,
        candidate_count: usize,
    ) {
        let before_edge = self
            .before
            .get(before_id)
            .copied()
            .expect("fallback before edge should exist");
        let after_edge = self
            .after
            .get(after_id)
            .copied()
            .expect("fallback after edge should exist");
        self.matches.push(EdgeMatch {
            before_id: before_id.to_string(),
            after_id: after_id.to_string(),
            before: before_edge,
            after: after_edge,
            method: EdgeMatchMethod::Fallback {
                rule,
                candidate_count,
            },
        });
        self.before_unmatched.remove(before_id);
        self.after_unmatched.remove(after_id);
    }
}

fn edge_label_style_key(edge: &Edge) -> String {
    format!("{}|{}", edge_label_key(edge), edge_style_key(edge))
}

fn edge_label_key(edge: &Edge) -> String {
    match &edge.label {
        Some(label) => format!("some:{label}"),
        None => "none:".to_string(),
    }
}

fn edge_style_key(edge: &Edge) -> String {
    format!(
        "{}|{}|{}|{}",
        edge.stroke.as_mmds_str(),
        edge.arrow_start.as_mmds_str(),
        edge.arrow_end.as_mmds_str(),
        edge.minlen
    )
}

fn edge_endpoints_exist_in_both_outputs(
    before_edge: &Edge,
    after_edge: &Edge,
    before_nodes: &BTreeMap<String, &Node>,
    after_nodes: &BTreeMap<String, &Node>,
) -> bool {
    [
        before_edge.source.as_str(),
        before_edge.target.as_str(),
        after_edge.source.as_str(),
        after_edge.target.as_str(),
    ]
    .into_iter()
    .all(|id| before_nodes.contains_key(id) && after_nodes.contains_key(id))
}

fn push_edge_removed_added(events: &mut Vec<Change>, correspondences: &EdgeCorrespondences<'_>) {
    for (id, _) in &correspondences.removed {
        events.push(edge_change(ChangeKind::EdgeRemoved, id, Some(id), None));
    }

    for (id, _) in &correspondences.added {
        events.push(edge_change(ChangeKind::EdgeAdded, id, None, Some(id)));
    }
}

fn edge_match_evidence(edge_match: &EdgeMatch<'_>) -> Vec<String> {
    match edge_match.method {
        EdgeMatchMethod::Id => Vec::new(),
        EdgeMatchMethod::IdReconnected => vec![format!(
            "matched_by=id_reconnected; before_id={}; after_id={}",
            edge_match.before_id, edge_match.after_id
        )],
        EdgeMatchMethod::EndpointLabel => vec![format!(
            "matched_by=endpoint_label; before_id={}; after_id={}",
            edge_match.before_id, edge_match.after_id
        )],
        EdgeMatchMethod::Fallback {
            rule,
            candidate_count,
        } => vec![format!(
            "matched_by=fallback; before_id={}; after_id={}; rule={rule}; candidate_count={candidate_count}",
            edge_match.before_id, edge_match.after_id
        )],
    }
}

fn push_node_semantic_events(
    events: &mut Vec<Change>,
    before_output: &Document,
    after_output: &Document,
    before: &BTreeMap<String, &Node>,
    after: &BTreeMap<String, &Node>,
) {
    for (id, before_node) in before {
        let Some(after_node) = after.get(id) else {
            continue;
        };

        if before_node.label != after_node.label {
            events.push(node_change(ChangeKind::NodeLabelChanged, id));
        }
        if before_node.shape != after_node.shape {
            events.push(node_change(ChangeKind::NodeShapeChanged, id));
        }
        if before_node.parent != after_node.parent {
            events.push(node_change(ChangeKind::NodeParentChanged, id));
        }
        if node_style_payload(before_output, id) != node_style_payload(after_output, id) {
            events.push(node_change(ChangeKind::NodeStyleChanged, id));
        }
    }
}

fn push_edge_semantic_events(events: &mut Vec<Change>, correspondences: &EdgeCorrespondences<'_>) {
    for edge_match in &correspondences.matches {
        let before_edge = edge_match.before;
        let after_edge = edge_match.after;

        if before_edge.source != after_edge.source || before_edge.target != after_edge.target {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::EdgeReconnected,
                edge_match,
                Vec::new(),
            ));
        }
        if before_edge.from_subgraph != after_edge.from_subgraph
            || before_edge.to_subgraph != after_edge.to_subgraph
        {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::EdgeEndpointIntentChanged,
                edge_match,
                Vec::new(),
            ));
        }
        if before_edge.label != after_edge.label {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::EdgeLabelChanged,
                edge_match,
                Vec::new(),
            ));
        }
        if edge_style_changed(before_edge, after_edge) {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::EdgeStyleChanged,
                edge_match,
                Vec::new(),
            ));
        }
    }
}

fn push_node_geometry_events(
    events: &mut Vec<Change>,
    before: &BTreeMap<String, &Node>,
    after: &BTreeMap<String, &Node>,
) {
    for (id, before_node) in before {
        let Some(after_node) = after.get(id) else {
            continue;
        };

        let dx = after_node.position.x - before_node.position.x;
        let dy = after_node.position.y - before_node.position.y;
        let distance = dx.hypot(dy);
        if distance > DISPLAY_EPS {
            events.push(node_change_with_evidence(
                ChangeKind::NodeMoved,
                id,
                vec![format!(
                    "displacement dx={dx:.2}; dy={dy:.2}; distance={distance:.2}"
                )],
            ));
        }

        let width_delta = after_node.size.width - before_node.size.width;
        let height_delta = after_node.size.height - before_node.size.height;
        if width_delta.abs() > DISPLAY_EPS || height_delta.abs() > DISPLAY_EPS {
            events.push(node_change_with_evidence(
                ChangeKind::NodeResized,
                id,
                vec![format!(
                    "size width_delta={width_delta:.2}; height_delta={height_delta:.2}"
                )],
            ));
        }
    }
}

fn push_global_reflow_event(
    events: &mut Vec<Change>,
    before_output: &Document,
    after_output: &Document,
    before: &BTreeMap<String, &Node>,
    after: &BTreeMap<String, &Node>,
) {
    let mut unchanged_count = 0usize;
    let mut moved_ids = Vec::new();

    for (id, before_node) in before {
        let Some(after_node) = after.get(id) else {
            continue;
        };
        if !node_semantics_same(before_output, after_output, id, before_node, after_node) {
            continue;
        }

        unchanged_count += 1;
        let dx = after_node.position.x - before_node.position.x;
        let dy = after_node.position.y - before_node.position.y;
        if dx.hypot(dy) > DISPLAY_EPS {
            moved_ids.push(id.clone());
        }
    }

    if moved_ids.len() >= 3 && moved_ids.len() * 5 >= unchanged_count.max(1) {
        events.push(document_change_with_evidence(
            ChangeKind::GlobalReflowDetected,
            vec![format!(
                "unchanged_nodes_moved={}/{}; threshold=min(3 nodes, 20 percent); sample={:?}",
                moved_ids.len(),
                unchanged_count,
                moved_ids.iter().take(5).collect::<Vec<_>>()
            )],
        ));
    }
}

fn node_semantics_same(
    before_output: &Document,
    after_output: &Document,
    id: &str,
    before: &Node,
    after: &Node,
) -> bool {
    before.label == after.label
        && before.shape == after.shape
        && before.parent == after.parent
        && node_style_payload(before_output, id) == node_style_payload(after_output, id)
}

fn push_edge_geometry_events(
    events: &mut Vec<Change>,
    correspondences: &EdgeCorrespondences<'_>,
    before_nodes: &BTreeMap<String, &Node>,
    after_nodes: &BTreeMap<String, &Node>,
) {
    for edge_match in &correspondences.matches {
        let before_edge = edge_match.before;
        let after_edge = edge_match.after;

        let path = path_delta(before_edge, after_edge);
        if path.changed {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::EdgeRerouted,
                edge_match,
                vec![format!(
                    "path point_count_delta={}; bend_count_delta={}; length_delta={:.2}; envelope_changed={}",
                    path.point_count_delta, path.bend_count_delta, path.length_delta, path.envelope_changed
                )],
            ));
        }

        for (endpoint, before_face, after_face) in [
            (
                "source",
                endpoint_face(before_edge, before_nodes, Endpoint::Source),
                endpoint_face(after_edge, after_nodes, Endpoint::Source),
            ),
            (
                "target",
                endpoint_face(before_edge, before_nodes, Endpoint::Target),
                endpoint_face(after_edge, after_nodes, Endpoint::Target),
            ),
        ] {
            if before_face != after_face {
                events.push(edge_change_for_match_with_evidence(
                    ChangeKind::EndpointFaceChanged,
                    edge_match,
                    vec![format!(
                        "visible {endpoint} endpoint face {before_face}->{after_face}"
                    )],
                ));
            }
        }

        for (endpoint, before_port, after_port) in [
            (
                "source",
                before_edge.source_port.as_ref(),
                after_edge.source_port.as_ref(),
            ),
            (
                "target",
                before_edge.target_port.as_ref(),
                after_edge.target_port.as_ref(),
            ),
        ] {
            if !same_port_intent(before_port, after_port) {
                events.push(edge_change_for_match_with_evidence(
                    ChangeKind::PortIntentChanged,
                    edge_match,
                    vec![format!(
                        "logical {endpoint}_port {}->{}",
                        port_summary(before_port),
                        port_summary(after_port)
                    )],
                ));
            }
        }

        push_label_geometry_events(events, edge_match, before_edge, after_edge);
        push_path_port_divergence_events(
            events,
            edge_match,
            before_edge,
            after_edge,
            before_nodes,
            after_nodes,
        );
    }
}

fn push_subgraph_semantic_events(
    events: &mut Vec<Change>,
    before: &BTreeMap<String, &Subgraph>,
    after: &BTreeMap<String, &Subgraph>,
) {
    for (id, before_subgraph) in before {
        let Some(after_subgraph) = after.get(id) else {
            continue;
        };

        if before_subgraph.title != after_subgraph.title {
            events.push(subgraph_change(ChangeKind::SubgraphTitleChanged, id));
        }
        if before_subgraph.direction != after_subgraph.direction {
            events.push(subgraph_change(ChangeKind::SubgraphDirectionChanged, id));
        }
        if before_subgraph.parent != after_subgraph.parent {
            events.push(subgraph_change(ChangeKind::SubgraphParentChanged, id));
        }
        if string_set(&before_subgraph.children) != string_set(&after_subgraph.children)
            || string_set(&before_subgraph.concurrent_regions)
                != string_set(&after_subgraph.concurrent_regions)
        {
            events.push(subgraph_change(ChangeKind::SubgraphMembershipChanged, id));
        }
        if before_subgraph.invisible != after_subgraph.invisible {
            events.push(subgraph_change(ChangeKind::SubgraphVisibilityChanged, id));
        }
    }
}

fn push_subgraph_geometry_events(
    events: &mut Vec<Change>,
    before: &BTreeMap<String, &Subgraph>,
    after: &BTreeMap<String, &Subgraph>,
) {
    for (id, before_subgraph) in before {
        let Some(after_subgraph) = after.get(id) else {
            continue;
        };

        if option_bounds_changed(
            before_subgraph.bounds.as_ref(),
            after_subgraph.bounds.as_ref(),
        ) {
            events.push(subgraph_change_with_evidence(
                ChangeKind::SubgraphBoundsChanged,
                id,
                vec![option_bounds_evidence(
                    "subgraph_bounds",
                    before_subgraph.bounds.as_ref(),
                    after_subgraph.bounds.as_ref(),
                )],
            ));
        }
    }
}

fn push_label_geometry_events(
    events: &mut Vec<Change>,
    edge_match: &EdgeMatch<'_>,
    before: &Edge,
    after: &Edge,
) {
    if before.label_side != after.label_side {
        events.push(edge_change_for_match_with_evidence(
            ChangeKind::LabelSideChanged,
            edge_match,
            vec![format!(
                "label_side {:?}->{:?}",
                before.label_side, after.label_side
            )],
        ));
    }

    match (&before.label_rect, &after.label_rect) {
        (Some(before_rect), Some(after_rect)) => {
            let (before_x, before_y) = rect_center(before_rect);
            let (after_x, after_y) = rect_center(after_rect);
            let dx = after_x - before_x;
            let dy = after_y - before_y;
            let width_delta = after_rect.width - before_rect.width;
            let height_delta = after_rect.height - before_rect.height;

            if dx.hypot(dy) > DISPLAY_EPS {
                events.push(edge_change_for_match_with_evidence(
                    ChangeKind::LabelMoved,
                    edge_match,
                    vec![format!("label_rect center_dx={dx:.2}; center_dy={dy:.2}")],
                ));
            }
            if width_delta.abs() > DISPLAY_EPS || height_delta.abs() > DISPLAY_EPS {
                events.push(edge_change_for_match_with_evidence(
                    ChangeKind::LabelResized,
                    edge_match,
                    vec![format!(
                        "label_rect width_delta={width_delta:.2}; height_delta={height_delta:.2}"
                    )],
                ));
            }
        }
        (None, Some(_)) | (Some(_), None) => {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::LabelMoved,
                edge_match,
                vec!["label_rect presence changed".to_string()],
            ));
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::LabelResized,
                edge_match,
                vec!["label_rect presence changed".to_string()],
            ));
        }
        (None, None) => {
            let before_pos = before.label_position.as_ref();
            let after_pos = after.label_position.as_ref();
            if option_position_moved(before_pos, after_pos) {
                events.push(edge_change_for_match_with_evidence(
                    ChangeKind::LabelMoved,
                    edge_match,
                    vec!["label_position changed without label_rect".to_string()],
                ));
            }
        }
    }
}

fn push_path_port_divergence_events(
    events: &mut Vec<Change>,
    edge_match: &EdgeMatch<'_>,
    before_edge: &Edge,
    after_edge: &Edge,
    before_nodes: &BTreeMap<String, &Node>,
    after_nodes: &BTreeMap<String, &Node>,
) {
    for (endpoint, before_path_face, after_path_face, before_port_face, after_port_face) in [
        (
            "source",
            endpoint_face(before_edge, before_nodes, Endpoint::Source),
            endpoint_face(after_edge, after_nodes, Endpoint::Source),
            before_edge
                .source_port
                .as_ref()
                .map(|port| port.face.as_str()),
            after_edge
                .source_port
                .as_ref()
                .map(|port| port.face.as_str()),
        ),
        (
            "target",
            endpoint_face(before_edge, before_nodes, Endpoint::Target),
            endpoint_face(after_edge, after_nodes, Endpoint::Target),
            before_edge
                .target_port
                .as_ref()
                .map(|port| port.face.as_str()),
            after_edge
                .target_port
                .as_ref()
                .map(|port| port.face.as_str()),
        ),
    ] {
        let before_diverged = path_port_diverged(before_path_face.as_str(), before_port_face);
        let after_diverged = path_port_diverged(after_path_face.as_str(), after_port_face);
        if before_diverged != after_diverged {
            events.push(edge_change_for_match_with_evidence(
                ChangeKind::PathPortDivergenceChanged,
                edge_match,
                vec![format!(
                    "path_port_divergence {endpoint} {before_diverged}->{after_diverged}; visible {before_path_face}->{after_path_face}; logical {endpoint}_port {:?}->{:?}",
                    before_port_face, after_port_face
                )],
            ));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Source,
    Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DerivedEndpointFace {
    Face(String),
    Ambiguous,
    Missing,
}

impl DerivedEndpointFace {
    fn as_str(&self) -> &str {
        match self {
            Self::Face(face) => face.as_str(),
            Self::Ambiguous => "ambiguous",
            Self::Missing => "missing",
        }
    }
}

impl std::fmt::Display for DerivedEndpointFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy)]
struct PathDelta {
    point_count_delta: isize,
    bend_count_delta: isize,
    length_delta: f64,
    envelope_changed: bool,
    changed: bool,
}

fn path_delta(before: &Edge, after: &Edge) -> PathDelta {
    let before_path = before.path.as_deref().unwrap_or(&[]);
    let after_path = after.path.as_deref().unwrap_or(&[]);
    let point_count_delta = after_path.len() as isize - before_path.len() as isize;
    let bend_count_delta = bend_count(after_path) as isize - bend_count(before_path) as isize;
    let length_delta = path_length(after_path) - path_length(before_path);
    let envelope_changed = path_envelope_changed(before_path, after_path);
    let changed = point_count_delta != 0
        || bend_count_delta != 0
        || length_delta.abs() > DISPLAY_EPS
        || envelope_changed;

    PathDelta {
        point_count_delta,
        bend_count_delta,
        length_delta,
        envelope_changed,
        changed,
    }
}

fn path_length(path: &[[f64; 2]]) -> f64 {
    path.windows(2)
        .map(|window| {
            let dx = window[1][0] - window[0][0];
            let dy = window[1][1] - window[0][1];
            dx.hypot(dy)
        })
        .sum()
}

fn bend_count(path: &[[f64; 2]]) -> usize {
    path.windows(3)
        .filter(|window| {
            let first = segment_axis(window[0], window[1]);
            let second = segment_axis(window[1], window[2]);
            first != SegmentAxis::Point && second != SegmentAxis::Point && first != second
        })
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentAxis {
    Horizontal,
    Vertical,
    Diagonal,
    Point,
}

fn segment_axis(a: [f64; 2], b: [f64; 2]) -> SegmentAxis {
    let dx = (b[0] - a[0]).abs();
    let dy = (b[1] - a[1]).abs();

    match (dx <= COORD_EPS, dy <= COORD_EPS) {
        (true, true) => SegmentAxis::Point,
        (false, true) => SegmentAxis::Horizontal,
        (true, false) => SegmentAxis::Vertical,
        (false, false) => SegmentAxis::Diagonal,
    }
}

fn path_envelope_changed(before: &[[f64; 2]], after: &[[f64; 2]]) -> bool {
    match (path_bounds(before), path_bounds(after)) {
        (Some(before), Some(after)) => bounds_changed(&before, &after),
        (None, None) => false,
        _ => true,
    }
}

fn path_bounds(path: &[[f64; 2]]) -> Option<Bounds> {
    let first = path.first()?;
    let (mut min_x, mut max_x) = (first[0], first[0]);
    let (mut min_y, mut max_y) = (first[1], first[1]);

    for point in path.iter().skip(1) {
        min_x = min_x.min(point[0]);
        max_x = max_x.max(point[0]);
        min_y = min_y.min(point[1]);
        max_y = max_y.max(point[1]);
    }

    Some(Bounds {
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

fn endpoint_face(
    edge: &Edge,
    nodes: &BTreeMap<String, &Node>,
    endpoint: Endpoint,
) -> DerivedEndpointFace {
    let Some(path) = &edge.path else {
        return DerivedEndpointFace::Missing;
    };
    let point = match endpoint {
        Endpoint::Source => path.first(),
        Endpoint::Target => path.last(),
    };
    let Some(point) = point else {
        return DerivedEndpointFace::Missing;
    };
    let node_id = match endpoint {
        Endpoint::Source => &edge.source,
        Endpoint::Target => &edge.target,
    };
    let Some(node) = nodes.get(node_id) else {
        return DerivedEndpointFace::Missing;
    };

    derive_face_for_point(*point, node)
}

fn derive_face_for_point(point: [f64; 2], node: &Node) -> DerivedEndpointFace {
    let left = node.position.x - node.size.width / 2.0;
    let right = node.position.x + node.size.width / 2.0;
    let top = node.position.y - node.size.height / 2.0;
    let bottom = node.position.y + node.size.height / 2.0;
    let candidates = [
        ("left", (point[0] - left).abs()),
        ("right", (point[0] - right).abs()),
        ("top", (point[1] - top).abs()),
        ("bottom", (point[1] - bottom).abs()),
    ]
    .into_iter()
    .filter(|(_, distance)| *distance <= COORD_EPS)
    .map(|(face, _)| face)
    .collect::<Vec<_>>();

    match candidates.as_slice() {
        [] => DerivedEndpointFace::Missing,
        [face] => DerivedEndpointFace::Face((*face).to_string()),
        _ => DerivedEndpointFace::Ambiguous,
    }
}

fn same_port_intent(before: Option<&Port>, after: Option<&Port>) -> bool {
    match (before, after) {
        (None, None) => true,
        (Some(before), Some(after)) => {
            before.face == after.face
                && (before.fraction - after.fraction).abs() <= COORD_EPS
                && before.group_size == after.group_size
        }
        _ => false,
    }
}

fn port_summary(port: Option<&Port>) -> String {
    port.map(|port| {
        format!(
            "{}@{:.3}/{}",
            port.face.as_str(),
            port.fraction,
            port.group_size
        )
    })
    .unwrap_or_else(|| "none".to_string())
}

fn path_port_diverged(path_face: &str, port_face: Option<&str>) -> bool {
    match (path_face, port_face) {
        ("ambiguous", Some(_)) => true,
        ("missing", _) | (_, None) => false,
        (face, Some(port)) => face != port,
    }
}

fn rect_center(rect: &Rect) -> (f64, f64) {
    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn option_position_moved(before: Option<&Position>, after: Option<&Position>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            let dx = after.x - before.x;
            let dy = after.y - before.y;
            dx.hypot(dy) > DISPLAY_EPS
        }
        (None, None) => false,
        _ => true,
    }
}

fn bounds_changed(before: &Bounds, after: &Bounds) -> bool {
    (after.width - before.width).abs() > DISPLAY_EPS
        || (after.height - before.height).abs() > DISPLAY_EPS
}

fn bounds_evidence(label: &str, before: &Bounds, after: &Bounds) -> String {
    format!(
        "{label} width_delta={:.2}; height_delta={:.2}",
        after.width - before.width,
        after.height - before.height
    )
}

fn option_bounds_changed(before: Option<&Bounds>, after: Option<&Bounds>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => bounds_changed(before, after),
        (None, None) => false,
        _ => true,
    }
}

fn option_bounds_evidence(label: &str, before: Option<&Bounds>, after: Option<&Bounds>) -> String {
    match (before, after) {
        (Some(before), Some(after)) => bounds_evidence(label, before, after),
        (None, Some(after)) => {
            format!(
                "{label} added width={:.2}; height={:.2}",
                after.width, after.height
            )
        }
        (Some(before), None) => {
            format!(
                "{label} removed width={:.2}; height={:.2}",
                before.width, before.height
            )
        }
        (None, None) => format!("{label} unchanged"),
    }
}

fn link_related_geometry(events: &mut [Change]) {
    let geometry_events = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind.is_geometry())
        .map(|(index, event)| {
            (
                index,
                event.subject.clone(),
                event.edge_ids.clone(),
                event.kind,
            )
        })
        .collect::<Vec<_>>();

    for event in events.iter_mut() {
        if !event.kind.can_have_related_geometry() {
            continue;
        }

        for (index, subject, edge_ids, kind) in &geometry_events {
            // Edge ids are positional, so an edge subject alone can name different
            // edges on the two sides; edges must also share their before/after ids.
            if *subject == event.subject && *edge_ids == event.edge_ids {
                event.related_change_ids.push(*index);
                event
                    .evidence
                    .push(format!("related_geometry change_id={index}; kind={kind:?}"));
            }
        }
    }
}

fn node_style_payload<'a>(output: &'a Document, node_id: &str) -> Option<&'a Value> {
    output
        .extensions
        .get(NODE_STYLE_EXTENSION_NAMESPACE)?
        .get("nodes")?
        .as_object()?
        .get(node_id)
}

fn edge_style_changed(before: &Edge, after: &Edge) -> bool {
    before.stroke != after.stroke
        || before.arrow_start != after.arrow_start
        || before.arrow_end != after.arrow_end
        || before.minlen != after.minlen
}

fn string_set(values: &[String]) -> BTreeSet<&str> {
    values.iter().map(String::as_str).collect()
}

fn node_change(kind: ChangeKind, id: &str) -> Change {
    node_change_with_evidence(kind, id, Vec::new())
}

fn node_change_with_evidence(kind: ChangeKind, id: &str, evidence: Vec<String>) -> Change {
    Change {
        kind,
        subject: Subject::Node(id.to_string()),
        evidence,
        related_change_ids: Vec::new(),
        edge_ids: None,
    }
}

fn edge_change(
    kind: ChangeKind,
    subject_id: &str,
    before_id: Option<&str>,
    after_id: Option<&str>,
) -> Change {
    edge_change_with_evidence(kind, subject_id, before_id, after_id, Vec::new())
}

fn edge_change_with_evidence(
    kind: ChangeKind,
    subject_id: &str,
    before_id: Option<&str>,
    after_id: Option<&str>,
    evidence: Vec<String>,
) -> Change {
    Change {
        kind,
        subject: Subject::Edge(subject_id.to_string()),
        evidence,
        related_change_ids: Vec::new(),
        edge_ids: Some(EdgeIds {
            before_id: before_id.map(str::to_string),
            after_id: after_id.map(str::to_string),
        }),
    }
}

fn edge_change_for_match_with_evidence(
    kind: ChangeKind,
    edge_match: &EdgeMatch<'_>,
    mut evidence: Vec<String>,
) -> Change {
    evidence.extend(edge_match_evidence(edge_match));
    edge_change_with_evidence(
        kind,
        &edge_match.after_id,
        Some(&edge_match.before_id),
        Some(&edge_match.after_id),
        evidence,
    )
}

fn subgraph_change(kind: ChangeKind, id: &str) -> Change {
    subgraph_change_with_evidence(kind, id, Vec::new())
}

fn subgraph_change_with_evidence(kind: ChangeKind, id: &str, evidence: Vec<String>) -> Change {
    Change {
        kind,
        subject: Subject::Subgraph(id.to_string()),
        evidence,
        related_change_ids: Vec::new(),
        edge_ids: None,
    }
}
