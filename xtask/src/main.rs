mod architecture;
mod lint;

use std::path::{Path, PathBuf};

use anyhow::Result;

const REPO_ROOT_ENV: &str = "MMDFLUX_REPO_ROOT";

fn main() {
    if let Err(err) = try_main() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        "architecture" => run_architecture_command(&args),
        "lint" => run_lint_command(&args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => anyhow::bail!("unknown xtask subcommand `{other}`"),
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

fn print_help() {
    eprintln!(
        "\
cargo xtask <command>

Commands:
    architecture    Run the repo architecture suite
    lint            Run clippy and architecture boundary checks

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
