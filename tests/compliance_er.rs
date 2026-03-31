//! ER diagram compliance tests and snapshot assertions.
//!
//! Locks ER rendering output with deterministic text and SVG snapshots.
//! Generate snapshots: `GENERATE_ER_TEXT_SNAPSHOTS=1 cargo nextest run --test compliance_er -E 'test(er_text_snapshots)'`
//! Generate SVG:       `GENERATE_ER_SVG_SNAPSHOTS=1 cargo nextest run --test compliance_er -E 'test(er_svg_snapshots)'`

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::builtins::default_registry;
use mmdflux::{OutputFormat, RenderConfig};

fn er_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("er")
}

fn er_text_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("er")
}

fn er_svg_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join("er")
}

fn list_er_fixtures() -> Vec<String> {
    let dir = er_fixture_dir();
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read ER fixtures dir: {e}"))
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

fn render_er_text(fixture: &str) -> String {
    let path = er_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("Failed to render ER fixture")
}

fn render_er_svg(fixture: &str) -> String {
    let path = er_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("Failed to render ER fixture as SVG")
}

// --- Text snapshots ---

#[test]
fn er_text_snapshots() {
    let snapshot_dir = er_text_snapshot_dir();
    let regenerate = std::env::var("GENERATE_ER_TEXT_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_er_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_er_text(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.txt"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing ER text snapshot: {}. Set GENERATE_ER_TEXT_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "ER text snapshot mismatch for {fixture}. Set GENERATE_ER_TEXT_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- SVG snapshots ---

#[test]
fn er_svg_snapshots() {
    let snapshot_dir = er_svg_snapshot_dir();
    let regenerate = std::env::var("GENERATE_ER_SVG_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_er_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_er_svg(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.svg"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing ER SVG snapshot: {}. Set GENERATE_ER_SVG_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "ER SVG snapshot mismatch for {fixture}. Set GENERATE_ER_SVG_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- Compliance assertions ---

#[test]
fn er_all_fixtures_parse() {
    for fixture in list_er_fixtures() {
        let path = er_fixture_dir().join(&fixture);
        let input = fs::read_to_string(&path).unwrap();
        let instance = default_registry()
            .create("er")
            .expect("er should be registered");
        assert!(
            instance.parse(&input).is_ok(),
            "Failed to parse ER fixture: {fixture}"
        );
    }
}

#[test]
fn er_all_fixtures_render_text() {
    for fixture in list_er_fixtures() {
        let output = render_er_text(&fixture);
        assert!(
            !output.is_empty(),
            "Empty text output for ER fixture: {fixture}"
        );
    }
}

#[test]
fn er_all_fixtures_render_svg() {
    for fixture in list_er_fixtures() {
        let output = render_er_svg(&fixture);
        assert!(
            output.starts_with("<svg"),
            "Invalid SVG output for ER fixture: {fixture}"
        );
    }
}
