//! Pre-engine wrap pass.
//!
//! `prepare_wrapped_labels` computes `diagram::Edge.wrapped_label_lines`
//! once per edge using the render's configured `ProportionalTextMetrics` and
//! `max_width`. Every downstream wrap consumer (layered kernel sizing scan,
//! `populate_label_geometry`, SVG text, routed SVG replay, MMDS routed
//! replay) reads the same artifact instead of recomputing, which is the
//! divergence bug plan 0145 task 1.7 and plan 0147 design.md §6.2 set out
//! to eliminate.
//!
//! Module lives at the `graph` tier per plan 0147 design.md §4.5 and
//! `boundaries.toml:17-30` (`graph` allowed deps are `errors` and `format`).
//! The runtime call site is `runtime::graph_family::render_graph_family`
//! per design.md §6.1.

use crate::graph::Edge;
use crate::graph::measure::{ProportionalTextMetrics, wrap_lines};

/// Greedy-wrap every labeled edge's `label` against `max_width` using
/// `metrics`, persisting the result as `edge.wrapped_label_lines`.
///
/// Idempotent: edges that already carry a wrap artifact are left alone so
/// repeated invocations (e.g. MMDS hydrate ↔ render round-trip) never
/// double-wrap. Edges without a label or with an empty label are skipped
/// — callers in that case read `edge.label` directly via the legacy path.
///
/// Passing `max_width = None` disables wrap entirely (dagre-parity
/// fallback); all artifacts stay `None` and legacy single-line measurement
/// continues.
pub fn prepare_wrapped_labels(
    edges: &mut [Edge],
    metrics: &ProportionalTextMetrics,
    max_width: Option<f64>,
) {
    let Some(max_width) = max_width else {
        return;
    };
    for edge in edges.iter_mut() {
        if edge.wrapped_label_lines.is_some() {
            continue;
        }
        let Some(label) = edge.label.as_deref() else {
            continue;
        };
        if label.is_empty() {
            continue;
        }
        edge.wrapped_label_lines = Some(wrap_lines(metrics, label, max_width));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Edge;
    use crate::graph::measure::default_proportional_text_metrics;

    #[test]
    fn prepare_wrapped_labels_populates_wrapped_lines_for_labeled_edges() {
        let metrics = default_proportional_text_metrics();
        let mut edges = vec![
            Edge::new("A", "B").with_label("this is a deliberately long label"),
            Edge::new("B", "C"),
        ];
        prepare_wrapped_labels(&mut edges, &metrics, Some(120.0));
        assert!(edges[0].wrapped_label_lines.is_some());
        assert!(edges[0].wrapped_label_lines.as_ref().unwrap().len() >= 2);
        assert!(edges[1].wrapped_label_lines.is_none());
    }

    #[test]
    fn prepare_wrapped_labels_none_max_width_leaves_wrapped_lines_none() {
        let metrics = default_proportional_text_metrics();
        let mut edges = vec![Edge::new("A", "B").with_label("anything")];
        prepare_wrapped_labels(&mut edges, &metrics, None);
        assert!(edges[0].wrapped_label_lines.is_none());
    }

    #[test]
    fn prepare_wrapped_labels_skips_empty_label() {
        let metrics = default_proportional_text_metrics();
        let mut edges = vec![Edge::new("A", "B").with_label("")];
        prepare_wrapped_labels(&mut edges, &metrics, Some(200.0));
        assert!(edges[0].wrapped_label_lines.is_none());
    }

    #[test]
    fn prepare_wrapped_labels_is_idempotent_for_already_wrapped_edges() {
        let metrics = default_proportional_text_metrics();
        let mut edges = vec![Edge::new("A", "B").with_label("long enough label to wrap")];
        edges[0].wrapped_label_lines = Some(vec!["custom".to_string(), "override".to_string()]);
        prepare_wrapped_labels(&mut edges, &metrics, Some(60.0));
        assert_eq!(
            edges[0].wrapped_label_lines.as_deref(),
            Some(vec!["custom".to_string(), "override".to_string()].as_slice()),
            "idempotent: pre-populated wrap must not be overwritten"
        );
    }
}
