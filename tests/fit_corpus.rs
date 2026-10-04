//! Corpus regression snapshots for fitted text rendering.
//!
//! Renders every graph-family fixture plus the chat-style supplement in
//! `tests/fixtures/fit/` at 40 and 80 columns and compares one line per
//! fixture against `tests/snapshots/fit/ladder-{width}.txt`. Regenerate with
//! `GENERATE_FIT_SNAPSHOTS=1`.

use std::fs;
use std::path::{Path, PathBuf};

use mmdflux::{FitOptions, FitOutcome, Fitted, OutputFormat, RenderConfig, render_diagram_fitted};

const FAMILIES: [&str; 4] = ["flowchart", "class", "state", "fit"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// `(family/name, source)` for every top-level fixture, sorted by path.
fn corpus() -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for family in FAMILIES {
        let dir = root().join("tests/fixtures").join(family);
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
            .expect("fixture directory should exist")
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "mmd"))
            .collect();
        paths.sort();
        for path in paths {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            entries.push((
                format!("{family}/{name}"),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn outcome_name(outcome: FitOutcome) -> String {
    serde_json::to_value(outcome)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn snapshot_line(path: &str, fitted: &Fitted) -> String {
    let report = &fitted.report;
    let size = report.size.expect("text fits report a size");
    let levers = if report.applied.is_empty() {
        "-".to_string()
    } else {
        report
            .applied
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "{path}\t{}\t{}x{}\t{levers}",
        outcome_name(report.outcome),
        size.width,
        size.height
    )
}

struct Run {
    lines: Vec<String>,
    graph_fits: usize,
    chat_fits: usize,
}

fn run(width: usize) -> Run {
    let config = RenderConfig::default();
    let fit = FitOptions::max_width(width);
    let mut run = Run {
        lines: Vec::new(),
        graph_fits: 0,
        chat_fits: 0,
    };
    for (path, input) in corpus() {
        let fitted = render_diagram_fitted(&input, OutputFormat::Text, &config, &fit)
            .unwrap_or_else(|error| panic!("{path}: {}", error.message));
        let fits = matches!(
            fitted.report.outcome,
            FitOutcome::AsAuthored | FitOutcome::Fitted
        );
        if fits {
            if path.starts_with("fit/") {
                run.chat_fits += 1;
            } else {
                run.graph_fits += 1;
            }
        }
        run.lines.push(snapshot_line(&path, &fitted));
    }
    run
}

fn check_snapshot(width: usize, lines: &[String]) {
    let path = root().join(format!("tests/snapshots/fit/ladder-{width}.txt"));
    let actual = format!("{}\n", lines.join("\n"));
    if std::env::var_os("GENERATE_FIT_SNAPSHOTS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing {}; run with GENERATE_FIT_SNAPSHOTS=1",
            path.display()
        )
    });
    if expected != actual {
        let diff: Vec<String> = expected
            .lines()
            .zip(actual.lines())
            .filter(|(old, new)| old != new)
            .map(|(old, new)| format!("- {old}\n+ {new}"))
            .collect();
        panic!(
            "fit snapshot ladder-{width} changed ({} lines differ, {} vs {} lines); \
             regenerate with GENERATE_FIT_SNAPSHOTS=1\n{}",
            diff.len(),
            expected.lines().count(),
            actual.lines().count(),
            diff.join("\n")
        );
    }
}

#[test]
fn fit_corpus_at_80_columns() {
    let run = run(80);
    check_snapshot(80, &run.lines);
    assert!(
        run.graph_fits >= 145,
        "{} graph fixtures fit at 80",
        run.graph_fits
    );
    assert!(
        run.chat_fits >= 8,
        "{} of 9 chat diagrams fit at 80",
        run.chat_fits
    );
}

#[test]
fn fit_corpus_at_40_columns() {
    let run = run(40);
    check_snapshot(40, &run.lines);
    assert!(
        run.graph_fits >= 130,
        "{} graph fixtures fit at 40",
        run.graph_fits
    );
}
