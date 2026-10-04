//! CLI tests for `--max-width` and the fit flags.

use std::process::Output;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

const CI_PIPELINE: &str = "tests/fixtures/flowchart/ci_pipeline.mmd";
const COMPLEX: &str = "tests/fixtures/flowchart/complex.mmd";

fn mmdflux() -> Command {
    cargo_bin_cmd!("mmdflux")
}

fn run(args: &[&str]) -> Output {
    mmdflux().args(args).output().expect("mmdflux should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stderr_lines(output: &Output) -> Vec<String> {
    stderr(output).lines().map(str::to_string).collect()
}

/// The JSON report printed as the last stderr line.
fn report(output: &Output) -> Value {
    let lines = stderr_lines(output);
    let last = lines.last().expect("stderr should have a report line");
    serde_json::from_str(last).unwrap_or_else(|_| panic!("not a JSON report: {last}"))
}

#[test]
fn max_width_fits_and_notes_the_levers() {
    let output = run(&["--max-width", "80", CI_PIPELINE]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).lines().all(|line| line.width() <= 80));
    let lines = stderr_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("note: fitted to 80 columns: "));
    assert!(lines[0].ends_with("(as authored 169x9)"), "{}", lines[0]);
}

#[test]
fn quiet_suppresses_the_note() {
    let output = run(&["-q", "--max-width", "80", CI_PIPELINE]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stderr(&output), "");
}

#[test]
fn a_diagram_that_already_fits_is_unchanged() {
    let fixture = "tests/fixtures/flowchart/simple.mmd";
    let plain = run(&[fixture]);
    let fitted = run(&["--max-width", "80", fixture]);
    assert_eq!(stdout(&fitted), stdout(&plain));
    assert_eq!(stderr(&fitted), "");
}

#[test]
fn a_miss_prints_the_narrowest_with_a_warning() {
    let output = run(&["--max-width", "20", COMPLEX]);
    assert_eq!(output.status.code(), Some(0));
    assert!(!stdout(&output).is_empty());
    let lines = stderr_lines(&output);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("warning: no layout fits --max-width 20; printed the narrowest, "),
        "{}",
        lines[0]
    );
}

#[test]
fn require_fit_exits_3_without_output() {
    let path = std::env::temp_dir().join(format!(
        "mmdflux-fit-{}.txt",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path_arg = path.to_string_lossy().into_owned();
    let output = run(&[
        "--max-width",
        "20",
        "--require-fit",
        "-o",
        &path_arg,
        COMPLEX,
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout(&output), "");
    assert!(!path.exists(), "-o must not be written on a miss");
    assert!(
        stderr(&output).starts_with("Error: no layout fits --max-width 20; narrowest is "),
        "{}",
        stderr(&output)
    );
}

#[test]
fn fit_report_json_is_the_last_stderr_line() {
    let fitted = run(&["--max-width", "80", "--fit-report", "json", CI_PIPELINE]);
    let json = report(&fitted);
    assert_eq!(json["outcome"], "fitted");
    for key in [
        "budget",
        "size",
        "asAuthored",
        "applied",
        "authoredDirection",
        "direction",
        "stableFor",
        "leverScope",
        "attempts",
        "solves",
    ] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
    assert!(json.get("trail").is_none());

    let quiet = run(&[
        "-q",
        "--max-width",
        "80",
        "--fit-report",
        "json",
        CI_PIPELINE,
    ]);
    assert_eq!(stderr_lines(&quiet).len(), 1);
    assert_eq!(report(&quiet)["outcome"], "fitted");

    let miss = run(&["--max-width", "20", "--fit-report", "json", COMPLEX]);
    assert_eq!(report(&miss)["outcome"], "bestAttempt");

    let required = run(&[
        "--max-width",
        "20",
        "--require-fit",
        "--fit-report",
        "json",
        COMPLEX,
    ]);
    assert_eq!(required.status.code(), Some(3));
    assert_eq!(report(&required)["outcome"], "bestAttempt");
}

#[test]
fn fit_direction_keep_keeps_the_authored_direction() {
    let output = run(&[
        "--max-width",
        "80",
        "--fit-direction",
        "keep",
        "--fit-report",
        "json",
        CI_PIPELINE,
    ]);
    assert_eq!(report(&output)["direction"], "LR");
}

#[test]
fn fit_truncate_enables_label_truncation() {
    let output = run(&[
        "--max-width",
        "80",
        "--fit-truncate",
        "--fit-report",
        "json",
        "tests/fixtures/state/concurrent_three.mmd",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let json = report(&output);
    assert_eq!(json["outcome"], "fitted");
    assert!(
        json["applied"]
            .as_array()
            .unwrap()
            .iter()
            .any(|lever| lever["lever"] == "labelTruncation"),
        "{json}"
    );
}

#[test]
fn other_formats_ignore_max_width_with_a_warning() {
    for format in ["svg", "mmds"] {
        let plain = run(&["-f", format, CI_PIPELINE]);
        let output = run(&["-f", format, "--max-width", "80", CI_PIPELINE]);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(stdout(&output), stdout(&plain), "{format}");
        assert!(
            stderr(&output).contains(&format!(
                "warning: --max-width applies to text and ascii output; ignored for {format}"
            )),
            "{format}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn fit_flags_are_usage_errors_without_max_width_or_with_lint() {
    for args in [
        vec!["--require-fit", CI_PIPELINE],
        vec!["--fit-truncate", CI_PIPELINE],
        vec!["--fit-direction", "keep", CI_PIPELINE],
        vec!["--fit-report", "json", CI_PIPELINE],
        vec!["--max-width", "0", CI_PIPELINE],
        vec!["--max-width", "80", "--lint", CI_PIPELINE],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn debug_lists_every_attempt() {
    let output = run(&["--debug", "--max-width", "40", CI_PIPELINE]);
    assert!(
        stderr(&output).contains("fit attempt 1: as authored -> 169x9 valid=true fits=false"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn mmds_input_uses_render_only_levers() {
    let json = stdout(&run(&["-f", "mmds", CI_PIPELINE]));
    let output = mmdflux()
        .args(["--max-width", "80", "--fit-report", "json"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(report(&output)["leverScope"], "renderOnly");
}

#[test]
fn sequence_diagrams_warn_when_they_do_not_fit() {
    let output = run(&[
        "--max-width",
        "10",
        "--fit-report",
        "json",
        "tests/fixtures/sequence/activation_nested.mmd",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let lines = stderr_lines(&output);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("warning: no layout fits --max-width 10; ")),
        "{lines:?}"
    );
    assert_eq!(report(&output)["leverScope"], "unavailable");
}
