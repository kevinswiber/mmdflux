//! Integrity facts about a painted graph drawing.
//!
//! The audit records what a reader would call a broken drawing (missing
//! labels, lost arrowheads, rank order violations, overlapping nodes and
//! displaced edge labels) so a compacted candidate can be compared against
//! the as-authored drawing. Facts are computed on the painted canvas of the
//! graph actually rendered, so a candidate with rewritten labels is checked
//! against its own labels.

use std::collections::{BTreeMap, BTreeSet};

use super::edge::{PlacedEdgeLabel, arrow_glyph, exit_direction_from_segments};
use super::label_util::effective_edge_label;
use crate::graph::grid::{AttachDirection, GridLayout, NodeBounds, Point, RoutedEdge};
use crate::graph::{Arrow, Direction, Graph, Node, Stroke};
use crate::render::text::{Canvas, CharSet};

/// Broken-drawing facts computed on a painted canvas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DrawingAudit {
    /// Nodes with a label line not found inside the node's box.
    pub(crate) missing_node_labels: BTreeSet<String>,
    /// Visible labeled edges with a label word absent from the drawing.
    pub(crate) missing_edge_label_words: BTreeSet<usize>,
    /// Edges whose endpoint boxes are not separated in rank order.
    pub(crate) rank_order_violations: BTreeSet<usize>,
    /// Node pairs (lexicographically ordered) whose boxes intersect.
    pub(crate) node_overlaps: BTreeSet<(String, String)>,
    /// Per node, the number of arrowheads drawn into it.
    pub(crate) arrows_in: BTreeMap<String, usize>,
    /// Edges whose placed label sits outside the endpoints' rank span.
    pub(crate) displaced_edge_labels: BTreeSet<usize>,
}

/// A fact present in a candidate drawing but not in the as-authored one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // compared by the runtime width fit
pub(crate) enum AuditRegression {
    NodeLabelMissing(String),
    EdgeLabelWordMissing(usize),
    RankOrder(usize),
    NodeOverlap(String, String),
    ArrowLost {
        node: String,
        authored: usize,
        candidate: usize,
    },
    EdgeLabelDisplaced(usize),
}

#[cfg_attr(not(test), allow(dead_code))] // compared by the runtime width fit
impl DrawingAudit {
    /// Facts this audit has that `authored` does not, plus nodes whose
    /// arrowhead count dropped. Nodes absent from either audit are ignored.
    pub(crate) fn regressions_against(&self, authored: &DrawingAudit) -> Vec<AuditRegression> {
        let mut regressions = Vec::new();
        regressions.extend(
            self.missing_node_labels
                .difference(&authored.missing_node_labels)
                .cloned()
                .map(AuditRegression::NodeLabelMissing),
        );
        regressions.extend(
            self.missing_edge_label_words
                .difference(&authored.missing_edge_label_words)
                .copied()
                .map(AuditRegression::EdgeLabelWordMissing),
        );
        regressions.extend(
            self.rank_order_violations
                .difference(&authored.rank_order_violations)
                .copied()
                .map(AuditRegression::RankOrder),
        );
        regressions.extend(
            self.node_overlaps
                .difference(&authored.node_overlaps)
                .cloned()
                .map(|(a, b)| AuditRegression::NodeOverlap(a, b)),
        );
        for (node, &candidate) in &self.arrows_in {
            if let Some(&authored_count) = authored.arrows_in.get(node)
                && candidate < authored_count
            {
                regressions.push(AuditRegression::ArrowLost {
                    node: node.clone(),
                    authored: authored_count,
                    candidate,
                });
            }
        }
        regressions.extend(
            self.displaced_edge_labels
                .difference(&authored.displaced_edge_labels)
                .copied()
                .map(AuditRegression::EdgeLabelDisplaced),
        );
        regressions
    }
}

/// Everything the audit reads from one paint.
pub(crate) struct AuditInputs<'a> {
    pub(crate) diagram: &'a Graph,
    pub(crate) layout: &'a GridLayout,
    pub(crate) canvas: &'a Canvas,
    pub(crate) charset: &'a CharSet,
    pub(crate) routed_edges: &'a [RoutedEdge],
    pub(crate) placed_labels: &'a [PlacedEdgeLabel],
}

/// Compute the audit facts for a painted drawing.
pub(crate) fn audit_drawing(inputs: &AuditInputs<'_>) -> DrawingAudit {
    DrawingAudit {
        missing_node_labels: missing_node_labels(inputs),
        missing_edge_label_words: missing_edge_label_words(inputs),
        rank_order_violations: rank_order_violations(inputs.diagram, inputs.layout),
        node_overlaps: node_overlaps(inputs.layout),
        arrows_in: arrows_in(inputs),
        displaced_edge_labels: displaced_edge_labels(inputs),
    }
}

/// Owner cells of one canvas row between `x0` (inclusive) and `x1`
/// (exclusive), continuation cells skipped.
fn row_text(canvas: &Canvas, y: usize, x0: usize, x1: usize) -> String {
    (x0..x1)
        .filter_map(|x| canvas.get(x, y))
        .filter(|cell| !cell.is_continuation)
        .map(|cell| cell.ch)
        .collect()
}

fn missing_node_labels(inputs: &AuditInputs<'_>) -> BTreeSet<String> {
    let mut missing = BTreeSet::new();
    for (id, bounds) in &inputs.layout.node_bounds {
        let Some(node) = inputs.diagram.nodes.get(id) else {
            continue;
        };
        let rows: Vec<String> = (bounds.y..bounds.y + bounds.height)
            .map(|y| row_text(inputs.canvas, y, bounds.x, bounds.x + bounds.width))
            .collect();
        let lost = node
            .label
            .split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty() && *line != Node::SEPARATOR)
            .any(|line| !rows.iter().any(|row| row.contains(line)));
        if lost {
            missing.insert(id.clone());
        }
    }
    missing
}

fn painted_text(canvas: &Canvas) -> String {
    (0..canvas.height())
        .map(|y| row_text(canvas, y, 0, canvas.width()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn missing_edge_label_words(inputs: &AuditInputs<'_>) -> BTreeSet<usize> {
    let mut missing = BTreeSet::new();
    let mut painted: Option<String> = None;
    for edge in &inputs.diagram.edges {
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        let Some(label) = effective_edge_label(edge) else {
            continue;
        };
        let text = painted.get_or_insert_with(|| painted_text(inputs.canvas));
        if label.split_whitespace().any(|word| !text.contains(word)) {
            missing.insert(edge.index);
        }
    }
    missing
}

/// Primary-axis span `(start, end_exclusive)` of a node box.
fn primary_span(bounds: &NodeBounds, direction: Direction) -> (usize, usize) {
    match direction {
        Direction::TopDown | Direction::BottomTop => (bounds.y, bounds.y + bounds.height),
        Direction::LeftRight | Direction::RightLeft => (bounds.x, bounds.x + bounds.width),
    }
}

pub(crate) fn rank_order_violations(diagram: &Graph, layout: &GridLayout) -> BTreeSet<usize> {
    let root = diagram.direction;
    let mut violations = BTreeSet::new();
    for edge in &diagram.edges {
        if edge.from == edge.to {
            continue;
        }
        let uses_root = |id: &str| layout.node_directions.get(id) == Some(&root);
        if !uses_root(&edge.from) || !uses_root(&edge.to) {
            continue;
        }
        let (Some(from_pos), Some(to_pos)) = (
            layout.grid_positions.get(&edge.from),
            layout.grid_positions.get(&edge.to),
        ) else {
            continue;
        };
        if from_pos.layer == to_pos.layer {
            continue;
        }
        let (Some(from_bounds), Some(to_bounds)) = (
            layout.node_bounds.get(&edge.from),
            layout.node_bounds.get(&edge.to),
        ) else {
            continue;
        };
        let (lower, higher) = if from_pos.layer < to_pos.layer {
            (from_bounds, to_bounds)
        } else {
            (to_bounds, from_bounds)
        };
        let (_, lower_end) = primary_span(lower, root);
        let (higher_start, _) = primary_span(higher, root);
        if lower_end > higher_start {
            violations.insert(edge.index);
        }
    }
    violations
}

fn boxes_intersect(a: &NodeBounds, b: &NodeBounds) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

pub(crate) fn node_overlaps(layout: &GridLayout) -> BTreeSet<(String, String)> {
    let mut ids: Vec<&String> = layout.node_bounds.keys().collect();
    ids.sort();
    let mut overlaps = BTreeSet::new();
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            if boxes_intersect(&layout.node_bounds[*a], &layout.node_bounds[*b]) {
                overlaps.insert(((*a).clone(), (*b).clone()));
            }
        }
    }
    overlaps
}

fn contains_point(bounds: &NodeBounds, x: usize, y: usize) -> bool {
    x >= bounds.x && x < bounds.x + bounds.width && y >= bounds.y && y < bounds.y + bounds.height
}

/// Whether the arrowhead the paint path draws at `point` (type `arrow`,
/// entering along `direction`) is still on the canvas, points into `node`'s
/// box, and is fed by a cell outside every node box.
fn arrow_lands(
    inputs: &AuditInputs<'_>,
    point: &Point,
    arrow: Arrow,
    direction: AttachDirection,
    node: &str,
) -> bool {
    let (Some(bounds), Some(glyph)) = (
        inputs.layout.node_bounds.get(node),
        arrow_glyph(inputs.charset, arrow, direction),
    ) else {
        return false;
    };
    if inputs.canvas.get(point.x, point.y).map(|cell| cell.ch) != Some(glyph) {
        return false;
    }
    let (dx, dy): (isize, isize) = match direction {
        AttachDirection::Top => (0, 1),
        AttachDirection::Bottom => (0, -1),
        AttachDirection::Left => (1, 0),
        AttachDirection::Right => (-1, 0),
    };
    let ahead = point
        .x
        .checked_add_signed(dx)
        .zip(point.y.checked_add_signed(dy));
    let behind = point
        .x
        .checked_add_signed(-dx)
        .zip(point.y.checked_add_signed(-dy))
        .and_then(|(x, y)| inputs.canvas.get(x, y));
    ahead.is_some_and(|(x, y)| contains_point(bounds, x, y))
        && behind.is_some_and(|cell| !cell.is_node)
}

fn arrows_in(inputs: &AuditInputs<'_>) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for id in inputs.layout.node_bounds.keys() {
        if inputs.diagram.nodes.contains_key(id) {
            counts.insert(id.clone(), 0);
        }
    }
    for routed in inputs.routed_edges {
        let edge = &routed.edge;
        if edge.stroke == Stroke::Invisible {
            continue;
        }
        if edge.arrow_end != Arrow::None
            && arrow_lands(
                inputs,
                &routed.end,
                edge.arrow_end,
                routed.entry_direction,
                &edge.to,
            )
        {
            *counts.entry(edge.to.clone()).or_default() += 1;
        }
        if edge.arrow_start != Arrow::None && !routed.is_self_edge {
            // The paint path orients the source arrowhead by the launch
            // direction; one step along it from the start lies in the source.
            let exit = exit_direction_from_segments(&routed.segments);
            if arrow_lands(inputs, &routed.start, edge.arrow_start, exit, &edge.from) {
                *counts.entry(edge.from.clone()).or_default() += 1;
            }
        }
    }
    counts
}

fn displaced_edge_labels(inputs: &AuditInputs<'_>) -> BTreeSet<usize> {
    let root = inputs.diagram.direction;
    let mut displaced = BTreeSet::new();
    for placed in inputs.placed_labels {
        let Some(edge) = inputs.diagram.edges.get(placed.edge_index) else {
            continue;
        };
        if edge.from == edge.to {
            continue;
        }
        let (Some(from), Some(to)) = (
            inputs.layout.node_bounds.get(&edge.from),
            inputs.layout.node_bounds.get(&edge.to),
        ) else {
            continue;
        };
        let (from_start, from_end) = primary_span(from, root);
        let (to_start, to_end) = primary_span(to, root);
        let low = from_start.min(to_start).saturating_sub(1);
        // Spans are end-exclusive; the last cell is `end - 1`, widened by 1.
        let high = from_end.max(to_end);
        let center = match root {
            Direction::TopDown | Direction::BottomTop => placed.y + placed.height / 2,
            Direction::LeftRight | Direction::RightLeft => placed.x + placed.width / 2,
        };
        if center < low || center > high {
            displaced.insert(placed.edge_index);
        }
    }
    displaced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit_with_arrows(entries: &[(&str, usize)]) -> DrawingAudit {
        DrawingAudit {
            arrows_in: entries
                .iter()
                .map(|(id, count)| (id.to_string(), *count))
                .collect(),
            ..DrawingAudit::default()
        }
    }

    #[test]
    fn identical_audits_have_no_regressions() {
        let mut audit = audit_with_arrows(&[("A", 1), ("B", 2)]);
        audit.missing_node_labels.insert("C".into());
        audit.rank_order_violations.insert(3);
        assert!(audit.regressions_against(&audit.clone()).is_empty());
    }

    #[test]
    fn new_missing_node_label_is_a_regression() {
        let authored = DrawingAudit::default();
        let mut candidate = DrawingAudit::default();
        candidate.missing_node_labels.insert("A".into());
        assert_eq!(
            candidate.regressions_against(&authored),
            vec![AuditRegression::NodeLabelMissing("A".into())]
        );
    }

    #[test]
    fn arrow_count_drop_is_a_regression_and_missing_nodes_are_ignored() {
        let authored = audit_with_arrows(&[("A", 2), ("B", 1)]);
        let candidate = audit_with_arrows(&[("A", 1), ("C", 0)]);
        assert_eq!(
            candidate.regressions_against(&authored),
            vec![AuditRegression::ArrowLost {
                node: "A".into(),
                authored: 2,
                candidate: 1,
            }]
        );
    }

    #[test]
    fn overlap_present_in_both_audits_is_not_a_regression() {
        let mut authored = DrawingAudit::default();
        authored.node_overlaps.insert(("A".into(), "B".into()));
        let mut candidate = authored.clone();
        candidate.node_overlaps.insert(("B".into(), "C".into()));
        assert_eq!(
            candidate.regressions_against(&authored),
            vec![AuditRegression::NodeOverlap("B".into(), "C".into())]
        );
    }

    fn bounds(x: usize, y: usize, width: usize, height: usize) -> NodeBounds {
        NodeBounds {
            x,
            y,
            width,
            height,
            layout_center_x: None,
            layout_center_y: None,
        }
    }

    fn two_node_layout(direction: Direction, a: NodeBounds, b: NodeBounds) -> (Graph, GridLayout) {
        use crate::graph::Edge;
        use crate::graph::grid::GridPos;

        let mut graph = Graph::new(direction);
        graph.add_node(Node::new("A"));
        graph.add_node(Node::new("B"));
        graph.add_edge(Edge::new("A", "B"));
        let mut layout = GridLayout::default();
        layout.node_bounds.insert("A".into(), a);
        layout.node_bounds.insert("B".into(), b);
        layout
            .grid_positions
            .insert("A".into(), GridPos { layer: 0, pos: 0 });
        layout
            .grid_positions
            .insert("B".into(), GridPos { layer: 1, pos: 0 });
        layout.node_directions.insert("A".into(), direction);
        layout.node_directions.insert("B".into(), direction);
        (graph, layout)
    }

    #[test]
    fn rank_order_holds_for_separated_layers_in_every_direction() {
        for direction in [
            Direction::TopDown,
            Direction::BottomTop,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            let (graph, layout) =
                two_node_layout(direction, bounds(0, 0, 5, 3), bounds(6, 4, 5, 3));
            assert!(
                rank_order_violations(&graph, &layout).is_empty(),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn overlapping_layers_violate_rank_order_bottom_top() {
        let (graph, layout) =
            two_node_layout(Direction::BottomTop, bounds(0, 0, 5, 3), bounds(0, 2, 5, 3));
        assert_eq!(rank_order_violations(&graph, &layout), BTreeSet::from([0]));
    }

    #[test]
    fn overlapping_layers_violate_rank_order_right_left() {
        let (graph, layout) =
            two_node_layout(Direction::RightLeft, bounds(0, 0, 5, 3), bounds(3, 0, 5, 3));
        assert_eq!(rank_order_violations(&graph, &layout), BTreeSet::from([0]));
    }

    #[test]
    fn intersecting_boxes_are_reported_once_in_id_order() {
        let (_, layout) =
            two_node_layout(Direction::TopDown, bounds(0, 0, 5, 3), bounds(4, 2, 5, 3));
        assert_eq!(
            node_overlaps(&layout),
            BTreeSet::from([("A".to_string(), "B".to_string())])
        );
    }
}
