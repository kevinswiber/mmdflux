use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;

fn mmdflux() -> Command {
    cargo_bin_cmd!("mmdflux")
}

#[test]
fn test_lint_valid_input_exit_0() {
    mmdflux()
        .arg("--lint")
        .write_stdin("graph TD\nA --> B\n")
        .assert()
        .success();
}

#[test]
fn test_lint_invalid_input_exit_1() {
    mmdflux()
        .arg("--lint")
        .write_stdin("not valid mermaid")
        .assert()
        .failure();
}

#[test]
fn test_lint_human_readable_error_on_stderr() {
    mmdflux()
        .arg("--lint")
        .write_stdin("not valid mermaid")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_lint_valid_no_output() {
    mmdflux()
        .arg("--lint")
        .write_stdin("graph TD\nA --> B\n")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn test_lint_with_warnings() {
    mmdflux()
        .arg("--lint")
        .write_stdin("graph TD\nA --> B\nstyle A fill:#f9f,stroke-width:4px\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"))
        .stderr(predicate::str::contains("stroke-width"));
}

#[test]
fn test_lint_json_valid_input() {
    let output = mmdflux()
        .args(["--lint", "-f", "json"])
        .write_stdin("graph TD\nA --> B\n")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["valid"], true);
    assert!(json["errors"].as_array().unwrap().is_empty());
}

#[test]
fn test_lint_json_invalid_input() {
    let output = mmdflux()
        .args(["--lint", "-f", "json"])
        .write_stdin("not valid mermaid")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["valid"], false);
    assert!(!json["errors"].as_array().unwrap().is_empty());
    assert_eq!(json["errors"][0]["severity"], "error");
}

#[test]
fn test_lint_json_with_warnings() {
    let output = mmdflux()
        .args(["--lint", "-f", "json"])
        .write_stdin("graph TD\nA --> B\nstyle A fill:#f9f,stroke-width:4px\n")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["valid"], true);
    assert!(!json["warnings"].as_array().unwrap().is_empty());
    assert!(
        json["warnings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("stroke-width")
    );
}
