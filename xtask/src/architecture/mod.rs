pub(crate) mod boundaries;
pub(crate) mod watch;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use self::boundaries::SemanticBoundariesSuiteOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchitectureSuite {
    All,
    Boundaries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArchitectureOptions {
    pub(crate) suite: ArchitectureSuite,
    pub(crate) watch: bool,
    pub(crate) timings: bool,
    pub(crate) verbose: bool,
}

#[derive(Debug)]
pub(crate) struct ArchitectureContext {
    repo_root: PathBuf,
    boundaries: boundaries::SemanticBoundariesContext,
}

#[derive(Debug, Clone)]
pub(crate) struct SuiteExecution {
    pub(crate) suite: ArchitectureSuite,
    pub(crate) duration: Duration,
    pub(crate) error: Option<String>,
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

impl SuiteExecution {
    pub(crate) fn is_success(&self) -> bool {
        self.error.is_none()
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
    if options.watch {
        return watch::run(options, ArchitectureContext::new());
    }

    let mut context = ArchitectureContext::new();
    for suite in selected_suites(options.suite) {
        run_suite(
            &mut context,
            *suite,
            options.timings,
            options.verbose,
            false,
        )?;
    }
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
    --watch, -w      Rerun the selected suite when files change"
}

pub(crate) fn run_suites_collect(
    context: &mut ArchitectureContext,
    options: ArchitectureOptions,
    quiet_boundaries: bool,
) -> Vec<SuiteExecution> {
    selected_suites(options.suite)
        .iter()
        .map(|suite| {
            let started = Instant::now();
            let result = run_suite(
                context,
                *suite,
                options.timings,
                options.verbose,
                quiet_boundaries,
            );
            SuiteExecution {
                suite: *suite,
                duration: started.elapsed(),
                error: result.err().map(|error| format!("{error:#}")),
            }
        })
        .collect()
}

pub(crate) fn suite_name(suite: ArchitectureSuite) -> &'static str {
    match suite {
        ArchitectureSuite::All => "architecture",
        ArchitectureSuite::Boundaries => "boundaries",
    }
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
        timings: false,
        verbose: false,
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
            "--timings" | "-t" => options.timings = true,
            "--verbose" | "-v" => options.verbose = true,
            other => bail!("unknown `cargo xtask {expected_command}` argument `{other}`"),
        }
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

    #[test]
    fn architecture_defaults_to_all_suites() {
        let parsed = parse_architecture_args(["architecture"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::All,
                watch: false,
                timings: false,
                verbose: false,
            }
        );
    }

    #[test]
    fn architecture_accepts_named_suites_and_flags() {
        let parsed = parse_architecture_args(["architecture", "boundaries", "--timings"]).unwrap();
        assert_eq!(
            parsed,
            ArchitectureOptions {
                suite: ArchitectureSuite::Boundaries,
                watch: false,
                timings: true,
                verbose: false,
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
                watch: false,
                timings: false,
                verbose: true,
            }
        );
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
