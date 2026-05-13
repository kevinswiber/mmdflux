//! Pre-engine wrap pass.
//!
//! `prepare_wrapped_labels` computes `diagram::Edge.wrapped_label_lines`
//! once per edge using the render's configured `ProportionalTextMetrics` and
//! `max_width`. Every downstream wrap consumer (layered kernel sizing scan,
//! `populate_label_geometry`, SVG text, routed SVG replay, MMDS routed
//! replay) reads the same artifact instead of recomputing, which is the
//! divergence class this pass is meant to eliminate.
//!
//! Module lives at the `graph` tier per `boundaries.toml:17-30` (`graph`
//! allowed deps are `errors` and `format`).
//! The runtime call site is `runtime::graph_family::render_graph_family`
//! so wrapping happens before engine sizing.

use crate::graph::Edge;
use crate::graph::measure::{
    ProportionalTextMetrics, TextMetricsProvider, edge_text_style_key,
    wrap_lines_with_provider_for_style,
};

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
    prepare_wrapped_labels_with_provider(edges, metrics, max_width);
}

pub(crate) fn prepare_wrapped_labels_with_provider(
    edges: &mut [Edge],
    provider: &dyn TextMetricsProvider,
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
        let style = edge_text_style_key(provider, edge);
        edge.wrapped_label_lines = Some(wrap_lines_with_provider_for_style(
            provider, &style, label, max_width,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Edge;
    use crate::graph::measure::{
        GraphTextStyleKey, TextMetricsProvider, default_proportional_text_metrics,
    };

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

    #[test]
    fn styled_edge_label_wrapping_uses_edge_effective_text_style_before_solve() {
        let provider = StyleWrapProvider::new()
            .with_line_width("narrow", "alpha", 20.0)
            .with_line_width("narrow", "beta", 20.0)
            .with_line_width("wide", "alpha", 50.0)
            .with_line_width("wide", "beta", 50.0);
        let mut narrow = Edge::new("A", "B").with_label("alpha beta");
        narrow.style.font_family = Some("narrow".to_string());
        let mut wide = Edge::new("B", "C").with_label("alpha beta");
        wide.style.font_family = Some("wide".to_string());
        let mut edges = vec![narrow, wide];

        prepare_wrapped_labels_with_provider(&mut edges, &provider, Some(60.0));

        assert_eq!(
            edges[0].wrapped_label_lines.as_deref(),
            Some(vec!["alpha beta".to_string()].as_slice())
        );
        assert_eq!(
            edges[1].wrapped_label_lines.as_deref(),
            Some(vec!["alpha".to_string(), "beta".to_string()].as_slice())
        );
    }

    #[derive(Default)]
    struct StyleWrapProvider {
        line_widths: std::collections::BTreeMap<(GraphTextStyleKey, String), f64>,
    }

    impl StyleWrapProvider {
        fn new() -> Self {
            Self::default()
        }

        fn with_line_width(mut self, style: &str, text: &str, width: f64) -> Self {
            self.line_widths
                .insert((GraphTextStyleKey::test(style), text.to_string()), width);
            self
        }
    }

    impl TextMetricsProvider for StyleWrapProvider {
        fn measure_line_width(&self, _text: &str) -> f64 {
            0.0
        }

        fn measure_scalar_width(&self, _ch: char) -> f64 {
            0.0
        }

        fn measure_line_width_for_style(&self, style: &GraphTextStyleKey, text: &str) -> f64 {
            self.line_widths
                .get(&(style.clone(), text.to_string()))
                .copied()
                .unwrap_or(0.0)
        }

        fn measure_scalar_width_for_style(&self, _style: &GraphTextStyleKey, ch: char) -> f64 {
            if ch == ' ' { 5.0 } else { 0.0 }
        }

        fn font_size(&self) -> f64 {
            16.0
        }

        fn line_height(&self) -> f64 {
            24.0
        }

        fn node_padding_x(&self) -> f64 {
            15.0
        }

        fn node_padding_y(&self) -> f64 {
            15.0
        }

        fn label_padding_x(&self) -> f64 {
            4.0
        }

        fn label_padding_y(&self) -> f64 {
            2.0
        }
    }
}
