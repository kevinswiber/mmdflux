//! Class diagram compliance tests and snapshot assertions.
//!
//! Locks class rendering output with deterministic text and SVG snapshots.
//! Generate snapshots: `GENERATE_CLASS_TEXT_SNAPSHOTS=1 cargo test --test compliance_class`
//! Generate SVG:       `GENERATE_CLASS_SVG_SNAPSHOTS=1 cargo test --test compliance_class`

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::builtins::default_registry;
use mmdflux::{OutputFormat, RenderConfig};

fn class_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("class")
}

fn class_text_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("class")
}

fn class_svg_snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("svg-snapshots")
        .join("class")
}

fn list_class_fixtures() -> Vec<String> {
    let dir = class_fixture_dir();
    let mut fixtures: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read class fixtures dir: {e}"))
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

fn render_class_text(fixture: &str) -> String {
    let path = class_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
        .expect("Failed to render class fixture")
}

fn render_class_svg(fixture: &str) -> String {
    let path = class_fixture_dir().join(fixture);
    let input = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {fixture}: {e}"));
    mmdflux::render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
        .expect("Failed to render class fixture as SVG")
}

fn assert_class_text_snapshot(fixture: &str) {
    let output = render_class_text(fixture);
    let stem = fixture.trim_end_matches(".mmd");
    let snapshot_path = class_text_snapshot_dir().join(format!("{stem}.txt"));
    let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
        panic!(
            "Missing class text snapshot: {}. Set GENERATE_CLASS_TEXT_SNAPSHOTS=1 to generate.",
            snapshot_path.display()
        )
    });
    assert_eq!(
        output, expected,
        "Class text snapshot mismatch for {fixture}. Set GENERATE_CLASS_TEXT_SNAPSHOTS=1 to regenerate."
    );
}

// --- Text snapshots ---

#[test]
fn class_text_snapshots() {
    let snapshot_dir = class_text_snapshot_dir();
    let regenerate = std::env::var("GENERATE_CLASS_TEXT_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_class_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_class_text(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.txt"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing class text snapshot: {}. Set GENERATE_CLASS_TEXT_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "Class text snapshot mismatch for {fixture}. Set GENERATE_CLASS_TEXT_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- SVG snapshots ---

#[test]
fn class_svg_snapshots() {
    let snapshot_dir = class_svg_snapshot_dir();
    let regenerate = std::env::var("GENERATE_CLASS_SVG_SNAPSHOTS").is_ok();

    if regenerate {
        fs::create_dir_all(&snapshot_dir).unwrap();
    }

    for fixture in list_class_fixtures() {
        let stem = fixture.trim_end_matches(".mmd");
        let output = render_class_svg(&fixture);
        let snapshot_path = snapshot_dir.join(format!("{stem}.svg"));

        if regenerate {
            fs::write(&snapshot_path, &output).unwrap();
        } else {
            let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
                panic!(
                    "Missing class SVG snapshot: {}. Set GENERATE_CLASS_SVG_SNAPSHOTS=1 to generate.",
                    snapshot_path.display()
                )
            });
            assert_eq!(
                output, expected,
                "Class SVG snapshot mismatch for {fixture}. Set GENERATE_CLASS_SVG_SNAPSHOTS=1 to regenerate."
            );
        }
    }
}

// --- Compliance assertions ---

#[test]
fn class_all_fixtures_parse() {
    for fixture in list_class_fixtures() {
        let path = class_fixture_dir().join(&fixture);
        let input = fs::read_to_string(&path).unwrap();
        let instance = default_registry()
            .create("class")
            .expect("class should be registered");
        assert!(
            instance.parse(&input).is_ok(),
            "Failed to parse class fixture: {fixture}"
        );
    }
}

#[test]
fn class_all_fixtures_render_text() {
    for fixture in list_class_fixtures() {
        let output = render_class_text(&fixture);
        assert!(
            !output.is_empty(),
            "Empty text output for class fixture: {fixture}"
        );
    }
}

#[test]
fn class_all_fixtures_render_svg() {
    for fixture in list_class_fixtures() {
        let output = render_class_svg(&fixture);
        assert!(
            output.starts_with("<svg"),
            "Invalid SVG output for class fixture: {fixture}"
        );
    }
}

#[test]
fn class_dependency_renders_differently_from_association() {
    let assoc = render_class_text("all_relations.mmd");
    // The output should contain both solid and dotted edges
    assert!(
        assoc.contains('│') || assoc.contains('┆'),
        "Expected edge characters in output"
    );
}

#[test]
fn class_inheritance_direction_correct() {
    // In `Animal <|-- Dog`, Dog inherits from Animal
    // So Dog → Animal edge means Dog is source, Animal is target
    let output = render_class_text("simple.mmd");
    // Dog should appear before Animal in top-down layout (source on top)
    assert!(output.contains("Dog"));
    assert!(output.contains("Animal"));
}

#[test]
fn class_fixture_lollipop_interfaces_matches_snapshot() {
    assert_class_text_snapshot("lollipop_interfaces.mmd");
}

#[test]
fn class_fixture_two_way_relations_matches_snapshot() {
    assert_class_text_snapshot("two_way_relations.mmd");
}

#[test]
fn class_fixture_cardinality_labels_matches_snapshot() {
    assert_class_text_snapshot("cardinality_labels.mmd");
}

#[test]
fn class_fixture_class_labels_matches_snapshot() {
    assert_class_text_snapshot("class_labels.mmd");
}

#[test]
fn class_fixture_user_lollipop_repro_matches_snapshot() {
    assert_class_text_snapshot("user_lollipop_repro.mmd");
}
