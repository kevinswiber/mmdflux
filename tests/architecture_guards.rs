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

fn strip_cfg_test_items(source: &str) -> String {
    let mut kept = String::new();
    let mut skip_next_item = false;
    let mut skip_block_depth = 0usize;

    for line in source.lines() {
        let trimmed = line.trim();

        if skip_block_depth > 0 {
            skip_block_depth += trimmed.matches('{').count();
            skip_block_depth = skip_block_depth.saturating_sub(trimmed.matches('}').count());
            continue;
        }

        if skip_next_item {
            if trimmed.contains('{') {
                skip_block_depth += trimmed.matches('{').count();
                skip_block_depth = skip_block_depth.saturating_sub(trimmed.matches('}').count());
            }
            if !trimmed.ends_with(';') && skip_block_depth == 0 {
                continue;
            }
            skip_next_item = false;
            continue;
        }

        if trimmed.starts_with("#[cfg(") && trimmed.contains("test") {
            skip_next_item = true;
            continue;
        }

        kept.push_str(line);
        kept.push('\n');
    }

    kept
}

fn extract_cfg_test_items(source: &str) -> String {
    let mut test_code = String::new();
    let mut in_test_block = false;
    let mut block_depth = 0usize;
    let mut cfg_test_next = false;

    for line in source.lines() {
        let trimmed = line.trim();

        if in_test_block {
            test_code.push_str(line);
            test_code.push('\n');
            block_depth += trimmed.matches('{').count();
            block_depth = block_depth.saturating_sub(trimmed.matches('}').count());
            if block_depth == 0 {
                in_test_block = false;
            }
            continue;
        }

        if cfg_test_next {
            if trimmed.contains('{') {
                in_test_block = true;
                block_depth = trimmed.matches('{').count();
                block_depth = block_depth.saturating_sub(trimmed.matches('}').count());
                test_code.push_str(line);
                test_code.push('\n');
                if block_depth == 0 {
                    in_test_block = false;
                }
            }
            cfg_test_next = false;
            continue;
        }

        if trimmed.starts_with("#[cfg(") && trimmed.contains("test") {
            cfg_test_next = true;
        }
    }

    test_code
}

fn assert_no_test_imports(dir: &Path, forbidden: &[&str], message: &str) {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);

    for path in files {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        let content = std::fs::read_to_string(&path).unwrap();

        let test_source = if file_name == "tests.rs" || file_name.ends_with("_tests.rs") {
            content
        } else {
            extract_cfg_test_items(&content)
        };

        if test_source.is_empty() {
            continue;
        }

        for needle in forbidden {
            assert!(
                !test_source.contains(needle),
                "{message}: forbidden import `{needle}` found in test code in {}",
                path.display()
            );
        }
    }
}

fn assert_no_production_imports(dir: &Path, forbidden: &[&str], message: &str) {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);

    for path in files {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if file_name == "tests.rs" || file_name.ends_with("_tests.rs") {
            continue;
        }
        let content = std::fs::read_to_string(&path).unwrap();
        let production = strip_cfg_test_items(&content);
        for needle in forbidden {
            assert!(
                !production.contains(needle),
                "{message}: forbidden import `{needle}` found in {}",
                path.display()
            );
        }
    }
}

fn assert_no_full_source_imports(dir: &Path, forbidden: &[&str], message: &str) {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);

    for path in files {
        let content = std::fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            assert!(
                !content.contains(needle),
                "{message}: forbidden import `{needle}` found in {}",
                path.display()
            );
        }
    }
}

fn assert_no_regression_test_imports(dir: &Path, forbidden: &[&str], message: &str) {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);

    for path in files {
        if path.file_name().and_then(|name| name.to_str()) != Some("regression_tests.rs") {
            continue;
        }
        let content = std::fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            assert!(
                !content.contains(needle),
                "{message}: forbidden import `{needle}` found in {}",
                path.display()
            );
        }
    }
}

fn testing_shim_import_needles() -> [String; 2] {
    let hidden_module = "testing";
    [
        format!("crate::{hidden_module}"),
        format!("mmdflux::{hidden_module}"),
    ]
}

fn lib_rs_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    std::fs::read_to_string(&path).unwrap()
}

fn parse_pub_modules_from_lib_rs() -> BTreeSet<String> {
    let content = lib_rs_source();
    let mut modules = BTreeSet::new();
    let mut hide_next_pub_mod = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "#[doc(hidden)]" {
            hide_next_pub_mod = true;
            continue;
        }

        if trimmed.starts_with("pub mod ") {
            let module = trimmed
                .strip_prefix("pub mod ")
                .and_then(|s| s.strip_suffix(';'))
                .map(str::to_string);
            if let Some(module) = module
                && !hide_next_pub_mod
            {
                modules.insert(module);
            }
            hide_next_pub_mod = false;
            continue;
        }

        if !trimmed.is_empty() && !trimmed.starts_with("#[") {
            hide_next_pub_mod = false;
        }
    }

    modules
}

fn parse_pub_use_re_exports_from_lib_rs() -> BTreeSet<String> {
    let content = lib_rs_source();
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
fn registry_source_no_longer_imports_diagrams() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/registry.rs");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(!content.contains("use crate::diagrams"));
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
        "into_payload()",
        "payload::Diagram",
        "render/ owns output production",
        "render::graph owns geometry-based graph-family emitters",
        "runtime owns graph-family solve-result dispatch",
        "render::diagram owns family-local renderers",
        "graph/ owns graph-family IR, float-space geometry, and shared policy/measurement helpers",
        "mmds/ is the MMDS contract and output namespace",
        "MMDS is a frontend, not a logical diagram type",
        "engines do not know about diagram types",
        "own layout building / measurement adapters",
        "flat top-level contract modules own the stable public contract",
        "web main.ts is composition only",
    ] {
        assert!(
            rules.contains(required),
            "dependency rules must mention: {required}"
        );
    }

    for required in [
        "builtins",
        "payload",
        "graph::grid",
        "timeline::sequence",
        "render::graph::text",
        "render_svg_from_routed_geometry",
        "directory-module shell",
        "src/render/graph/svg/edges/mod.rs",
        "src/graph/grid/routing/mod.rs",
        "src/graph/routing/orthogonal/mod.rs",
    ] {
        assert!(
            rules.contains(required),
            "dependency rules must mention the new boundary artifact: {required}"
        );
    }

    assert!(
        !rules.contains("graph_family_pipeline"),
        "dependency rules must not mention the deleted graph_family_pipeline shim"
    );
}

#[test]
fn dependency_rules_distinguish_supported_tiers_from_internal_modules() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/architecture/dependency-rules.md");
    let rules = std::fs::read_to_string(&path).unwrap();

    for required in [
        "high-level runtime facade",
        "supported low-level API",
        "builtins",
        "registry",
        "payload",
        "mmds",
        "internal implementation modules",
    ] {
        assert!(
            rules.contains(required),
            "dependency rules should distinguish the public tiers from internal modules: {required}"
        );
    }
}

#[test]
fn dependency_rules_document_mentions_layered_kernel_boundary() {
    let content = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/architecture/dependency-rules.md"),
    )
    .unwrap();

    assert!(content.contains("layered::kernel"));
    assert!(content.contains("layout building / measurement adapters"));
}

#[test]
fn engine_graph_docs_describe_layered_kernel_and_bridge_split() {
    let content = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/graph/mod.rs"),
    )
    .unwrap();

    assert!(content.contains("algorithms::layered::kernel"));
    assert!(content.contains("layout building / measurement adapters"));
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
        !repo_root.join("src/render/graph/backends").exists(),
        "graph-family solve-result adapters should not live under render::graph"
    );
    assert!(
        !repo_root
            .join("src/render/graph/layout_building.rs")
            .exists(),
        "render::graph should not keep a layout-building compatibility shim"
    );
    for relative_path in [
        "src/graph/attachment.rs",
        "src/graph/routing/mod.rs",
        "src/graph/routing/float_core.rs",
        "src/graph/routing/orthogonal/mod.rs",
        "src/graph/direction_policy.rs",
        "src/graph/measure.rs",
        "src/graph/projection.rs",
        "src/graph/space.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            path.exists(),
            "{} should exist under graph/",
            path.display()
        );
    }

    for relative_path in [
        "src/graph/routing.rs",
        "src/graph/routing_core.rs",
        "src/graph/orthogonal_router.rs",
        "src/graph/routing/orthogonal.rs",
        "src/render/graph/text_routing_core.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            !path.exists(),
            "{} should stay removed after the routing split",
            path.display()
        );
    }

    for relative_path in [
        "src/graph/grid/attachments.rs",
        "src/graph/grid/backward.rs",
        "src/graph/grid/bounds.rs",
        "src/graph/grid/intersect.rs",
        "src/graph/grid/routing/mod.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            path.exists(),
            "{} should exist under graph::grid",
            path.display()
        );
    }

    for relative_path in [
        "src/render/graph/grid_routing/mod.rs",
        "src/render/graph/grid_routing/attachments.rs",
        "src/render/graph/grid_routing/backward.rs",
        "src/render/graph/grid_routing/bounds.rs",
        "src/render/graph/grid_routing/router.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            !path.exists(),
            "{} should stay removed once grid routing is graph-owned",
            path.display()
        );
    }
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

    for relative_path in ["src/config.rs", "src/errors.rs", "src/format.rs"] {
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
    assert!(
        !repo_root.join("src/mmds/contract.rs").exists(),
        "MMDS contract helpers should now live in src/mmds/mod.rs"
    );

    for relative_path in [
        "src/frontends.rs",
        "src/mmds/detect.rs",
        "src/mmds/parse.rs",
        "src/mmds/hydrate.rs",
        "src/mmds/replay.rs",
        "src/mmds/mermaid.rs",
        "src/mmds/output.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(path.exists(), "{} should exist", path.display());
    }

    for relative_path in [
        "src/frontends/mmds/mod.rs",
        "src/frontends/mmds/detect.rs",
        "src/frontends/mmds/parse.rs",
        "src/frontends/mmds/hydrate.rs",
        "src/frontends/mmds/render_input.rs",
    ] {
        let path = repo_root.join(relative_path);
        assert!(
            !path.exists(),
            "{} should be removed after moving MMDS implementation under src/mmds/",
            path.display()
        );
    }
}

#[test]
fn mermaid_is_a_top_level_namespace_and_frontends_is_a_file_boundary() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frontends = std::fs::read_to_string(repo_root.join("src/frontends.rs")).unwrap();

    assert!(
        repo_root.join("src/frontends.rs").exists(),
        "frontends should be a root module file boundary"
    );
    assert!(
        !repo_root.join("src/frontends/mod.rs").exists(),
        "legacy directory-root frontends module should be removed"
    );
    assert!(
        !repo_root.join("src/frontends").exists(),
        "frontends should no longer require a source directory once it is a file boundary"
    );
    assert!(
        repo_root.join("src/mermaid/mod.rs").exists(),
        "mermaid should be a top-level source-ingestion namespace"
    );
    assert!(
        !repo_root.join("src/frontends/mermaid").exists(),
        "frontends::mermaid should be removed after promoting mermaid to a top-level module"
    );
    assert!(
        !frontends.contains("pub mod mmds"),
        "frontends.rs should only own source-format detection, not an inlined mmds compatibility namespace"
    );
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
    assert!(
        repo_root
            .join("src/engines/graph/algorithms/layered/float_layout.rs")
            .exists()
    );
    assert!(
        repo_root
            .join("src/engines/graph/algorithms/layered/float_router.rs")
            .exists()
    );
    assert!(
        !repo_root
            .join("src/engines/graph/algorithms/layered/svg_layout.rs")
            .exists()
    );
    assert!(
        !repo_root
            .join("src/engines/graph/algorithms/layered/svg_router.rs")
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

#[test]
fn engine_graph_root_does_not_flatten_contracts_barrel() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/graph/mod.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        !content.contains("use contracts::*"),
        "engines::graph should not flatten the contracts barrel"
    );
}

#[test]
fn engine_graph_low_level_modules_are_explicit_public_api() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/graph/mod.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    for required in ["pub mod contracts;", "pub mod registry;"] {
        assert!(
            content.contains(required),
            "engines::graph should keep the low-level engine module public: {required}"
        );
    }

    for forbidden in [
        "#[doc(hidden)]\npub mod contracts;",
        "#[doc(hidden)]\npub mod registry;",
    ] {
        assert!(
            !content.contains(forbidden),
            "engines::graph low-level modules should not be hidden: {forbidden}"
        );
    }
}

#[test]
fn render_graph_source_keeps_legacy_solve_and_render_types_non_public() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/graph/mod.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    for forbidden in [
        "pub fn render(",
        "pub fn compute_text_layout(",
        "pub struct RenderOptions",
        "pub struct SvgOptions",
        "mod backward_policy;",
        "mod route_policy;",
        "pub(crate) mod text_router;",
        "pub(crate) mod text_routing_core;",
    ] {
        assert!(
            !content.contains(forbidden),
            "legacy direct render surface should not remain public: {forbidden}"
        );
    }

    for required in [
        "pub use self::svg::SvgRenderOptions",
        "pub struct TextRenderOptions",
        "pub fn render_svg_from_geometry(",
        "pub fn render_text_from_geometry(",
    ] {
        assert!(
            content.contains(required),
            "render::graph should expose the render-only geometry API: {required}"
        );
    }
}

#[test]
fn render_graph_does_not_restore_transitional_graph_shims() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for path in [
        "src/render/graph/backward_policy.rs",
        "src/render/graph/route_policy.rs",
        "src/render/graph/routing.rs",
    ] {
        assert!(
            !repo_root.join(path).exists(),
            "render::graph should not restore the deleted transitional shim: {path}"
        );
    }
}

#[test]
fn repository_has_no_testing_shim_imports() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let needles = testing_shim_import_needles();
    let forbidden = [needles[0].as_str(), needles[1].as_str()];

    assert_no_full_source_imports(
        &repo_root.join("src"),
        &forbidden,
        "source code must not depend on the removed root-level testing shim",
    );
    assert_no_full_source_imports(
        &repo_root.join("tests"),
        &forbidden,
        "integration tests must not depend on the removed root-level testing shim",
    );
}

#[test]
fn module_dependency_map_does_not_reference_testing() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/architecture/module-dependency-scc-dag.mmd");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        !content.contains("testing"),
        "module dependency map should be regenerated after shim removal: {}",
        path.display()
    );
}

#[test]
fn module_dependency_map_no_longer_has_frontends_render_timeline_cycle() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/architecture/module-dependency-scc-dag.mmd");
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(
        !content.contains("frontends, render, timeline"),
        "module dependency SCC DAG should be regenerated after breaking the frontends/render/timeline cycle: {}",
        path.display()
    );
    assert!(
        content.contains("largest-scc-size: 1"),
        "module dependency SCC DAG should record the reduced SCC size after the cycle split: {}",
        path.display()
    );
}

#[test]
fn render_svg_source_does_not_define_layout_builders() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/graph/svg/mod.rs");
    let content = std::fs::read_to_string(&path).unwrap();

    for forbidden in [
        "build_float_layout_with_flags",
        "crate::engines::graph::algorithms::layered::LayoutConfig",
    ] {
        assert!(
            !content.contains(forbidden),
            "render::graph::svg should not define or import solve/build responsibilities: {forbidden}"
        );
    }
}

#[test]
fn render_svg_legacy_flat_modules_stay_removed() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for removed in ["src/render/graph/svg.rs", "src/render/graph/svg_metrics.rs"] {
        let path = repo_root.join(removed);
        assert!(
            !path.exists(),
            "legacy flat SVG render module should stay removed: {}",
            path.display()
        );
    }
}

#[test]
fn svg_edges_uses_directory_module_shell() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(!repo_root.join("src/render/graph/svg/edges.rs").exists());
    assert!(repo_root.join("src/render/graph/svg/edges/mod.rs").exists());
}

#[test]
fn remaining_mega_files_have_directory_module_replacements() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for (old, replacement) in [
        (
            "src/render/graph/svg/edges.rs",
            "src/render/graph/svg/edges/mod.rs",
        ),
        ("src/graph/grid/routing.rs", "src/graph/grid/routing/mod.rs"),
        (
            "src/graph/routing/orthogonal.rs",
            "src/graph/routing/orthogonal/mod.rs",
        ),
    ] {
        assert!(
            !repo_root.join(old).exists(),
            "{old} should stay removed once the directory-module replacement lands"
        );
        assert!(
            repo_root.join(replacement).exists(),
            "{replacement} should remain the replacement shell for removed mega-file {old}"
        );
    }
}

#[test]
fn svg_edges_splits_endpoint_basis_and_marker_helpers() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for required in [
        "src/render/graph/svg/edges/endpoints.rs",
        "src/render/graph/svg/edges/basis.rs",
        "src/render/graph/svg/edges/markers.rs",
    ] {
        assert!(
            repo_root.join(required).exists(),
            "svg::edges should keep split helper modules for endpoint, basis, and marker logic: {required}"
        );
    }
}

#[test]
fn graph_grid_routing_uses_directory_module_shell() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(!repo_root.join("src/graph/grid/routing.rs").exists());
    assert!(repo_root.join("src/graph/grid/routing/mod.rs").exists());
}

#[test]
fn graph_routing_orthogonal_uses_directory_module_shell() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(!repo_root.join("src/graph/routing/orthogonal.rs").exists());
    assert!(
        repo_root
            .join("src/graph/routing/orthogonal/mod.rs")
            .exists()
    );
}

#[test]
fn graph_grid_sources_use_graph_owned_projection_types_and_direct_mmds_replay() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let grid_layout = repo_root.join("src/graph/grid/layout.rs");
    let grid_derive_dir = repo_root.join("src/graph/grid/derive");
    let mmds_replay = repo_root.join("src/mmds/replay.rs");

    let mut contents = vec![
        std::fs::read_to_string(&grid_layout).unwrap(),
        std::fs::read_to_string(&mmds_replay).unwrap(),
    ];
    let mut derive_files = Vec::new();
    collect_rust_files(&grid_derive_dir, &mut derive_files);
    assert!(
        !derive_files.is_empty(),
        "expected to scan graph::grid::derive sources"
    );
    for path in derive_files {
        contents.push(std::fs::read_to_string(&path).unwrap());
    }

    for forbidden in [
        "crate::engines::graph::",
        "crate::engines::graph::algorithms::layered::GridLayoutConfig",
        "crate::engines::graph::algorithms::layered::Rect",
        "unreachable!(\"text adapter requires layered engine hints\")",
        "crate::runtime::",
    ] {
        assert!(
            contents.iter().all(|content| !content.contains(forbidden)),
            "graph::grid derivation and direct MMDS replay should not rely on layered-owned bridge types or runtime solve fallback: {forbidden}"
        );
    }
}

#[test]
fn layered_kernel_does_not_keep_grid_layout_config_alias() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let layered_mod = repo_root.join("src/engines/graph/algorithms/layered/mod.rs");
    let layered_mod = std::fs::read_to_string(&layered_mod).unwrap();

    assert!(
        !repo_root
            .join("src/engines/graph/algorithms/layered/grid_layout_config.rs")
            .exists(),
        "layered should not keep a compatibility alias file for graph-owned GridLayoutConfig"
    );

    for forbidden in [
        "mod grid_layout_config;",
        "pub use grid_layout_config::GridLayoutConfig;",
    ] {
        assert!(
            !layered_mod.contains(forbidden),
            "layered should not expose graph-owned GridLayoutConfig through a secondary namespace: {forbidden}"
        );
    }
}

#[test]
fn layered_module_declares_internal_kernel_boundary() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let layered_mod =
        std::fs::read_to_string(repo_root.join("src/engines/graph/algorithms/layered/mod.rs"))
            .unwrap();

    assert!(layered_mod.contains("pub(crate) mod kernel;"));
    assert!(!layered_mod.contains("pub mod kernel;"));
    assert!(
        repo_root
            .join("src/engines/graph/algorithms/layered/kernel/mod.rs")
            .exists()
    );
}

#[test]
fn layered_kernel_stays_graph_agnostic() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/engines/graph/algorithms/layered/kernel");
    let forbidden = &["crate::graph::"];
    assert_no_production_imports(
        &dir,
        forbidden,
        "layered::kernel should stay graph-agnostic",
    );
    assert_no_test_imports(
        &dir,
        forbidden,
        "layered::kernel tests should stay graph-agnostic",
    );
}

#[test]
fn layered_public_pure_modules_move_under_kernel() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for relative_path in [
        "src/engines/graph/algorithms/layered/graph.rs",
        "src/engines/graph/algorithms/layered/types.rs",
        "src/engines/graph/algorithms/layered/pipeline.rs",
        "src/engines/graph/algorithms/layered/debug.rs",
        "src/engines/graph/algorithms/layered/normalize.rs",
        "src/engines/graph/algorithms/layered/support.rs",
    ] {
        assert!(
            !repo_root.join(relative_path).exists(),
            "{relative_path} should move under layered/kernel/"
        );
    }

    for relative_path in [
        "src/engines/graph/algorithms/layered/kernel/graph.rs",
        "src/engines/graph/algorithms/layered/kernel/types.rs",
        "src/engines/graph/algorithms/layered/kernel/pipeline.rs",
        "src/engines/graph/algorithms/layered/kernel/debug.rs",
        "src/engines/graph/algorithms/layered/kernel/normalize.rs",
        "src/engines/graph/algorithms/layered/kernel/support.rs",
    ] {
        assert!(
            repo_root.join(relative_path).exists(),
            "{relative_path} should exist under layered/kernel/"
        );
    }
}

#[test]
fn layered_private_phase_modules_live_under_kernel() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for relative_path in [
        "src/engines/graph/algorithms/layered/kernel/acyclic.rs",
        "src/engines/graph/algorithms/layered/kernel/rank.rs",
        "src/engines/graph/algorithms/layered/kernel/rank_core.rs",
        "src/engines/graph/algorithms/layered/kernel/order.rs",
        "src/engines/graph/algorithms/layered/kernel/position.rs",
        "src/engines/graph/algorithms/layered/kernel/bk.rs",
        "src/engines/graph/algorithms/layered/kernel/border.rs",
        "src/engines/graph/algorithms/layered/kernel/nesting.rs",
        "src/engines/graph/algorithms/layered/kernel/network_simplex.rs",
        "src/engines/graph/algorithms/layered/kernel/parent_dummy_chains.rs",
        "src/engines/graph/algorithms/layered/kernel/regression_tests.rs",
    ] {
        assert!(
            repo_root.join(relative_path).exists(),
            "{relative_path} should exist under layered/kernel/"
        );
    }

    for relative_path in [
        "src/engines/graph/algorithms/layered/acyclic.rs",
        "src/engines/graph/algorithms/layered/rank.rs",
        "src/engines/graph/algorithms/layered/rank_core.rs",
        "src/engines/graph/algorithms/layered/order.rs",
        "src/engines/graph/algorithms/layered/position.rs",
        "src/engines/graph/algorithms/layered/bk.rs",
        "src/engines/graph/algorithms/layered/border.rs",
        "src/engines/graph/algorithms/layered/nesting.rs",
        "src/engines/graph/algorithms/layered/network_simplex.rs",
        "src/engines/graph/algorithms/layered/parent_dummy_chains.rs",
        "src/engines/graph/algorithms/layered/regression_tests.rs",
    ] {
        assert!(
            !repo_root.join(relative_path).exists(),
            "{relative_path} should move under layered/kernel/"
        );
    }

    assert!(
        repo_root
            .join("src/engines/graph/algorithms/layered/layout_building_tests.rs")
            .exists(),
        "layout_building_tests.rs should remain at the outer layered bridge"
    );
}

#[test]
fn layered_root_owns_only_kernel_and_graph_family_bridge_modules() {
    let layered_mod = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engines/graph/algorithms/layered/mod.rs"),
    )
    .unwrap();

    for forbidden in [
        "pub(crate) mod acyclic;",
        "pub(crate) mod rank;",
        "pub(crate) mod order;",
        "pub(crate) mod position;",
        "pub(crate) mod bk;",
    ] {
        assert!(
            !layered_mod.contains(forbidden),
            "layered root should not directly declare pure kernel phase modules: {forbidden}"
        );
    }

    for required in [
        "pub(crate) mod kernel;",
        "pub(crate) mod adapter;",
        "pub(crate) mod layout_building;",
        "pub(crate) mod float_layout;",
    ] {
        assert!(
            layered_mod.contains(required),
            "layered root should keep the kernel + bridge shape: {required}"
        );
    }
}

#[test]
fn layered_bridge_modules_import_kernel_explicitly() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for relative_path in [
        "src/engines/graph/algorithms/layered/adapter.rs",
        "src/engines/graph/algorithms/layered/measurement.rs",
        "src/engines/graph/algorithms/layered/layout_building.rs",
        "src/engines/graph/algorithms/layered/layout_subgraph_ops.rs",
        "src/engines/graph/algorithms/layered/float_layout.rs",
        "src/engines/graph/algorithms/layered/float_router.rs",
        "src/engines/graph/algorithms/layered/layout_building_tests.rs",
    ] {
        let content = std::fs::read_to_string(repo_root.join(relative_path)).unwrap();
        assert!(
            content.contains("super::kernel"),
            "{relative_path} should reference layered::kernel explicitly"
        );
    }
}

#[test]
fn graph_does_not_import_render_or_layered_kernel() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/graph");
    let forbidden: &[&str] = &[
        "crate::render::",
        "crate::engines::graph::algorithms::layered",
    ];
    assert_no_production_imports(
        &dir,
        forbidden,
        "graph/ should remain render-agnostic and layered-kernel agnostic",
    );
    assert_no_test_imports(
        &dir,
        forbidden,
        "graph/ tests should remain render-agnostic and layered-kernel agnostic",
    );
}

#[test]
fn graph_and_engine_sources_do_not_restore_render_format_internal_names() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert_no_full_source_imports(
        &repo_root.join("src/graph"),
        &[
            "SvgTextMetrics",
            "default_svg_text_metrics",
            "svg_node_dimensions",
            "text_node_dimensions",
            "text_edge_label_dimensions",
            "DEFAULT_FONT_FAMILY",
        ],
        "graph/ should use grid/proportional terminology for shared measurement",
    );

    assert_no_full_source_imports(
        &repo_root.join("src/engines"),
        &[
            "MeasurementMode::Text",
            "MeasurementMode::Svg",
            "build_svg_layout_with_flags",
            "build_node_directions_svg",
            "effective_edge_direction_svg",
            "route_svg_edge_",
        ],
        "engines/ should use float/grid/proportional terminology for internal layout helpers",
    );
}

#[test]
fn generated_dependency_maps_no_longer_reference_graph_family_pipeline() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for relative in [
        "docs/architecture/module-dependency-map.mmd",
        "docs/architecture/module-dependency-map-c4.mmd",
        "docs/architecture/module-dependency-scc-dag.mmd",
    ] {
        let content = std::fs::read_to_string(repo_root.join(relative)).unwrap();

        for required in ["payload", "builtins", "timeline"] {
            assert!(
                content.contains(required),
                "{relative} should mention the refreshed module tree entry: {required}"
            );
        }

        assert!(
            !content.contains("graph_family_pipeline"),
            "{relative} should not mention the deleted graph_family_pipeline shim"
        );

        for forbidden in ["testing", "mod_testing", "Component(mod_testing"] {
            assert!(
                !content.contains(forbidden),
                "{relative} should not mention removed testing module artifacts: {forbidden}"
            );
        }
    }
}

#[test]
fn generated_dependency_maps_show_builtins_owning_builtin_wiring() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let flat =
        std::fs::read_to_string(repo_root.join("docs/architecture/module-dependency-map.mmd"))
            .unwrap();
    let c4 =
        std::fs::read_to_string(repo_root.join("docs/architecture/module-dependency-map-c4.mmd"))
            .unwrap();

    assert!(
        !flat.contains("mod_registry --> mod_diagrams"),
        "registry should no longer depend on diagrams in the flat dependency map"
    );
    assert!(
        flat.contains("mod_builtins --> mod_diagrams"),
        "builtins should own builtin diagram wiring in the flat dependency map"
    );

    assert!(
        !c4.contains("Rel(mod_registry, mod_diagrams, \"uses\")"),
        "registry should no longer depend on diagrams in the C4 dependency map"
    );
    assert!(
        c4.contains("Rel(mod_builtins, mod_diagrams, \"uses\")"),
        "builtins should own builtin diagram wiring in the C4 dependency map"
    );
}

#[test]
fn graph_family_instances_do_not_import_runtime_or_render() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for path in [
        repo_root.join("src/diagrams/flowchart/instance.rs"),
        repo_root.join("src/diagrams/class/instance.rs"),
    ] {
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("crate::runtime"),
            "graph-family instance should not import runtime: {}",
            path.display()
        );
        assert!(
            !content.contains("crate::render::"),
            "graph-family instance should not import render: {}",
            path.display()
        );
        assert!(
            !content.contains("crate::engines::"),
            "graph-family instance should not import engines: {}",
            path.display()
        );
    }
}

#[test]
fn diagrams_do_not_import_render_or_engines() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/diagrams");
    let forbidden: &[&str] = &["crate::render::", "crate::engines::"];
    assert_no_production_imports(
        &dir,
        forbidden,
        "diagrams should stop at parse/compile/payload construction",
    );
    assert_no_test_imports(
        &dir,
        forbidden,
        "diagram tests should stop at parse/compile/payload construction",
    );
}

#[test]
fn graph_and_render_sources_do_not_import_runtime_test_support_shim() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for dir in [repo_root.join("src/graph"), repo_root.join("src/render")] {
        assert_no_full_source_imports(
            &dir,
            &["crate::runtime::test_support_tests"],
            "graph/ and render/ tests should localize helpers instead of importing runtime test shims",
        );
    }

    assert!(
        !repo_root.join("src/runtime/test_support_tests.rs").exists(),
        "runtime test shim should stay removed once graph/render tests localize their helpers",
    );

    let runtime_mod = std::fs::read_to_string(repo_root.join("src/runtime/mod.rs")).unwrap();
    assert!(
        !runtime_mod.contains("test_support_tests"),
        "runtime::mod should not expose a cross-layer test support shim",
    );
}

#[test]
fn owner_local_regression_suites_do_not_reach_back_up_the_pipeline() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let forbidden = &[
        "crate::engines::",
        "mmdflux::engines::",
        "crate::mermaid::",
        "mmdflux::mermaid::",
        "crate::diagrams::",
        "mmdflux::diagrams::",
        "crate::render_diagram(",
        "mmdflux::render_diagram(",
        "RenderConfig",
    ];

    for dir in [
        repo_root.join("src/graph"),
        repo_root.join("src/render"),
        repo_root.join("src/mmds"),
    ] {
        assert_no_regression_test_imports(
            &dir,
            forbidden,
            "owner-local regression suites should not depend on cross-pipeline setup",
        );
    }
}

#[test]
fn graph_root_does_not_own_backward_policy_module() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!repo_root.join("src/graph/backward_policy.rs").exists());
    assert!(
        !repo_root
            .join("src/graph/routing/backward_policy.rs")
            .exists()
    );
    assert!(repo_root.join("src/graph/attachment.rs").exists());
}

#[test]
fn graph_surface_does_not_export_preview_only_helpers() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/graph/mod.rs");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(!content.contains("snap_path_to_grid_preview"));
}

#[test]
fn graph_module_uses_grid_namespace_not_grid_projection_module() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/graph/mod.rs");
    let content = std::fs::read_to_string(path).unwrap();

    assert!(
        content.contains("pub mod grid;"),
        "src/graph/mod.rs should export the new graph::grid namespace"
    );
    assert!(
        !content.contains("pub mod grid_projection;"),
        "src/graph/mod.rs should stop exporting graph::grid_projection"
    );
}

#[test]
fn graph_grid_namespace_does_not_import_render() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/graph/grid");
    let forbidden: &[&str] = &["crate::render::"];
    assert_no_production_imports(&dir, forbidden, "graph::grid should remain render-agnostic");
    assert_no_test_imports(
        &dir,
        forbidden,
        "graph::grid tests should remain render-agnostic",
    );
}

#[test]
fn graph_grid_uses_explicit_routing_helper_modules() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !repo_root
            .join("src/render/graph/grid_routing/core.rs")
            .exists()
    );
    for required in [
        "src/graph/grid/attachments.rs",
        "src/graph/grid/bounds.rs",
        "src/graph/grid/backward.rs",
        "src/graph/grid/intersect.rs",
        "src/graph/grid/routing/mod.rs",
        "src/graph/grid/routing/types.rs",
        "src/graph/grid/routing/attachment_resolution.rs",
        "src/graph/grid/routing/border_nudging.rs",
        "src/graph/grid/routing/orthogonal.rs",
        "src/graph/grid/routing/probe.rs",
        "src/graph/grid/routing/route_variants.rs",
        "src/graph/grid/routing/self_edges.rs",
        "src/graph/grid/routing/draw_path.rs",
        "src/graph/grid/routing/path_selection.rs",
    ] {
        assert!(repo_root.join(required).exists(), "missing {required}");
    }
}

#[test]
fn graph_grid_routing_helpers_import_owned_modules_directly() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for helper in [
        "src/graph/grid/routing/attachment_resolution.rs",
        "src/graph/grid/routing/border_nudging.rs",
        "src/graph/grid/routing/draw_path.rs",
        "src/graph/grid/routing/orthogonal.rs",
        "src/graph/grid/routing/path_selection.rs",
        "src/graph/grid/routing/probe.rs",
        "src/graph/grid/routing/route_variants.rs",
        "src/graph/grid/routing/self_edges.rs",
        "src/graph/grid/routing/types.rs",
    ] {
        let content = std::fs::read_to_string(repo_root.join(helper)).unwrap();
        assert!(
            !content.contains("use super::{"),
            "{helper} should import sibling helpers or owning grid modules directly instead of the parent routing namespace",
        );
    }
}

#[test]
fn render_graph_facade_exposes_text_namespace_and_routed_svg_entrypoint() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/graph/mod.rs");
    let content = std::fs::read_to_string(path).unwrap();

    for required in ["pub mod text;", "pub fn render_svg_from_routed_geometry("] {
        assert!(
            content.contains(required),
            "render::graph should keep explicit low-level and routed-svg APIs: {required}"
        );
    }

    for forbidden in [
        "pub mod text_replay;",
        "pub use self::grid_routing::router::{RoutedEdge, Segment, route_all_edges};",
        "pub use self::text_adapter::geometry_to_text_layout_with_routed;",
        "pub mod text_canvas;",
        "pub(crate) mod text_edge;",
        "pub(crate) mod text_shape;",
        "pub(crate) mod text_subgraph;",
        "pub use self::text_types::Layout;",
        "pub use crate::graph::grid_projection::GridLayoutConfig;",
    ] {
        assert!(
            !content.contains(forbidden),
            "render::graph should keep low-level text replay helpers out of the facade root: {forbidden}"
        );
    }
}

#[test]
fn render_root_does_not_reexport_intersect() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/mod.rs");
    let content = std::fs::read_to_string(path).unwrap();

    assert!(
        content.contains("pub mod text;"),
        "render root should expose the shared text namespace"
    );
    assert!(
        !content.contains("pub mod primitives;"),
        "render root should stop exposing the vague primitives bucket"
    );
    assert!(
        !content.contains("pub use primitives::intersect"),
        "render root should not re-export low-level text replay intersection helpers"
    );
}

#[test]
fn sequence_renderer_does_not_import_diagrams_sequence() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/diagram/sequence/text.rs");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(!content.contains("crate::diagrams::sequence"));
}

#[test]
fn sequence_instance_does_not_import_runtime_or_render() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/diagrams/sequence/instance.rs");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(!content.contains("crate::runtime"));
    assert!(!content.contains("crate::render::"));
}

#[test]
fn engines_do_not_import_render() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/engines");
    let forbidden: &[&str] = &["crate::render::"];
    assert_no_production_imports(
        &dir,
        forbidden,
        "engines/ should not import render-owned modules",
    );
    assert_no_test_imports(
        &dir,
        forbidden,
        "engine tests should not import render-owned modules",
    );
}

#[test]
fn engines_use_engine_owned_solve_profile_terms() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/engines");
    let forbidden: &[&str] = &["crate::format::OutputFormat", "crate::config::RenderConfig"];
    assert_no_production_imports(
        &dir,
        forbidden,
        "engines/ should receive solve-profile instructions instead of format/config mapping",
    );
    assert_no_test_imports(
        &dir,
        forbidden,
        "engine tests should not import format/config mapping",
    );

    let contracts =
        std::fs::read_to_string(repo_root.join("src/engines/graph/contracts.rs")).unwrap();
    for forbidden in [
        "pub output_format:",
        "pub path_simplification:",
        "from_config(",
    ] {
        assert!(
            !contracts.contains(forbidden),
            "GraphSolveRequest should not expose legacy format/config request field or constructor: {forbidden}"
        );
    }

    let measurement = std::fs::read_to_string(
        repo_root.join("src/engines/graph/algorithms/layered/measurement.rs"),
    )
    .unwrap();
    assert!(
        !measurement.contains("for_format("),
        "layered measurement should not map render formats inside engines"
    );
}

#[test]
fn render_does_not_import_engine_adapters_or_layered_config() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = repo_root.join("src/render");
    let forbidden: &[&str] = &[
        "crate::engines::graph::GraphSolveResult",
        "crate::engines::graph::GraphSolveRequest",
        "crate::engines::graph::EngineConfig",
        "crate::engines::graph::solve_graph_family",
        "crate::engines::graph::flux::FluxLayeredEngine",
        "crate::engines::graph::mermaid::MermaidLayeredEngine",
        "crate::engines::graph::algorithms::layered::",
    ];
    assert_no_production_imports(
        &dir,
        forbidden,
        "render/ should remain engine-result and layered-config agnostic",
    );
    assert_no_test_imports(
        &dir,
        forbidden,
        "render tests should remain engine-result and layered-config agnostic",
    );
}

#[test]
fn float_render_consumers_do_not_depend_on_render_grid_routing() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let svg = std::fs::read_to_string(repo_root.join("src/render/graph/svg/mod.rs")).unwrap();

    assert!(
        !svg.contains("crate::render::graph::grid_routing::"),
        "float render helpers should not depend on render-owned grid routing internals"
    );
}

#[test]
fn runtime_does_not_contain_diagram_specific_validation_logic() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(
        !repo_root.join("src/lint.rs").exists(),
        "lint.rs should be deleted after validation logic moved to diagrams/"
    );

    let runtime = std::fs::read_to_string(repo_root.join("src/runtime/mod.rs")).unwrap();

    for forbidden in [
        "crate::lint",
        "mermaid::detect_diagram_type",
        "parse_flowchart_with_options",
        "collect_unsupported_warnings",
        "collect_subgraph_warnings",
        "STRICT_PARSE_WARNING_PREFIX",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "runtime/mod.rs should not contain diagram-specific validation logic: {forbidden}"
        );
    }
}
