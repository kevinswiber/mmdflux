use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
struct CommandRun {
    status_code: i32,
    output: String,
}

#[test]
fn architecture_watch_noninteractive_boundaries_runs_once() {
    let temp_path = std::env::temp_dir().join(format!(
        "xtask-boundaries-empty-{}-{}.toml",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&temp_path, "version = 1\n[modules]\n").unwrap();

    let run = run_xtask(
        &["architecture", "boundaries", "--watch"],
        &[("SEMANTIC_BOUNDARIES_CONFIG", temp_path.to_str().unwrap())],
    );

    let _ = fs::remove_file(&temp_path);

    assert_eq!(run.status_code, 0);
    assert!(run.output.contains("[run 1] PASS boundaries"));
    assert!(!run.output.contains("[watch] waiting for changes..."));
}

#[test]
fn architecture_watch_noninteractive_preserves_failure_status() {
    let temp_path = std::env::temp_dir().join(format!(
        "xtask-boundaries-invalid-{}-{}.toml",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&temp_path, "version = 2\n").unwrap();

    let run = run_xtask(
        &["architecture", "boundaries", "--watch"],
        &[("SEMANTIC_BOUNDARIES_CONFIG", temp_path.to_str().unwrap())],
    );

    let _ = fs::remove_file(&temp_path);

    assert_ne!(run.status_code, 0);
    assert!(
        run.output
            .contains("unsupported semantic boundaries config version 2")
    );
    assert!(run.output.contains("last run failed"));
}

fn run_xtask(args: &[&str], envs: &[(&str, &str)]) -> CommandRun {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }

    let output = command
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
