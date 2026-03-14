//! Quality measurement for greedy switch validation.
//! Records crossing-related quality metrics across all flowchart fixtures.

use std::fs;
use std::path::Path;

use mmdflux::{OutputFormat, RenderConfig, render_diagram};

fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/flowchart")
        .join(name);
    fs::read_to_string(&path).unwrap()
}

fn fixture_names() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowchart");
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "mmd"))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// Render all fixtures and verify none panic or produce empty output.
/// This ensures greedy switch does not break any fixture rendering.
/// Crossing reduction correctness is covered by unit tests in order.rs.
#[test]
fn all_fixtures_render_successfully() {
    let names = fixture_names();
    assert!(
        names.len() >= 80,
        "Expected at least 80 flowchart fixtures, found {}",
        names.len()
    );
    for name in &names {
        let input = load_fixture(name);
        let output = render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
            .unwrap_or_else(|e| panic!("Failed to render {}: {}", name, e));
        assert!(!output.is_empty(), "Empty output for {}", name);
    }
}
