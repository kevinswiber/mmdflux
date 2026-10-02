use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::{Value, json};

fn mmdflux() -> Command {
    cargo_bin_cmd!("mmdflux")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("mmdflux-diff-{name}-{}-{nanos}", process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const BEFORE: &str = "graph TD\nA-->B\nC-->D\ninv[Inventory]\n";
const AFTER: &str = "graph TD\nB-->|go|C\nC-->D\ninv[Stock]\n";

fn diff_json(args: &[&str]) -> Value {
    let output = mmdflux().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "diff failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("diff output should be JSON")
}

fn kinds(wire: &Value) -> Vec<String> {
    wire["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["kind"].as_str().unwrap().to_string())
        .collect()
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn diff_emits_model_layer_wire_json_by_default() {
    let dir = TempDir::new("default");
    let before = dir.file("before.mmd", BEFORE);
    let after = dir.file("after.mmd", AFTER);

    let wire = diff_json(&["diff", path_str(&before), path_str(&after)]);

    assert_eq!(wire["schema"], "mmdflux.diff.v1");
    let mut kinds = kinds(&wire);
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            "EdgeAdded",
            "EdgeRemoved",
            "NodeLabelChanged",
            "NodeRemoved"
        ]
    );
    assert!(
        wire["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change["layer"] == "model" && change.get("evidence").is_none())
    );
}

#[test]
fn diff_layer_all_includes_geometry_and_evidence_is_opt_in() {
    let dir = TempDir::new("layer");
    let before = dir.file("before.mmd", BEFORE);
    let after = dir.file("after.mmd", AFTER);

    let wire = diff_json(&[
        "diff",
        "--layer",
        "all",
        "--geometry-level",
        "routed",
        "--evidence",
        path_str(&before),
        path_str(&after),
    ]);

    let changes = wire["changes"].as_array().unwrap();
    assert!(changes.iter().any(|change| change["layer"] == "geometry"));
    assert!(
        changes
            .iter()
            .all(|change| change.get("evidence").is_some())
    );
    assert_eq!(wire["after_geometry_level"], "routed");
}

#[test]
fn diff_reads_a_pair_envelope_from_stdin() {
    let envelope = json!({ "before": BEFORE, "after": AFTER }).to_string();
    let output = mmdflux()
        .args(["diff", "--pair", "-"])
        .write_stdin(envelope)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(kinds(&wire).contains(&"EdgeRemoved".to_string()));
}

#[test]
fn diff_accepts_mmds_and_mermaid_inputs_together() {
    let dir = TempDir::new("mixed");
    let mmds = mmdflux()
        .args(["-f", "mmds"])
        .write_stdin(BEFORE)
        .output()
        .unwrap();
    assert!(mmds.status.success());
    let before = dir.file("before.json", &String::from_utf8(mmds.stdout).unwrap());
    let after = dir.file("after.mmd", AFTER);

    let wire = diff_json(&["diff", path_str(&before), path_str(&after)]);
    assert!(kinds(&wire).contains(&"NodeLabelChanged".to_string()));
}

#[test]
fn diff_summary_format_prints_one_line_per_change() {
    let dir = TempDir::new("summary");
    let before = dir.file("before.mmd", BEFORE);
    let after = dir.file("after.mmd", AFTER);

    mmdflux()
        .args(["diff", "-f", "summary", path_str(&before), path_str(&after)])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "4 changes: 1 added, 2 removed, 1 changed, 0 moved",
        ))
        .stdout(predicate::str::contains("- EdgeRemoved edge e0 A --> B"))
        .stdout(predicate::str::contains(
            "+ EdgeAdded edge e0 B --> C \"go\"",
        ))
        .stdout(predicate::str::contains(
            "~ NodeLabelChanged node inv \"Stock\": \"Inventory\" -> \"Stock\"",
        ));
}

#[test]
fn diff_exit_code_flag_reports_whether_documents_differ() {
    let dir = TempDir::new("exit");
    let before = dir.file("before.mmd", BEFORE);
    let after = dir.file("after.mmd", AFTER);

    mmdflux()
        .args(["diff", "--exit-code", path_str(&before), path_str(&before)])
        .assert()
        .code(0);
    mmdflux()
        .args(["diff", "--exit-code", path_str(&before), path_str(&after)])
        .assert()
        .code(1);
    mmdflux()
        .args(["diff", path_str(&before), path_str(&after)])
        .assert()
        .code(0);
}

#[test]
fn diff_refuses_mismatched_diagram_types_unless_forced() {
    let dir = TempDir::new("types");
    let before = dir.file("before.mmd", "flowchart LR\napi[API] --> auth[Auth]\n");
    let after = dir.file(
        "after.mmd",
        "classDiagram\nclass api\nclass auth\napi --> auth\n",
    );

    mmdflux()
        .args(["diff", path_str(&before), path_str(&after)])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "cannot diff a flowchart diagram against a class diagram",
        ));

    let wire = diff_json(&["diff", "--force", path_str(&before), path_str(&after)]);
    assert_eq!(kinds(&wire)[0], "DiagramTypeChanged");
}

#[test]
fn diff_reports_inputs_it_cannot_compare() {
    let dir = TempDir::new("errors");
    let flowchart = dir.file("flow.mmd", "graph TD\nA-->B\n");
    let sequence = dir.file("seq.mmd", "sequenceDiagram\nA->>B: hi\n");

    mmdflux()
        .args(["diff", path_str(&flowchart), path_str(&sequence)])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("after:"));
    mmdflux()
        .args(["diff", path_str(&flowchart), "/nonexistent/after.mmd"])
        .assert()
        .code(2);
    mmdflux()
        .args(["diff", path_str(&flowchart)])
        .assert()
        .failure();
}

#[test]
fn top_level_help_lists_the_diff_subcommand() {
    mmdflux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("diff"));
}

// `diff` and `help` were valid input filenames before the subcommand existed.
// The legacy `mmdflux [OPTIONS] [INPUT]` reading still wins when a file of that
// name exists and the arguments don't form a valid diff invocation.

#[test]
fn a_file_named_diff_still_renders_as_the_legacy_input() {
    let dir = TempDir::new("legacy-diff");
    dir.file("diff", "graph TD\nA-->B\n");

    mmdflux()
        .current_dir(&dir.0)
        .arg("diff")
        .assert()
        .success()
        .stdout(predicate::str::contains("A").and(predicate::str::contains("B")));
    mmdflux()
        .current_dir(&dir.0)
        .args(["diff", "-f", "ascii"])
        .assert()
        .success()
        .stdout(predicate::str::contains("+"));
    mmdflux()
        .current_dir(&dir.0)
        .args(["-f", "ascii", "diff"])
        .assert()
        .success();
}

#[test]
fn a_file_named_diff_does_not_shadow_the_diff_subcommand() {
    let dir = TempDir::new("legacy-diff-shadow");
    dir.file("diff", "graph TD\nA-->B\n");
    dir.file("before.mmd", BEFORE);
    dir.file("after.mmd", AFTER);

    let output = mmdflux()
        .current_dir(&dir.0)
        .args(["diff", "before.mmd", "after.mmd"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let wire: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(wire["schema"], "mmdflux.diff.v1");

    // Neither reading parses: the diff subcommand's error is reported.
    mmdflux()
        .current_dir(&dir.0)
        .args(["diff", "before.mmd"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("AFTER"));
}

#[test]
fn bare_diff_without_a_file_named_diff_prints_diff_usage() {
    let dir = TempDir::new("no-legacy-diff");

    mmdflux()
        .current_dir(&dir.0)
        .arg("diff")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("mmdflux diff"));
}

#[test]
fn a_file_named_help_renders_as_the_legacy_input() {
    let dir = TempDir::new("legacy-help");
    dir.file("help", "graph TD\nA-->B\n");

    mmdflux()
        .current_dir(&dir.0)
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("A").and(predicate::str::contains("Usage").not()));
}

#[test]
fn diff_maps_initialization_failures_to_exit_status_two() {
    let pair = json!({"before": "graph TD\nA-->B\n", "after": "graph TD\nA-->B\n"}).to_string();

    mmdflux()
        .env("MMDFLUX_LOG", "[")
        .args(["diff", "--pair", "-", "--exit-code"])
        .write_stdin(pair)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid log filter"));
}

const UNION_BEFORE: &str = "graph TD\nA[Start]-->B[Build]\nB-->C[Old test]\nX[Lint]-->A\n";
const UNION_AFTER: &str = "graph TD\nA[Start]-->B[Build]\nB-->C[New test]\nB-->D[Deploy]\n";

#[test]
fn diff_emit_union_draws_both_sides_as_text_by_default() {
    let dir = TempDir::new("union-text");
    let before = dir.file("before.mmd", UNION_BEFORE);
    let after = dir.file("after.mmd", UNION_AFTER);

    let output = mmdflux()
        .args(["diff", "--emit", "union", "--markers", "--color", "off"])
        .arg(&before)
        .arg(&after)
        .output()
        .unwrap();

    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    for label in ["- Lint", "~ New test", "+ Deploy", "Start", "Build"] {
        assert!(text.contains(label), "{label} missing:\n{text}");
    }
    assert!(
        !text.contains('\u{1b}'),
        "--color off should be plain:\n{text}"
    );
}

#[test]
fn diff_emit_union_colors_text_when_asked() {
    let dir = TempDir::new("union-color");
    let before = dir.file("before.mmd", UNION_BEFORE);
    let after = dir.file("after.mmd", UNION_AFTER);

    let output = mmdflux()
        .args(["diff", "--emit", "union", "--color", "always"])
        .arg(&before)
        .arg(&after)
        .output()
        .unwrap();

    let text = String::from_utf8(output.stdout).unwrap();
    // #2ea043 (added) and #8b949e (removed) as 24-bit foreground colors.
    assert!(text.contains("\u{1b}[38;2;46;160;67m"), "{text}");
    assert!(text.contains("\u{1b}[38;2;139;148;158m"), "{text}");
}

#[test]
fn diff_emit_union_svg_carries_default_styles() {
    let dir = TempDir::new("union-svg");
    let before = dir.file("before.mmd", UNION_BEFORE);
    let after = dir.file("after.mmd", UNION_AFTER);

    mmdflux()
        .args(["diff", "--emit", "union", "-f", "svg"])
        .arg(&before)
        .arg(&after)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("<svg")
                .and(predicate::str::contains("#2ea043"))
                .and(predicate::str::contains("#d29922"))
                .and(predicate::str::contains("#8b949e")),
        );
}

#[test]
fn diff_emit_union_mmds_is_laid_out_and_tagged() {
    let dir = TempDir::new("union-mmds");
    let before = dir.file("before.mmd", UNION_BEFORE);
    let after = dir.file("after.mmd", UNION_AFTER);

    let union = diff_json(&[
        "diff",
        "--emit",
        "union",
        "-f",
        "mmds",
        before.to_str().unwrap(),
        after.to_str().unwrap(),
    ]);

    assert_eq!(union["nodes"].as_array().unwrap().len(), 5);
    assert_eq!(
        union["extensions"]["org.mmdflux.diff.v1"]["nodes"],
        json!({"C": "changed", "D": "added", "X": "removed"})
    );
    assert_eq!(
        union["extensions"]["org.mmdflux.diff.v1"]["edges"]["e3"],
        json!({"status": "removed", "before_id": "e2", "after_id": null})
    );
    let ghost = &union["edges"][3];
    assert_eq!(
        (&ghost["source"], &ghost["target"]),
        (&json!("X"), &json!("A"))
    );
    assert_eq!(ghost["stroke"], "dotted");

    // The output is a regular MMDS document: it renders again as-is.
    let rendered = mmdflux()
        .args(["-f", "text"])
        .write_stdin(serde_json::to_string(&union).unwrap())
        .output()
        .unwrap();
    assert!(rendered.status.success());
    assert!(String::from_utf8_lossy(&rendered.stdout).contains("Lint"));
}

#[test]
fn diff_emit_union_exit_code_reports_tagged_items() {
    let dir = TempDir::new("union-exit");
    let before = dir.file("before.mmd", UNION_BEFORE);
    let after = dir.file("after.mmd", UNION_AFTER);
    let same = dir.file("same.mmd", UNION_AFTER);

    mmdflux()
        .args(["diff", "--emit", "union", "--exit-code"])
        .arg(&before)
        .arg(&after)
        .assert()
        .code(1);
    mmdflux()
        .args(["diff", "--emit", "union", "--exit-code"])
        .arg(&after)
        .arg(&same)
        .assert()
        .code(0);
}

#[test]
fn diff_rejects_formats_that_do_not_match_emit() {
    let dir = TempDir::new("union-formats");
    let before = dir.file("before.mmd", UNION_BEFORE);
    let after = dir.file("after.mmd", UNION_AFTER);

    mmdflux()
        .args(["diff", "--emit", "union", "-f", "json"])
        .arg(&before)
        .arg(&after)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--emit union prints text, svg or mmds",
        ));
    mmdflux()
        .args(["diff", "-f", "svg"])
        .arg(&before)
        .arg(&after)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("-f svg needs --emit union"));
}

fn union_pair(before: &str, after: &str, format: &str) -> process::Output {
    let pair = json!({"before": before, "after": after}).to_string();
    mmdflux()
        .args(["diff", "--pair", "-", "--emit", "union", "-f", format])
        .write_stdin(pair)
        .output()
        .unwrap()
}

fn union_pair_mmds(before: &str, after: &str) -> Value {
    let output = union_pair(before, after, "mmds");
    assert!(
        output.status.success(),
        "union failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("union output should be MMDS JSON")
}

/// Materialize `source` as MMDS JSON with its edges renamed to `ids`, as an
/// imported document with non-positional edge ids might arrive.
fn mmds_with_edge_ids(source: &str, ids: &[&str]) -> String {
    let document = mmdflux::materialize_diagram(source, &mmdflux::RenderConfig::default()).unwrap();
    let mut document = serde_json::to_value(&document).unwrap();
    let edges = document["edges"].as_array_mut().unwrap();
    assert_eq!(edges.len(), ids.len());
    for (edge, id) in edges.iter_mut().zip(ids) {
        edge["id"] = json!(id);
    }
    document.to_string()
}

#[test]
fn diff_emit_union_renders_nested_subgraphs_in_every_format() {
    let nested = "graph TD\nsubgraph Outer\nsubgraph Inner\nA\nend\nend\n";
    let removed_inner = (
        "graph TD\nsubgraph Outer\nsubgraph Inner[Gone]\nA\nend\nB\nend\n",
        "graph TD\nsubgraph Outer\nB\nend\n",
    );
    for (before, after) in [(nested, nested), removed_inner] {
        for format in ["text", "svg", "mmds"] {
            let output = union_pair(before, after, format);
            assert!(
                output.status.success(),
                "-f {format} failed for {before:?} -> {after:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    let union = union_pair_mmds(removed_inner.0, removed_inner.1);
    assert_eq!(
        union["extensions"]["org.mmdflux.diff.v1"]["subgraphs"]["Inner"],
        "removed"
    );
}

#[test]
fn diff_emit_union_mmds_tags_sparse_imported_edges_under_output_ids() {
    let before = mmds_with_edge_ids("graph TD\nA-->|old|B\n", &["e7"]);
    let after = mmds_with_edge_ids("graph TD\nA-->|new|B\n", &["e7"]);

    let union = union_pair_mmds(&before, &after);

    let output_ids: Vec<&str> = union["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["id"].as_str().unwrap())
        .collect();
    assert_eq!(output_ids, ["e0"]);
    assert_eq!(
        union["extensions"]["org.mmdflux.diff.v1"]["edges"],
        json!({"e0": {"status": "changed", "before_id": "e7", "after_id": "e7"}})
    );
}

#[test]
fn diff_emit_union_mmds_keeps_removed_ghost_apart_from_colliding_after_id() {
    let before = mmds_with_edge_ids("graph TD\nX-->A\n", &["e0"]);
    let after = mmds_with_edge_ids("graph TD\nA-->B\n", &["e1"]);

    let union = union_pair_mmds(&before, &after);

    let edges = union["edges"].as_array().unwrap();
    let tags = &union["extensions"]["org.mmdflux.diff.v1"]["edges"];
    let styles = &union["extensions"]["org.mmdflux.node-style.v1"]["edges"];
    assert_eq!(edges.len(), 2);
    for edge in edges {
        let id = edge["id"].as_str().unwrap();
        let (status, color) = if edge["source"] == "A" {
            ("added", "#2ea043")
        } else {
            ("removed", "#8b949e")
        };
        assert_eq!(tags[id]["status"], status, "edge {id}: {edge}");
        assert_eq!(styles[id]["stroke"], color, "edge {id}: {edge}");
    }
}
