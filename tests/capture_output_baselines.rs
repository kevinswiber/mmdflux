//! One-shot baseline capture utility.
//! Run with: cargo test --test capture_output_baselines -- --ignored
//!
//! Renders all fixtures listed in the baseline manifest and writes output files
//! to `tests/baselines/`. Run this when intentionally refreshing the frozen
//! regression outputs.

use std::collections::HashMap;
use std::path::Path;

use mmdflux::builtins::default_registry;
use mmdflux::{OutputFormat, RenderConfig, render_diagram};

#[derive(serde::Deserialize)]
struct BaselineManifest {
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

#[test]
#[ignore] // Only run manually to capture baselines
fn capture_all_baselines() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join("tests/baselines/manifest.json");
    let content = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest: BaselineManifest = serde_json::from_str(&content).unwrap();

    let registry = default_registry();
    let mut captured = 0;

    let mut fixtures: Vec<_> = manifest.fixture_outputs.iter().collect();
    fixtures.sort_by_key(|(k, _)| (*k).clone());

    for (fixture_path, contract) in &fixtures {
        let full_path = manifest_dir.join(fixture_path);
        let input = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("Missing fixture {fixture_path}: {e}"));

        registry
            .detect(&input)
            .unwrap_or_else(|| panic!("Cannot detect {fixture_path}"));

        if contract.text {
            let output = render_diagram(&input, OutputFormat::Text, &RenderConfig::default())
                .unwrap_or_else(|e| panic!("Text render failed for {fixture_path}: {e}"));

            let out_path = manifest_dir.join(
                fixture_path
                    .replace("tests/fixtures/", "tests/baselines/text/")
                    .replace(".mmd", ".txt"),
            );
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            std::fs::write(&out_path, &output).unwrap();
            captured += 1;
        }

        if contract.svg {
            let output = render_diagram(&input, OutputFormat::Svg, &RenderConfig::default())
                .unwrap_or_else(|e| panic!("SVG render failed for {fixture_path}: {e}"));

            let out_path = manifest_dir.join(
                fixture_path
                    .replace("tests/fixtures/", "tests/baselines/svg/")
                    .replace(".mmd", ".svg"),
            );
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            std::fs::write(&out_path, &output).unwrap();
            captured += 1;
        }

        if contract.mmds {
            let output = render_diagram(&input, OutputFormat::Mmds, &RenderConfig::default())
                .unwrap_or_else(|e| panic!("MMDS render failed for {fixture_path}: {e}"));

            let out_path = manifest_dir.join(
                fixture_path
                    .replace("tests/fixtures/", "tests/baselines/mmds/")
                    .replace(".mmd", ".json"),
            );
            std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
            std::fs::write(&out_path, &output).unwrap();
            captured += 1;
        }
    }

    eprintln!(
        "Captured {captured} baseline files from {} fixtures",
        fixtures.len()
    );
}
