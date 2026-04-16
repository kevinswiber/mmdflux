//! Plan 0147 Task 1.4: sequencing test for `normalize_br_tags` → `wrap_lines`.
//!
//! `internal_tests` is exempt from the semantic boundary graph (see
//! `boundaries.toml:14`), so this module may freely import from both
//! `diagrams::flowchart::compiler` and `graph::measure` — the same boundary
//! that `graph::measure::wrap_lines` cannot cross on its own.

use crate::builtins::default_registry;
use crate::diagrams::flowchart::compiler::normalize_br_tags;
use crate::format::OutputFormat;
use crate::graph::measure::{
    DEFAULT_PROPORTIONAL_FONT_SIZE, DEFAULT_PROPORTIONAL_NODE_PADDING_X,
    DEFAULT_PROPORTIONAL_NODE_PADDING_Y, ProportionalTextMetrics, wrap_lines,
};
use crate::payload::Diagram as Payload;
use crate::runtime::config::RenderConfig;

#[test]
fn wrap_lines_respects_br_normalized_input() {
    let metrics = ProportionalTextMetrics::new(
        DEFAULT_PROPORTIONAL_FONT_SIZE,
        DEFAULT_PROPORTIONAL_NODE_PADDING_X,
        DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
    );
    // Caller is responsible for normalizing <br> → '\n' before calling wrap_lines.
    // This test documents the contract: wrap_lines is called AFTER normalize_br_tags.
    let raw = "yes<br>some very long continuation";
    let normalized = normalize_br_tags(raw);
    assert_eq!(
        normalized, "yes\nsome very long continuation",
        "normalize_br_tags must replace <br> with a hard '\\n' break"
    );

    // max_width 100 keeps every word under the threshold so the test
    // exercises the normalize_br_tags → wrap_lines sequencing without
    // incidentally hitting the char-split fallback path.
    let lines = wrap_lines(&metrics, &normalized, 100.0);
    assert_eq!(
        lines[0], "yes",
        "first segment must stand alone post-normalization, got {lines:?}"
    );
    assert!(
        lines.len() >= 2,
        "continuation segment must wrap across further lines, got {lines:?}"
    );
    let rest: String = lines[1..].join(" ");
    assert_eq!(
        rest, "some very long continuation",
        "second-segment tokens round-trip in order"
    );
}

/// Phase 1.5b integration gate: runtime wrap pass populates
/// `diagram::Edge.wrapped_label_lines` before the engine runs, so every
/// downstream consumer sees the same wrap decision instead of
/// recomputing. See plan 0147 design.md §6.1.
#[test]
fn runtime_render_populates_wrapped_label_lines_for_long_label() {
    let input =
        "graph TD\n    A[Start] -->|this is a deliberately long label that will wrap| B[End]\n";
    let payload = default_registry()
        .create("flowchart")
        .expect("flowchart should be registered")
        .parse(input)
        .expect("fixture should parse")
        .into_payload()
        .expect("fixture should build a payload");
    let Payload::Flowchart(mut graph) = payload else {
        panic!("flowchart input should yield a flowchart payload");
    };

    // The wrap artifact starts life as None on every parsed edge…
    assert!(graph.edges.iter().all(|e| e.wrapped_label_lines.is_none()));

    // …and the runtime wrap pass populates it for every labeled edge.
    crate::runtime::graph_family::render_graph_family(
        "flowchart",
        &mut graph,
        OutputFormat::Svg,
        &RenderConfig::default(),
    )
    .expect("runtime render should succeed");

    let labeled = graph
        .edges
        .iter()
        .filter(|e| e.label.is_some())
        .collect::<Vec<_>>();
    assert!(!labeled.is_empty(), "fixture must carry a labeled edge");
    for edge in labeled {
        let wrapped = edge
            .wrapped_label_lines
            .as_ref()
            .expect("runtime wrap pass must populate wrapped_label_lines before engine solve");
        assert!(
            wrapped.len() >= 2,
            "long label must wrap into multiple lines, got {wrapped:?}"
        );
    }
}

#[test]
fn wrap_lines_respects_br_case_insensitive_variants() {
    let metrics = ProportionalTextMetrics::new(
        DEFAULT_PROPORTIONAL_FONT_SIZE,
        DEFAULT_PROPORTIONAL_NODE_PADDING_X,
        DEFAULT_PROPORTIONAL_NODE_PADDING_Y,
    );
    for raw in ["a<br>b", "a<BR>b", "a<br/>b", "a<br />b"] {
        let normalized = normalize_br_tags(raw);
        let lines = wrap_lines(&metrics, &normalized, 1000.0);
        assert_eq!(
            lines,
            vec!["a".to_string(), "b".to_string()],
            "variant {raw:?} must normalize to a \\n-delimited pair"
        );
    }
}
