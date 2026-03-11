//! Old-vs-new comparison harness — renders all locked fixtures and compares outputs
//! against baseline snapshots captured from main branch before the rewrite.
//!
//! This harness ensures the architecture rewrite does not silently change any
//! rendering output. Any intentional change must be reviewed and explicitly approved.

use std::collections::HashMap;
use std::path::Path;

use mmdflux::diagram::{OutputFormat, RenderConfig};
use mmdflux::registry::default_registry;

#[derive(serde::Deserialize)]
struct CutoverManifest {
    fixture_outputs: HashMap<String, FixtureContract>,
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    rust_exports: serde_json::Value,
    #[allow(dead_code)]
    wasm_exports: serde_json::Value,
    #[allow(dead_code)]
    npm_packages: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct FixtureContract {
    text: bool,
    svg: bool,
    mmds: bool,
}

fn load_manifest() -> CutoverManifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cutover/baseline_manifest.json");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cutover manifest missing: {}", e));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("cutover manifest invalid: {}", e))
}

/// Load the frozen text baseline for a fixture.
fn load_text_baseline(fixture_path: &str) -> String {
    let snapshot_path = fixture_path
        .replace("tests/fixtures/", "tests/cutover/baselines/text/")
        .replace(".mmd", ".txt");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&snapshot_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("text baseline missing for {fixture_path}: {e}"))
}

/// Load the frozen MMDS baseline for a fixture.
fn load_mmds_baseline(fixture_path: &str) -> String {
    let snapshot_path = fixture_path
        .replace("tests/fixtures/", "tests/cutover/baselines/mmds/")
        .replace(".mmd", ".json");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&snapshot_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("MMDS baseline missing for {fixture_path}: {e}"))
}

/// Load the frozen SVG baseline for a fixture.
fn load_svg_baseline(fixture_path: &str) -> String {
    let snapshot_path = fixture_path
        .replace("tests/fixtures/", "tests/cutover/baselines/svg/")
        .replace(".mmd", ".svg");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&snapshot_path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("SVG baseline missing for {fixture_path}: {e}"))
}

/// Render a fixture through the registry pipeline for a given format.
fn render_fixture(fixture_path: &str, format: OutputFormat) -> String {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture_path);
    let input =
        std::fs::read_to_string(&full_path).unwrap_or_else(|e| panic!("fixture missing: {e}"));

    let registry = default_registry();
    let diagram_id = registry
        .detect(&input)
        .unwrap_or_else(|| panic!("could not detect diagram type for {fixture_path}"));
    let mut instance = registry.create(diagram_id).unwrap();
    instance.parse(&input).unwrap();
    instance
        .render(format, &RenderConfig::default())
        .unwrap_or_else(|e| panic!("render failed for {fixture_path}: {e}"))
}

#[test]
fn cutover_text_outputs_match_baselines() {
    let manifest = load_manifest();
    let mut failures = Vec::new();

    for (fixture_path, contract) in &manifest.fixture_outputs {
        if !contract.text {
            continue;
        }
        let expected = load_text_baseline(fixture_path);
        let actual = render_fixture(fixture_path, OutputFormat::Text);

        if expected != actual {
            failures.push(format!(
                "TEXT MISMATCH: {fixture_path}\n  expected {} bytes, got {} bytes",
                expected.len(),
                actual.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "Text output regressions detected:\n{}",
        failures.join("\n")
    );
}

#[test]
fn cutover_svg_outputs_match_baselines() {
    let manifest = load_manifest();
    let mut failures = Vec::new();

    for (fixture_path, contract) in &manifest.fixture_outputs {
        if !contract.svg {
            continue;
        }
        let expected = load_svg_baseline(fixture_path);
        let actual = render_fixture(fixture_path, OutputFormat::Svg);

        if expected != actual {
            failures.push(format!(
                "SVG MISMATCH: {fixture_path}\n  expected {} bytes, got {} bytes",
                expected.len(),
                actual.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "SVG output regressions detected:\n{}",
        failures.join("\n")
    );
}

#[test]
fn cutover_mmds_outputs_match_baselines() {
    let manifest = load_manifest();
    let mut failures = Vec::new();

    for (fixture_path, contract) in &manifest.fixture_outputs {
        if !contract.mmds {
            continue;
        }
        let expected = load_mmds_baseline(fixture_path);
        let actual = render_fixture(fixture_path, OutputFormat::Mmds);

        if expected != actual {
            failures.push(format!(
                "MMDS MISMATCH: {fixture_path}\n  expected {} bytes, got {} bytes",
                expected.len(),
                actual.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "MMDS output regressions detected:\n{}",
        failures.join("\n")
    );
}

#[test]
fn cutover_registry_detects_all_manifest_fixtures() {
    let manifest = load_manifest();
    let registry = default_registry();
    let mut failures = Vec::new();

    for fixture_path in manifest.fixture_outputs.keys() {
        let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture_path);
        let input = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("MISSING FIXTURE: {fixture_path}: {e}"));
                continue;
            }
        };

        if registry.detect(&input).is_none() {
            failures.push(format!("DETECTION FAILED: {fixture_path}"));
        }
    }

    assert!(
        failures.is_empty(),
        "Registry detection failures:\n{}",
        failures.join("\n")
    );
}
