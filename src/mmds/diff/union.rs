//! Union documents: both sides of a snapshot diff in one MMDS document.
//!
//! [`union_document`] holds every node, edge and subgraph of the `after` document plus
//! the items the `before` document had and `after` dropped ("ghosts"). Each item the
//! diff touched is tagged under the [`EXTENSION_NAMESPACE`] extension as `added`,
//! `removed` or `changed`, and by default styled through the node style extension so
//! text and SVG renderers draw the highlight without extra configuration.
//!
//! Union edges get their own ids. MMDS edge ids are positional, so `before` `e0` and
//! `after` `e0` can be different edges, and imported documents may carry any ids at
//! all. Union edges are numbered densely, `e0..eN`, in union order: the `after` edges
//! in `after` order, then the removed edges in `before` order. For documents mmdflux
//! produced, surviving and added edges therefore keep their `after` ids. The
//! extension records every union edge's `before_id` and `after_id`, and edge styles
//! are keyed by union id. Dense ids in array order are what a relayout assigns, so
//! the tags still apply to a relaid-out union.
//!
//! The union keeps the geometry each item had on its own side, which does not form a
//! coherent layout. Lay it out again before rendering, for example with
//! [`crate::render_document_with_relayout`].
//!
//! Extension payload:
//!
//! ```json
//! {
//!   "nodes": { "D": "added", "X": "removed", "C": "changed" },
//!   "edges": {
//!     "e0": { "before_id": "e0", "after_id": "e0" },
//!     "e2": { "status": "added", "before_id": null, "after_id": "e2" },
//!     "e3": { "status": "removed", "before_id": "e2", "after_id": null }
//!   },
//!   "subgraphs": { "S": "changed" }
//! }
//! ```

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{Map, Value, json};

use super::{
    ChangeKind, DERIVED_RENDER_EXTENSION_PREFIX, Diff, diff_documents, edge_correspondences,
    edges_by_id, nodes_by_id,
};
use crate::graph::{GeometryLevel, Stroke};
use crate::mmds::{
    Document, NODE_STYLE_EXTENSION_NAMESPACE, NODE_STYLE_PROFILE, Subject,
    TEXT_MEASUREMENTS_EXTENSION_NAMESPACE,
};

/// Extension namespace carrying union tags and edge correspondence.
pub const EXTENSION_NAMESPACE: &str = "org.mmdflux.diff.v1";

/// Default stroke and text color for added items.
pub const ADDED_COLOR: &str = "#2ea043";
/// Default stroke and text color for changed items.
pub const CHANGED_COLOR: &str = "#d29922";
/// Default stroke and text color for removed items.
pub const REMOVED_COLOR: &str = "#8b949e";

/// Dash pattern for removed nodes in SVG.
const REMOVED_DASHARRAY: &str = "5 5";

/// Options for [`union_document`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnionOptions {
    /// Apply the default added, changed and removed styles. On by default.
    pub style: bool,
    /// Prefix labels with `+ `, `~ ` and `- ` so the union reads without color.
    pub markers: bool,
}

impl Default for UnionOptions {
    fn default() -> Self {
        Self {
            style: true,
            markers: false,
        }
    }
}

impl UnionOptions {
    /// Set whether the default styles are applied.
    #[must_use]
    pub fn with_style(mut self, style: bool) -> Self {
        self.style = style;
        self
    }

    /// Set whether labels get `+ `, `~ ` and `- ` markers.
    #[must_use]
    pub fn with_markers(mut self, markers: bool) -> Self {
        self.markers = markers;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Added,
    Removed,
    Changed,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Added => ADDED_COLOR,
            Self::Removed => REMOVED_COLOR,
            Self::Changed => CHANGED_COLOR,
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Added => "+ ",
            Self::Removed => "- ",
            Self::Changed => "~ ",
        }
    }
}

/// Build the union of `before` and `after`, tagged with what changed between them.
///
/// The result is a layout-level document whose metadata comes from `after`. Removed
/// nodes and subgraphs keep their `before` label, shape and parent when that parent is
/// in the union; otherwise they move to the top level.
pub fn union_document(before: &Document, after: &Document, options: &UnionOptions) -> Document {
    let diff = diff_documents(before, after);
    let tags = Tags::from_diff(&diff);

    let mut union = after.clone();
    union.extensions.retain(|namespace, _| {
        !namespace.starts_with(DERIVED_RENDER_EXTENSION_PREFIX)
            && namespace != TEXT_MEASUREMENTS_EXTENSION_NAMESPACE
    });

    // Subgraphs first, so removed nodes can find their surviving parents.
    let after_subgraph_ids: HashSet<&str> =
        after.subgraphs.iter().map(|sg| sg.id.as_str()).collect();
    let removed_subgraphs: Vec<_> = before
        .subgraphs
        .iter()
        .filter(|sg| !after_subgraph_ids.contains(sg.id.as_str()))
        .collect();
    let union_subgraph_ids: HashSet<String> = after_subgraph_ids
        .iter()
        .map(|id| id.to_string())
        .chain(removed_subgraphs.iter().map(|sg| sg.id.clone()))
        .collect();
    let surviving_parent = |parent: &Option<String>| {
        parent
            .as_ref()
            .filter(|parent| union_subgraph_ids.contains(parent.as_str()))
            .cloned()
    };
    for subgraph in &removed_subgraphs {
        let mut ghost = (*subgraph).clone();
        ghost.parent = surviving_parent(&subgraph.parent);
        union.subgraphs.push(ghost);
    }
    restore_removed_concurrent_regions(&mut union, before, &removed_subgraphs);

    let after_node_ids: HashSet<&str> = after.nodes.iter().map(|node| node.id.as_str()).collect();
    for node in before
        .nodes
        .iter()
        .filter(|node| !after_node_ids.contains(node.id.as_str()))
    {
        let mut ghost = node.clone();
        ghost.parent = surviving_parent(&node.parent);
        union.nodes.push(ghost);
    }
    rebuild_subgraph_children(&mut union);

    let before_edges = edges_by_id(before);
    let after_edges = edges_by_id(after);
    let correspondences = edge_correspondences(
        &before_edges,
        &after_edges,
        &nodes_by_id(before),
        &nodes_by_id(after),
    );
    let before_id_for_after: BTreeMap<&str, &str> = correspondences
        .matches
        .iter()
        .map(|edge_match| (edge_match.after_id.as_str(), edge_match.before_id.as_str()))
        .collect();

    // `after` edges first, renumbered densely in `after` order; their styles follow
    // them to the union ids.
    let after_edge_styles = style_section(after, "edges").cloned();
    let mut union_edge_styles = Map::new();
    let mut edge_entries = Map::new();
    let mut edge_statuses: Vec<Option<Status>> = Vec::new();
    for (index, edge) in after.edges.iter().enumerate() {
        let union_id = format!("e{index}");
        let before_id = before_id_for_after.get(edge.id.as_str()).copied();
        let status = if before_id.is_none() {
            Some(Status::Added)
        } else if tags.changed_edges.contains(edge.id.as_str()) {
            Some(Status::Changed)
        } else {
            None
        };
        if let Some(style) = after_edge_styles
            .as_ref()
            .and_then(|styles| styles.get(&edge.id))
        {
            union_edge_styles.insert(union_id.clone(), style.clone());
        }
        edge_entries.insert(
            union_id.clone(),
            edge_entry(status, before_id, Some(edge.id.as_str())),
        );
        union.edges[index].id = union_id;
        edge_statuses.push(status);
    }

    // Removed edges in `before` declaration order, numbered after the `after` edges.
    let removed_ids: BTreeSet<&str> = correspondences
        .removed
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    let before_edge_styles = style_section(before, "edges");
    for edge in before
        .edges
        .iter()
        .filter(|edge| removed_ids.contains(edge.id.as_str()))
    {
        let union_id = format!("e{}", union.edges.len());
        let mut ghost = edge.clone();
        ghost.id = union_id.clone();
        ghost.from_subgraph = surviving_parent(&edge.from_subgraph);
        ghost.to_subgraph = surviving_parent(&edge.to_subgraph);
        if ghost.stroke != Stroke::Invisible {
            ghost.stroke = Stroke::Dotted;
        }
        if let Some(style) = before_edge_styles.and_then(|styles| styles.get(&edge.id)) {
            union_edge_styles.insert(union_id.clone(), style.clone());
        }
        edge_entries.insert(
            union_id,
            edge_entry(Some(Status::Removed), Some(edge.id.as_str()), None),
        );
        union.edges.push(ghost);
        edge_statuses.push(Some(Status::Removed));
    }
    strip_routed_geometry(&mut union);

    let mut node_tags: BTreeMap<String, Status> = tags.nodes.clone();
    for node in &union.nodes {
        if !after_node_ids.contains(node.id.as_str()) {
            node_tags.insert(node.id.clone(), Status::Removed);
        }
    }
    let mut subgraph_tags: BTreeMap<String, Status> = tags.subgraphs.clone();
    for subgraph in &removed_subgraphs {
        subgraph_tags.insert(subgraph.id.clone(), Status::Removed);
    }

    merge_removed_styles(&mut union, before, &node_tags, &subgraph_tags);
    replace_edge_styles(&mut union, union_edge_styles);

    if options.style {
        apply_default_styles(&mut union, &node_tags, &subgraph_tags, &edge_statuses);
    }
    if options.markers {
        apply_markers(&mut union, &node_tags, &subgraph_tags, &edge_statuses);
    }

    if union
        .extensions
        .contains_key(NODE_STYLE_EXTENSION_NAMESPACE)
        && !union
            .profiles
            .iter()
            .any(|profile| profile == NODE_STYLE_PROFILE)
    {
        union.profiles.push(NODE_STYLE_PROFILE.to_string());
    }

    let mut extension = Map::new();
    extension.insert("nodes".to_string(), status_map(&node_tags));
    extension.insert("edges".to_string(), Value::Object(edge_entries));
    extension.insert("subgraphs".to_string(), status_map(&subgraph_tags));
    union
        .extensions
        .insert(EXTENSION_NAMESPACE.to_string(), extension);

    union
}

/// Node, subgraph and edge statuses taken from the snapshot diff's model changes.
struct Tags {
    nodes: BTreeMap<String, Status>,
    subgraphs: BTreeMap<String, Status>,
    changed_edges: HashSet<String>,
}

impl Tags {
    fn from_diff(diff: &Diff) -> Self {
        let mut nodes = BTreeMap::new();
        let mut subgraphs = BTreeMap::new();
        let mut changed_edges = HashSet::new();
        for change in diff.changes.iter().filter(|change| change.kind.is_model()) {
            let status = match change.kind {
                ChangeKind::NodeAdded | ChangeKind::SubgraphAdded | ChangeKind::EdgeAdded => {
                    Status::Added
                }
                ChangeKind::NodeRemoved | ChangeKind::SubgraphRemoved | ChangeKind::EdgeRemoved => {
                    Status::Removed
                }
                _ => Status::Changed,
            };
            match &change.subject {
                Subject::Node(id) => {
                    nodes.entry(id.clone()).or_insert(status);
                }
                Subject::Subgraph(id) => {
                    subgraphs.entry(id.clone()).or_insert(status);
                }
                Subject::Edge(_) if status == Status::Changed => {
                    if let Some(after_id) = change
                        .edge_ids
                        .as_ref()
                        .and_then(|ids| ids.after_id.clone())
                    {
                        changed_edges.insert(after_id);
                    }
                }
                _ => {}
            }
        }
        Self {
            nodes,
            subgraphs,
            changed_edges,
        }
    }
}

fn edge_entry(status: Option<Status>, before_id: Option<&str>, after_id: Option<&str>) -> Value {
    let mut entry = Map::new();
    if let Some(status) = status {
        entry.insert("status".to_string(), json!(status.name()));
    }
    entry.insert("before_id".to_string(), json!(before_id));
    entry.insert("after_id".to_string(), json!(after_id));
    Value::Object(entry)
}

fn status_map(tags: &BTreeMap<String, Status>) -> Value {
    Value::Object(
        tags.iter()
            .map(|(id, status)| (id.clone(), json!(status.name())))
            .collect(),
    )
}

/// Make removed region subgraphs concurrent regions of their union parent again
/// when `before` listed them as regions of that parent.
fn restore_removed_concurrent_regions(
    union: &mut Document,
    before: &Document,
    removed_subgraphs: &[&crate::mmds::Subgraph],
) {
    for removed in removed_subgraphs {
        let Some(parent_id) = removed.parent.as_deref() else {
            continue;
        };
        let was_region = before
            .subgraphs
            .iter()
            .any(|sg| sg.id == parent_id && sg.concurrent_regions.contains(&removed.id));
        if !was_region {
            continue;
        }
        if let Some(parent) = union.subgraphs.iter_mut().find(|sg| sg.id == parent_id)
            && !parent.concurrent_regions.contains(&removed.id)
        {
            parent.concurrent_regions.push(removed.id.clone());
        }
    }
}

/// Recompute subgraph membership from the union's parent links.
///
/// MMDS `children` lists a subgraph's direct nodes only; nested subgraphs belong
/// to their parent through their own `parent` link. Each subgraph keeps its own
/// child order, gains ghost nodes that joined it, and keeps only the concurrent
/// regions that are still its child subgraphs.
fn rebuild_subgraph_children(union: &mut Document) {
    let node_parents: BTreeMap<String, Option<String>> = union
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.parent.clone()))
        .collect();
    let subgraph_parents: BTreeMap<String, Option<String>> = union
        .subgraphs
        .iter()
        .map(|sg| (sg.id.clone(), sg.parent.clone()))
        .collect();
    let node_order: Vec<String> = union.nodes.iter().map(|node| node.id.clone()).collect();

    for subgraph in &mut union.subgraphs {
        let id = subgraph.id.as_str();
        let is_child_node =
            |child: &String| node_parents.get(child).cloned().flatten().as_deref() == Some(id);
        let is_child_subgraph =
            |child: &String| subgraph_parents.get(child).cloned().flatten().as_deref() == Some(id);
        let mut children: Vec<String> = subgraph
            .children
            .iter()
            .filter(|child| is_child_node(child))
            .cloned()
            .collect();
        for node in &node_order {
            if is_child_node(node) && !children.contains(node) {
                children.push(node.clone());
            }
        }
        let regions: Vec<String> = subgraph
            .concurrent_regions
            .iter()
            .filter(|region| is_child_subgraph(region))
            .cloned()
            .collect();
        subgraph.children = children;
        subgraph.concurrent_regions = regions;
    }
}

/// Replace the union's edge styles with `styles`, keyed by union edge id.
fn replace_edge_styles(union: &mut Document, styles: Map<String, Value>) {
    if styles.is_empty() {
        if let Some(extension) = union.extensions.get_mut(NODE_STYLE_EXTENSION_NAMESPACE) {
            extension.remove("edges");
            if extension.is_empty() {
                union.extensions.remove(NODE_STYLE_EXTENSION_NAMESPACE);
            }
        }
        return;
    }
    *style_section_mut(union, "edges") = styles;
}

fn style_section<'a>(document: &'a Document, section: &str) -> Option<&'a Map<String, Value>> {
    document
        .extensions
        .get(NODE_STYLE_EXTENSION_NAMESPACE)?
        .get(section)?
        .as_object()
}

fn style_section_mut<'a>(document: &'a mut Document, section: &str) -> &'a mut Map<String, Value> {
    let extension = document
        .extensions
        .entry(NODE_STYLE_EXTENSION_NAMESPACE.to_string())
        .or_default();
    let entry = extension
        .entry(section.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("style section is an object")
}

/// Carry the `before` styles of removed nodes and subgraphs into the union.
fn merge_removed_styles(
    union: &mut Document,
    before: &Document,
    node_tags: &BTreeMap<String, Status>,
    subgraph_tags: &BTreeMap<String, Status>,
) {
    for (section, tags) in [("nodes", node_tags), ("subgraphs", subgraph_tags)] {
        let Some(before_styles) = style_section(before, section) else {
            continue;
        };
        let removed: Vec<(String, Value)> = tags
            .iter()
            .filter(|(_, status)| **status == Status::Removed)
            .filter_map(|(id, _)| Some((id.clone(), before_styles.get(id)?.clone())))
            .collect();
        if !removed.is_empty() {
            style_section_mut(union, section).extend(removed);
        }
    }
}

fn set_style(entry: &mut Value, key: &str, value: &str) {
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry
        .as_object_mut()
        .expect("style entry is an object")
        .insert(key.to_string(), json!(value));
}

/// Stroke and text colors per status; no fills, so light and dark backgrounds both work.
fn apply_default_styles(
    union: &mut Document,
    node_tags: &BTreeMap<String, Status>,
    subgraph_tags: &BTreeMap<String, Status>,
    edge_statuses: &[Option<Status>],
) {
    for (section, tags) in [("nodes", node_tags), ("subgraphs", subgraph_tags)] {
        if tags.is_empty() {
            continue;
        }
        let styles = style_section_mut(union, section);
        for (id, status) in tags {
            let entry = styles.entry(id.clone()).or_insert_with(|| json!({}));
            set_style(entry, "stroke", status.color());
            set_style(entry, "color", status.color());
            if *status == Status::Removed && section == "nodes" {
                set_style(entry, "stroke-dasharray", REMOVED_DASHARRAY);
            }
        }
    }

    let edge_ids: Vec<(String, Status)> = union
        .edges
        .iter()
        .zip(edge_statuses)
        .filter_map(|(edge, status)| Some((edge.id.clone(), (*status)?)))
        .collect();
    if edge_ids.is_empty() {
        return;
    }
    let styles = style_section_mut(union, "edges");
    for (id, status) in edge_ids {
        let entry = styles.entry(id).or_insert_with(|| json!({}));
        set_style(entry, "stroke", status.color());
    }
}

fn apply_markers(
    union: &mut Document,
    node_tags: &BTreeMap<String, Status>,
    subgraph_tags: &BTreeMap<String, Status>,
    edge_statuses: &[Option<Status>],
) {
    for node in &mut union.nodes {
        if let Some(status) = node_tags.get(&node.id) {
            node.label = format!("{}{}", status.marker(), node.label);
        }
    }
    for subgraph in &mut union.subgraphs {
        if let Some(status) = subgraph_tags.get(&subgraph.id) {
            subgraph.title = format!("{}{}", status.marker(), subgraph.title);
        }
    }
    for (edge, status) in union.edges.iter_mut().zip(edge_statuses) {
        if let (Some(status), Some(label)) = (status, edge.label.as_mut()) {
            *label = format!("{}{label}", status.marker());
        }
    }
}

/// Each side's routed geometry belongs to its own layout; keep layout-level fields only.
fn strip_routed_geometry(union: &mut Document) {
    union.geometry_level = GeometryLevel::Layout;
    union.metadata.diagnostics = None;
    for edge in &mut union.edges {
        edge.path = None;
        edge.label_position = None;
        edge.is_backward = None;
        edge.source_port = None;
        edge.target_port = None;
        edge.label_rect = None;
    }
    for subgraph in &mut union.subgraphs {
        subgraph.bounds = None;
    }
}
