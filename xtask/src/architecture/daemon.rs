#![allow(dead_code)]

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::boundaries::{BoundariesRunReport, BoundaryViolation};

const WORKTREE_TARGET_DIR: &str = "target";
const XTASK_TARGET_DIR: &str = "xtask";
const DAEMON_METADATA_FILE: &str = "boundaries-daemon.json";
const DAEMON_SOCKET_FILE: &str = "boundaries.sock";
const WINDOWS_PIPE_PREFIX: &str = r"\\.\pipe\mmdflux-boundaries-";
const DAEMON_DISCOVERY_ROOT_ENV: &str = "XTASK_BOUNDARIES_DAEMON_DISCOVERY_ROOT";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(crate) const DAEMON_PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DaemonTransport {
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DaemonFreshness {
    IdleClean,
    Dirty,
    Running,
    IdleFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DaemonRenderOptions {
    pub(crate) verbose: bool,
    pub(crate) timings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DaemonRequest {
    Check {
        wait_for_fresh: bool,
        verbose: bool,
        timings: bool,
        no_color: bool,
    },
    NotifyDirty,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DaemonResponse {
    Check(CheckResponse),
    NotifyDirtyAck,
    Status(StatusResponse),
    Error {
        retry_locally: bool,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CheckResponse {
    pub(crate) freshness: DaemonFreshness,
    pub(crate) generation: u64,
    pub(crate) reused_warm_context: bool,
    pub(crate) duration_ms: u128,
    pub(crate) success: bool,
    pub(crate) rendered_output: String,
    pub(crate) summary: Option<String>,
    pub(crate) timings_output: Option<String>,
    #[serde(default)]
    pub(crate) violations: Vec<BoundaryViolation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StatusResponse {
    pub(crate) freshness: DaemonFreshness,
    pub(crate) generation: u64,
    pub(crate) last_started_at: Option<String>,
    pub(crate) last_finished_at: Option<String>,
    pub(crate) last_success: Option<bool>,
    pub(crate) render_options: DaemonRenderOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonCheckResult {
    Reused(CheckResponse),
    RetryLocally { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DaemonStatusResult {
    Live(StatusResponse),
    Unavailable { reason: String },
}

#[derive(Debug, Clone)]
struct LoadedDaemonMetadata {
    metadata: DaemonMetadata,
    metadata_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DaemonMetadata {
    pub(crate) protocol_version: u32,
    pub(crate) repo_root: PathBuf,
    pub(crate) worktree_id: String,
    pub(crate) transport: DaemonTransport,
    pub(crate) pid: u32,
    pub(crate) started_at: String,
    pub(crate) binary_version: String,
}

impl DaemonMetadata {
    pub(crate) fn for_repo(repo_root: &Path) -> Self {
        let worktree_id = worktree_id_for_repo(repo_root);
        Self {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            repo_root: repo_root.to_path_buf(),
            worktree_id: worktree_id.clone(),
            transport: daemon_transport_for_repo(repo_root, &worktree_id),
            pid: std::process::id(),
            started_at: unix_timestamp_string(),
            binary_version: daemon_binary_version(),
        }
    }

    pub(crate) fn metadata_path(&self) -> PathBuf {
        daemon_metadata_path(&self.repo_root, &self.worktree_id)
    }

    pub(crate) fn validate_for_repo(&self, repo_root: &Path) -> Result<()> {
        if self.protocol_version != DAEMON_PROTOCOL_VERSION {
            bail!(
                "daemon protocol mismatch: expected {}, found {}",
                DAEMON_PROTOCOL_VERSION,
                self.protocol_version
            );
        }

        if self.repo_root != repo_root {
            bail!(
                "daemon repo root mismatch: expected {}, found {}",
                repo_root.display(),
                self.repo_root.display()
            );
        }

        let expected_worktree_id = worktree_id_for_repo(repo_root);
        if self.worktree_id != expected_worktree_id {
            bail!(
                "daemon worktree mismatch: expected {}, found {}",
                expected_worktree_id,
                self.worktree_id
            );
        }

        let expected_transport = daemon_transport_for_repo(repo_root, &expected_worktree_id);
        if self.transport != expected_transport {
            bail!("daemon transport mismatch for {}", repo_root.display());
        }

        if self.binary_version != daemon_binary_version() {
            bail!(
                "daemon binary version mismatch: expected {}, found {}",
                daemon_binary_version(),
                self.binary_version
            );
        }

        Ok(())
    }
}

pub(crate) fn daemon_metadata_path(repo_root: &Path, worktree_id: &str) -> PathBuf {
    daemon_worktree_dir(repo_root, worktree_id).join(DAEMON_METADATA_FILE)
}

pub(crate) fn daemon_transport_for_repo(repo_root: &Path, worktree_id: &str) -> DaemonTransport {
    if cfg!(windows) {
        DaemonTransport::NamedPipe {
            name: format!("{WINDOWS_PIPE_PREFIX}{worktree_id}"),
        }
    } else {
        DaemonTransport::UnixSocket {
            path: daemon_worktree_dir(repo_root, worktree_id).join(DAEMON_SOCKET_FILE),
        }
    }
}

pub(crate) fn worktree_id_for_repo(repo_root: &Path) -> String {
    let identity = repo_identity_path(repo_root);
    let mut hash = FNV_OFFSET_BASIS;
    for byte in identity.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

pub(crate) trait DaemonEndpoint {
    fn cleanup(&mut self) -> Result<()>;
}

pub(crate) trait DaemonTransportBinder {
    type Endpoint: DaemonEndpoint;

    fn bind(
        &self,
        metadata: &DaemonMetadata,
        state: SharedDaemonState,
        render_options: DaemonRenderOptions,
    ) -> Result<Self::Endpoint>;
}

#[derive(Debug, Clone)]
pub(crate) struct SharedDaemonState {
    inner: Arc<SharedDaemonStateInner>,
}

#[derive(Debug)]
struct SharedDaemonStateInner {
    snapshot: Mutex<SharedDaemonSnapshot>,
    updates: Condvar,
}

#[derive(Debug, Clone)]
struct SharedDaemonSnapshot {
    freshness: DaemonFreshness,
    generation: u64,
    last_started_at: Option<String>,
    last_finished_at: Option<String>,
    last_report: Option<BoundariesRunReport>,
}

impl Default for SharedDaemonState {
    fn default() -> Self {
        Self {
            inner: Arc::new(SharedDaemonStateInner {
                snapshot: Mutex::new(SharedDaemonSnapshot::default()),
                updates: Condvar::new(),
            }),
        }
    }
}

impl Default for SharedDaemonSnapshot {
    fn default() -> Self {
        Self {
            freshness: DaemonFreshness::Dirty,
            generation: 0,
            last_started_at: None,
            last_finished_at: None,
            last_report: None,
        }
    }
}

impl SharedDaemonState {
    pub(crate) fn note_dirty(&self) {
        let mut snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("daemon state mutex poisoned");
        if snapshot.freshness != DaemonFreshness::Running {
            snapshot.freshness = DaemonFreshness::Dirty;
        }
        self.inner.updates.notify_all();
    }

    pub(crate) fn begin_run(&self) {
        let mut snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("daemon state mutex poisoned");
        snapshot.freshness = DaemonFreshness::Running;
        snapshot.last_started_at = Some(unix_timestamp_string());
        self.inner.updates.notify_all();
    }

    pub(crate) fn complete_run(&self, report: BoundariesRunReport) {
        let mut snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("daemon state mutex poisoned");
        snapshot.generation += 1;
        snapshot.last_finished_at = Some(unix_timestamp_string());
        snapshot.freshness = if report.success {
            DaemonFreshness::IdleClean
        } else {
            DaemonFreshness::IdleFailed
        };
        snapshot.last_report = Some(report);
        self.inner.updates.notify_all();
    }

    pub(crate) fn handle_status(&self, render_options: DaemonRenderOptions) -> StatusResponse {
        let snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("daemon state mutex poisoned");
        StatusResponse {
            freshness: snapshot.freshness,
            generation: snapshot.generation,
            last_started_at: snapshot.last_started_at.clone(),
            last_finished_at: snapshot.last_finished_at.clone(),
            last_success: snapshot.last_report.as_ref().map(|report| report.success),
            render_options,
        }
    }

    pub(crate) fn handle_request(
        &self,
        request: DaemonRequest,
        hosted_options: DaemonRenderOptions,
    ) -> DaemonResponse {
        match request {
            DaemonRequest::NotifyDirty => {
                self.note_dirty();
                DaemonResponse::NotifyDirtyAck
            }
            DaemonRequest::Status => DaemonResponse::Status(self.handle_status(hosted_options)),
            DaemonRequest::Check {
                wait_for_fresh,
                verbose,
                timings,
                ..
            } => {
                let requested_options = DaemonRenderOptions { verbose, timings };
                if requested_options != hosted_options {
                    return DaemonResponse::Error {
                        retry_locally: true,
                        message: format!(
                            "daemon render options mismatch: daemon verbose={}, timings={}, request verbose={}, timings={}",
                            hosted_options.verbose,
                            hosted_options.timings,
                            requested_options.verbose,
                            requested_options.timings
                        ),
                    };
                }

                let mut snapshot = self
                    .inner
                    .snapshot
                    .lock()
                    .expect("daemon state mutex poisoned");
                while wait_for_fresh
                    && matches!(
                        snapshot.freshness,
                        DaemonFreshness::Dirty | DaemonFreshness::Running
                    )
                {
                    snapshot = self
                        .inner
                        .updates
                        .wait(snapshot)
                        .expect("daemon state mutex poisoned");
                }

                match snapshot.last_report.as_ref() {
                    Some(report) => DaemonResponse::Check(CheckResponse {
                        freshness: snapshot.freshness,
                        generation: snapshot.generation,
                        reused_warm_context: true,
                        duration_ms: 0,
                        success: report.success,
                        rendered_output: report.rendered_output.clone(),
                        summary: report.summary.clone(),
                        timings_output: report.timings_output.clone(),
                        violations: report.violations.clone(),
                    }),
                    None => DaemonResponse::Error {
                        retry_locally: true,
                        message: "daemon has no completed boundaries result yet".to_string(),
                    },
                }
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct DaemonHost<E: DaemonEndpoint> {
    metadata: DaemonMetadata,
    metadata_path: PathBuf,
    state: SharedDaemonState,
    render_options: DaemonRenderOptions,
    endpoint: Option<E>,
}

impl<E: DaemonEndpoint> DaemonHost<E> {
    pub(crate) fn start_with_binder(
        repo_root: &Path,
        binder: &impl DaemonTransportBinder<Endpoint = E>,
        render_options: DaemonRenderOptions,
    ) -> Result<Self> {
        let metadata = DaemonMetadata::for_repo(repo_root);
        let metadata_path = metadata.metadata_path();
        if let Some(parent) = metadata_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let state = SharedDaemonState::default();
        let endpoint = binder.bind(&metadata, state.clone(), render_options)?;
        if let Err(error) = write_metadata_file(&metadata_path, &metadata) {
            let mut endpoint = endpoint;
            let _ = endpoint.cleanup();
            return Err(error);
        }
        Ok(Self {
            metadata,
            metadata_path,
            state,
            render_options,
            endpoint: Some(endpoint),
        })
    }

    pub(crate) fn metadata(&self) -> &DaemonMetadata {
        &self.metadata
    }

    pub(crate) fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub(crate) fn note_dirty(&self) {
        self.state.note_dirty();
    }

    pub(crate) fn begin_run(&self) {
        self.state.begin_run();
    }

    pub(crate) fn complete_run(&self, report: BoundariesRunReport) {
        self.state.complete_run(report);
    }

    pub(crate) fn handle_request(&self, request: DaemonRequest) -> DaemonResponse {
        self.state.handle_request(request, self.render_options)
    }

    pub(crate) fn handle_status(&self) -> StatusResponse {
        self.state.handle_status(self.render_options)
    }

    pub(crate) fn shutdown(mut self) {
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if let Some(endpoint) = self.endpoint.as_mut() {
            let _ = endpoint.cleanup();
        }
        self.endpoint = None;
        let _ = remove_file_if_exists(&self.metadata_path);
    }
}

impl<E: DaemonEndpoint> Drop for DaemonHost<E> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PlatformDaemonBinder;

#[derive(Debug)]
pub(crate) enum PlatformDaemonEndpoint {
    #[cfg(unix)]
    UnixSocket {
        path: PathBuf,
        shutdown: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    },
    NamedPipe {
        _name: String,
    },
}

impl DaemonTransportBinder for PlatformDaemonBinder {
    type Endpoint = PlatformDaemonEndpoint;

    fn bind(
        &self,
        metadata: &DaemonMetadata,
        state: SharedDaemonState,
        render_options: DaemonRenderOptions,
    ) -> Result<Self::Endpoint> {
        match &metadata.transport {
            #[cfg(unix)]
            DaemonTransport::UnixSocket { path } => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                remove_file_if_exists(path)?;
                let listener = std::os::unix::net::UnixListener::bind(path).with_context(|| {
                    format!("failed to bind unix daemon socket {}", path.display())
                })?;
                let shutdown = Arc::new(AtomicBool::new(false));
                let thread = Some(spawn_unix_daemon_server(
                    listener,
                    state,
                    render_options,
                    Arc::clone(&shutdown),
                ));
                Ok(PlatformDaemonEndpoint::UnixSocket {
                    path: path.clone(),
                    shutdown,
                    thread,
                })
            }
            #[cfg(not(unix))]
            DaemonTransport::UnixSocket { path } => bail!(
                "unix socket daemon transport is unavailable on this platform: {}",
                path.display()
            ),
            DaemonTransport::NamedPipe { name } => Ok(PlatformDaemonEndpoint::NamedPipe {
                _name: name.clone(),
            }),
        }
    }
}

impl DaemonEndpoint for PlatformDaemonEndpoint {
    fn cleanup(&mut self) -> Result<()> {
        match self {
            #[cfg(unix)]
            Self::UnixSocket {
                path,
                shutdown,
                thread,
            } => {
                shutdown.store(true, Ordering::SeqCst);
                let _ = std::os::unix::net::UnixStream::connect(&*path);
                if let Some(thread) = thread.take() {
                    let _ = thread.join();
                }
                remove_file_if_exists(path)
            }
            Self::NamedPipe { .. } => Ok(()),
        }
    }
}

pub(crate) fn try_request_check(
    repo_root: &Path,
    render_options: DaemonRenderOptions,
) -> DaemonCheckResult {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            return DaemonCheckResult::RetryLocally {
                reason: "no daemon metadata found".to_string(),
            };
        }
        Err(error) => {
            return DaemonCheckResult::RetryLocally {
                reason: format!("{error:#}"),
            };
        }
    };

    match send_request(
        &loaded.metadata,
        &DaemonRequest::Check {
            wait_for_fresh: true,
            verbose: render_options.verbose,
            timings: render_options.timings,
            no_color: true,
        },
    ) {
        Ok(DaemonResponse::Check(response)) => DaemonCheckResult::Reused(response),
        Ok(DaemonResponse::Error {
            retry_locally,
            message,
        }) if retry_locally => DaemonCheckResult::RetryLocally { reason: message },
        Ok(other) => DaemonCheckResult::RetryLocally {
            reason: format!("unexpected daemon response: {other:?}"),
        },
        Err(error) => DaemonCheckResult::RetryLocally {
            reason: stale_cleanup_reason(loaded, error),
        },
    }
}

pub(crate) fn try_notify_dirty(repo_root: &Path) -> bool {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(metadata)) => metadata,
        _ => return false,
    };

    matches!(
        send_request(&loaded.metadata, &DaemonRequest::NotifyDirty),
        Ok(DaemonResponse::NotifyDirtyAck)
    )
}

pub(crate) fn query_status(repo_root: &Path) -> DaemonStatusResult {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            return DaemonStatusResult::Unavailable {
                reason: "no warm boundaries daemon for this worktree".to_string(),
            };
        }
        Err(error) => {
            return DaemonStatusResult::Unavailable {
                reason: format!("no warm boundaries daemon for this worktree ({error:#})"),
            };
        }
    };

    match send_request(&loaded.metadata, &DaemonRequest::Status) {
        Ok(DaemonResponse::Status(status)) => DaemonStatusResult::Live(status),
        Ok(other) => DaemonStatusResult::Unavailable {
            reason: format!(
                "no warm boundaries daemon for this worktree (unexpected response: {other:?})"
            ),
        },
        Err(error) => DaemonStatusResult::Unavailable {
            reason: format!(
                "no warm boundaries daemon for this worktree ({})",
                stale_cleanup_reason(loaded, error)
            ),
        },
    }
}

fn daemon_worktree_dir(repo_root: &Path, worktree_id: &str) -> PathBuf {
    repo_root
        .join(WORKTREE_TARGET_DIR)
        .join(XTASK_TARGET_DIR)
        .join(worktree_id)
}

fn load_metadata_for_repo(repo_root: &Path) -> Result<Option<LoadedDaemonMetadata>> {
    let discovery_root = daemon_discovery_root(repo_root);
    let metadata_path = DaemonMetadata::for_repo(&discovery_root).metadata_path();
    let json = match std::fs::read_to_string(&metadata_path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", metadata_path.display()));
        }
    };

    let metadata: DaemonMetadata = serde_json::from_str(&json)
        .with_context(|| {
            format!(
                "failed to parse daemon metadata {}",
                metadata_path.display()
            )
        })
        .map_err(|error| cleanup_invalid_metadata(&metadata_path, error))?;
    metadata
        .validate_for_repo(&discovery_root)
        .map_err(|error| cleanup_invalid_metadata(&metadata_path, error))?;
    Ok(Some(LoadedDaemonMetadata {
        metadata,
        metadata_path,
    }))
}

fn daemon_discovery_root(repo_root: &Path) -> PathBuf {
    std::env::var_os(DAEMON_DISCOVERY_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.to_path_buf())
}

fn send_request(metadata: &DaemonMetadata, request: &DaemonRequest) -> Result<DaemonResponse> {
    match &metadata.transport {
        #[cfg(unix)]
        DaemonTransport::UnixSocket { path } => send_request_over_unix_socket(path, request),
        #[cfg(not(unix))]
        DaemonTransport::UnixSocket { path } => bail!(
            "unix socket daemon transport is unavailable on this platform: {}",
            path.display()
        ),
        DaemonTransport::NamedPipe { name } => {
            bail!("named pipe daemon transport is not yet supported for client reuse: {name}")
        }
    }
}

#[cfg(unix)]
fn send_request_over_unix_socket(path: &Path, request: &DaemonRequest) -> Result<DaemonResponse> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("failed to connect to daemon socket {}", path.display()))?;
    serde_json::to_writer(&mut stream, request)
        .with_context(|| format!("failed to write daemon request {}", path.display()))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .with_context(|| format!("failed to finish daemon request {}", path.display()))?;
    let response = serde_json::from_reader(BufReader::new(stream))
        .with_context(|| format!("failed to read daemon response {}", path.display()))?;
    Ok(response)
}

fn repo_identity_path(repo_root: &Path) -> PathBuf {
    std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf())
}

fn daemon_binary_version() -> String {
    format!("xtask-boundaries-daemon-v{DAEMON_PROTOCOL_VERSION}")
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

fn write_metadata_file(path: &Path, metadata: &DaemonMetadata) -> Result<()> {
    let json =
        serde_json::to_vec_pretty(metadata).context("failed to serialize daemon metadata")?;
    std::fs::write(path, json)
        .with_context(|| format!("failed to write daemon metadata {}", path.display()))
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn cleanup_invalid_metadata(metadata_path: &Path, error: anyhow::Error) -> anyhow::Error {
    let _ = remove_file_if_exists(metadata_path);
    error
}

fn stale_cleanup_reason(loaded: LoadedDaemonMetadata, error: anyhow::Error) -> String {
    if should_cleanup_stale_transport(&error) {
        maybe_cleanup_stale_transport(&loaded);
        format!("{error:#} (removed stale daemon metadata)")
    } else {
        format!("{error:#}")
    }
}

fn maybe_cleanup_stale_transport(loaded: &LoadedDaemonMetadata) {
    match &loaded.metadata.transport {
        #[cfg(unix)]
        DaemonTransport::UnixSocket { path } => {
            let _ = remove_file_if_exists(path);
        }
        DaemonTransport::NamedPipe { .. } => {}
        #[cfg(not(unix))]
        DaemonTransport::UnixSocket { .. } => {}
    }
    let _ = remove_file_if_exists(&loaded.metadata_path);
}

fn should_cleanup_stale_transport(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| {
                matches!(
                    io_error.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                )
            })
    })
}

#[cfg(unix)]
fn spawn_unix_daemon_server(
    listener: std::os::unix::net::UnixListener,
    state: SharedDaemonState,
    render_options: DaemonRenderOptions,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    let _ = handle_unix_client(stream, &state, render_options);
                }
                Err(error) if shutdown.load(Ordering::SeqCst) => {
                    let _ = error;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}

#[cfg(unix)]
fn handle_unix_client(
    mut stream: std::os::unix::net::UnixStream,
    state: &SharedDaemonState,
    render_options: DaemonRenderOptions,
) -> Result<()> {
    let request: DaemonRequest = serde_json::from_reader(BufReader::new(
        stream
            .try_clone()
            .context("failed to clone daemon stream for request read")?,
    ))
    .context("failed to decode daemon request")?;
    let response = state.handle_request(request, render_options);
    serde_json::to_writer(&mut stream, &response).context("failed to encode daemon response")?;
    stream.flush().context("failed to flush daemon response")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{
        DaemonFreshness, DaemonMetadata, DaemonRenderOptions, DaemonRequest, DaemonResponse,
        DaemonTransport, SharedDaemonState,
    };
    use crate::architecture::boundaries::BoundariesRunReport;

    fn render_options() -> DaemonRenderOptions {
        DaemonRenderOptions {
            verbose: false,
            timings: false,
        }
    }

    #[test]
    fn daemon_metadata_uses_worktree_specific_target_xtask_paths() {
        let repo_root = Path::new("/tmp/example-repo/worktrees/feature-a");
        let metadata = DaemonMetadata::for_repo(repo_root);
        assert_eq!(metadata.repo_root, repo_root);
        assert!(!metadata.worktree_id.is_empty());
        assert_eq!(
            metadata.metadata_path(),
            repo_root
                .join("target")
                .join("xtask")
                .join(&metadata.worktree_id)
                .join("boundaries-daemon.json")
        );
        match &metadata.transport {
            DaemonTransport::UnixSocket { path } => assert_eq!(
                path,
                &repo_root
                    .join("target")
                    .join("xtask")
                    .join(&metadata.worktree_id)
                    .join("boundaries.sock")
            ),
            DaemonTransport::NamedPipe { name } => {
                assert!(name.contains(&metadata.worktree_id));
            }
        }
    }

    #[test]
    fn different_worktrees_do_not_share_daemon_identity() {
        let left = DaemonMetadata::for_repo(Path::new("/tmp/example-repo/worktrees/feature-a"));
        let right = DaemonMetadata::for_repo(Path::new("/tmp/example-repo/worktrees/feature-b"));
        assert_ne!(left.worktree_id, right.worktree_id);
    }

    #[test]
    fn daemon_request_round_trips_through_json() {
        let request = DaemonRequest::Check {
            wait_for_fresh: true,
            verbose: false,
            timings: true,
            no_color: true,
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn daemon_metadata_round_trips_through_json() {
        let metadata = DaemonMetadata::for_repo(Path::new("/tmp/example-repo/worktrees/feature-a"));
        let json = serde_json::to_string(&metadata).unwrap();
        let decoded: DaemonMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, metadata);
    }

    #[test]
    fn stale_or_mismatched_metadata_is_rejected() {
        let metadata = DaemonMetadata {
            protocol_version: 99,
            ..DaemonMetadata::for_repo(Path::new("/tmp/example-repo"))
        };
        assert!(
            metadata
                .validate_for_repo(Path::new("/tmp/example-repo"))
                .is_err()
        );
    }

    #[test]
    fn check_request_waits_for_rerun_when_daemon_is_dirty() {
        let daemon = SharedDaemonState::default();
        daemon.note_dirty();
        let waiting_daemon = daemon.clone();
        let (sender, receiver) = mpsc::channel();

        let waiting_thread = std::thread::spawn(move || {
            let response = waiting_daemon.handle_request(
                DaemonRequest::Check {
                    wait_for_fresh: true,
                    verbose: false,
                    timings: false,
                    no_color: true,
                },
                render_options(),
            );
            sender.send(response).unwrap();
        });

        std::thread::sleep(Duration::from_millis(20));
        daemon.complete_run(failing_report());

        let response = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        waiting_thread.join().unwrap();

        match response {
            DaemonResponse::Check(check) => {
                assert_eq!(check.freshness, DaemonFreshness::IdleFailed);
                assert!(!check.success);
                assert_eq!(
                    check.summary.as_deref(),
                    Some("error: architecture boundaries failed with 1 violation")
                );
            }
            other => panic!("expected check response, found {other:?}"),
        }
    }

    #[test]
    fn status_request_reports_generation_and_last_outcome() {
        let daemon = SharedDaemonState::default();
        daemon.begin_run();
        daemon.complete_run(passing_report());

        let status = daemon.handle_status(render_options());

        assert_eq!(status.freshness, DaemonFreshness::IdleClean);
        assert_eq!(status.generation, 1);
        assert_eq!(status.last_success, Some(true));
        assert!(status.last_started_at.is_some());
        assert!(status.last_finished_at.is_some());
        assert_eq!(status.render_options, render_options());
    }

    #[test]
    fn notify_dirty_transitions_idle_to_dirty() {
        let daemon = SharedDaemonState::default();
        daemon.begin_run();
        daemon.complete_run(passing_report());
        assert_eq!(
            daemon.handle_status(render_options()).freshness,
            DaemonFreshness::IdleClean
        );

        let response = daemon.handle_request(DaemonRequest::NotifyDirty, render_options());

        assert_eq!(response, DaemonResponse::NotifyDirtyAck);
        assert_eq!(
            daemon.handle_status(render_options()).freshness,
            DaemonFreshness::Dirty
        );
    }

    fn passing_report() -> BoundariesRunReport {
        BoundariesRunReport {
            success: true,
            rendered_output: String::new(),
            summary: None,
            timings_output: None,
            violations: Vec::new(),
        }
    }

    fn failing_report() -> BoundariesRunReport {
        BoundariesRunReport {
            success: false,
            rendered_output: "error[boundaries]: forbidden dependency ...".to_string(),
            summary: Some("error: architecture boundaries failed with 1 violation".to_string()),
            timings_output: None,
            violations: Vec::new(),
        }
    }
}
