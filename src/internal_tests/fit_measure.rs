//! Measured text entry points: painted cell extent agrees with the output.

use std::fs;
use std::path::Path;

use crate::builtins::default_registry;
use crate::diagrams::sequence::compiler;
use crate::engines::graph::EngineConfig;
use crate::engines::graph::algorithms::layered::run_layered_layout;
use crate::engines::graph::contracts::MeasurementMode;
use crate::format::display_width;
use crate::graph::Graph;
use crate::graph::geometry::GraphGeometry;
use crate::mermaid::sequence::parse_sequence;
use crate::payload::Diagram;
use crate::render::graph::{
    GraphTextDrawing, render_text_from_geometry, render_text_from_geometry_measured,
};
use crate::render::text::{CellExtent, CharSet};
use crate::render::timeline;
use crate::timeline::sequence::layout::layout;
use crate::{OutputFormat, RenderConfig, TextColorMode};

const GRAPH_FIXTURES: &[&str] = &[
    "flowchart/simple.mmd",
    "flowchart/ci_pipeline.mmd",
    "state/cjk_labels.mmd",
    "class/all_relations.mmd",
];

const MODES: &[(OutputFormat, TextColorMode)] = &[
    (OutputFormat::Text, TextColorMode::Plain),
    (OutputFormat::Ascii, TextColorMode::Plain),
    (OutputFormat::Text, TextColorMode::Ansi),
];

pub(crate) fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()))
}

pub(crate) fn strip_sgr(input: &str) -> String {
    let mut stripped = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        stripped.push(ch);
    }
    stripped
}

pub(crate) fn string_extent(text: &str) -> CellExtent {
    let visible = strip_sgr(text);
    if visible.trim().is_empty() {
        return CellExtent::default();
    }
    CellExtent {
        width: visible.lines().map(display_width).max().unwrap_or(0),
        height: visible.lines().count(),
    }
}

/// Parse a graph-family fixture into its payload graph.
pub(crate) fn fixture_graph(fixture: &str) -> Graph {
    let input = load_fixture(fixture);
    let registry = default_registry();
    let id = registry.detect(&input).expect("detect");
    let parsed = registry
        .create(id)
        .expect("diagram instance")
        .parse(&input)
        .expect("parse");
    match parsed.into_payload().expect("payload") {
        Diagram::Flowchart(graph) | Diagram::Class(graph) | Diagram::State(graph) => graph,
        Diagram::Sequence(_) => panic!("{fixture} is not a graph-family fixture"),
    }
}

/// Lay a graph-family fixture out with the layered engine in grid mode.
pub(crate) fn layout_fixture(fixture: &str) -> (Graph, GraphGeometry) {
    let graph = fixture_graph(fixture);
    let config = EngineConfig::Layered(RenderConfig::default().layout.into());
    let geometry = run_layered_layout(&MeasurementMode::Grid, &graph, &config).expect("layout");
    (graph, geometry)
}

fn measured(
    fixture: &str,
    format: OutputFormat,
    text_color_mode: TextColorMode,
) -> GraphTextDrawing {
    let (graph, geometry) = layout_fixture(fixture);
    let config = RenderConfig {
        text_color_mode,
        ..RenderConfig::default()
    };
    let options = config.text_render_options(format);
    let drawing = render_text_from_geometry_measured(&graph, &geometry, None, &options, false);
    let plain = render_text_from_geometry(&graph, &geometry, None, &options);
    assert_eq!(drawing.text, plain, "{fixture}: wrapper diverged");
    drawing
}

#[test]
fn measured_graph_extent_matches_output_cells() {
    for fixture in GRAPH_FIXTURES {
        for &(format, color) in MODES {
            let drawing = measured(fixture, format, color);
            assert_eq!(
                drawing.extent,
                string_extent(&drawing.text),
                "{fixture} {format:?} {color:?}"
            );
            assert!(drawing.audit.is_none());
        }
    }
}

#[test]
fn measured_sequence_extent_matches_output_cells() {
    let input = load_fixture("sequence/nested_blocks.mmd");
    let parsed = parse_sequence(&input).expect("parse");
    let model = compiler::compile(&parsed.statements).expect("compile");
    let sequence_layout = layout(&model);
    for charset in [CharSet::unicode(), CharSet::ascii()] {
        let (text, extent) = timeline::render_measured(&sequence_layout, &charset);
        assert_eq!(text, timeline::render(&sequence_layout, &charset));
        assert_eq!(extent, string_extent(&text));
        assert!(extent.width > 0);
    }
}
