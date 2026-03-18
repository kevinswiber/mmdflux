use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::architecture;

#[derive(Debug)]
pub(crate) struct LintOptions {
    pub(crate) json: bool,
}

pub(crate) fn parse_lint_args<I, S>(args: I) -> Result<LintOptions>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    match args.next() {
        Some(first) if first.as_ref() == "lint" => {}
        Some(first) => bail!(
            "expected `cargo xtask lint`, got `cargo xtask {}`",
            first.as_ref()
        ),
        None => bail!("missing `cargo xtask lint` invocation"),
    }

    let mut options = LintOptions { json: false };
    for arg in args {
        match arg.as_ref() {
            "--json" => options.json = true,
            other => bail!("unknown `cargo xtask lint` argument `{other}`"),
        }
    }
    Ok(options)
}

pub(crate) fn run(options: LintOptions) -> Result<()> {
    if options.json {
        run_lint_json()
    } else {
        run_lint_text()
    }
}

pub(crate) fn help_text() -> &'static str {
    "\
cargo xtask lint [options]

Runs clippy and architecture boundary checks.

Options:
    --json    Output cargo-compatible JSON diagnostics (for IDE integration)"
}

fn run_lint_json() -> Result<()> {
    let clippy_success = run_clippy_json_streaming()?;

    let arch_options = architecture::ArchitectureOptions {
        suite: architecture::ArchitectureSuite::Boundaries,
        json: true,
        watch: false,
        background: false,
        notify_dirty: false,
        timings: false,
        verbose: false,
        fresh: false,
        status: false,
    };
    let repo_root = repo_root();
    let report = architecture::run_boundaries_report(arch_options)?;

    architecture::json_output::emit_violations_json(&report.violations, &repo_root)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let combined_success = clippy_success && report.success;
    architecture::json_output::emit_build_finished(combined_success)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Exit 0 — rust-analyzer reads build-finished, not exit code
    Ok(())
}

fn run_lint_text() -> Result<()> {
    let status = Command::new("cargo")
        .args([
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ])
        .status()
        .context("failed to run cargo clippy")?;

    if !status.success() {
        bail!("cargo clippy failed");
    }

    let arch_options = architecture::ArchitectureOptions {
        suite: architecture::ArchitectureSuite::Boundaries,
        watch: false,
        background: false,
        notify_dirty: false,
        json: false,
        timings: false,
        verbose: false,
        fresh: false,
        status: false,
    };
    architecture::run(arch_options)
}

fn run_clippy_json_streaming() -> Result<bool> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "clippy",
        "--locked",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--message-format=json-diagnostic-rendered-ansi",
        "--",
        "-D",
        "warnings",
    ]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn().context("failed to spawn cargo clippy")?;
    let stdout = child.stdout.take().expect("stdout piped");
    let reader = BufReader::new(stdout);

    let mut clippy_success = true;

    for line in reader.lines() {
        let line = line.context("failed to read clippy output")?;

        if let Some(success) = extract_build_finished_success(&line) {
            clippy_success = success;
            continue;
        }

        println!("{line}");
        std::io::stdout().flush()?;
    }

    let status = child.wait().context("failed to wait for cargo clippy")?;
    if !status.success() {
        clippy_success = false;
    }

    Ok(clippy_success)
}

fn extract_build_finished_success(line: &str) -> Option<bool> {
    let json: serde_json::Value = serde_json::from_str(line).ok()?;
    if json.get("reason")?.as_str()? != "build-finished" {
        return None;
    }
    json.get("success")?.as_bool()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under the repository root")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_build_finished_success_returns_correct_value() {
        assert_eq!(
            extract_build_finished_success(r#"{"reason":"build-finished","success":true}"#),
            Some(true)
        );
        assert_eq!(
            extract_build_finished_success(r#"{"reason":"build-finished","success":false}"#),
            Some(false)
        );
        assert_eq!(
            extract_build_finished_success(r#"{"reason":"compiler-message"}"#),
            None
        );
        assert_eq!(extract_build_finished_success("not json"), None);
        assert_eq!(extract_build_finished_success(""), None);
    }

    #[test]
    fn parse_lint_args_defaults() {
        let opts = parse_lint_args(["lint"]).unwrap();
        assert!(!opts.json);
    }

    #[test]
    fn parse_lint_args_json_flag() {
        let opts = parse_lint_args(["lint", "--json"]).unwrap();
        assert!(opts.json);
    }

    #[test]
    fn parse_lint_args_rejects_unknown() {
        let err = parse_lint_args(["lint", "--foo"]).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }
}
