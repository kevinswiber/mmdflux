use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, thread};

const HOST_DISCOVERY_ROOT_ENV: &str = "XTASK_BOUNDARIES_HOST_DISCOVERY_ROOT";
const WORKTREE_TARGET_DIR: &str = "target";
const XTASK_TARGET_DIR: &str = "xtask";
const HOST_METADATA_FILE: &str = "architecture-host.json";
const HOST_SOCKET_FILE: &str = "architecture.sock";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

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
fn architecture_help_reflects_subcommand_structure() {
    let help = architecture_help_run();
    assert_eq!(help.status_code, 0);
    assert!(help.output.contains("check"));
    assert!(help.output.contains("host"));
    assert!(help.output.contains("--notify-dirty"));
    assert!(!help.output.contains("daemon"));
    assert!(!help.output.contains("--background"));
}

#[test]
fn top_level_layers_command_is_rejected() {
    let legacy = run_xtask(&["layers"]);
    assert_ne!(legacy.status_code, 0);
    assert!(legacy.output.contains("unknown xtask subcommand `layers`"));
}

#[test]
fn boundaries_falls_back_locally_when_no_host_is_present() {
    let harness = BoundariesCliHarness::without_host();

    let outcome = harness.run(&["architecture", "check"]);

    assert_ne!(outcome.status_code, 0);
    assert!(outcome.output.contains("unsupported policy version: 99"));
}

#[cfg(unix)]
#[test]
fn boundaries_uses_host_when_metadata_and_protocol_match() {
    let harness = BoundariesCliHarness::with_live_host(HostFixture::successful_check());

    let outcome = harness.run(&["architecture", "check"]);

    assert_eq!(outcome.status_code, 0);
    assert_eq!(harness.host_requests(), 1);
    assert!(!outcome.output.contains("unsupported policy version: 99"));
}

#[test]
fn boundaries_falls_back_locally_on_protocol_mismatch() {
    let harness = BoundariesCliHarness::with_incompatible_metadata();

    let outcome = harness.run(&["architecture", "check"]);

    assert_ne!(outcome.status_code, 0);
    assert!(outcome.output.contains("unsupported policy version: 99"));
    assert_eq!(harness.host_requests(), 0);
}

#[test]
fn boundaries_ignores_host_metadata_from_a_different_worktree() {
    let harness = BoundariesCliHarness::with_foreign_worktree_metadata();

    let outcome = harness.run(&["architecture", "check"]);

    assert_ne!(outcome.status_code, 0);
    assert!(outcome.output.contains("unsupported policy version: 99"));
    assert_eq!(harness.host_requests(), 0);
}

#[test]
fn boundaries_status_reports_when_no_host_is_running() {
    let harness = BoundariesCliHarness::without_host();

    let outcome = harness.run(&["architecture", "check", "--status"]);

    assert_eq!(outcome.status_code, 0);
    assert!(outcome.output.contains("no warm boundaries host"));
}

#[cfg(unix)]
#[test]
fn boundaries_fresh_bypasses_host_reuse() {
    let harness = BoundariesCliHarness::with_live_host(HostFixture::successful_check());

    let outcome = harness.run(&["architecture", "check", "--fresh", "--verbose"]);

    assert_ne!(outcome.status_code, 0);
    assert_eq!(harness.host_requests(), 0);
    assert!(
        outcome
            .output
            .contains("running local boundaries check (--fresh)")
    );
    assert!(outcome.output.contains("unsupported policy version: 99"));
}

#[cfg(unix)]
#[test]
fn stale_unix_host_metadata_is_removed_after_transport_failure() {
    let harness = BoundariesCliHarness::with_stale_transport_metadata();

    let outcome = harness.run(&["architecture", "check", "--status"]);

    assert_eq!(outcome.status_code, 0);
    assert!(outcome.output.contains("removed stale host metadata"));
    assert!(!harness.metadata_path().exists());
}

fn boundary_suite_run() -> &'static CommandRun {
    BOUNDARY_SUITE_RUN.get_or_init(|| run_xtask(&["architecture", "--timings", "--fresh"]))
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

#[derive(Debug)]
struct BoundariesCliHarness {
    discovery_root: PathBuf,
    config_path: PathBuf,
    host: Option<HostFixture>,
}

impl BoundariesCliHarness {
    fn without_host() -> Self {
        Self::new(None, HarnessMode::NoMetadata)
    }

    #[cfg(unix)]
    fn with_live_host(host: HostFixture) -> Self {
        Self::new(Some(host), HarnessMode::LiveHost)
    }

    fn with_incompatible_metadata() -> Self {
        Self::new(None, HarnessMode::ProtocolMismatch)
    }

    fn with_foreign_worktree_metadata() -> Self {
        Self::new(None, HarnessMode::ForeignWorktree)
    }

    #[cfg(unix)]
    fn with_stale_transport_metadata() -> Self {
        Self::new(None, HarnessMode::StaleTransport)
    }

    fn new(mut host: Option<HostFixture>, mode: HarnessMode) -> Self {
        let discovery_root = unique_temp_dir("xbd-root");
        let config_path = discovery_root.join("invalid-boundaries.toml");
        fs::create_dir_all(&discovery_root).unwrap();
        fs::write(&config_path, "version = 99\n").unwrap();

        let metadata_path = host_metadata_path(&discovery_root);
        if let Some(parent) = metadata_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        match mode {
            HarnessMode::NoMetadata => {}
            HarnessMode::LiveHost => {
                let host = host.as_mut().expect("live host fixture is required");
                host.start_for_discovery_root(&discovery_root);
                fs::write(&metadata_path, host.metadata_json()).unwrap();
            }
            HarnessMode::ProtocolMismatch => {
                fs::write(&metadata_path, incompatible_metadata_json(&discovery_root)).unwrap();
            }
            HarnessMode::ForeignWorktree => {
                fs::write(
                    &metadata_path,
                    foreign_worktree_metadata_json(&discovery_root),
                )
                .unwrap();
            }
            HarnessMode::StaleTransport => {
                fs::write(
                    &metadata_path,
                    stale_transport_metadata_json(&discovery_root),
                )
                .unwrap();
            }
        }

        Self {
            discovery_root,
            config_path,
            host,
        }
    }

    fn run(&self, args: &[&str]) -> CommandRun {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.args(args);
        command.env("SEMANTIC_BOUNDARIES_CONFIG", &self.config_path);
        command.env(HOST_DISCOVERY_ROOT_ENV, &self.discovery_root);

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

    fn host_requests(&self) -> usize {
        self.host
            .as_ref()
            .map_or(0, |host| host.requests.load(Ordering::SeqCst))
    }

    fn metadata_path(&self) -> PathBuf {
        host_metadata_path(&self.discovery_root)
    }
}

impl Drop for BoundariesCliHarness {
    fn drop(&mut self) {
        if let Some(host) = self.host.take() {
            drop(host);
        }
        let _ = fs::remove_dir_all(&self.discovery_root);
    }
}

#[derive(Debug, Clone, Copy)]
enum HarnessMode {
    NoMetadata,
    LiveHost,
    ProtocolMismatch,
    ForeignWorktree,
    StaleTransport,
}

#[cfg(unix)]
#[derive(Debug)]
struct HostFixture {
    requests: Arc<AtomicUsize>,
    response_json: String,
    socket_path: Option<PathBuf>,
    thread: Option<thread::JoinHandle<()>>,
    metadata_json: Option<String>,
}

#[cfg(unix)]
impl HostFixture {
    fn successful_check() -> Self {
        Self::new(
            serde_json::json!({
                "Check": {
                    "freshness": "IdleClean",
                    "generation": 1,
                    "reused_warm_context": true,
                    "duration_ms": 0,
                    "success": true,
                    "rendered_output": "",
                    "summary": null,
                    "timings_output": null
                }
            })
            .to_string(),
        )
    }

    fn new(response_json: String) -> Self {
        Self {
            requests: Arc::new(AtomicUsize::new(0)),
            response_json,
            socket_path: None,
            thread: None,
            metadata_json: None,
        }
    }

    fn start_for_discovery_root(&mut self, discovery_root: &Path) {
        let socket_path = host_socket_path(discovery_root);
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        self.thread = Some(start_unix_host(
            socket_path.clone(),
            Arc::clone(&self.requests),
            self.response_json.clone(),
        ));
        self.socket_path = Some(socket_path.clone());
        self.metadata_json = Some(
            serde_json::json!({
                "protocol_version": 1,
                "repo_root": discovery_root,
                "worktree_id": worktree_id_for_repo(discovery_root),
                "binary_version": "xtask-boundaries-host-v1",
                "transport": {
                    "UnixSocket": {
                        "path": socket_path
                    }
                },
                "leader": {
                    "pid": std::process::id(),
                    "started_at": "0"
                },
                "standbys": []
            })
            .to_string(),
        );
    }

    fn metadata_json(&self) -> &str {
        self.metadata_json
            .as_deref()
            .expect("host metadata should be configured")
    }
}

#[cfg(unix)]
impl Drop for HostFixture {
    fn drop(&mut self) {
        if let Some(socket_path) = &self.socket_path {
            #[cfg(unix)]
            {
                use std::os::unix::net::UnixStream;

                let _ = UnixStream::connect(socket_path);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(unix)]
fn start_unix_host(
    socket_path: PathBuf,
    requests: Arc<AtomicUsize>,
    response_json: String,
) -> thread::JoinHandle<()> {
    use std::os::unix::net::UnixListener;

    let listener = UnixListener::bind(&socket_path).unwrap();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            requests.fetch_add(1, Ordering::SeqCst);
            let mut input = Vec::new();
            let _ = stream.read_to_end(&mut input);
            let _ = stream.write_all(response_json.as_bytes());
            let _ = stream.flush();
        }
        let _ = fs::remove_file(socket_path);
    })
}

fn incompatible_metadata_json(discovery_root: &Path) -> String {
    serde_json::json!({
        "protocol_version": 99,
        "repo_root": discovery_root,
        "worktree_id": worktree_id_for_repo(discovery_root),
        "binary_version": "xtask-boundaries-host-v1",
        "transport": {
            transport_key(): transport_value(discovery_root)
        },
        "leader": {
            "pid": std::process::id(),
            "started_at": "0"
        },
        "standbys": []
    })
    .to_string()
}

fn foreign_worktree_metadata_json(discovery_root: &Path) -> String {
    let foreign_root = discovery_root.join("foreign-worktree");
    serde_json::json!({
        "protocol_version": 1,
        "repo_root": foreign_root,
        "worktree_id": worktree_id_for_repo(&foreign_root),
        "binary_version": "xtask-boundaries-host-v1",
        "transport": {
            transport_key(): transport_value(&foreign_root)
        },
        "leader": {
            "pid": std::process::id(),
            "started_at": "0"
        },
        "standbys": []
    })
    .to_string()
}

fn stale_transport_metadata_json(discovery_root: &Path) -> String {
    serde_json::json!({
        "protocol_version": 1,
        "repo_root": discovery_root,
        "worktree_id": worktree_id_for_repo(discovery_root),
        "binary_version": "xtask-boundaries-host-v1",
        "transport": {
            "UnixSocket": {
                "path": host_socket_path(discovery_root)
            }
        },
        "leader": {
            "pid": std::process::id(),
            "started_at": "0"
        },
        "standbys": []
    })
    .to_string()
}

fn host_metadata_path(discovery_root: &Path) -> PathBuf {
    discovery_root
        .join(WORKTREE_TARGET_DIR)
        .join(XTASK_TARGET_DIR)
        .join(worktree_id_for_repo(discovery_root))
        .join(HOST_METADATA_FILE)
}

fn host_socket_path(discovery_root: &Path) -> PathBuf {
    discovery_root
        .join(WORKTREE_TARGET_DIR)
        .join(XTASK_TARGET_DIR)
        .join(worktree_id_for_repo(discovery_root))
        .join(HOST_SOCKET_FILE)
}

fn transport_key() -> &'static str {
    if cfg!(windows) {
        "NamedPipe"
    } else {
        "UnixSocket"
    }
}

fn transport_value(discovery_root: &Path) -> serde_json::Value {
    if cfg!(windows) {
        serde_json::json!({
            "name": format!(r"\\.\pipe\mmdflux-architecture-{}", worktree_id_for_repo(discovery_root))
        })
    } else {
        serde_json::json!({
            "path": host_socket_path(discovery_root)
        })
    }
}

fn worktree_id_for_repo(repo_root: &Path) -> String {
    let identity = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let mut hash = FNV_OFFSET_BASIS;
    for byte in identity.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let base = Path::new("/tmp");
    if base.exists() {
        base.join(unique)
    } else {
        std::env::temp_dir().join(unique)
    }
}
