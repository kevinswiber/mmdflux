//! CLI integration tests for mmdflux binary.

use std::path::Path;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn mmdflux() -> Command {
    cargo_bin_cmd!("mmdflux")
}

fn strip_ansi(input: &str) -> String {
    let mut stripped = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }

        stripped.push(ch);
    }

    stripped
}

// =============================================================================
// Debug Flag Tests
// =============================================================================

#[test]
fn cli_debug_shows_detected_diagram_type() {
    mmdflux()
        .arg("--debug")
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        .stderr(predicate::str::contains("Detected diagram type: flowchart"));
}

// =============================================================================
// SVG Format Tests
// =============================================================================

#[test]
fn cli_svg_format_renders_flowchart() {
    mmdflux()
        .args(["--format", "svg"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("<svg"));
}

#[test]
fn cli_svg_defaults_to_flux_layered_behavior() {
    let input = "graph TD\nA[Start] --> B{Check}\nB --> C[Yes]\nB --> D[No]\nD --> A\n";

    let default = mmdflux()
        .args(["--format", "svg", "--edge-preset", "straight"])
        .write_stdin(input)
        .output()
        .expect("default render should execute");
    assert!(
        default.status.success(),
        "default render failed: stderr={}",
        String::from_utf8_lossy(&default.stderr)
    );

    let explicit = mmdflux()
        .args([
            "--format",
            "svg",
            "--edge-preset",
            "straight",
            "--layout-engine",
            "flux-layered",
        ])
        .write_stdin(input)
        .output()
        .expect("flux-layered render should execute");
    assert!(
        explicit.status.success(),
        "flux-layered render failed: stderr={}",
        String::from_utf8_lossy(&explicit.stderr)
    );

    assert_eq!(
        default.stdout, explicit.stdout,
        "default svg render should match explicit flux-layered"
    );
}

#[test]
fn cli_svg_defaults_to_smooth_step_on_flux_layered() {
    let input = "graph TD\nA[Start] --> B{Check}\nB --> C[Yes]\nB --> D[No]\nD --> A\n";

    let default = mmdflux()
        .args(["--format", "svg"])
        .write_stdin(input)
        .output()
        .expect("default render should execute");
    assert!(
        default.status.success(),
        "default render failed: stderr={}",
        String::from_utf8_lossy(&default.stderr)
    );

    let explicit = mmdflux()
        .args(["--format", "svg", "--edge-preset", "smooth-step"])
        .write_stdin(input)
        .output()
        .expect("smooth-step render should execute");
    assert!(
        explicit.status.success(),
        "smooth-step render failed: stderr={}",
        String::from_utf8_lossy(&explicit.stderr)
    );

    assert_eq!(
        default.stdout, explicit.stdout,
        "default svg render should match explicit smooth-step on flux-layered"
    );
}

#[test]
fn cli_svg_mermaid_layered_keeps_engine_default_when_no_style_is_selected() {
    let input = "graph TD\nA[Start] --> B{Check}\nB --> C[Yes]\nB --> D[No]\nD --> A\n";

    let default = mmdflux()
        .args(["--format", "svg", "--layout-engine", "mermaid-layered"])
        .write_stdin(input)
        .output()
        .expect("mermaid-layered render should execute");
    assert!(
        default.status.success(),
        "mermaid-layered render failed: stderr={}",
        String::from_utf8_lossy(&default.stderr)
    );

    let explicit = mmdflux()
        .args([
            "--format",
            "svg",
            "--layout-engine",
            "mermaid-layered",
            "--edge-preset",
            "basis",
        ])
        .write_stdin(input)
        .output()
        .expect("mermaid-layered basis render should execute");
    assert!(
        explicit.status.success(),
        "mermaid-layered basis render failed: stderr={}",
        String::from_utf8_lossy(&explicit.stderr)
    );

    assert_eq!(
        default.stdout, explicit.stdout,
        "mermaid-layered auto svg render should still match explicit basis"
    );
}

#[test]
fn cli_rejects_removed_routing_mode_flag() {
    mmdflux()
        .args(["--format", "svg", "--routing-mode", "polyline"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unexpected argument '--routing-mode' found",
        ));
}

#[test]
fn cli_rejects_removed_svg_edge_path_style_flag() {
    mmdflux()
        .args(["--format", "svg", "--svg-edge-path-style", "orthogonal"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unexpected argument '--svg-edge-path-style' found",
        ));
}

#[test]
fn cli_color_flag_accepts_off_auto_and_always() {
    for value in ["off", "auto", "always"] {
        mmdflux()
            .args(["--color", value])
            .write_stdin("graph TD\nA-->B")
            .assert()
            .success();
    }
}

#[test]
fn cli_color_always_emits_ansi_for_styled_nodes() {
    let input = include_str!("fixtures/flowchart/style-basic.mmd");

    let plain = mmdflux()
        .args(["--color", "off"])
        .write_stdin(input)
        .output()
        .expect("plain styled render should execute");
    assert!(
        plain.status.success(),
        "plain styled render failed: stderr={}",
        String::from_utf8_lossy(&plain.stderr)
    );

    let ansi = mmdflux()
        .args(["--color", "always"])
        .write_stdin(input)
        .output()
        .expect("ansi styled render should execute");
    assert!(
        ansi.status.success(),
        "ansi styled render failed: stderr={}",
        String::from_utf8_lossy(&ansi.stderr)
    );

    let plain_stdout =
        String::from_utf8(plain.stdout).expect("plain styled render should be utf-8");
    let ansi_stdout = String::from_utf8(ansi.stdout).expect("ansi styled render should be utf-8");

    assert!(ansi_stdout.contains("38;2;"));
    assert!(ansi_stdout.contains("48;2;"));
    assert_eq!(strip_ansi(&ansi_stdout), plain_stdout);
}

#[test]
fn cli_color_auto_preserves_plain_output_for_same_fixture() {
    let input = include_str!("fixtures/flowchart/style-basic.mmd");

    let plain = mmdflux()
        .args(["--color", "off"])
        .write_stdin(input)
        .output()
        .expect("plain styled render should execute");
    assert!(
        plain.status.success(),
        "plain styled render failed: stderr={}",
        String::from_utf8_lossy(&plain.stderr)
    );

    let auto = mmdflux()
        .args(["--color", "auto"])
        .write_stdin(input)
        .output()
        .expect("auto styled render should execute");
    assert!(
        auto.status.success(),
        "auto styled render failed: stderr={}",
        String::from_utf8_lossy(&auto.stderr)
    );

    let plain_stdout =
        String::from_utf8(plain.stdout).expect("plain styled render should be utf-8");
    let auto_stdout = String::from_utf8(auto.stdout).expect("auto styled render should be utf-8");

    assert!(!auto_stdout.contains("\u{1b}["));
    assert_eq!(auto_stdout, plain_stdout);
}

#[test]
fn cli_explicit_color_always_overrides_no_color_env() {
    let input = include_str!("fixtures/flowchart/style-basic.mmd");

    let ansi = mmdflux()
        .args(["--color", "always"])
        .env("NO_COLOR", "1")
        .write_stdin(input)
        .output()
        .expect("ansi styled render should execute");
    assert!(
        ansi.status.success(),
        "ansi styled render failed: stderr={}",
        String::from_utf8_lossy(&ansi.stderr)
    );

    let ansi_stdout = String::from_utf8(ansi.stdout).expect("ansi styled render should be utf-8");

    assert!(ansi_stdout.contains("38;2;"));
    assert!(ansi_stdout.contains("48;2;"));
}

// =============================================================================
// Phase 7: Style Taxonomy Tests (7.2 — new flags and types)
// =============================================================================

// --- --edge-preset flag ---

#[test]
fn cli_accepts_edge_preset_straight() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "straight"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_edge_preset_polyline() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "polyline"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_edge_preset_step() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "step"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_edge_preset_curved_step() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "curved-step"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_edge_preset_smooth_step() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "smooth-step"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_edge_preset_legacy_smoothstep_alias() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "smoothstep"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_edge_preset_basis() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "basis"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_rejects_legacy_edge_preset_bezier() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "bezier"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown edge preset"));
}

#[test]
fn cli_rejects_edge_preset_direct_as_not_a_preset() {
    mmdflux()
        .args(["--format", "svg", "--edge-preset", "direct"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("routing style")
                .or(predicate::str::contains("--routing-style direct"))
                .or(predicate::str::contains("straight")),
        );
}

// --- --routing-style flag ---

#[test]
fn cli_accepts_routing_style_polyline() {
    mmdflux()
        .args(["--format", "svg", "--routing-style", "polyline"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_routing_style_orthogonal() {
    mmdflux()
        .args(["--format", "svg", "--routing-style", "orthogonal"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_routing_style_direct() {
    mmdflux()
        .args(["--format", "svg", "--routing-style", "direct"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

// --- --curve flag ---

#[test]
fn cli_accepts_curve_basis() {
    mmdflux()
        .args(["--format", "svg", "--curve", "basis"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_accepts_curve_linear_rounded() {
    mmdflux()
        .args(["--format", "svg", "--curve", "linear-rounded"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

#[test]
fn cli_rejects_curve_legacy_alias() {
    mmdflux()
        .args(["--format", "svg", "--curve", "bezier"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure();
}

#[test]
fn cli_rejects_unknown_curve_token_with_actionable_message() {
    mmdflux()
        .args(["--format", "svg", "--curve", "spline"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown curve"))
        .stderr(predicate::str::contains(
            "expected one of: basis, linear, linear-sharp, linear-rounded",
        ));
}

#[test]
fn cli_help_mentions_curve_flag() {
    mmdflux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--curve"));
}

#[test]
fn cli_help_omits_legacy_curve_flags() {
    mmdflux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--interpolation-style").not())
        .stdout(predicate::str::contains("--corner-style").not());
}

// --- deprecated style flags removed ---

#[test]
fn cli_rejects_legacy_interpolation_style_flag() {
    mmdflux()
        .args(["--format", "svg", "--interpolation-style", "linear"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unexpected argument '--interpolation-style' found",
        ));
}

#[test]
fn cli_rejects_legacy_corner_style_flag() {
    mmdflux()
        .args(["--format", "svg", "--corner-style", "rounded"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unexpected argument '--corner-style' found",
        ));
}

// --- precedence: explicit low-level > preset ---

#[test]
fn cli_explicit_routing_style_overrides_preset() {
    // --routing-style polyline + --edge-preset step (which expands to orthogonal)
    // should produce polyline routing (explicit wins over preset).
    // We can't directly observe the routing style in output, so just check it doesn't error.
    mmdflux()
        .args([
            "--format",
            "svg",
            "--edge-preset",
            "step",
            "--routing-style",
            "polyline",
        ])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
}

// =============================================================================
// Phase 7: Terminology Tests (7.1)
// =============================================================================

#[test]
fn cli_rejects_legacy_svg_edge_curve_flag() {
    mmdflux()
        .args(["--format", "svg", "--svg-edge-curve", "orthogonal"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unexpected argument '--svg-edge-curve' found",
        ));
}

// =============================================================================
// Basic CLI Functionality
// =============================================================================

#[test]
fn cli_renders_flowchart_to_stdout() {
    mmdflux()
        .write_stdin("graph TD\nA[Start]-->B[End]")
        .assert()
        .success()
        .stdout(predicate::str::contains("Start"))
        .stdout(predicate::str::contains("End"));
}

#[test]
fn cli_renders_ascii_mode() {
    mmdflux()
        .args(["--format", "ascii"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        // ASCII mode uses + for corners, not Unicode box-drawing
        .stdout(predicate::str::contains("+"));
}

#[test]
fn cli_unknown_diagram_type_errors() {
    mmdflux()
        .write_stdin("unknownDiagram\nA->>B: hello")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown diagram type"));
}

#[test]
fn cli_sequence_diagram_renders() {
    mmdflux()
        .write_stdin("sequenceDiagram\nparticipant A\nparticipant B\nA->>B: hello")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn cli_sequence_svg_errors() {
    mmdflux()
        .args(["--format", "svg"])
        .write_stdin("sequenceDiagram\nA->>B: hello")
        .assert()
        .failure()
        .stderr(predicate::str::contains("do not support svg"));
}

// =============================================================================
// Engine Selection Tests
// =============================================================================

#[test]
fn cli_accepts_flux_layered_engine() {
    mmdflux()
        .args([
            "--layout-engine",
            "flux-layered",
            "tests/fixtures/flowchart/simple.mmd",
        ])
        .assert()
        .success();
}

#[test]
fn cli_accepts_mermaid_layered_engine() {
    mmdflux()
        .args([
            "--format",
            "svg",
            "--layout-engine",
            "mermaid-layered",
            "tests/fixtures/flowchart/simple.mmd",
        ])
        .assert()
        .success();
}

#[test]
fn cli_rejects_legacy_dagre_with_migration() {
    let output = mmdflux()
        .args([
            "--layout-engine",
            "dagre",
            "tests/fixtures/flowchart/simple.mmd",
        ])
        .output()
        .expect("command should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("flux-layered"),
        "error should suggest flux-layered: {stderr}"
    );
}

#[test]
fn cli_default_engine_is_flux_layered() {
    let default_out = mmdflux()
        .arg("tests/fixtures/flowchart/simple.mmd")
        .output()
        .expect("default render should execute");
    let explicit_out = mmdflux()
        .args([
            "--layout-engine",
            "flux-layered",
            "tests/fixtures/flowchart/simple.mmd",
        ])
        .output()
        .expect("flux-layered render should execute");
    assert!(default_out.status.success(), "default render failed");
    assert!(explicit_out.status.success(), "flux-layered render failed");
    assert_eq!(
        default_out.stdout, explicit_out.stdout,
        "default should match explicit flux-layered"
    );
}

#[test]
fn cli_layout_engine_flux_layered_matches_default() {
    let default_assert = mmdflux().write_stdin("graph TD\nA-->B").assert().success();
    let default_out = String::from_utf8_lossy(&default_assert.get_output().stdout).to_string();

    let explicit_assert = mmdflux()
        .args(["--layout-engine", "flux-layered"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();
    let explicit_out = String::from_utf8_lossy(&explicit_assert.get_output().stdout).to_string();

    assert_eq!(default_out, explicit_out);
}

#[test]
fn cli_layout_engine_unknown_fails_cleanly() {
    mmdflux()
        .args(["--layout-engine", "nonexistent"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown engine"));
}

#[test]
fn cli_layout_engine_unknown_fails_for_class() {
    mmdflux()
        .args(["--layout-engine", "nonexistent"])
        .write_stdin("classDiagram\nA --> B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown engine"));
}

#[test]
fn cli_layout_engine_rejected_for_sequence() {
    mmdflux()
        .args(["--layout-engine", "flux-layered"])
        .write_stdin("sequenceDiagram\nA->>B: hello")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "layout engine selection is not supported for sequence diagrams",
        ));
}

#[test]
fn cli_layout_engine_unavailable_fails_cleanly() {
    // Without engine-elk feature compiled, this should fail with actionable error
    #[cfg(not(feature = "engine-elk"))]
    mmdflux()
        .args(["--layout-engine", "elk-layered"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("engine-elk"));
}

#[test]
fn cli_rejects_legacy_cose_with_migration() {
    mmdflux()
        .args(["--layout-engine", "cose"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("flux-layered"));
}

// =============================================================================
// MMDS JSON Output Tests
// =============================================================================

#[test]
fn cli_json_output_is_mmds_layout_by_default() {
    mmdflux()
        .args(["--format", "mmds"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\": 1"))
        .stdout(predicate::str::contains("\"geometry_level\": \"layout\""))
        .stdout(predicate::str::contains("\"metadata\""))
        .stdout(predicate::str::contains("\"bounds\""))
        .stdout(predicate::str::contains("\"nodes\""))
        .stdout(predicate::str::contains("\"position\""))
        .stdout(predicate::str::contains("\"size\""))
        .stdout(predicate::str::contains("\"id\": \"e0\""))
        .stdout(predicate::str::contains("\"path\"").not());
}

#[test]
fn cli_json_routed_level_includes_paths() {
    mmdflux()
        .args(["--format", "mmds", "--geometry-level", "routed"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"geometry_level\": \"routed\""))
        .stdout(predicate::str::contains("\"path\""))
        .stdout(predicate::str::contains("\"is_backward\""));
}

#[test]
fn cli_json_routed_level_accepts_path_simplification_lossless() {
    mmdflux()
        .args([
            "--format",
            "mmds",
            "--geometry-level",
            "routed",
            "--path-simplification",
            "lossless",
        ])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"geometry_level\": \"routed\""))
        .stdout(predicate::str::contains("\"path\""));
}

#[test]
fn cli_json_class_diagram_produces_mmds() {
    mmdflux()
        .args(["--format", "mmds"])
        .write_stdin("classDiagram\nA --> B")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\": 1"))
        .stdout(predicate::str::contains("\"geometry_level\": \"layout\""))
        .stdout(predicate::str::contains("\"diagram_type\": \"class\""));
}

#[test]
fn cli_json_errors_for_unsupported_type() {
    mmdflux()
        .args(["--format", "mmds"])
        .write_stdin("sequenceDiagram\nA->>B: hello")
        .assert()
        .failure()
        .stderr(predicate::str::contains("do not support mmds"));
}

#[test]
fn cli_json_alias_maps_to_mmds() {
    mmdflux()
        .args(["--format", "json"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"geometry_level\": \"layout\""));
}

#[test]
fn cli_renders_routed_mmds_as_text_by_ignoring_paths() {
    mmdflux()
        .write_stdin(include_str!("fixtures/mmds/positioned/routed-basic.json"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Start"));
}

#[test]
fn cli_renders_positioned_mmds_to_svg() {
    mmdflux()
        .args(["--format", "svg"])
        .write_stdin(include_str!("fixtures/mmds/positioned/routed-basic.json"))
        .assert()
        .success()
        .stdout(predicate::str::starts_with("<svg"));
}

#[test]
fn cli_mmds_includes_defaults_block_and_omits_default_edge_fields() {
    let assert = mmdflux()
        .args(["--format", "mmds"])
        .write_stdin("graph TD\nA-->B")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["defaults"]["node"]["shape"], "rectangle");
    assert_eq!(parsed["defaults"]["edge"]["stroke"], "solid");
    assert_eq!(parsed["defaults"]["edge"]["arrow_start"], "none");
    assert_eq!(parsed["defaults"]["edge"]["arrow_end"], "normal");
    assert_eq!(parsed["defaults"]["edge"]["minlen"], 1);
    let edge = &parsed["edges"][0];
    assert!(edge.get("stroke").is_none());
    assert!(edge.get("arrow_start").is_none());
    assert!(edge.get("arrow_end").is_none());
    assert!(edge.get("minlen").is_none());
    assert!(parsed.get("subgraphs").is_none());
}

#[test]
fn cli_mmds_keeps_non_default_edge_fields() {
    mmdflux()
        .args(["--format", "mmds"])
        .write_stdin("graph TD\nA -.-> B\nC --x D\nE ----> F")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"stroke\": \"dotted\""))
        .stdout(predicate::str::contains("\"arrow_end\": \"cross\""))
        .stdout(predicate::str::contains("\"minlen\": 3"));
}

#[test]
fn cli_mmds_emits_node_style_extension_when_styles_exist() {
    let assert = mmdflux()
        .args(["--format", "mmds"])
        .write_stdin(include_str!("fixtures/flowchart/style-basic.mmd"))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert!(
        parsed["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile == "mmdflux-node-style-v1")
    );
    assert_eq!(
        parsed["extensions"]["org.mmdflux.node-style.v1"]["nodes"]["A"]["fill"],
        "#ffeeaa"
    );
    assert_eq!(
        parsed["extensions"]["org.mmdflux.node-style.v1"]["nodes"]["A"]["color"],
        "#111"
    );
}

// =============================================================================
// All-Fixtures Smoke Test
// =============================================================================

/// Discover all flowchart fixture files from tests/fixtures/flowchart/.
fn discover_flowchart_fixtures() -> Vec<std::path::PathBuf> {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("flowchart");
    let mut fixtures: Vec<_> = std::fs::read_dir(&fixtures_dir)
        .expect("fixtures directory should exist")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "mmd") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    fixtures.sort();
    fixtures
}

#[test]
fn cli_renders_all_flowchart_fixtures_successfully() {
    let fixtures = discover_flowchart_fixtures();
    assert!(
        !fixtures.is_empty(),
        "should discover at least one fixture file"
    );

    let snapshots_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("flowchart");

    for fixture_path in &fixtures {
        let fixture_name = fixture_path.file_stem().unwrap().to_str().unwrap();
        let input = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", fixture_path.display()));

        // Fixture must render successfully with non-empty output
        let assert = mmdflux().write_stdin(input.as_str()).assert().success();
        let output = assert.get_output();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.trim().is_empty(),
            "fixture {fixture_name} produced empty output"
        );

        // If a snapshot exists, CLI output must match it exactly
        let snapshot_path = snapshots_dir.join(format!("{fixture_name}.txt"));
        if snapshot_path.exists() {
            let expected = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|e| {
                panic!("failed to read snapshot {}: {e}", snapshot_path.display())
            });
            assert_eq!(
                stdout.as_ref(),
                expected.as_str(),
                "CLI output for fixture {fixture_name} differs from snapshot"
            );
        }
    }
}

#[test]
fn cli_mermaid_format_generates_mermaid_from_mmds_input() {
    mmdflux()
        .args(["--format", "mermaid"])
        .write_stdin(include_str!("fixtures/mmds/positioned/routed-basic.json"))
        .assert()
        .success()
        .stdout(predicate::str::starts_with("flowchart"))
        .stdout(predicate::str::contains("-->"));
}

#[test]
fn cli_mermaid_format_roundtrip_preserves_topology() {
    // Generate MMDS from Mermaid
    let mmds_output = mmdflux()
        .args(["--format", "mmds"])
        .write_stdin("flowchart TD\nA[Start] --> B[End]")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    // Convert MMDS back to Mermaid
    mmdflux()
        .args(["--format", "mermaid"])
        .write_stdin(mmds_output)
        .assert()
        .success()
        .stdout(predicate::str::starts_with("flowchart TD"))
        .stdout(predicate::str::contains("A[Start]"))
        .stdout(predicate::str::contains("B[End]"))
        .stdout(predicate::str::contains("A --> B"));
}

#[test]
fn cli_mermaid_format_errors_for_non_mmds_input() {
    mmdflux()
        .args(["--format", "mermaid"])
        .write_stdin("flowchart TD\nA --> B")
        .assert()
        .failure()
        .stderr(predicate::str::contains("do not support mermaid"));
}

// --- Task 5.3: Lineage naming policy ---

#[test]
fn cli_help_spacing_flags_do_not_say_dagre() {
    mmdflux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dagre").not());
}

#[test]
fn cli_help_layout_engine_does_not_suggest_bare_dagre() {
    mmdflux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--layout-engine dagre").not());
}

// --- Task 4.5: MMDS engine metadata ---

#[test]
fn cli_mmds_routed_default_engine_is_flux_layered() {
    mmdflux()
        .args([
            "--format",
            "mmds",
            "--geometry-level",
            "routed",
            "tests/fixtures/flowchart/simple.mmd",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"engine\": \"flux-layered\""));
}
