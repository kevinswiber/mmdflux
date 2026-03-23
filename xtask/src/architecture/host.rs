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
const HOST_METADATA_FILE: &str = "architecture-host.json";
const HOST_SOCKET_FILE: &str = "architecture.sock";
const WINDOWS_PIPE_PREFIX: &str = r"\\.\pipe\mmdflux-architecture-";
const HOST_DISCOVERY_ROOT_ENV: &str = "XTASK_BOUNDARIES_HOST_DISCOVERY_ROOT";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(crate) const HOST_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HostTransport {
    UnixSocket { path: PathBuf },
    NamedPipe { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HostFreshness {
    IdleClean,
    Dirty,
    Running,
    IdleFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HostRenderOptions {
    pub(crate) verbose: bool,
    pub(crate) timings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HostRequest {
    Check {
        wait_for_fresh: bool,
        verbose: bool,
        timings: bool,
        no_color: bool,
    },
    NotifyDirty,
    Status,
    Shutdown,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HostResponse {
    Check(CheckResponse),
    NotifyDirtyAck,
    Status(StatusResponse),
    ShuttingDown,
    Graph(super::boundaries::BoundaryGraph),
    Error {
        retry_locally: bool,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CheckResponse {
    pub(crate) freshness: HostFreshness,
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
    pub(crate) freshness: HostFreshness,
    pub(crate) generation: u64,
    pub(crate) last_started_at: Option<String>,
    pub(crate) last_finished_at: Option<String>,
    pub(crate) last_success: Option<bool>,
    pub(crate) render_options: HostRenderOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostCheckResult {
    Reused(CheckResponse),
    RetryLocally { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostStatusResult {
    Live(StatusResponse),
    Unavailable { reason: String },
}

#[derive(Debug, Clone)]
struct LoadedCluster {
    cluster: ClusterMetadata,
    metadata_path: PathBuf,
}

/// Lifecycle state of a host process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostState {
    /// Host is registered but has not yet completed its first boundaries run.
    Warming,
    /// Host has completed warm-up and is serving requests.
    /// This is the default for backward compatibility with metadata files that
    /// predate the `state` field.
    #[default]
    Ready,
}

/// How long a host can stay in `Warming` before being treated as stuck/dead.
const WARMUP_TIMEOUT_SECS: u64 = 120;

/// A single host process in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HostEntry {
    pub(crate) pid: u32,
    pub(crate) started_at: String,
    #[serde(default)]
    pub(crate) state: HostState,
}

impl HostEntry {
    /// A host is effectively alive if its PID exists and it hasn't been stuck
    /// warming up beyond the timeout.
    pub(crate) fn is_effectively_alive(&self) -> bool {
        if !is_pid_alive(self.pid) {
            return false;
        }
        if self.state == HostState::Warming
            && let Ok(started) = self.started_at.parse::<u64>()
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now.saturating_sub(started) > WARMUP_TIMEOUT_SECS {
                return false;
            }
        }
        true
    }
}

/// Cluster-aware metadata file. One leader owns the socket; zero or more
/// standbys stay warm and are candidates for leader election.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ClusterMetadata {
    pub(crate) protocol_version: u32,
    pub(crate) repo_root: PathBuf,
    pub(crate) worktree_id: String,
    pub(crate) binary_version: String,
    pub(crate) transport: HostTransport,
    pub(crate) leader: Option<HostEntry>,
    #[serde(default)]
    pub(crate) standbys: Vec<HostEntry>,
}

impl ClusterMetadata {
    pub(crate) fn empty_for_repo(repo_root: &Path) -> Self {
        let worktree_id = worktree_id_for_repo(repo_root);
        Self {
            protocol_version: HOST_PROTOCOL_VERSION,
            repo_root: repo_root.to_path_buf(),
            worktree_id: worktree_id.clone(),
            binary_version: host_binary_version(),
            transport: host_transport_for_repo(repo_root, &worktree_id),
            leader: None,
            standbys: Vec::new(),
        }
    }

    pub(crate) fn metadata_path(&self) -> PathBuf {
        host_metadata_path(&self.repo_root, &self.worktree_id)
    }

    pub(crate) fn validate_for_repo(&self, repo_root: &Path) -> Result<()> {
        if self.protocol_version != HOST_PROTOCOL_VERSION {
            bail!(
                "host protocol mismatch: expected {}, found {}",
                HOST_PROTOCOL_VERSION,
                self.protocol_version
            );
        }

        if self.repo_root != repo_root {
            bail!(
                "host repo root mismatch: expected {}, found {}",
                repo_root.display(),
                self.repo_root.display()
            );
        }

        let expected_worktree_id = worktree_id_for_repo(repo_root);
        if self.worktree_id != expected_worktree_id {
            bail!(
                "host worktree mismatch: expected {}, found {}",
                expected_worktree_id,
                self.worktree_id
            );
        }

        let expected_transport = host_transport_for_repo(repo_root, &expected_worktree_id);
        if self.transport != expected_transport {
            bail!("host transport mismatch for {}", repo_root.display());
        }

        if self.binary_version != host_binary_version() {
            bail!(
                "host binary version mismatch: expected {}, found {}",
                host_binary_version(),
                self.binary_version
            );
        }

        Ok(())
    }

    /// Remove dead or stuck hosts and return whether the leader was pruned.
    pub(crate) fn prune_dead(&mut self) -> bool {
        let leader_dead = self
            .leader
            .as_ref()
            .is_some_and(|entry| !entry.is_effectively_alive());
        if leader_dead {
            self.leader = None;
        }
        self.standbys.retain(|entry| entry.is_effectively_alive());
        leader_dead
    }

    /// Elect the newest standby as leader (in `Warming` state). Returns the promoted PID if any.
    pub(crate) fn elect_leader(&mut self) -> Option<u32> {
        if self.leader.is_some() {
            return None;
        }
        let newest_idx = self
            .standbys
            .iter()
            .enumerate()
            .max_by_key(|(_, entry)| &entry.started_at)?
            .0;
        let mut promoted = self.standbys.remove(newest_idx);
        promoted.state = HostState::Warming;
        self.leader = Some(promoted);
        self.leader.as_ref().map(|entry| entry.pid)
    }

    /// Register a host as standby. Returns true if it was added.
    pub(crate) fn register_standby(&mut self, pid: u32, started_at: String) -> bool {
        if self.leader.as_ref().is_some_and(|e| e.pid == pid) {
            return false;
        }
        if self.standbys.iter().any(|e| e.pid == pid) {
            return false;
        }
        self.standbys.push(HostEntry {
            pid,
            started_at,
            state: HostState::Warming,
        });
        true
    }

    /// Remove a host (from leader or standbys) by PID.
    pub(crate) fn deregister(&mut self, pid: u32) {
        if self.leader.as_ref().is_some_and(|e| e.pid == pid) {
            self.leader = None;
        }
        self.standbys.retain(|e| e.pid != pid);
    }

    pub(crate) fn has_living_leader(&self) -> bool {
        self.leader
            .as_ref()
            .is_some_and(|entry| is_pid_alive(entry.pid))
    }
}

/// Backward-compatible alias used by v3 client functions during migration.
pub(crate) type HostMetadata = ClusterMetadata;

pub(crate) fn host_metadata_path(repo_root: &Path, worktree_id: &str) -> PathBuf {
    host_worktree_dir(repo_root, worktree_id).join(HOST_METADATA_FILE)
}

pub(crate) fn host_transport_for_repo(repo_root: &Path, worktree_id: &str) -> HostTransport {
    if cfg!(windows) {
        HostTransport::NamedPipe {
            name: format!("{WINDOWS_PIPE_PREFIX}{worktree_id}"),
        }
    } else {
        HostTransport::UnixSocket {
            path: host_worktree_dir(repo_root, worktree_id).join(HOST_SOCKET_FILE),
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

pub(crate) trait HostEndpoint {
    fn cleanup(&mut self) -> Result<()>;
}

pub(crate) trait HostTransportBinder {
    type Endpoint: HostEndpoint;

    fn bind(
        &self,
        metadata: &HostMetadata,
        state: SharedHostState,
        render_options: HostRenderOptions,
    ) -> Result<Self::Endpoint>;
}

#[derive(Debug, Clone)]
pub(crate) struct SharedHostState {
    inner: Arc<SharedHostStateInner>,
}

#[derive(Debug)]
struct SharedHostStateInner {
    snapshot: Mutex<SharedHostSnapshot>,
    updates: Condvar,
    shutdown_requested: AtomicBool,
    /// When set, the shutdown handler also sets this flag — used to wake
    /// the filesystem event source that polls on a separate `Arc<AtomicBool>`.
    interrupt_flag: Option<Arc<AtomicBool>>,
    /// Set by `note_dirty` when a `NotifyDirty` request arrives via socket.
    /// The watch loop polls this to trigger a re-run without waiting for
    /// a filesystem event.
    external_dirty_flag: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct SharedHostSnapshot {
    freshness: HostFreshness,
    generation: u64,
    last_started_at: Option<String>,
    last_finished_at: Option<String>,
    last_report: Option<BoundariesRunReport>,
    last_graph: Option<super::boundaries::BoundaryGraph>,
}

impl SharedHostState {
    fn with_interrupt_flag(interrupt_flag: Arc<AtomicBool>) -> Self {
        Self {
            inner: Arc::new(SharedHostStateInner {
                snapshot: Mutex::new(SharedHostSnapshot::default()),
                updates: Condvar::new(),
                shutdown_requested: AtomicBool::new(false),
                interrupt_flag: Some(interrupt_flag),
                external_dirty_flag: Arc::new(AtomicBool::new(false)),
            }),
        }
    }
}

impl Default for SharedHostState {
    fn default() -> Self {
        Self {
            inner: Arc::new(SharedHostStateInner {
                snapshot: Mutex::new(SharedHostSnapshot::default()),
                updates: Condvar::new(),
                shutdown_requested: AtomicBool::new(false),
                interrupt_flag: None,
                external_dirty_flag: Arc::new(AtomicBool::new(false)),
            }),
        }
    }
}

impl Default for SharedHostSnapshot {
    fn default() -> Self {
        Self {
            freshness: HostFreshness::Dirty,
            generation: 0,
            last_started_at: None,
            last_finished_at: None,
            last_report: None,
            last_graph: None,
        }
    }
}

impl SharedHostState {
    /// Get a clone of the external dirty flag for the watch loop to poll.
    pub(crate) fn external_dirty_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.inner.external_dirty_flag)
    }

    pub(crate) fn note_dirty(&self) {
        let mut snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("host state mutex poisoned");
        if snapshot.freshness != HostFreshness::Running {
            snapshot.freshness = HostFreshness::Dirty;
        }
        self.inner.updates.notify_all();
    }

    /// Like `note_dirty`, but also signals the watch loop's event source to
    /// wake up. Used only for external socket requests (NotifyDirty), not for
    /// the watch loop's own change detection (which would cause an infinite loop).
    pub(crate) fn note_dirty_external(&self) {
        self.note_dirty();
        self.inner.external_dirty_flag.store(true, Ordering::SeqCst);
    }

    pub(crate) fn begin_run(&self) {
        let mut snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("host state mutex poisoned");
        snapshot.freshness = HostFreshness::Running;
        snapshot.last_started_at = Some(unix_timestamp_string());
        self.inner.updates.notify_all();
    }

    pub(crate) fn complete_run(
        &self,
        result: crate::architecture::boundaries::BoundariesRunResult,
    ) {
        let mut snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("host state mutex poisoned");
        snapshot.generation += 1;
        snapshot.last_finished_at = Some(unix_timestamp_string());
        snapshot.freshness = if result.report.success {
            HostFreshness::IdleClean
        } else {
            HostFreshness::IdleFailed
        };
        snapshot.last_graph = Some(result.graph);
        snapshot.last_report = Some(result.report);
        self.inner.updates.notify_all();
    }

    pub(crate) fn handle_status(&self, render_options: HostRenderOptions) -> StatusResponse {
        let snapshot = self
            .inner
            .snapshot
            .lock()
            .expect("host state mutex poisoned");
        StatusResponse {
            freshness: snapshot.freshness,
            generation: snapshot.generation,
            last_started_at: snapshot.last_started_at.clone(),
            last_finished_at: snapshot.last_finished_at.clone(),
            last_success: snapshot.last_report.as_ref().map(|report| report.success),
            render_options,
        }
    }

    pub(crate) fn is_shutdown_requested(&self) -> bool {
        self.inner.shutdown_requested.load(Ordering::SeqCst)
    }

    pub(crate) fn handle_request(
        &self,
        request: HostRequest,
        hosted_options: HostRenderOptions,
    ) -> HostResponse {
        match request {
            HostRequest::NotifyDirty => {
                self.note_dirty_external();
                HostResponse::NotifyDirtyAck
            }
            HostRequest::Status => HostResponse::Status(self.handle_status(hosted_options)),
            HostRequest::Shutdown => {
                self.inner.shutdown_requested.store(true, Ordering::SeqCst);
                if let Some(flag) = &self.inner.interrupt_flag {
                    flag.store(true, Ordering::SeqCst);
                }
                self.inner.updates.notify_all();
                HostResponse::ShuttingDown
            }
            HostRequest::Check {
                wait_for_fresh,
                verbose,
                timings,
                ..
            } => {
                let requested_options = HostRenderOptions { verbose, timings };
                if requested_options != hosted_options {
                    return HostResponse::Error {
                        retry_locally: true,
                        message: format!(
                            "host render options mismatch: host verbose={}, timings={}, request verbose={}, timings={}",
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
                    .expect("host state mutex poisoned");
                while wait_for_fresh
                    && matches!(
                        snapshot.freshness,
                        HostFreshness::Dirty | HostFreshness::Running
                    )
                {
                    snapshot = self
                        .inner
                        .updates
                        .wait(snapshot)
                        .expect("host state mutex poisoned");
                }

                match snapshot.last_report.as_ref() {
                    Some(report) => HostResponse::Check(CheckResponse {
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
                    None => HostResponse::Error {
                        retry_locally: true,
                        message: "host has no completed boundaries result yet".to_string(),
                    },
                }
            }
            HostRequest::Graph => {
                let snapshot = self
                    .inner
                    .snapshot
                    .lock()
                    .expect("host state mutex poisoned");
                match snapshot.last_graph.as_ref() {
                    Some(graph) => HostResponse::Graph(graph.clone()),
                    None => HostResponse::Error {
                        retry_locally: true,
                        message: "host has no boundary graph yet".to_string(),
                    },
                }
            }
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ArchitectureHost<E: HostEndpoint> {
    metadata: HostMetadata,
    metadata_path: PathBuf,
    state: SharedHostState,
    render_options: HostRenderOptions,
    endpoint: Option<E>,
}

#[allow(dead_code)]
impl<E: HostEndpoint> ArchitectureHost<E> {
    pub(crate) fn start_with_binder(
        repo_root: &Path,
        binder: &impl HostTransportBinder<Endpoint = E>,
        render_options: HostRenderOptions,
        interrupt_flag: Option<Arc<AtomicBool>>,
    ) -> Result<Self> {
        let template = HostMetadata::empty_for_repo(repo_root);
        let metadata_path = template.metadata_path();
        if let Some(parent) = metadata_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let state = match interrupt_flag {
            Some(flag) => SharedHostState::with_interrupt_flag(flag),
            None => SharedHostState::default(),
        };
        let endpoint = binder.bind(&template, state.clone(), render_options)?;

        // Merge into existing metadata (preserving standbys) under flock.
        let metadata = {
            #[cfg(unix)]
            {
                let lock = MetadataLock::acquire(repo_root)
                    .context("failed to acquire cluster lock during host start")?;
                let mut cluster = lock.read()?;
                cluster.leader = Some(HostEntry {
                    pid: std::process::id(),
                    started_at: unix_timestamp_string(),
                    state: HostState::Ready,
                });
                // Inherit transport from template (socket path etc.)
                cluster.transport = template.transport;
                lock.write(&cluster)?;
                cluster
            }
            #[cfg(not(unix))]
            {
                let mut metadata = template;
                metadata.leader = Some(HostEntry {
                    pid: std::process::id(),
                    started_at: unix_timestamp_string(),
                });
                if let Err(error) = write_metadata_file(&metadata_path, &metadata) {
                    let mut endpoint = endpoint;
                    let _ = endpoint.cleanup();
                    return Err(error);
                }
                metadata
            }
        };

        Ok(Self {
            metadata,
            metadata_path,
            state,
            render_options,
            endpoint: Some(endpoint),
        })
    }

    pub(crate) fn metadata(&self) -> &HostMetadata {
        &self.metadata
    }

    pub(crate) fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub(crate) fn note_dirty(&self) {
        self.state.note_dirty();
    }

    /// Get the external dirty flag for the watch loop to poll.
    pub(crate) fn external_dirty_flag(&self) -> Arc<AtomicBool> {
        self.state.external_dirty_flag()
    }

    pub(crate) fn begin_run(&self) {
        self.state.begin_run();
    }

    pub(crate) fn complete_run(
        &self,
        result: crate::architecture::boundaries::BoundariesRunResult,
    ) {
        self.state.complete_run(result);
    }

    pub(crate) fn handle_request(&self, request: HostRequest) -> HostResponse {
        self.state.handle_request(request, self.render_options)
    }

    pub(crate) fn handle_status(&self) -> StatusResponse {
        self.state.handle_status(self.render_options)
    }

    pub(crate) fn is_shutdown_requested(&self) -> bool {
        self.state.is_shutdown_requested()
    }

    pub(crate) fn shutdown(mut self) {
        self.cleanup();
    }

    fn cleanup(&mut self) {
        if let Some(endpoint) = self.endpoint.as_mut() {
            let _ = endpoint.cleanup();
        }
        self.endpoint = None;
        // Deregister this host from the cluster rather than nuking the file.
        let my_pid = std::process::id();
        #[cfg(unix)]
        {
            let repo_root = &self.metadata.repo_root;
            if let Ok(lock) = MetadataLock::acquire(repo_root)
                && let Ok(cluster) = lock.mutate(|cluster| {
                    cluster.deregister(my_pid);
                })
                && cluster.leader.is_none()
                && cluster.standbys.is_empty()
            {
                let _ = remove_file_if_exists(&self.metadata_path);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = remove_file_if_exists(&self.metadata_path);
        }
    }
}

impl<E: HostEndpoint> Drop for ArchitectureHost<E> {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PlatformHostBinder;

#[derive(Debug)]
pub(crate) enum PlatformHostEndpoint {
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

impl HostTransportBinder for PlatformHostBinder {
    type Endpoint = PlatformHostEndpoint;

    fn bind(
        &self,
        metadata: &HostMetadata,
        state: SharedHostState,
        render_options: HostRenderOptions,
    ) -> Result<Self::Endpoint> {
        match &metadata.transport {
            #[cfg(unix)]
            HostTransport::UnixSocket { path } => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                remove_file_if_exists(path)?;
                let listener = std::os::unix::net::UnixListener::bind(path).with_context(|| {
                    format!("failed to bind unix host socket {}", path.display())
                })?;
                let shutdown = Arc::new(AtomicBool::new(false));
                let thread = Some(spawn_unix_host_server(
                    listener,
                    state,
                    render_options,
                    Arc::clone(&shutdown),
                ));
                Ok(PlatformHostEndpoint::UnixSocket {
                    path: path.clone(),
                    shutdown,
                    thread,
                })
            }
            #[cfg(not(unix))]
            HostTransport::UnixSocket { path } => bail!(
                "unix socket host transport is unavailable on this platform: {}",
                path.display()
            ),
            HostTransport::NamedPipe { name } => Ok(PlatformHostEndpoint::NamedPipe {
                _name: name.clone(),
            }),
        }
    }
}

impl HostEndpoint for PlatformHostEndpoint {
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
    render_options: HostRenderOptions,
) -> HostCheckResult {
    let request = HostRequest::Check {
        wait_for_fresh: true,
        verbose: render_options.verbose,
        timings: render_options.timings,
        no_color: true,
    };

    for attempt in 0..2 {
        let loaded = match load_metadata_for_repo(repo_root) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => {
                return HostCheckResult::RetryLocally {
                    reason: "no host metadata found".to_string(),
                };
            }
            Err(error) => {
                return HostCheckResult::RetryLocally {
                    reason: format!("{error:#}"),
                };
            }
        };

        // Don't try the socket if the leader is still warming up.
        if let Some(leader) = &loaded.cluster.leader
            && leader.state == HostState::Warming
        {
            return HostCheckResult::RetryLocally {
                reason: format!("host (pid {}) is warming up", leader.pid),
            };
        }

        match send_request(&loaded.cluster, &request) {
            Ok(HostResponse::Check(response)) => return HostCheckResult::Reused(response),
            Ok(HostResponse::Error {
                retry_locally,
                message,
            }) if retry_locally => {
                return HostCheckResult::RetryLocally { reason: message };
            }
            Ok(other) => {
                return HostCheckResult::RetryLocally {
                    reason: format!("unexpected host response: {other:?}"),
                };
            }
            Err(error) => {
                if attempt == 0 && should_cleanup_stale_transport(&error) {
                    // Leader may have died. Re-read metadata (prunes dead) and retry.
                    maybe_cleanup_stale_transport(&loaded);
                    continue;
                }
                return HostCheckResult::RetryLocally {
                    reason: stale_cleanup_reason(loaded, error),
                };
            }
        }
    }

    HostCheckResult::RetryLocally {
        reason: "host connection failed after retry".to_string(),
    }
}

/// Try to get the boundary graph from a running host. Returns `None` if no
/// host is available or it hasn't completed a run yet — caller should fall
/// back to local graph collection.
pub(crate) fn try_request_graph(repo_root: &Path) -> Option<super::boundaries::BoundaryGraph> {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(loaded)) => loaded,
        _ => return None,
    };

    if let Some(leader) = &loaded.cluster.leader
        && leader.state == HostState::Warming
    {
        return None;
    }

    match send_request(&loaded.cluster, &HostRequest::Graph) {
        Ok(HostResponse::Graph(graph)) => Some(graph),
        _ => None,
    }
}

/// Poll metadata until the leader transitions out of `Warming`, then send a check request.
/// Returns `RetryLocally` if the timeout expires or the host disappears.
pub(crate) fn poll_until_ready_then_check(
    repo_root: &Path,
    render_options: HostRenderOptions,
    timeout: std::time::Duration,
) -> HostCheckResult {
    let deadline = std::time::Instant::now() + timeout;
    let poll_interval = std::time::Duration::from_secs(2);

    loop {
        if std::time::Instant::now() >= deadline {
            return HostCheckResult::RetryLocally {
                reason: "timed out waiting for host to finish warming up".to_string(),
            };
        }

        std::thread::sleep(poll_interval);

        match load_metadata_for_repo(repo_root) {
            Ok(Some(loaded)) => {
                let is_warming = loaded
                    .cluster
                    .leader
                    .as_ref()
                    .is_some_and(|e| e.state == HostState::Warming);
                if is_warming {
                    continue;
                }
                // Leader is ready — send the check request.
                return try_request_check(repo_root, render_options);
            }
            Ok(None) => {
                return HostCheckResult::RetryLocally {
                    reason: "host metadata disappeared while waiting for warm-up".to_string(),
                };
            }
            Err(_) => continue,
        }
    }
}

pub(crate) fn try_notify_dirty(repo_root: &Path) -> bool {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(metadata)) => metadata,
        _ => return false,
    };

    matches!(
        send_request(&loaded.cluster, &HostRequest::NotifyDirty),
        Ok(HostResponse::NotifyDirtyAck)
    )
}

/// Send a shutdown request to an existing host. Returns true if the host acknowledged.
pub(crate) fn request_shutdown(repo_root: &Path) -> bool {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(metadata)) => metadata,
        _ => return false,
    };

    matches!(
        send_request(&loaded.cluster, &HostRequest::Shutdown),
        Ok(HostResponse::ShuttingDown)
    )
}

/// Check if a living leader PID exists in the cluster metadata (no socket connection).
pub(crate) fn has_living_leader(repo_root: &Path) -> bool {
    load_metadata_for_repo(repo_root)
        .ok()
        .flatten()
        .is_some_and(|loaded| loaded.cluster.has_living_leader())
}

pub(crate) fn query_status(repo_root: &Path) -> HostStatusResult {
    let loaded = match load_metadata_for_repo(repo_root) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            return HostStatusResult::Unavailable {
                reason: "no warm boundaries host for this worktree".to_string(),
            };
        }
        Err(error) => {
            return HostStatusResult::Unavailable {
                reason: format!("no warm boundaries host for this worktree ({error:#})"),
            };
        }
    };

    // If the leader is still warming up, don't try the socket — it won't be bound yet.
    if let Some(leader) = &loaded.cluster.leader
        && leader.state == HostState::Warming
    {
        return HostStatusResult::Unavailable {
            reason: format!("host (pid {}) is warming up", leader.pid),
        };
    }

    match send_request(&loaded.cluster, &HostRequest::Status) {
        Ok(HostResponse::Status(status)) => HostStatusResult::Live(status),
        Ok(other) => HostStatusResult::Unavailable {
            reason: format!(
                "no warm boundaries host for this worktree (unexpected response: {other:?})"
            ),
        },
        Err(error) => HostStatusResult::Unavailable {
            reason: format!(
                "no warm boundaries host for this worktree ({})",
                stale_cleanup_reason(loaded, error)
            ),
        },
    }
}

fn host_worktree_dir(repo_root: &Path, worktree_id: &str) -> PathBuf {
    repo_root
        .join(WORKTREE_TARGET_DIR)
        .join(XTASK_TARGET_DIR)
        .join(worktree_id)
}

fn load_metadata_for_repo(repo_root: &Path) -> Result<Option<LoadedCluster>> {
    let discovery_root = host_discovery_root(repo_root);
    let metadata_path = HostMetadata::empty_for_repo(&discovery_root).metadata_path();
    // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
    let json = match std::fs::read_to_string(&metadata_path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", metadata_path.display()));
        }
    };

    let mut cluster: ClusterMetadata = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse host metadata {}", metadata_path.display()))
        .map_err(|error| cleanup_invalid_metadata(&metadata_path, error))?;
    cluster
        .validate_for_repo(&discovery_root)
        .map_err(|error| cleanup_invalid_metadata(&metadata_path, error))?;
    cluster.prune_dead();
    if cluster.leader.is_none() && cluster.standbys.is_empty() {
        let _ = remove_file_if_exists(&metadata_path);
        return Ok(None);
    }
    Ok(Some(LoadedCluster {
        cluster,
        metadata_path,
    }))
}

fn host_discovery_root(repo_root: &Path) -> PathBuf {
    std::env::var_os(HOST_DISCOVERY_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.to_path_buf())
}

fn send_request(metadata: &HostMetadata, request: &HostRequest) -> Result<HostResponse> {
    match &metadata.transport {
        #[cfg(unix)]
        HostTransport::UnixSocket { path } => send_request_over_unix_socket(path, request),
        #[cfg(not(unix))]
        HostTransport::UnixSocket { path } => bail!(
            "unix socket host transport is unavailable on this platform: {}",
            path.display()
        ),
        HostTransport::NamedPipe { name } => {
            bail!("named pipe host transport is not yet supported for client reuse: {name}")
        }
    }
}

#[cfg(unix)]
fn send_request_over_unix_socket(path: &Path, request: &HostRequest) -> Result<HostResponse> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("failed to connect to host socket {}", path.display()))?;
    serde_json::to_writer(&mut stream, request)
        .with_context(|| format!("failed to write host request {}", path.display()))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .with_context(|| format!("failed to finish host request {}", path.display()))?;
    let response = serde_json::from_reader(BufReader::new(stream))
        .with_context(|| format!("failed to read host response {}", path.display()))?;
    Ok(response)
}

fn repo_identity_path(repo_root: &Path) -> PathBuf {
    std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf())
}

fn host_binary_version() -> String {
    format!("xtask-boundaries-host-v{HOST_PROTOCOL_VERSION}")
}

pub(crate) fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

/// Advisory file lock on the cluster metadata. Serializes concurrent reads
/// and writes from multiple host processes. Uses a `.lock` sidecar file so
/// the metadata JSON can be atomically replaced.
#[cfg(unix)]
pub(crate) struct MetadataLock {
    _file: std::fs::File,
    repo_root: PathBuf,
    metadata_path: PathBuf,
}

#[cfg(unix)]
impl MetadataLock {
    /// Acquire an exclusive lock on the metadata sidecar. Blocks until acquired.
    pub(crate) fn acquire(repo_root: &Path) -> Result<Self> {
        use std::os::unix::io::AsRawFd;

        let worktree_id = worktree_id_for_repo(repo_root);
        let metadata_path = host_metadata_path(repo_root, &worktree_id);
        let lock_path = metadata_path.with_extension("lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;
        let rc = unsafe { libc_flock(file.as_raw_fd(), LOCK_EX) };
        if rc != 0 {
            bail!(
                "failed to acquire metadata lock {}: {}",
                lock_path.display(),
                std::io::Error::last_os_error()
            );
        }
        Ok(Self {
            _file: file,
            repo_root: repo_root.to_path_buf(),
            metadata_path,
        })
    }

    /// Read the current cluster metadata (or create an empty one if missing).
    pub(crate) fn read(&self) -> Result<ClusterMetadata> {
        match std::fs::read_to_string(&self.metadata_path) {
            Ok(json) => {
                let cluster: ClusterMetadata = serde_json::from_str(&json).with_context(|| {
                    format!(
                        "failed to parse host metadata {}",
                        self.metadata_path.display()
                    )
                })?;
                Ok(cluster)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ClusterMetadata::empty_for_repo(&self.repo_root))
            }
            Err(error) => Err(error)
                .with_context(|| format!("failed to read {}", self.metadata_path.display())),
        }
    }

    /// Write the cluster metadata atomically.
    pub(crate) fn write(&self, cluster: &ClusterMetadata) -> Result<()> {
        write_metadata_file(&self.metadata_path, cluster)
    }

    /// Read, apply a mutation, write back. Returns the mutated cluster.
    pub(crate) fn mutate(&self, f: impl FnOnce(&mut ClusterMetadata)) -> Result<ClusterMetadata> {
        let mut cluster = self.read()?;
        f(&mut cluster);
        self.write(&cluster)?;
        Ok(cluster)
    }
}

// Inline flock binding to avoid a libc dependency.
#[cfg(unix)]
const LOCK_EX: i32 = 2;

#[cfg(unix)]
unsafe fn libc_flock(fd: i32, operation: i32) -> i32 {
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    unsafe { flock(fd, operation) }
}

fn write_metadata_file(path: &Path, metadata: &ClusterMetadata) -> Result<()> {
    let json = serde_json::to_vec_pretty(metadata).context("failed to serialize host metadata")?;
    std::fs::write(path, json)
        .with_context(|| format!("failed to write host metadata {}", path.display()))
}

fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true // conservatively assume alive on non-unix
    }
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

fn stale_cleanup_reason(loaded: LoadedCluster, error: anyhow::Error) -> String {
    if should_cleanup_stale_transport(&error) {
        let cleaned = maybe_cleanup_stale_transport(&loaded);
        if cleaned {
            format!("{error:#} (removed stale host metadata)")
        } else {
            format!("{error:#}")
        }
    } else {
        format!("{error:#}")
    }
}

/// Attempt to clean up stale transport/metadata. Returns true if the metadata
/// file was removed.
fn maybe_cleanup_stale_transport(loaded: &LoadedCluster) -> bool {
    // Don't touch anything if the leader is still warming up (socket not bound yet).
    let leader_warming = loaded
        .cluster
        .leader
        .as_ref()
        .is_some_and(|e| e.state == HostState::Warming && e.is_effectively_alive());
    if leader_warming {
        return false;
    }

    match &loaded.cluster.transport {
        #[cfg(unix)]
        HostTransport::UnixSocket { path } => {
            let _ = remove_file_if_exists(path);
        }
        HostTransport::NamedPipe { .. } => {}
        #[cfg(not(unix))]
        HostTransport::UnixSocket { .. } => {}
    }
    // Only remove the metadata file if no standbys and no warming leader.
    if loaded.cluster.standbys.is_empty() {
        let _ = remove_file_if_exists(&loaded.metadata_path);
        return true;
    }
    false
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
fn spawn_unix_host_server(
    listener: std::os::unix::net::UnixListener,
    state: SharedHostState,
    render_options: HostRenderOptions,
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
    state: &SharedHostState,
    render_options: HostRenderOptions,
) -> Result<()> {
    let request: HostRequest = serde_json::from_reader(BufReader::new(
        stream
            .try_clone()
            .context("failed to clone host stream for request read")?,
    ))
    .context("failed to decode host request")?;
    let response = state.handle_request(request, render_options);
    serde_json::to_writer(&mut stream, &response).context("failed to encode host response")?;
    stream.flush().context("failed to flush host response")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use super::{
        HostFreshness, HostMetadata, HostRenderOptions, HostRequest, HostResponse, HostTransport,
        SharedHostState,
    };
    use crate::architecture::boundaries::{BoundariesRunReport, BoundaryGraph};

    fn render_options() -> HostRenderOptions {
        HostRenderOptions {
            verbose: false,
            timings: false,
        }
    }

    #[test]
    fn host_metadata_uses_worktree_specific_target_xtask_paths() {
        let repo_root = Path::new("/tmp/example-repo/worktrees/feature-a");
        let metadata = HostMetadata::empty_for_repo(repo_root);
        assert_eq!(metadata.repo_root, repo_root);
        assert!(!metadata.worktree_id.is_empty());
        assert_eq!(
            metadata.metadata_path(),
            repo_root
                .join("target")
                .join("xtask")
                .join(&metadata.worktree_id)
                .join("architecture-host.json")
        );
        match &metadata.transport {
            HostTransport::UnixSocket { path } => assert_eq!(
                path,
                &repo_root
                    .join("target")
                    .join("xtask")
                    .join(&metadata.worktree_id)
                    .join("architecture.sock")
            ),
            HostTransport::NamedPipe { name } => {
                assert!(name.contains(&metadata.worktree_id));
            }
        }
    }

    #[test]
    fn different_worktrees_do_not_share_host_identity() {
        let left = HostMetadata::empty_for_repo(Path::new("/tmp/example-repo/worktrees/feature-a"));
        let right =
            HostMetadata::empty_for_repo(Path::new("/tmp/example-repo/worktrees/feature-b"));
        assert_ne!(left.worktree_id, right.worktree_id);
    }

    #[test]
    fn host_request_round_trips_through_json() {
        let request = HostRequest::Check {
            wait_for_fresh: true,
            verbose: false,
            timings: true,
            no_color: true,
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: HostRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn host_metadata_round_trips_through_json() {
        let metadata =
            HostMetadata::empty_for_repo(Path::new("/tmp/example-repo/worktrees/feature-a"));
        let json = serde_json::to_string(&metadata).unwrap();
        let decoded: HostMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, metadata);
    }

    #[test]
    fn stale_or_mismatched_metadata_is_rejected() {
        let metadata = HostMetadata {
            protocol_version: 99,
            ..HostMetadata::empty_for_repo(Path::new("/tmp/example-repo"))
        };
        assert!(
            metadata
                .validate_for_repo(Path::new("/tmp/example-repo"))
                .is_err()
        );
    }

    #[test]
    fn check_request_waits_for_rerun_when_host_is_dirty() {
        let host = SharedHostState::default();
        host.note_dirty();
        let waiting_host = host.clone();
        let (sender, receiver) = mpsc::channel();

        let waiting_thread = std::thread::spawn(move || {
            let response = waiting_host.handle_request(
                HostRequest::Check {
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
        host.complete_run(failing_result());

        let response = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        waiting_thread.join().unwrap();

        match response {
            HostResponse::Check(check) => {
                assert_eq!(check.freshness, HostFreshness::IdleFailed);
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
        let host = SharedHostState::default();
        host.begin_run();
        host.complete_run(passing_result());

        let status = host.handle_status(render_options());

        assert_eq!(status.freshness, HostFreshness::IdleClean);
        assert_eq!(status.generation, 1);
        assert_eq!(status.last_success, Some(true));
        assert!(status.last_started_at.is_some());
        assert!(status.last_finished_at.is_some());
        assert_eq!(status.render_options, render_options());
    }

    #[test]
    fn notify_dirty_transitions_idle_to_dirty() {
        let host = SharedHostState::default();
        host.begin_run();
        host.complete_run(passing_result());
        assert_eq!(
            host.handle_status(render_options()).freshness,
            HostFreshness::IdleClean
        );

        let response = host.handle_request(HostRequest::NotifyDirty, render_options());

        assert_eq!(response, HostResponse::NotifyDirtyAck);
        assert_eq!(
            host.handle_status(render_options()).freshness,
            HostFreshness::Dirty
        );
    }

    #[test]
    fn shutdown_sets_interrupt_flag() {
        let interrupt = Arc::new(AtomicBool::new(false));
        let host = SharedHostState::with_interrupt_flag(Arc::clone(&interrupt));

        let response = host.handle_request(HostRequest::Shutdown, render_options());

        assert_eq!(response, HostResponse::ShuttingDown);
        assert!(host.is_shutdown_requested());
        assert!(interrupt.load(Ordering::SeqCst));
    }

    #[test]
    fn shutdown_without_interrupt_flag_still_works() {
        let host = SharedHostState::default();

        let response = host.handle_request(HostRequest::Shutdown, render_options());

        assert_eq!(response, HostResponse::ShuttingDown);
        assert!(host.is_shutdown_requested());
    }

    fn passing_result() -> crate::architecture::boundaries::BoundariesRunResult {
        crate::architecture::boundaries::BoundariesRunResult {
            report: BoundariesRunReport {
                success: true,
                rendered_output: String::new(),
                summary: None,
                timings_output: None,
                violations: Vec::new(),
            },
            graph: BoundaryGraph::new(std::collections::BTreeSet::new()),
        }
    }

    fn failing_result() -> crate::architecture::boundaries::BoundariesRunResult {
        crate::architecture::boundaries::BoundariesRunResult {
            report: BoundariesRunReport {
                success: false,
                rendered_output: "error[boundaries]: forbidden dependency ...".to_string(),
                summary: Some("error: architecture boundaries failed with 1 violation".to_string()),
                timings_output: None,
                violations: Vec::new(),
            },
            graph: BoundaryGraph::new(std::collections::BTreeSet::new()),
        }
    }
}
