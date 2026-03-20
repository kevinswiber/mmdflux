pub(crate) mod boundaries;
pub(crate) mod host;
pub(crate) mod json_output;
pub(crate) mod watch;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use self::boundaries::{BoundariesRunReport, SemanticBoundariesSuiteOptions};
use self::host::{HostCheckResult, HostRenderOptions, HostStatusResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RenderFlags {
    pub(crate) timings: bool,
    pub(crate) verbose: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchitectureCommand {
    Check {
        render: RenderFlags,
        notify_dirty: bool,
        fresh: bool,
        fast_exit: bool,
    },
    CheckWatch {
        render: RenderFlags,
    },
    CheckStatus,
    CheckJson {
        render: RenderFlags,
        notify_dirty: bool,
        fresh: bool,
        fast_exit: bool,
    },
    Host {
        render: RenderFlags,
    },
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

pub(crate) fn parse_architecture_args<I, S>(args: I) -> Result<ArchitectureCommand>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parse_command_args(args, "architecture")
}

pub(crate) fn run(command: ArchitectureCommand) -> Result<()> {
    match command {
        ArchitectureCommand::Check {
            render,
            notify_dirty,
            fresh,
            fast_exit,
        } => {
            let mut context = ArchitectureContext::new();
            if let Some(report) = try_run_boundaries_via_host(
                context.repo_root(),
                render,
                notify_dirty,
                fresh,
                fast_exit,
                false,
            ) {
                emit_boundaries_report(report)
            } else {
                run_boundaries(&mut context, render.timings, render.verbose, false)
            }
        }
        ArchitectureCommand::CheckWatch { render } => {
            watch::run_watch(render, ArchitectureContext::new())
        }
        ArchitectureCommand::CheckStatus => {
            print_boundaries_host_status(ArchitectureContext::new().repo_root())
        }
        ArchitectureCommand::CheckJson {
            render,
            notify_dirty,
            fresh,
            fast_exit,
        } => run_boundaries_json(render, notify_dirty, fresh, fast_exit),
        ArchitectureCommand::Host { render } => watch::run_host(render, ArchitectureContext::new()),
    }
}

pub(crate) fn run_boundaries_report(
    render: RenderFlags,
    notify_dirty: bool,
    fresh: bool,
    fast_exit: bool,
    json: bool,
) -> Result<BoundariesRunReport> {
    let context = ArchitectureContext::new();
    let repo_root = context.repo_root().to_path_buf();

    match try_run_boundaries_via_host(&repo_root, render, notify_dirty, fresh, fast_exit, json) {
        Some(report) => Ok(report),
        None => {
            let mut context = ArchitectureContext::new();
            run_boundaries_watch_report(&mut context, render)
        }
    }
}

fn run_boundaries_json(
    render: RenderFlags,
    notify_dirty: bool,
    fresh: bool,
    fast_exit: bool,
) -> Result<()> {
    let repo_root = ArchitectureContext::new().repo_root().to_path_buf();
    let report = run_boundaries_report(render, notify_dirty, fresh, fast_exit, true)?;

    json_output::emit_violations_json(&report.violations, &repo_root)
        .map_err(|e| anyhow::anyhow!("failed to emit JSON violations: {e}"))?;
    json_output::emit_build_finished(report.success)
        .map_err(|e| anyhow::anyhow!("failed to emit build-finished: {e}"))?;

    Ok(())
}

pub(crate) fn help_text() -> &'static str {
    "\
cargo xtask architecture [subcommand] [options]

Run the semantic module dependency guard.

Subcommands:
    check            One-shot boundaries check (default if no subcommand)
    host             Run check, then watch and host results for one-shot reuse

Check options:
    --watch, -w      Rerun boundaries when files change (interactive, requires TTY)
    --status         Print warm host status for this worktree
    --fresh          Run the local boundaries check and bypass host reuse
    --fast-exit      Don't wait for a warming host; fall back to local immediately
    --json           Output cargo-compatible JSON diagnostics (for IDE integration)
    --notify-dirty   Tell the host to mark itself dirty before checking (for hooks)
    --timings, -t    Print phase timing breakdown
    --verbose, -v    Print verbose diagnostics and debug context

Host options:
    --timings, -t    Print phase timing breakdown
    --verbose, -v    Print verbose diagnostics and debug context"
}

pub(crate) fn run_boundaries_watch_report(
    context: &mut ArchitectureContext,
    render: RenderFlags,
) -> Result<boundaries::BoundariesRunReport> {
    boundaries::run_with_context_report(
        &mut context.boundaries,
        SemanticBoundariesSuiteOptions {
            timings: render.timings,
            quiet: true,
            verbose: render.verbose,
        },
    )
}

fn try_run_boundaries_via_host(
    repo_root: &Path,
    render: RenderFlags,
    notify_dirty: bool,
    fresh: bool,
    fast_exit: bool,
    json: bool,
) -> Option<BoundariesRunReport> {
    if fresh {
        if !json {
            eprintln!("running local boundaries check (--fresh)");
        }
        return None;
    }

    if notify_dirty {
        host::try_notify_dirty(repo_root);
    }

    let render_options = HostRenderOptions {
        verbose: render.verbose,
        timings: render.timings,
    };
    let result = host::try_request_check(repo_root, render_options);

    // If the host is warming up and we're not in fast-exit mode, poll until ready.
    let result = match &result {
        HostCheckResult::RetryLocally { reason }
            if !fast_exit && reason.contains("is warming up") =>
        {
            if !json {
                eprintln!("[host] waiting for host to finish warming up...");
            }
            host::poll_until_ready_then_check(
                repo_root,
                render_options,
                std::time::Duration::from_secs(60),
            )
        }
        _ => result,
    };

    match result {
        HostCheckResult::Reused(response) => {
            if render.verbose && !json {
                eprintln!(
                    "[host] reused warm boundaries host (generation {}, freshness {:?})",
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
        HostCheckResult::RetryLocally { reason } => {
            if render.verbose && !json {
                eprintln!("[host] falling back to local boundaries run: {reason}");
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

fn print_boundaries_host_status(repo_root: &Path) -> Result<()> {
    match host::query_status(repo_root) {
        HostStatusResult::Live(status) => {
            eprintln!(
                "warm boundaries host: freshness={:?}, generation={}, verbose={}, timings={}",
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
        HostStatusResult::Unavailable { reason } => {
            eprintln!("{reason}");
        }
    }
    Ok(())
}

fn run_boundaries(
    context: &mut ArchitectureContext,
    timings: bool,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    boundaries::run_with_context(
        &mut context.boundaries,
        SemanticBoundariesSuiteOptions {
            timings,
            quiet,
            verbose,
        },
    )
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under the repository root")
        .to_path_buf()
}

fn parse_command_args<I, S>(args: I, expected_command: &str) -> Result<ArchitectureCommand>
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

    let mut subcommand: Option<&str> = None;
    let mut watch = false;
    let mut notify_dirty = false;
    let mut json = false;
    let mut timings = false;
    let mut verbose = false;
    let mut fresh = false;
    let mut fast_exit = false;
    let mut status = false;

    let args_collected: Vec<String> = args.map(|s| s.as_ref().to_string()).collect();

    for arg in &args_collected {
        let arg = arg.as_str();
        match arg {
            "check" => {
                if subcommand.is_some() {
                    bail!("multiple subcommands provided; unexpected `{arg}`");
                }
                subcommand = Some("check");
            }
            "host" => {
                if subcommand.is_some() {
                    bail!("multiple subcommands provided; unexpected `{arg}`");
                }
                subcommand = Some("host");
            }
            "--watch" | "-w" => watch = true,
            "--notify-dirty" => notify_dirty = true,
            "--json" => json = true,
            "--timings" | "-t" => timings = true,
            "--verbose" | "-v" => verbose = true,
            "--fresh" => fresh = true,
            "--fast-exit" => fast_exit = true,
            "--status" => status = true,
            other => bail!("unknown `cargo xtask {expected_command}` argument `{other}`"),
        }
    }

    let render = RenderFlags { timings, verbose };

    match subcommand {
        Some("host") => {
            if watch {
                bail!("`host` subcommand cannot be combined with `--watch` (host implies watch)");
            }
            if status {
                bail!("`host` subcommand cannot be combined with `--status`");
            }
            if json {
                bail!("`host` subcommand cannot be combined with `--json`");
            }
            if fresh {
                bail!("`host` subcommand cannot be combined with `--fresh`");
            }
            if notify_dirty {
                bail!("`host` subcommand cannot be combined with `--notify-dirty`");
            }
            Ok(ArchitectureCommand::Host { render })
        }
        Some("check") | None => {
            // "check" is the default subcommand
            if status {
                if watch || json || fresh || notify_dirty {
                    bail!("`--status` cannot be combined with other check flags");
                }
                return Ok(ArchitectureCommand::CheckStatus);
            }
            if watch {
                if json {
                    bail!("`--json` cannot be combined with `--watch`");
                }
                if fresh {
                    bail!("`--fresh` cannot be combined with `--watch`");
                }
                return Ok(ArchitectureCommand::CheckWatch { render });
            }
            if json {
                return Ok(ArchitectureCommand::CheckJson {
                    render,
                    notify_dirty,
                    fresh,
                    fast_exit,
                });
            }
            Ok(ArchitectureCommand::Check {
                render,
                notify_dirty,
                fresh,
                fast_exit,
            })
        }
        Some(other) => bail!("unknown subcommand `{other}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ArchitectureCommand, RenderFlags, parse_architecture_args};

    const DEFAULT_RENDER: RenderFlags = RenderFlags {
        timings: false,
        verbose: false,
    };

    #[test]
    fn default_is_check() {
        let cmd = parse_architecture_args(["architecture"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::Check {
                render: DEFAULT_RENDER,
                notify_dirty: false,
                fresh: false,
                fast_exit: false,
            }
        );
    }

    #[test]
    fn explicit_check_subcommand() {
        let cmd = parse_architecture_args(["architecture", "check"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::Check {
                render: DEFAULT_RENDER,
                notify_dirty: false,
                fresh: false,
                fast_exit: false,
            }
        );
    }

    #[test]
    fn check_with_timings() {
        let cmd = parse_architecture_args(["architecture", "check", "--timings"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::Check {
                render: RenderFlags {
                    timings: true,
                    verbose: false,
                },
                notify_dirty: false,
                fresh: false,
                fast_exit: false,
            }
        );
    }

    #[test]
    fn check_with_fresh() {
        let cmd = parse_architecture_args(["architecture", "--fresh"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::Check {
                render: DEFAULT_RENDER,
                notify_dirty: false,
                fresh: true,
                fast_exit: false,
            }
        );
    }

    #[test]
    fn check_with_status() {
        let cmd = parse_architecture_args(["architecture", "--status"]).unwrap();
        assert_eq!(cmd, ArchitectureCommand::CheckStatus);
    }

    #[test]
    fn check_with_watch() {
        let cmd = parse_architecture_args(["architecture", "--watch"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::CheckWatch {
                render: DEFAULT_RENDER,
            }
        );
    }

    #[test]
    fn check_watch_with_verbose() {
        let cmd =
            parse_architecture_args(["architecture", "check", "--watch", "--verbose"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::CheckWatch {
                render: RenderFlags {
                    timings: false,
                    verbose: true,
                },
            }
        );
    }

    #[test]
    fn check_with_json() {
        let cmd = parse_architecture_args(["architecture", "--json"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::CheckJson {
                render: DEFAULT_RENDER,
                notify_dirty: false,
                fresh: false,
                fast_exit: false,
            }
        );
    }

    #[test]
    fn host_subcommand() {
        let cmd = parse_architecture_args(["architecture", "host"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::Host {
                render: DEFAULT_RENDER,
            }
        );
    }

    #[test]
    fn host_with_timings_and_verbose() {
        let cmd =
            parse_architecture_args(["architecture", "host", "--timings", "--verbose"]).unwrap();
        assert_eq!(
            cmd,
            ArchitectureCommand::Host {
                render: RenderFlags {
                    timings: true,
                    verbose: true,
                },
            }
        );
    }

    #[test]
    fn host_rejects_watch() {
        let err = parse_architecture_args(["architecture", "host", "--watch"]).unwrap_err();
        assert!(err.to_string().contains("--watch"));
    }

    #[test]
    fn host_rejects_status() {
        let err = parse_architecture_args(["architecture", "host", "--status"]).unwrap_err();
        assert!(err.to_string().contains("--status"));
    }

    #[test]
    fn host_rejects_json() {
        let err = parse_architecture_args(["architecture", "host", "--json"]).unwrap_err();
        assert!(err.to_string().contains("--json"));
    }

    #[test]
    fn host_rejects_fresh() {
        let err = parse_architecture_args(["architecture", "host", "--fresh"]).unwrap_err();
        assert!(err.to_string().contains("--fresh"));
    }

    #[test]
    fn json_rejects_watch() {
        let err = parse_architecture_args(["architecture", "--json", "--watch"]).unwrap_err();
        assert!(err.to_string().contains("--json"));
    }

    #[test]
    fn status_rejects_other_flags() {
        let err = parse_architecture_args(["architecture", "--status", "--fresh"]).unwrap_err();
        assert!(err.to_string().contains("--status"));
    }

    #[test]
    fn help_text_reflects_new_structure() {
        let help = super::help_text();
        assert!(help.contains("check"));
        assert!(help.contains("host"));
        assert!(!help.contains("--background"));
        assert!(!help.contains("daemon"));
        // "boundaries" is fine in descriptions — just not as a subcommand/alias
        assert!(!help.contains("Aliases"));
    }

    #[test]
    fn rejects_unknown_args() {
        let err = parse_architecture_args(["architecture", "layers", "--timings"]).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    #[test]
    fn rejects_background_flag() {
        let err = parse_architecture_args(["architecture", "--background"]).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }
}
