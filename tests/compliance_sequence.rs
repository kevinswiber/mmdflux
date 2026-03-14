//! Sequence diagram compliance tests and snapshot assertions.
//!
//! Locks sequence rendering output with deterministic text snapshots.
//! Generate snapshots: `GENERATE_SEQUENCE_TEXT_SNAPSHOTS=1 cargo test --test compliance_sequence`

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::builtins::default_registry;
use mmdflux::{OutputFormat, RenderConfig};

fn sequence_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sequence")
}

fn sequence_text_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("sequence")
}

fn list_sequence_fixtures() -> Vec<String> {
    let dir = sequence_fixture_dir();
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read sequence fixtures dir: {e}"))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().is_some_and(|e| e == "mmd") {
                Some(path.file_name()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn render_sequence_text(fixture: &str) -> String {
    let path = sequence_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("Failed to render sequence fixture")
}

// --- Text snapshots ---

#[test]
fn sequence_text_snapshots() {
    let snapshot_dir = sequence_text_snapshot_dir();
    let regenerate = std::env::var("GENERATE_SEQUENCE_TEXT_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_sequence_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_sequence_text(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.txt"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing sequence text snapshot: {}. Set GENERATE_SEQUENCE_TEXT_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "Sequence text snapshot mismatch for {fixture}. Set GENERATE_SEQUENCE_TEXT_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- Compliance assertions ---

#[test]
fn sequence_all_fixtures_parse() {
    for fixture in list_sequence_fixtures() {
        let path = sequence_fixture_dir().join(&fixture);
        let input = fs::read_to_string(&path).unwrap();
        let instance = default_registry()
            .create("sequence")
            .expect("sequence should be registered");
        assert!(
            instance.parse(&input).is_ok(),
            "Failed to parse sequence fixture: {fixture}"
        );
    }
}

#[test]
fn sequence_all_fixtures_render_text() {
    for fixture in list_sequence_fixtures() {
        let output = render_sequence_text(&fixture);
        assert!(
            !output.is_empty(),
            "Empty text output for sequence fixture: {fixture}"
        );
    }
}

#[test]
fn sequence_all_fixtures_render_ascii() {
    for fixture in list_sequence_fixtures() {
        let path = sequence_fixture_dir().join(&fixture);
        let input = fs::read_to_string(&path).unwrap();
        let output = mmdflux::render_diagram(&input, OutputFormat::Ascii, &RenderConfig::default())
            .expect("render failed");
        assert!(
            !output.is_empty(),
            "Empty ASCII output for sequence fixture: {fixture}"
        );
    }
}

#[test]
fn sequence_svg_not_supported() {
    let result = mmdflux::render_diagram(
        "sequenceDiagram\nA->>B: hi",
        OutputFormat::Svg,
        &RenderConfig::default(),
    );
    assert!(result.is_err());
}

#[test]
fn sequence_dashed_uses_different_line_char() {
    let solid = render_sequence_text("simple.mmd");
    let dashed = render_sequence_text("dashed.mmd");
    // Dashed fixture should contain dotted horizontal char
    assert!(dashed.contains('┄'), "dashed output should use ┄");
    // Solid fixture should NOT contain dotted horizontal char
    assert!(!solid.contains('┄'), "solid output should not use ┄");
}

#[test]
fn sequence_autonumber_prefixes() {
    let output = render_sequence_text("autonumber.mmd");
    assert!(output.contains("1."), "should contain number 1");
    assert!(output.contains("2."), "should contain number 2");
    assert!(output.contains("3."), "should contain number 3");
}

#[test]
fn sequence_rendering_deterministic() {
    for fixture in list_sequence_fixtures() {
        let out1 = render_sequence_text(&fixture);
        let out2 = render_sequence_text(&fixture);
        assert_eq!(out1, out2, "Non-deterministic output for {fixture}");
    }
}
