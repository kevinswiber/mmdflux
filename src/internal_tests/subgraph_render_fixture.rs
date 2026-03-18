//! Subgraph rendering tests that require cross-boundary imports (runtime facade).
//! Moved from render::graph::text::subgraph to respect module boundary rules.

fn render_flowchart_fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error));
    crate::render_diagram(
        &input,
        crate::OutputFormat::Text,
        &crate::RenderConfig::default(),
    )
    .unwrap_or_else(|error| panic!("Failed to render fixture {name}: {error}"))
}

fn render_flowchart_fixture_ascii(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart")
        .join(name);
    let input = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Failed to read fixture {}: {}", path.display(), error));
    crate::render_diagram(
        &input,
        crate::OutputFormat::Ascii,
        &crate::RenderConfig::default(),
    )
    .unwrap_or_else(|error| panic!("Failed to render fixture {name}: {error}"))
}

#[test]
fn simple_subgraph_renders_title_and_nodes() {
    let output = render_flowchart_fixture("simple_subgraph.mmd");
    assert!(output.contains("Process"), "Should contain subgraph title");
    assert!(output.contains("Start"), "Should contain Start node");
    assert!(output.contains("Middle"), "Should contain Middle node");
    assert!(output.contains("End"), "Should contain End node");
}

#[test]
fn simple_subgraph_has_border() {
    let output = render_flowchart_fixture("simple_subgraph.mmd");
    assert!(
        output.contains('┌') && output.contains('┘'),
        "Should have box-drawing border characters"
    );
}

#[test]
fn subgraph_edges_renders_both_groups() {
    let output = render_flowchart_fixture("subgraph_edges.mmd");
    assert!(
        output.contains("Input"),
        "Should contain Input subgraph title"
    );
    assert!(output.contains("Data"), "Should contain Data node");
    assert!(output.contains("Config"), "Should contain Config node");
    assert!(output.contains("Result"), "Should contain Result node");
    assert!(output.contains("Log"), "Should contain Log node");
}

#[test]
fn subgraph_edges_borders_do_not_overlap() {
    let output = render_flowchart_fixture("subgraph_edges.mmd");
    let lines: Vec<&str> = output.lines().collect();

    let bottom_border_rows: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains('└'))
        .map(|(index, _)| index)
        .collect();
    let top_border_rows: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains('┌'))
        .map(|(index, _)| index)
        .collect();

    assert!(
        !bottom_border_rows.is_empty(),
        "Should have bottom border rows"
    );
    assert!(!top_border_rows.is_empty(), "Should have top border rows");

    let first_bottom = lines.iter().position(|line| line.contains('└')).unwrap();
    let second_top = lines
        .iter()
        .enumerate()
        .skip(first_bottom)
        .position(|(_, line)| line.contains('┌'))
        .map(|position| position + first_bottom);

    if let Some(second_top) = second_top {
        assert!(
            second_top > first_bottom,
            "Second subgraph top ({second_top}) should be below first subgraph bottom ({first_bottom})"
        );
    }
}

#[test]
fn subgraph_titles_preserved_with_cross_edges() {
    let output = render_flowchart_fixture("subgraph_edges.mmd");
    assert!(
        output.contains("Input"),
        "Input title should be intact in: {}",
        output
    );
    assert!(
        output.contains("Output"),
        "Output title should be intact in: {}",
        output
    );
}

#[test]
fn multi_subgraph_renders_both_groups() {
    let output = render_flowchart_fixture("multi_subgraph.mmd");
    assert!(output.contains("UI"), "Should contain UI node");
    assert!(output.contains("API"), "Should contain API node");
    assert!(output.contains("Server"), "Should contain Server node");
    assert!(output.contains("DB"), "Should contain DB node");
    let border_count = output.matches('┌').count();
    assert!(
        border_count >= 3,
        "Should have borders for subgraphs and nodes, got {} '┌' chars",
        border_count
    );
    assert!(output.contains("Frontend"), "Should contain Frontend title");
    assert!(output.contains("Backend"), "Should contain Backend title");
}

#[test]
fn simple_subgraph_ascii_mode() {
    let output = render_flowchart_fixture_ascii("simple_subgraph.mmd");
    assert!(output.contains("Process"), "ASCII: should contain title");
    assert!(output.contains("Start"), "ASCII: should contain Start");
    assert!(
        output.contains('+') && output.contains('-'),
        "ASCII mode should use +/- border characters"
    );
}

#[test]
fn subgraph_title_embedded_in_border() {
    let output = render_flowchart_fixture("simple_subgraph.mmd");
    assert!(
        output.contains("─ Process ─") || output.contains("- Process -"),
        "Title should be embedded in border: {}",
        output
    );
}
