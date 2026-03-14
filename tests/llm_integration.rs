//! End-to-end CLI integration tests for LLM integration features.
//!
//! Tests JSON output, lint mode, show-ids, and their combinations.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;

fn mmdflux() -> Command {
    cargo_bin_cmd!("mmdflux")
}

// =========================================================================
// JSON Output Tests
// =========================================================================

#[test]
fn json_from_file() {
    mmdflux()
        .args(["-f", "json", "tests/fixtures/flowchart/simple.mmd"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\":"));
}

#[test]
fn json_from_stdin() {
    mmdflux()
        .args(["-f", "json"])
        .write_stdin("graph TD\nA[Start] --> B[End]\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"nodes\""));
}

#[test]
fn json_has_required_top_level_fields() {
    let output = mmdflux()
        .args(["-f", "json"])
        .write_stdin("graph TD\nA --> B\n")
        .output()
        .unwrap();

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.get("version").is_some());
    assert!(json.get("defaults").is_some());
    assert!(json.get("metadata").is_some());
    assert!(json.get("nodes").is_some());
    assert!(json.get("edges").is_some());
    assert!(json.get("subgraphs").is_none());
}

#[test]
fn json_with_subgraphs() {
    let output = mmdflux()
        .args(["-f", "json"])
        .write_stdin("graph TD\nsubgraph sg1[Group]\nA --> B\nend\n")
        .output()
        .unwrap();

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!json["subgraphs"].as_array().unwrap().is_empty());
}

#[test]
fn json_edge_labels() {
    let output = mmdflux()
        .args(["-f", "json"])
        .write_stdin("graph TD\nA -->|yes| B\n")
        .output()
        .unwrap();

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let edges = json["edges"].as_array().unwrap();
    assert_eq!(edges[0]["label"], "yes");
}

// =========================================================================
// Lint Tests (file-based)
// =========================================================================

#[test]
fn lint_from_file() {
    mmdflux()
        .args(["--lint", "tests/fixtures/flowchart/simple.mmd"])
        .assert()
        .success();
}

#[test]
fn lint_invalid_exits_nonzero() {
    mmdflux()
        .arg("--lint")
        .write_stdin("graph TD\n-->>\n")
        .assert()
        .failure();
}

// =========================================================================
// Show IDs Tests
// =========================================================================

#[test]
fn show_ids_text_output() {
    mmdflux()
        .arg("--show-ids")
        .write_stdin("graph TD\nA[Start] --> B[End]\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("A: Start"));
}

#[test]
fn show_ids_json_output() {
    let output = mmdflux()
        .args(["-f", "json", "--show-ids"])
        .write_stdin("graph TD\nA[Start] --> B[End]\n")
        .output()
        .unwrap();

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let nodes = json["nodes"].as_array().unwrap();
    let node_a = nodes.iter().find(|n| n["id"] == "A").unwrap();
    assert_eq!(node_a["label"], "A: Start");
}

// =========================================================================
// Combination Tests
// =========================================================================

#[test]
fn lint_json_combination() {
    let output = mmdflux()
        .args(["--lint", "-f", "json"])
        .write_stdin("graph TD\nA --> B\n")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["valid"], true);
}

// =========================================================================
// Error Cases
// =========================================================================

#[test]
fn json_unsupported_diagram_type() {
    mmdflux()
        .args(["-f", "json"])
        .write_stdin("sequenceDiagram\nA->>B: hello\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("do not support mmds"));
}

// =========================================================================
// Fixture Smoke Tests
// =========================================================================

#[test]
fn json_all_fixtures_produce_valid_json() {
    let fixture_dir = std::path::Path::new("tests/fixtures/flowchart");
    for entry in std::fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "mmd") {
            let output = mmdflux().args(["-f", "json"]).arg(&path).output().unwrap();

            if output.status.success() {
                let _: Value = serde_json::from_slice(&output.stdout)
                    .unwrap_or_else(|e| panic!("Invalid JSON from {:?}: {}", path, e));
            }
        }
    }
}
