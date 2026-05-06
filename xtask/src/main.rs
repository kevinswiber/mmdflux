mod architecture;
mod font_metrics;
mod lint;
mod readme_assets;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};

const REPO_ROOT_ENV: &str = "MMDFLUX_REPO_ROOT";
const XTASK_LOG_ENV: &str = "MMDFLUX_XTASK_LOG";

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let parsed = parse_xtask_args(std::env::args().skip(1))?;
    init_tracing_for_options(&parsed.log)?;

    let Some(command) = parsed.command_args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    tracing::debug!(target: "xtask", command, "dispatch xtask command");

    match command {
        "architecture" => run_architecture_command(&parsed.command_args),
        "font-metrics" => run_font_metrics_command(&parsed.command_args),
        "lint" => run_lint_command(&parsed.command_args),
        "readme-assets" => run_readme_assets_command(&parsed.command_args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => anyhow::bail!("unknown xtask subcommand `{other}`"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedXtaskArgs {
    log: XtaskLogOptions,
    command_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XtaskLogOptions {
    filter: Option<String>,
    format: XtaskLogFormat,
    file: Option<PathBuf>,
}

impl Default for XtaskLogOptions {
    fn default() -> Self {
        Self {
            filter: None,
            format: XtaskLogFormat::Compact,
            file: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XtaskLogFormat {
    Compact,
    Pretty,
    Json,
}

impl XtaskLogFormat {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "compact" => Ok(Self::Compact),
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            other => bail!("unknown xtask log format `{other}`"),
        }
    }
}

fn parse_xtask_args<I, S>(args: I) -> Result<ParsedXtaskArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .peekable();
    let mut log = XtaskLogOptions::default();
    let mut command_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--log" => {
                log.filter = Some(next_global_arg(&mut args, "--log")?);
            }
            "--log-format" => {
                let value = next_global_arg(&mut args, "--log-format")?;
                log.format = XtaskLogFormat::parse(&value)?;
            }
            "--log-file" => {
                log.file = Some(PathBuf::from(next_global_arg(&mut args, "--log-file")?));
            }
            _ if arg.starts_with("--log=") => {
                log.filter = Some(
                    arg.strip_prefix("--log=")
                        .expect("prefix checked")
                        .to_string(),
                );
            }
            _ if arg.starts_with("--log-format=") => {
                let value = arg.strip_prefix("--log-format=").expect("prefix checked");
                log.format = XtaskLogFormat::parse(value)?;
            }
            _ if arg.starts_with("--log-file=") => {
                let value = arg.strip_prefix("--log-file=").expect("prefix checked");
                log.file = Some(PathBuf::from(value));
            }
            _ => {
                command_args.push(arg);
                command_args.extend(args);
                break;
            }
        }
    }

    Ok(ParsedXtaskArgs { log, command_args })
}

fn next_global_arg<I>(args: &mut I, flag: &str) -> Result<String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| anyhow::anyhow!("missing value for `{flag}`"))
}

fn resolve_xtask_log_filter(options: &XtaskLogOptions) -> Option<String> {
    resolve_xtask_log_filter_with(options, |name| std::env::var(name).ok())
}

fn resolve_xtask_log_filter_with<F>(options: &XtaskLogOptions, mut get_env: F) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    if let Some(filter) = options
        .filter
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(filter.to_string());
    }

    get_env(XTASK_LOG_ENV)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| get_env("RUST_LOG").filter(|value| !value.trim().is_empty()))
}

fn init_tracing_for_options(options: &XtaskLogOptions) -> Result<()> {
    let Some(filter) = resolve_xtask_log_filter(options) else {
        return Ok(());
    };

    let env_filter = tracing_subscriber::EnvFilter::try_new(&filter)
        .with_context(|| format!("invalid log filter: {filter}"))?;

    match &options.file {
        Some(path) => {
            let writer = SharedLogWriter::new(fs::File::create(path).with_context(|| {
                format!("failed to create xtask log file `{}`", path.display())
            })?);
            init_tracing_with_writer(env_filter, options.format, move || writer.clone())
        }
        None => init_tracing_with_writer(env_filter, options.format, io::stderr),
    }
}

fn init_tracing_with_writer<W>(
    env_filter: tracing_subscriber::EnvFilter,
    log_format: XtaskLogFormat,
    make_writer: W,
) -> Result<()>
where
    W: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static,
{
    let builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(make_writer)
        .with_ansi(false)
        .with_target(true);

    let result = match log_format {
        XtaskLogFormat::Compact => builder.compact().try_init(),
        XtaskLogFormat::Pretty => builder.pretty().try_init(),
        XtaskLogFormat::Json => builder.json().try_init(),
    };

    result.map_err(|err| anyhow::anyhow!("failed to initialize xtask tracing: {err}"))
}

#[derive(Clone)]
struct SharedLogWriter {
    file: Arc<Mutex<fs::File>>,
}

impl SharedLogWriter {
    fn new(file: fs::File) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
        }
    }
}

impl Write for SharedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("xtask log file lock poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("xtask log file lock poisoned"))?
            .flush()
    }
}

fn run_architecture_command(args: &[String]) -> Result<()> {
    if args.iter().skip(1).any(|arg| is_help_arg(arg)) {
        print_architecture_help();
        return Ok(());
    }

    let options = architecture::parse_architecture_args(args.iter().map(String::as_str))?;
    architecture::run(options)
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h")
}

fn run_lint_command(args: &[String]) -> Result<()> {
    if args.iter().skip(1).any(|arg| is_help_arg(arg)) {
        eprintln!("{}", lint::help_text());
        return Ok(());
    }

    let options = lint::parse_lint_args(args.iter().map(String::as_str))?;
    lint::run(options)
}

fn run_font_metrics_command(args: &[String]) -> Result<()> {
    if args.iter().skip(1).any(|arg| is_help_arg(arg)) {
        eprintln!("{}", font_metrics::help_text());
        return Ok(());
    }

    let options = font_metrics::parse_font_metrics_args(args.iter().map(String::as_str))?;
    font_metrics::run(options)
}

fn run_readme_assets_command(args: &[String]) -> Result<()> {
    if args.iter().skip(1).any(|arg| is_help_arg(arg)) {
        eprintln!("{}", readme_assets::help_text());
        return Ok(());
    }

    let options = readme_assets::parse_readme_assets_args(args.iter().map(String::as_str))?;
    readme_assets::run(options)
}

fn print_help() {
    eprintln!(
        "\
cargo xtask <command>

Commands:
    architecture    Run the repo architecture suite
    font-metrics    Generate recorded font metrics tables
    lint            Run clippy and architecture boundary checks
    readme-assets   Refresh README showcase assets

Run `cargo xtask <command> --help` for details."
    );
}

fn print_architecture_help() {
    eprintln!("{}", architecture::help_text());
}

/// Resolve the workspace root at runtime.
///
/// 1. `MMDFLUX_REPO_ROOT` env var (explicit override for worktrees / CI).
/// 2. Walk up from the current directory looking for a workspace-level
///    `Cargo.toml` (one that sits next to `boundaries.toml`).
/// 3. Fall back to the compile-time `CARGO_MANIFEST_DIR` parent.
pub(crate) fn repo_root() -> PathBuf {
    if let Some(root) = std::env::var_os(REPO_ROOT_ENV) {
        return PathBuf::from(root);
    }

    if let Ok(cwd) = std::env::current_dir()
        && let Some(root) = find_workspace_root(&cwd)
    {
        return root;
    }

    // Compile-time fallback — correct when running from the source tree.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under the repository root")
        .to_path_buf()
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("boundaries.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn tracing_global_log_flag_is_removed_before_subcommand_parse() {
        let parsed = parse_xtask_args(["--log", "xtask=debug", "architecture", "check"]).unwrap();

        assert_eq!(parsed.log.filter.as_deref(), Some("xtask=debug"));
        assert_eq!(parsed.log.format, XtaskLogFormat::Compact);
        assert_eq!(parsed.log.file, None);
        assert_eq!(parsed.command_args, ["architecture", "check"]);
    }

    #[test]
    fn tracing_global_log_format_and_file_parse_before_subcommand() {
        let parsed = parse_xtask_args([
            "--log-format",
            "json",
            "--log-file",
            "target/xtask.log",
            "architecture",
            "check",
        ])
        .unwrap();

        assert_eq!(parsed.log.filter, None);
        assert_eq!(parsed.log.format, XtaskLogFormat::Json);
        assert_eq!(parsed.log.file, Some(PathBuf::from("target/xtask.log")));
        assert_eq!(parsed.command_args, ["architecture", "check"]);
    }

    #[test]
    fn tracing_invalid_log_filter_is_rejected_before_subcommand_execution() {
        let parsed = parse_xtask_args(["--log", "[", "architecture", "check"]).unwrap();
        let error = init_tracing_for_options(&parsed.log).unwrap_err();

        assert!(error.to_string().contains("invalid log filter"));
    }

    #[test]
    fn tracing_log_filter_precedence_prefers_flag_then_xtask_env_then_rust_log() {
        let parsed = parse_xtask_args(["--log", "xtask=debug", "architecture"]).unwrap();
        let flag = resolve_xtask_log_filter_with(&parsed.log, |_| None);
        assert_eq!(flag.as_deref(), Some("xtask=debug"));

        let parsed = parse_xtask_args(["architecture"]).unwrap();
        let xtask_env = resolve_xtask_log_filter_with(&parsed.log, |name| match name {
            "MMDFLUX_XTASK_LOG" => Some("xtask=trace".to_string()),
            "RUST_LOG" => Some("off".to_string()),
            _ => None,
        });
        assert_eq!(xtask_env.as_deref(), Some("xtask=trace"));

        let rust_env = resolve_xtask_log_filter_with(&parsed.log, |name| match name {
            "RUST_LOG" => Some("xtask=debug".to_string()),
            _ => None,
        });
        assert_eq!(rust_env.as_deref(), Some("xtask=debug"));
    }

    #[test]
    fn tracing_mmdflux_log_is_not_consumed_by_xtask_logging() {
        let parsed = parse_xtask_args(["architecture"]).unwrap();
        let filter = resolve_xtask_log_filter_with(&parsed.log, |name| match name {
            "MMDFLUX_LOG" => Some("mmdflux::runtime::render=debug".to_string()),
            _ => None,
        });

        assert_eq!(filter, None);
    }
}
