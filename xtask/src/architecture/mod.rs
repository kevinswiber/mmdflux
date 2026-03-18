pub(crate) mod boundaries;
pub(crate) mod daemon;
pub(crate) mod json_output;
pub(crate) mod watch;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use self::boundaries::{BoundariesRunReport, SemanticBoundariesSuiteOptions};
use self::daemon::{DaemonCheckResult, DaemonRenderOptions, DaemonStatusResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchitectureSuite {
    All,
    Boundaries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchitectureOptions {
    pub(crate) suite: ArchitectureSuite,
    pub(crate) watch: bool,
    pub(crate) background: bool,
    pub(crate) notify_dirty: bool,
    pub(crate) json: bool,
    pub(crate) timings: bool,
    pub(crate) verbose: bool,
    pub(crate) fresh: bool,
    pub(crate) status: bool,
}

#[derive(Debug)]
pub(crate) struct ArchitectureContext {
    repo_root: PathBuf,
    boundaries: boundaries::SemanticBoundariesContext,
}

impl ArchitectureContext {
    fn new() -> Self {
        Self {
            repo_root: repo_root(),
            boundaries: boundaries::SemanticBoundariesContext::default(),
        }
    }

    pub(crate) fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub(crate) fn record_changes(&mut self, paths: &[PathBuf]) {
        self.boundaries.record_changes(paths);
    }
}

pub(crate) fn parse_architecture_args<I, S>(args: I) -> Result<ArchitectureOptions>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parse_command_args(args, "architecture")
}

pub(crate) fn run(options: ArchitectureOptions) -> Result<()> {
    if options.status {
        return print_boundaries_daemon_status(ArchitectureContext::new().repo_root());
    }
    if options.json {
        return run_boundaries_json(options);
    }
    if options.watch {
        return watch::run(options, ArchitectureContext::new());
    }

    let mut context = ArchitectureContext::new();
    for suite in selected_suites(options.suite) {
        match suite {
            ArchitectureSuite::Boundaries => {
                if let Some(report) = try_run_boundaries_via_daemon(context.repo_root(), options) {
                    emit_boundaries_report(report)?;
                } else {
                    run_suite(
                        &mut context,
                        *suite,
                        options.timings,
                        options.verbose,
                        false,
                    )?;
                }
            }
            ArchitectureSuite::All => {
                run_suite(
                    &mut context,
                    *suite,
                    options.timings,
                    options.verbose,
                    false,
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn run_boundaries_report(options: ArchitectureOptions) -> Result<BoundariesRunReport> {
    let context = ArchitectureContext::new();
    let repo_root = context.repo_root().to_path_buf();

    match try_run_boundaries_via_daemon(&repo_root, options) {
        Some(report) => Ok(report),
        None => {
            let mut context = ArchitectureContext::new();
            run_boundaries_watch_report(&mut context, options)
        }
    }
}

fn run_boundaries_json(options: ArchitectureOptions) -> Result<()> {
    let repo_root = ArchitectureContext::new().repo_root().to_path_buf();
    let report = run_boundaries_report(options)?;

    json_output::emit_violations_json(&report.violations, &repo_root)
        .map_err(|e| anyhow::anyhow!("failed to emit JSON violations: {e}"))?;
    json_output::emit_build_finished(report.success)
        .map_err(|e| anyhow::anyhow!("failed to emit build-finished: {e}"))?;

    Ok(())
}

pub(crate) fn help_text() -> &'static str {
    "\
cargo xtask architecture [suite] [options]

Suites:
    boundaries    Run the semantic module dependency guard

Options:
    --timings, -t    Print phase timing breakdown
    --verbose, -v    Print verbose suite diagnostics and debug context
    --fresh          Run the local boundaries check and bypass daemon reuse
    --status         Print warm-daemon status for this worktree
    --watch, -w      Rerun the selected suite when files change
    --background     Run watch mode without requiring a terminal (for hooks/daemons)
    --notify-dirty   Tell the daemon to mark itself dirty before checking (for hooks)
    --json           Output cargo-compatible JSON diagnostics (for IDE integration)"
}

pub(crate) fn run_boundaries_watch_report(
    context: &mut ArchitectureContext,
    options: ArchitectureOptions,
) -> Result<boundaries::BoundariesRunReport> {
    boundaries::run_with_context_report(
        &mut context.boundaries,
        SemanticBoundariesSuiteOptions {
            timings: options.timings,
            quiet: true,
            verbose: options.verbose,
        },
    )
}

pub(crate) fn suite_name(suite: ArchitectureSuite) -> &'static str {
    match suite {
        ArchitectureSuite::All => "architecture",
        ArchitectureSuite::Boundaries => "boundaries",
    }
}

fn try_run_boundaries_via_daemon(
    repo_root: &Path,
    options: ArchitectureOptions,
) -> Option<BoundariesRunReport> {
    if options.fresh {
        if !options.json {
            eprintln!("running local boundaries check (--fresh)");
        }
        return None;
    }

    if options.notify_dirty {
        daemon::try_notify_dirty(repo_root);
    }

    let render_options = DaemonRenderOptions {
        verbose: options.verbose,
        timings: options.timings,
    };
    match daemon::try_request_check(repo_root, render_options) {
        DaemonCheckResult::Reused(response) => {
            if options.verbose && !options.json {
                eprintln!(
                    "[daemon] reused warm boundaries daemon (generation {}, freshness {:?})",
                    response.generation, response.freshness
                );
            }
            Some(BoundariesRunReport {
                success: response.success,
                rendered_output: response.rendered_output,
                summary: response.summary,
                timings_output: response.timings_output,
                violations: response.violations,
            })
        }
        DaemonCheckResult::RetryLocally { reason } => {
            if options.verbose && !options.json {
                eprintln!("[daemon] falling back to local boundaries run: {reason}");
            }
            None
        }
    }
}

fn emit_boundaries_report(report: BoundariesRunReport) -> Result<()> {
    if let Some(timings_output) = &report.timings_output {
        eprint!("{timings_output}");
    }
    if report.success {
        return Ok(());
    }

    eprint!("{}", report.rendered_output);
    bail!(
        report
            .summary
            .unwrap_or_else(|| "error: architecture boundaries failed".to_string())
    )
}

fn print_boundaries_daemon_status(repo_root: &Path) -> Result<()> {
    match daemon::query_status(repo_root) {
        DaemonStatusResult::Live(status) => {
            eprintln!(
                "warm boundaries daemon: freshness={:?}, generation={}, verbose={}, timings={}",
                status.freshness,
                status.generation,
                status.render_options.verbose,
                status.render_options.timings
            );
            if let Some(last_success) = status.last_success {
                eprintln!("last_success={last_success}");
            }
            if let Some(last_finished_at) = status.last_finished_at {
                eprintln!("last_finished_at={last_finished_at}");
            }
        }
        DaemonStatusResult::Unavailable { reason } => {
            eprintln!("{reason}");
        }
    }
    Ok(())
}

pub(crate) fn selected_suites(selection: ArchitectureSuite) -> &'static [ArchitectureSuite] {
    const ALL_SUITES: [ArchitectureSuite; 1] = [ArchitectureSuite::Boundaries];
    const BOUNDARIES_ONLY: [ArchitectureSuite; 1] = [ArchitectureSuite::Boundaries];

    match selection {
        ArchitectureSuite::All => &ALL_SUITES,
        ArchitectureSuite::Boundaries => &BOUNDARIES_ONLY,
    }
}

fn run_suite(
    context: &mut ArchitectureContext,
    suite: ArchitectureSuite,
    timings: bool,
    verbose: bool,
    quiet_boundaries: bool,
) -> Result<()> {
    match suite {
        ArchitectureSuite::All => unreachable!("run_suite only accepts concrete suites"),
        ArchitectureSuite::Boundaries => boundaries::run_with_context(
            &mut context.boundaries,
            SemanticBoundariesSuiteOptions {
                timings,
                quiet: quiet_boundaries,
                verbose,
            },
        ),
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under the repository root")
        .to_path_buf()
}

fn parse_command_args<I, S>(args: I, expected_command: &str) -> Result<ArchitectureOptions>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();

    match args.next() {
        Some(first) if first.as_ref() == expected_command => {}
        Some(first) => bail!(
            "expected `cargo xtask {expected_command}`, got `cargo xtask {}`",
            first.as_ref()
        ),
        None => bail!("missing `cargo xtask {expected_command}` invocation"),
    }

    let mut options = ArchitectureOptions {
        suite: ArchitectureSuite::All,
        watch: false,
        background: false,
        notify_dirty: false,
        json: false,
        timings: false,
        verbose: false,
        fresh: false,
        status: false,
    };
    let mut suite_selected = false;

    for arg in args {
        let arg = arg.as_ref();
        match arg {
            "boundaries" => set_suite(
                &mut options,
                &mut suite_selected,
                ArchitectureSuite::Boundaries,
                arg,
            )?,
            "--watch" | "-w" => options.watch = true,
            "--background" => options.background = true,
            "--notify-dirty" => options.notify_dirty = true,
            "--json" => options.json = true,
            "--timings" | "-t" => options.timings = true,
            "--verbose" | "-v" => options.verbose = true,
            "--fresh" => options.fresh = true,
            "--status" => options.status = true,
            other => bail!("unknown `cargo xtask {expected_command}` argument `{other}`"),
        }
    }

    if options.background {
        options.watch = true;
    }
    if options.json && options.watch {
        bail!("`--json` cannot be combined with `--watch`");
    }
    if options.json && options.status {
        bail!("`--json` cannot be combined with `--status`");
    }
    if options.watch && options.status {
        bail!("`--status` cannot be combined with `--watch`");
    }
    if options.fresh && options.status {
        bail!("`--fresh` cannot be combined with `--status`");
    }

    Ok(options)
}

fn set_suite(
    options: &mut ArchitectureOptions,
    suite_selected: &mut bool,
    suite: ArchitectureSuite,
    arg: &str,
) -> Result<()> {
    if *suite_selected {
        bail!("multiple architecture suites provided; unexpected `{arg}`");
    }

    options.suite = suite;
    *suite_selected = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ArchitectureOptions, ArchitectureSuite, parse_architecture_args};

    const DEFAULT_OPTIONS: ArchitectureOptions = ArchitectureOptions {
        suite: ArchitectureSuite::All,
        watch: false,
        background: false,
        notify_dirty: false,
        json: false,
        timings: false,
        verbose: false,
        fresh: false,
        status: false,
    };

    #[test]
    fn architecture_defaults_to_all_suites() {
        let parsed = parse_architecture_args(["architecture"]).unwrap();
        assert_eq!(parsed, DEFAULT_OPTIONS);
    }

    #[test]
    fn architecture_accepts_named_suites_and_flags() {
        let parsed = parse_architecture_args(["architecture", "boundaries", "--timings"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::Boundaries,
                timings: true,
                ..DEFAULT_OPTIONS
            }
        );
    }

    #[test]
    fn architecture_accepts_verbose_flag() {
        let parsed = parse_architecture_args(["architecture", "boundaries", "--verbose"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::Boundaries,
                verbose: true,
                ..DEFAULT_OPTIONS
            }
        );
    }

    #[test]
    fn architecture_accepts_fresh_and_status_flags() {
        let parsed = parse_architecture_args(["architecture", "boundaries", "--fresh"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::Boundaries,
                fresh: true,
                ..DEFAULT_OPTIONS
            }
        );

        let parsed = parse_architecture_args(["architecture", "boundaries", "--status"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::Boundaries,
                status: true,
                ..DEFAULT_OPTIONS
            }
        );
    }

    #[test]
    fn architecture_accepts_json_flag() {
        let parsed = parse_architecture_args(["architecture", "boundaries", "--json"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::Boundaries,
                json: true,
                ..DEFAULT_OPTIONS
            }
        );
    }

    #[test]
    fn architecture_rejects_json_with_watch() {
        let error = parse_architecture_args(["architecture", "boundaries", "--json", "--watch"])
            .unwrap_err();
        assert!(error.to_string().contains("--json"));
    }

    #[test]
    fn architecture_rejects_json_with_status() {
        let error = parse_architecture_args(["architecture", "boundaries", "--json", "--status"])
            .unwrap_err();
        assert!(error.to_string().contains("--json"));
    }

    #[test]
    fn architecture_help_omits_retired_structure_suite() {
        let help = super::help_text();
        assert!(!help.contains("surface"));
        assert!(!help.contains("structure"));
        assert!(!help.contains("layers"));
    }

    #[test]
    fn architecture_rejects_layers_alias() {
        let error = parse_architecture_args(["architecture", "layers", "--timings"]).unwrap_err();
        assert!(error.to_string().contains("unknown"));
    }
}
