//! Cutover baseline tests — freeze the external contract before the architecture rewrite.
//!
//! These tests verify that a baseline manifest and architecture target document exist
//! and capture the locked external surfaces of the project. They fail until the
//! rewrite baseline is explicitly frozen.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// Manifest capturing the project's locked external surfaces.
#[derive(serde::Deserialize)]
struct CutoverManifest {
    version: u32,
    rust_exports: RustExports,
    wasm_exports: Vec<String>,
    npm_packages: Vec<String>,
    fixture_outputs: HashMap<String, FixtureContract>,
}

#[derive(serde::Deserialize)]
struct RustExports {
    modules: Vec<String>,
    re_exports: Vec<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct FixtureContract {
    text: bool,
    svg: bool,
    mmds: bool,
}

fn load_cutover_manifest() -> CutoverManifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cutover/baseline_manifest.json");
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read cutover manifest at {}: {}",
            path.display(),
            e
        )
    });
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse cutover manifest: {}", e))
}

/// Parse `src/lib.rs` to extract the actual `pub mod` declarations.
fn parse_pub_modules_from_lib_rs() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let content = std::fs::read_to_string(&path).unwrap();
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("pub mod ") {
                // "pub mod foo;" -> "foo"
                trimmed
                    .strip_prefix("pub mod ")
                    .and_then(|s| s.strip_suffix(';'))
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Parse `src/lib.rs` to extract the actual `pub use` re-exported symbols.
///
/// Handles both single-line (`pub use foo::Bar;`) and multi-line brace-delimited
/// (`pub use foo::{A, B, C};`) re-exports. Returns fully-qualified names like
/// `mmdflux::Bar`.
fn parse_pub_use_re_exports_from_lib_rs() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let content = std::fs::read_to_string(&path).unwrap();
    let mut result = BTreeSet::new();

    // Join all lines into one string, then split on `pub use` boundaries.
    // This handles multi-line `pub use module::{A, B, C};` blocks.
    let joined = content.replace('\n', " ");
    for segment in joined.split("pub use ").skip(1) {
        // Each segment runs up to the next ';'
        let Some(stmt) = segment.split(';').next() else {
            continue;
        };
        let stmt = stmt.trim();

        if let Some(brace_start) = stmt.find('{') {
            // Multi-symbol re-export: `module::{A, B, C}`
            let brace_end = stmt.find('}').unwrap_or(stmt.len());
            let symbols = &stmt[brace_start + 1..brace_end];
            for sym in symbols.split(',') {
                let sym = sym.trim();
                if !sym.is_empty() {
                    result.insert(format!("mmdflux::{sym}"));
                }
            }
        } else if let Some(colon_pos) = stmt.rfind("::") {
            // Single re-export: `module::Symbol`
            let sym = &stmt[colon_pos + 2..];
            result.insert(format!("mmdflux::{sym}"));
        }
    }
    result
}

#[test]
fn cutover_manifest_captures_locked_external_surfaces() {
    let manifest = load_cutover_manifest();

    assert_eq!(manifest.version, 1);

    // Rust public modules — derived from actual pub mod lines in src/lib.rs.
    let source_modules = parse_pub_modules_from_lib_rs();
    let manifest_modules: BTreeSet<String> =
        manifest.rust_exports.modules.iter().cloned().collect();

    let missing_mods: Vec<_> = source_modules.difference(&manifest_modules).collect();
    let extra_mods: Vec<_> = manifest_modules.difference(&source_modules).collect();

    assert!(
        missing_mods.is_empty() && extra_mods.is_empty(),
        "manifest modules do not match src/lib.rs pub mod surface:\n  \
         in lib.rs but not manifest: {missing_mods:?}\n  \
         in manifest but not lib.rs: {extra_mods:?}"
    );

    // Rust re-exports — derived from the actual pub use lines in src/lib.rs.
    // This ensures the manifest stays in sync with the real crate surface,
    // not just with a handwritten list in this test file.
    let source_re_exports = parse_pub_use_re_exports_from_lib_rs();
    let manifest_re_exports: BTreeSet<String> =
        manifest.rust_exports.re_exports.iter().cloned().collect();

    let missing_from_manifest: Vec<_> =
        source_re_exports.difference(&manifest_re_exports).collect();
    let extra_in_manifest: Vec<_> = manifest_re_exports.difference(&source_re_exports).collect();

    assert!(
        missing_from_manifest.is_empty() && extra_in_manifest.is_empty(),
        "manifest re-exports do not match src/lib.rs pub use surface:\n  \
         in lib.rs but not manifest: {missing_from_manifest:?}\n  \
         in manifest but not lib.rs: {extra_in_manifest:?}"
    );

    // wasm exports
    assert!(
        manifest.wasm_exports.contains(&"render".to_string()),
        "manifest must list wasm 'render' export"
    );

    // npm packages
    assert!(
        manifest.npm_packages.contains(&"@mmds/core".to_string()),
        "manifest must list @mmds/core"
    );

    // Fixture outputs — representative fixtures are listed
    assert!(
        manifest
            .fixture_outputs
            .contains_key("tests/fixtures/flowchart/simple.mmd"),
        "manifest must list simple.mmd fixture"
    );
    assert!(
        manifest
            .fixture_outputs
            .contains_key("tests/fixtures/class/simple.mmd"),
        "manifest must list class/simple.mmd fixture"
    );
    assert!(
        manifest
            .fixture_outputs
            .contains_key("tests/fixtures/sequence/simple.mmd"),
        "manifest must list sequence/simple.mmd fixture"
    );
}

#[test]
fn cutover_manifest_rust_exports_are_complete() {
    let manifest = load_cutover_manifest();

    // Modules: source-derived check (same as in the surface test, kept as a
    // separate focused assertion for clearer failure messages).
    let source_modules = parse_pub_modules_from_lib_rs();
    let manifest_modules: BTreeSet<String> =
        manifest.rust_exports.modules.iter().cloned().collect();
    assert_eq!(
        manifest_modules, source_modules,
        "manifest modules must exactly match pub mod declarations in src/lib.rs"
    );

    // Re-exports: source-derived check.
    let source_re_exports = parse_pub_use_re_exports_from_lib_rs();
    let manifest_re_exports: BTreeSet<String> =
        manifest.rust_exports.re_exports.iter().cloned().collect();
    assert_eq!(
        manifest_re_exports, source_re_exports,
        "manifest re-exports must exactly match pub use declarations in src/lib.rs"
    );
}

#[test]
fn cutover_manifest_fixture_outputs_cover_all_diagram_types() {
    let manifest = load_cutover_manifest();

    let has_flowchart = manifest
        .fixture_outputs
        .keys()
        .any(|k| k.contains("flowchart"));
    let has_class = manifest.fixture_outputs.keys().any(|k| k.contains("class"));
    let has_sequence = manifest
        .fixture_outputs
        .keys()
        .any(|k| k.contains("sequence"));

    assert!(has_flowchart, "manifest must include flowchart fixtures");
    assert!(has_class, "manifest must include class fixtures");
    assert!(has_sequence, "manifest must include sequence fixtures");
}

#[test]
fn dependency_rules_file_exists_and_lists_forbidden_cross_layer_imports() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/architecture/hard-cutover-target.md");
    let rules = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Target architecture document must exist at {}: {}",
            path.display(),
            e
        )
    });

    // Core dependency rules from the implementation plan
    assert!(
        rules.contains("diagrams do not render"),
        "rules must state: diagrams do not render"
    );
    assert!(
        rules.contains("formats do not parse"),
        "rules must state: formats do not parse"
    );
    assert!(
        rules.contains("web main.ts is composition only"),
        "rules must state: web main.ts is composition only"
    );
    assert!(
        rules.contains("engines do not know about diagram types"),
        "rules must state: engines do not know about diagram types"
    );
    assert!(
        rules.contains("api/ owns the stable public contract"),
        "rules must state: api/ owns the stable public contract"
    );
}

#[test]
fn dependency_rules_document_target_module_layout() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/architecture/hard-cutover-target.md");
    let rules = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Target architecture document must exist at {}: {}",
            path.display(),
            e
        )
    });

    // Target module structure must be documented
    assert!(
        rules.contains("src/api/"),
        "rules must document src/api/ module"
    );
    assert!(
        rules.contains("src/runtime/"),
        "rules must document src/runtime/ module"
    );
    assert!(
        rules.contains("src/graph/"),
        "rules must document src/graph/ module"
    );
    assert!(
        rules.contains("src/engines/"),
        "rules must document src/engines/ module"
    );
    assert!(
        rules.contains("src/formats/"),
        "rules must document src/formats/ module"
    );
    assert!(
        rules.contains("src/diagrams/"),
        "rules must document src/diagrams/ module"
    );
}
