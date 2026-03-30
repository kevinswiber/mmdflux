//! State diagram compliance tests and snapshot assertions.
//!
//! Validates state diagram rendering against text and SVG snapshots.
//! All tests are ignored until the state diagram type is wired up.
//!
//! Generate snapshots: `GENERATE_STATE_TEXT_SNAPSHOTS=1 cargo test --test compliance_state`
//! Generate SVG:       `GENERATE_STATE_SVG_SNAPSHOTS=1 cargo test --test compliance_state`

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::{OutputFormat, RenderConfig};

fn state_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("state")
}

fn state_text_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("state")
}

fn state_svg_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join("state")
}

fn list_state_fixtures() -> Vec<String> {
    let dir = state_fixture_dir();
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read state fixtures dir: {e}"))
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

fn render_state_text(fixture: &str) -> String {
    let path = state_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("Failed to render state fixture")
}

fn render_state_svg(fixture: &str) -> String {
    let path = state_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("Failed to render state fixture as SVG")
}

// --- Text snapshots ---

#[test]
#[ignore]
fn state_text_snapshots() {
    let snapshot_dir = state_text_snapshot_dir();
    let regenerate = std::env::var("GENERATE_STATE_TEXT_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_state_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_state_text(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.txt"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing state text snapshot: {}. Set GENERATE_STATE_TEXT_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "State text snapshot mismatch for {fixture}. Set GENERATE_STATE_TEXT_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- SVG snapshots ---

#[test]
#[ignore]
fn state_svg_snapshots() {
    let snapshot_dir = state_svg_snapshot_dir();
    let regenerate = std::env::var("GENERATE_STATE_SVG_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_state_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_state_svg(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.svg"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing state SVG snapshot: {}. Set GENERATE_STATE_SVG_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "State SVG snapshot mismatch for {fixture}. Set GENERATE_STATE_SVG_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- Compliance assertions ---

#[test]
#[ignore]
fn state_all_fixtures_parse() {
    for fixture in list_state_fixtures() {
        let path = state_fixture_dir().join(&fixture);
        let input = fs::read_to_string(&path).unwrap();
        mmdflux::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
            .unwrap_or_else(|e| panic!("Failed to parse and render state fixture {fixture}: {e}"));
    }
}

#[test]
#[ignore]
fn state_all_fixtures_render_text() {
    for fixture in list_state_fixtures() {
        let output = render_state_text(&fixture);
        assert!(
            !output.is_empty(),
            "Empty text output for state fixture: {fixture}"
        );
    }
}

#[test]
#[ignore]
fn state_all_fixtures_render_svg() {
    for fixture in list_state_fixtures() {
        let output = render_state_svg(&fixture);
        assert!(
            output.starts_with("<svg"),
            "Invalid SVG output for state fixture: {fixture}"
        );
    }
}
