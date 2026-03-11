//! Architecture guard tests for the stable public surface and module boundaries.
//!
//! These tests verify that the baseline manifest and dependency-rules document
//! remain aligned with the steady-state architecture.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// Manifest capturing the project's locked external surfaces.
#[derive(serde::Deserialize)]
struct BaselineManifest {
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

fn load_baseline_manifest() -> BaselineManifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/baselines/manifest.json");
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read baseline manifest at {}: {}",
            path.display(),
            e
        )
    });
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse baseline manifest: {}", e))
}

fn collect_rust_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("Failed to read source directory {}: {}", dir.display(), e))
    {
        let entry = entry.unwrap_or_else(|e| panic!("Failed to read directory entry: {e}"));
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn parse_pub_modules_from_lib_rs() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let content = std::fs::read_to_string(&path).unwrap();
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("pub mod ") {
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

fn parse_pub_use_re_exports_from_lib_rs() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let content = std::fs::read_to_string(&path).unwrap();
    let mut result = BTreeSet::new();

    let joined = content.replace('\n', " ");
    for segment in joined.split("pub use ").skip(1) {
        let Some(stmt) = segment.split(';').next() else {
            continue;
        };
        let stmt = stmt.trim();

        if let Some(brace_start) = stmt.find('{') {
            let brace_end = stmt.find('}').unwrap_or(stmt.len());
            let symbols = &stmt[brace_start + 1..brace_end];
            for sym in symbols.split(',') {
                let sym = sym.trim();
                if !sym.is_empty() {
                    result.insert(format!("mmdflux::{sym}"));
                }
            }
        } else if let Some(colon_pos) = stmt.rfind("::") {
            let sym = &stmt[colon_pos + 2..];
            result.insert(format!("mmdflux::{sym}"));
        }
    }
    result
}

#[test]
fn baseline_manifest_captures_locked_external_surfaces() {
    let manifest = load_baseline_manifest();

    assert_eq!(manifest.version, 1);

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

    assert!(
        manifest.wasm_exports.contains(&"render".to_string()),
        "manifest must list wasm 'render' export"
    );

    assert!(
        manifest.npm_packages.contains(&"@mmds/core".to_string()),
        "manifest must list @mmds/core"
    );

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
fn baseline_manifest_rust_exports_are_complete() {
    let manifest = load_baseline_manifest();

    let source_modules = parse_pub_modules_from_lib_rs();
    let manifest_modules: BTreeSet<String> =
        manifest.rust_exports.modules.iter().cloned().collect();
    assert_eq!(
        manifest_modules, source_modules,
        "manifest modules must exactly match pub mod declarations in src/lib.rs"
    );

    let source_re_exports = parse_pub_use_re_exports_from_lib_rs();
    let manifest_re_exports: BTreeSet<String> =
        manifest.rust_exports.re_exports.iter().cloned().collect();
    assert_eq!(
        manifest_re_exports, source_re_exports,
        "manifest re-exports must exactly match pub use declarations in src/lib.rs"
    );
}

#[test]
fn baseline_manifest_fixture_outputs_cover_all_diagram_types() {
    let manifest = load_baseline_manifest();

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
fn dependency_rules_file_exists_and_lists_current_ownership_boundaries() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/architecture/dependency-rules.md");
    let rules = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Dependency rules document must exist at {}: {}",
            path.display(),
            e
        )
    });

    for required in [
        "frontends own input formats",
        "diagrams do not parse source text directly",
        "diagrams do not render",
        "render/ owns output production",
        "render::graph::backends owns graph-family output targets",
        "render::diagram owns family-local renderers",
        "graph/ owns graph-family IR and solved geometry only",
        "mmds/ is the MMDS contract and output namespace",
        "MMDS is a frontend, not a logical diagram type",
        "engines do not know about diagram types",
        "flat top-level contract modules own the stable public contract",
        "web main.ts is composition only",
    ] {
        assert!(
            rules.contains(required),
            "dependency rules must mention: {required}"
        );
    }
}

#[test]
fn removed_transitional_module_roots_stay_gone() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for relative_path in [
        "src/api",
        "src/diagram.rs",
        "src/render.rs",
        "src/formats",
        "src/parser",
        "src/graph/builder.rs",
        "src/graph/render",
        "src/graph/routing.rs",
        "src/diagrams/sequence/render",
        "src/diagrams/mmds",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            !path.exists(),
            "{} should remain removed from the architecture",
            path.display()
        );
    }

    assert!(
        repo_root.join("src/render/mod.rs").exists(),
        "top-level render namespace must be directory-based"
    );
    assert!(
        repo_root.join("src/render/graph/backends").exists(),
        "graph-family output targets must live under render::graph::backends"
    );
}

#[test]
fn public_contract_modules_are_flat_and_api_namespace_is_gone() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib_rs = std::fs::read_to_string(repo_root.join("src/lib.rs")).unwrap();

    assert!(
        !lib_rs.contains("pub mod api;"),
        "lib.rs must not expose a public api module"
    );
    assert!(
        !repo_root.join("src/api").exists(),
        "src/api should be removed after the flat contract split"
    );

    for relative_path in [
        "src/config.rs",
        "src/diagnostics.rs",
        "src/errors.rs",
        "src/family.rs",
        "src/format.rs",
        "src/request.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(path.exists(), "{} should exist", path.display());
    }
}

#[test]
fn diagram_local_mermaid_parser_modules_are_gone() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for relative_path in [
        "src/diagrams/class/parser",
        "src/diagrams/sequence/parser",
        "src/diagrams/mmds/mod.rs",
        "src/diagrams/mmds/instance.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            !path.exists(),
            "{} should not exist after frontend promotion",
            path.display()
        );
    }
}

#[test]
fn shared_graph_family_modules_do_not_depend_on_flowchart_owned_paths() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked_files = 0usize;

    for relative_dir in ["src/engines", "src/render", "src/graph", "src/mmds"] {
        let dir = repo_root.join(relative_dir);
        let mut files = Vec::new();
        collect_rust_files(&dir, &mut files);

        for path in files {
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                !content.contains("diagrams::flowchart::engine")
                    && !content.contains("diagrams::flowchart::routing")
                    && !content.contains("diagrams::flowchart::render"),
                "shared graph-family module {} still depends on a flowchart-owned path",
                path.display()
            );
            checked_files += 1;
        }
    }

    assert!(
        checked_files > 0,
        "expected to scan shared graph-family source files"
    );
}

#[test]
fn default_registry_source_does_not_register_mmds() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/registry.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        !content.contains("mmds::definition()"),
        "default registry source must not register MMDS as a logical diagram"
    );
}

#[test]
fn render_root_stays_a_namespace_not_a_direct_render_facade() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/mod.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    for forbidden in [
        "pub use graph::RenderOptions",
        "pub use graph::SvgOptions",
        "pub use graph::render",
        "pub use graph::render_svg",
        "pub use graph::render_svg_from_geometry",
    ] {
        assert!(
            !content.contains(forbidden),
            "top-level render namespace must not re-export direct graph render APIs: {forbidden}"
        );
    }
}

#[test]
fn mmds_split_is_directory_based_and_explicit() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(
        repo_root.join("src/mmds/mod.rs").exists(),
        "mmds contract namespace must be directory-based"
    );
    assert!(
        !repo_root.join("src/mmds.rs").exists(),
        "legacy flat mmds.rs module should be removed"
    );

    for relative_path in [
        "src/frontends/mmds/detect.rs",
        "src/frontends/mmds/parse.rs",
        "src/frontends/mmds/hydrate.rs",
        "src/frontends/mmds/render_input.rs",
        "src/mmds/contract.rs",
        "src/mmds/mermaid.rs",
        "src/mmds/output.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(path.exists(), "{} should exist", path.display());
    }
}

#[test]
fn engine_taxonomy_uses_explicit_engine_and_algorithm_namespaces() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(repo_root.join("src/engines/graph/flux.rs").exists());
    assert!(repo_root.join("src/engines/graph/mermaid.rs").exists());
    assert!(
        repo_root
            .join("src/engines/graph/algorithms/layered")
            .exists()
    );
    assert!(!repo_root.join("src/engines/graph/cose.rs").exists());
    assert!(
        !repo_root
            .join("src/engines/graph/layered_engine.rs")
            .exists()
    );
}

#[test]
fn engine_graph_root_does_not_flatten_engine_taxonomy() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/graph/mod.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    for forbidden in [
        "pub use flux::FluxLayeredEngine",
        "pub use mermaid::MermaidLayeredEngine",
        "pub use algorithms::layered::MeasurementMode",
        "pub use algorithms::layered::run_layered_layout",
    ] {
        assert!(
            !content.contains(forbidden),
            "engines::graph root must not flatten the engine taxonomy: {forbidden}"
        );
    }
}
