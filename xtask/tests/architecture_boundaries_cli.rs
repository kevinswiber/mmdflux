use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug)]
struct CommandRun {
    status_code: i32,
    output: String,
}

static BOUNDARY_SUITE_RUN: OnceLock<CommandRun> = OnceLock::new();
static ARCHITECTURE_HELP_RUN: OnceLock<CommandRun> = OnceLock::new();

#[test]
fn architecture_boundaries_emits_timing_headers() {
    let canonical = boundary_suite_run();
    assert!(canonical.output.contains("qualified path scan"));
    assert!(canonical.output.contains("top-level boundary discovery"));
}

#[test]
fn architecture_help_only_mentions_canonical_suites() {
    let help = architecture_help_run();
    assert_eq!(help.status_code, 0);
    assert!(help.output.contains("boundaries"));
    assert!(!help.output.contains("surface"));
    assert!(!help.output.contains("structure"));
    assert!(!help.output.contains("layers"));
}

#[test]
fn top_level_layers_command_is_rejected() {
    let legacy = run_xtask(&["layers"]);
    assert_ne!(legacy.status_code, 0);
    assert!(legacy.output.contains("unknown xtask subcommand `layers`"));
}

fn boundary_suite_run() -> &'static CommandRun {
    BOUNDARY_SUITE_RUN.get_or_init(|| run_xtask(&["architecture", "boundaries", "--timings"]))
}

fn architecture_help_run() -> &'static CommandRun {
    ARCHITECTURE_HELP_RUN.get_or_init(|| run_xtask(&["architecture", "--help"]))
}

fn run_xtask(args: &[&str]) -> CommandRun {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run xtask {:?}: {error}", args));

    CommandRun {
        status_code: output.status.code().unwrap_or(-1),
        output: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}
