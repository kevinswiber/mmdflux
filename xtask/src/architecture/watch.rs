use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::{ArchitectureContext, RenderFlags, host, run_boundaries_watch_report};

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(350);
const INTERRUPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchEvent {
    Changes(Vec<PathBuf>),
    Interrupt,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchRunStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WatchLoopOutcome {
    reruns: usize,
    last_status: WatchRunStatus,
}

trait WatchEventSource {
    fn recv(&mut self) -> Result<WatchEvent>;
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WatchEvent>>;
    /// Wire an external dirty flag so the source wakes up on NotifyDirty.
    fn set_external_dirty_flag(&mut self, _flag: Arc<AtomicBool>) {}
}

trait WatchRunner {
    fn run(&mut self, run_number: usize, changes: &[PathBuf]) -> WatchRunStatus;

    fn on_waiting(&mut self) {}

    fn on_change_burst(&mut self, _paths: &[PathBuf]) {}
}

/// Interactive watch mode (`check --watch`). Requires TTY; without TTY does a single run.
/// If an existing host is available, connects as a client. Otherwise falls back to
/// running locally and becoming the host.
pub(crate) fn run_watch(render: RenderFlags, context: ArchitectureContext) -> Result<()> {
    let mut runner = ArchitectureWatchRunner::new(render, context);
    if !std::io::stdin().is_terminal() {
        return run_noninteractive(&mut runner);
    }

    let repo_root = runner.repo_root().to_path_buf();
    let render_options = host::HostRenderOptions {
        verbose: render.verbose,
        timings: render.timings,
    };

    // Try to connect to an existing host as a client
    if host_is_available(&repo_root, render_options) {
        eprintln!("[watch] connected to existing architecture host");
        return run_watch_as_client(&repo_root, render_options);
    }

    // No host available — fall back to becoming the host
    eprintln!("[watch] no existing host, starting local watch + host");
    run_interactive(&mut runner, render)
}

fn host_is_available(repo_root: &Path, render_options: host::HostRenderOptions) -> bool {
    matches!(
        host::query_status(repo_root),
        host::HostStatusResult::Live(status) if status.render_options == render_options
    )
}

fn run_watch_as_client(repo_root: &Path, render_options: host::HostRenderOptions) -> Result<()> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let mut source = NotifyEventSource::new(repo_root.to_path_buf(), Arc::clone(&interrupted))?;
    let interrupted_for_handler = Arc::clone(&interrupted);
    ctrlc::set_handler(move || {
        interrupted_for_handler.store(true, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler for architecture watch mode")?;

    // Initial check via host
    eprintln!("[watch] initial check via host");
    let mut run_number = 1usize;
    let mut last_status = run_client_check(repo_root, render_options, run_number, false);

    loop {
        eprintln!("[watch] waiting for changes...");
        let event = source.recv()?;
        let Some(_changes) = collect_change_burst(&mut source, event, DEBOUNCE_WINDOW)? else {
            break;
        };
        run_number += 1;
        last_status = run_client_check(repo_root, render_options, run_number, true);
    }

    if last_status == WatchRunStatus::Passed {
        Ok(())
    } else {
        bail!("last run failed")
    }
}

fn run_client_check(
    repo_root: &Path,
    render_options: host::HostRenderOptions,
    run_number: usize,
    notify_dirty: bool,
) -> WatchRunStatus {
    if notify_dirty {
        host::try_notify_dirty(repo_root);
    }

    match host::try_request_check(repo_root, render_options) {
        host::HostCheckResult::Reused(response) => {
            let verdict = if response.success { "PASS" } else { "FAIL" };
            eprintln!(
                "[run {run_number}] {verdict} {:<10} (via host)",
                "boundaries"
            );
            if let Some(timings_output) = &response.timings_output {
                eprint!("{timings_output}");
            }
            if !response.success {
                eprint!("{}", response.rendered_output);
                if let Some(summary) = &response.summary {
                    eprintln!("{summary}");
                }
            }
            if response.success {
                WatchRunStatus::Passed
            } else {
                WatchRunStatus::Failed
            }
        }
        host::HostCheckResult::RetryLocally { reason } => {
            eprintln!("[run {run_number}] FAIL boundaries  (host unavailable: {reason})");
            WatchRunStatus::Failed
        }
    }
}

/// Host mode (`host`). Joins the cluster as leader (warm up first) or standby (cold).
pub(crate) fn run_host(render: RenderFlags, context: ArchitectureContext) -> Result<()> {
    let repo_root = context.repo_root().to_path_buf();
    let mut runner = ArchitectureWatchRunner::new(render, context);
    let my_pid = std::process::id();

    // Acquire the cluster lock, decide our role BEFORE warming up.
    #[cfg(unix)]
    {
        let lock = host::MetadataLock::acquire(&repo_root)
            .context("failed to acquire cluster metadata lock")?;
        let mut cluster = lock.read()?;
        cluster.prune_dead();

        // Clean up stale socket if the leader died.
        if cluster.leader.is_none() {
            cleanup_stale_socket(&cluster);
        }

        if cluster.has_living_leader() {
            // Register as standby (cold — no warm-up until promotion).
            let started_at = host::unix_timestamp_string();
            cluster.register_standby(my_pid, started_at);
            lock.write(&cluster)?;
            drop(lock);
            eprintln!(
                "[host] registered as standby (leader pid {})",
                cluster.leader.as_ref().unwrap().pid
            );
            return run_standby(&mut runner, render, &repo_root, my_pid);
        }

        // No living leader — shut down old leader (if lingering) and promote self.
        if host::request_shutdown(&repo_root) {
            eprintln!("[host] shut down previous host");
            std::thread::sleep(Duration::from_millis(100));
        }

        // Register as leader directly.
        cluster.leader = Some(host::HostEntry {
            pid: my_pid,
            started_at: host::unix_timestamp_string(),
            state: host::HostState::Warming,
        });
        lock.write(&cluster)?;
        drop(lock);
    }

    // Warm up only when becoming leader.
    eprintln!("[host] warming up boundaries context...");
    let warm_result = runner.run_once(1, &[]);
    eprintln!("[host] promoted to leader");

    run_interactive_with_warm_result(&mut runner, render, Some(warm_result))
}

fn cleanup_stale_socket(cluster: &host::ClusterMetadata) {
    match &cluster.transport {
        #[cfg(unix)]
        host::HostTransport::UnixSocket { path } => {
            let _ = std::fs::remove_file(path);
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

/// Standby liveness poll interval — how often to check if the leader is still alive
/// when no file changes are happening.
const STANDBY_LIVENESS_POLL: Duration = Duration::from_secs(5);

/// Run as a standby: monitor leader liveness and promote to leader if it dies.
/// Standbys start cold (no warm-up) and only warm up on promotion.
fn run_standby(
    runner: &mut ArchitectureWatchRunner,
    render: RenderFlags,
    repo_root: &Path,
    my_pid: u32,
) -> Result<()> {
    let interrupted = Arc::new(AtomicBool::new(false));
    let mut source =
        NotifyEventSource::new(runner.repo_root().to_path_buf(), Arc::clone(&interrupted))?;
    let interrupted_for_handler = Arc::clone(&interrupted);
    ctrlc::set_handler(move || {
        interrupted_for_handler.store(true, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler for standby watch mode")?;

    loop {
        eprintln!(
            "[standby] waiting (polling leader every {}s)...",
            STANDBY_LIVENESS_POLL.as_secs()
        );

        // Poll with timeout so we check leader liveness even without file changes.
        let event = match source.recv_timeout(STANDBY_LIVENESS_POLL)? {
            Some(event) => event,
            None => {
                // Timeout — no file changes, but check leader liveness.
                if !leader_is_alive(repo_root) {
                    eprintln!("[standby] leader died, attempting promotion...");
                    if try_promote_to_leader(repo_root, my_pid)? {
                        eprintln!("[host] warming up boundaries context...");
                        let warm = runner.run_once(1, &[]);
                        eprintln!("[host] promoted to leader");
                        return promote_to_leader(
                            source,
                            runner,
                            render,
                            Arc::clone(&interrupted),
                            Some(warm),
                        );
                    }
                    eprintln!("[standby] another host was promoted, continuing as standby");
                }
                continue;
            }
        };

        if interrupted.load(Ordering::SeqCst) {
            break;
        }

        // Drain the change burst but we don't act on it — standbys stay cold.
        let Some(_changes) = collect_change_burst(&mut source, event, DEBOUNCE_WINDOW)? else {
            break;
        };

        // Check leader liveness on file changes too.
        if !leader_is_alive(repo_root) {
            eprintln!("[standby] leader died, attempting promotion...");
            if try_promote_to_leader(repo_root, my_pid)? {
                eprintln!("[host] warming up boundaries context...");
                let warm = runner.run_once(1, &[]);
                eprintln!("[host] promoted to leader");
                return promote_to_leader(
                    source,
                    runner,
                    render,
                    Arc::clone(&interrupted),
                    Some(warm),
                );
            }
            eprintln!("[standby] another host was promoted, continuing as standby");
        }
    }

    // Deregister from cluster on exit.
    deregister_from_cluster(repo_root, my_pid);
    Ok(())
}

/// Transition from standby to leader, reusing the existing event source and interrupt flag.
fn promote_to_leader(
    mut source: NotifyEventSource,
    runner: &mut ArchitectureWatchRunner,
    render: RenderFlags,
    interrupted: Arc<AtomicBool>,
    warm_result: Option<ArchitectureRunOutcome>,
) -> Result<()> {
    eprintln!("[watch] architecture (boundaries)");

    let binder = host::PlatformHostBinder;
    let repo_root = runner.repo_root().to_path_buf();
    let render_options = host::HostRenderOptions {
        verbose: render.verbose,
        timings: render.timings,
    };
    let outcome = run_architecture_watch_with_host(
        &mut source,
        runner,
        &repo_root,
        WatchHostConfig {
            binder,
            render_options,
            interrupt_flag: interrupted,
            warm_result,
            debounce_window: DEBOUNCE_WINDOW,
        },
    )?;
    if outcome.last_status == WatchRunStatus::Passed {
        return Ok(());
    }
    bail!("last run failed")
}

fn leader_is_alive(repo_root: &Path) -> bool {
    host::has_living_leader(repo_root)
}

#[cfg(unix)]
fn try_promote_to_leader(repo_root: &Path, my_pid: u32) -> Result<bool> {
    let lock = host::MetadataLock::acquire(repo_root)
        .context("failed to acquire cluster metadata lock for promotion")?;
    let mut cluster = lock.read()?;
    cluster.prune_dead();

    if cluster.has_living_leader() {
        return Ok(false);
    }

    cleanup_stale_socket(&cluster);

    // Newest standby wins.
    if let Some(promoted_pid) = cluster.elect_leader() {
        lock.write(&cluster)?;
        Ok(promoted_pid == my_pid)
    } else {
        Ok(false)
    }
}

#[cfg(not(unix))]
fn try_promote_to_leader(_repo_root: &Path, _my_pid: u32) -> Result<bool> {
    Ok(false)
}

fn deregister_from_cluster(repo_root: &Path, my_pid: u32) {
    #[cfg(unix)]
    {
        if let Ok(lock) = host::MetadataLock::acquire(repo_root) {
            let _ = lock.mutate(|cluster| {
                cluster.deregister(my_pid);
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (repo_root, my_pid);
    }
}

fn run_interactive(runner: &mut ArchitectureWatchRunner, render: RenderFlags) -> Result<()> {
    run_interactive_with_warm_result(runner, render, None)
}

fn run_interactive_with_warm_result(
    runner: &mut ArchitectureWatchRunner,
    render: RenderFlags,
    warm_result: Option<ArchitectureRunOutcome>,
) -> Result<()> {
    eprintln!("[watch] architecture (boundaries)");
    if warm_result.is_none() {
        eprintln!("[watch] initial run");
    }

    let interrupted = Arc::new(AtomicBool::new(false));
    let mut source =
        NotifyEventSource::new(runner.repo_root().to_path_buf(), Arc::clone(&interrupted))?;
    let interrupt_for_host = Arc::clone(&interrupted);
    ctrlc::set_handler(move || {
        interrupted.store(true, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler for architecture watch mode")?;

    let binder = host::PlatformHostBinder;
    let repo_root = runner.repo_root().to_path_buf();
    let render_options = host::HostRenderOptions {
        verbose: render.verbose,
        timings: render.timings,
    };
    let outcome = run_architecture_watch_with_host(
        &mut source,
        runner,
        &repo_root,
        WatchHostConfig {
            binder,
            render_options,
            interrupt_flag: interrupt_for_host,
            warm_result,
            debounce_window: DEBOUNCE_WINDOW,
        },
    )?;
    if outcome.last_status == WatchRunStatus::Passed {
        return Ok(());
    }

    bail!("last run failed")
}

fn run_noninteractive<R: WatchRunner>(runner: &mut R) -> Result<()> {
    match runner.run(1, &[]) {
        WatchRunStatus::Passed => Ok(()),
        WatchRunStatus::Failed => bail!("last run failed"),
    }
}

struct WatchHostConfig<B: host::HostTransportBinder> {
    binder: B,
    render_options: host::HostRenderOptions,
    interrupt_flag: Arc<AtomicBool>,
    warm_result: Option<ArchitectureRunOutcome>,
    debounce_window: Duration,
}

fn run_architecture_watch_with_host<S, B>(
    source: &mut S,
    runner: &mut ArchitectureWatchRunner,
    repo_root: &Path,
    config: WatchHostConfig<B>,
) -> Result<WatchLoopOutcome>
where
    S: WatchEventSource,
    B: host::HostTransportBinder,
{
    let debounce = config.debounce_window;
    let boundaries_host = start_watch_host(
        repo_root,
        &config.binder,
        config.render_options,
        config.interrupt_flag,
    )?;
    // Wire the host's dirty flag to the event source so NotifyDirty wakes the loop.
    source.set_external_dirty_flag(boundaries_host.external_dirty_flag());
    let mut run_number = 1usize;
    let initial = match config.warm_result {
        Some(warm) => warm,
        None => {
            boundaries_host.begin_run();
            runner.run_once(run_number, &[])
        }
    };
    boundaries_host.complete_run(initial.report);
    let mut last_status = initial.status;
    let mut reruns = 0usize;

    loop {
        if boundaries_host.is_shutdown_requested() {
            eprintln!("[host] shutdown requested, exiting");
            break;
        }
        runner.on_waiting();
        let event = source.recv()?;
        if boundaries_host.is_shutdown_requested() {
            eprintln!("[host] shutdown requested, exiting");
            break;
        }
        let Some(changes) = collect_change_burst(source, event, debounce)? else {
            break;
        };
        boundaries_host.note_dirty();
        runner.on_change_burst(&changes);
        reruns += 1;
        run_number += 1;
        boundaries_host.begin_run();
        let outcome = runner.run_once(run_number, &changes);
        boundaries_host.complete_run(outcome.report);
        last_status = outcome.status;
    }

    Ok(WatchLoopOutcome {
        reruns,
        last_status,
    })
}

fn start_watch_host<B: host::HostTransportBinder>(
    repo_root: &Path,
    binder: &B,
    render_options: host::HostRenderOptions,
    interrupt_flag: Arc<AtomicBool>,
) -> Result<host::ArchitectureHost<B::Endpoint>> {
    host::ArchitectureHost::start_with_binder(
        repo_root,
        binder,
        render_options,
        Some(interrupt_flag),
    )
    .with_context(|| {
        format!(
            "failed to start boundaries host for {}",
            repo_root.display()
        )
    })
}

#[cfg(test)]
fn run_watch_loop<S: WatchEventSource, R: WatchRunner>(
    source: &mut S,
    runner: &mut R,
    debounce_window: Duration,
) -> Result<WatchLoopOutcome> {
    let mut run_number = 1usize;
    let mut last_status = runner.run(run_number, &[]);
    let mut reruns = 0usize;

    loop {
        runner.on_waiting();
        let event = source.recv()?;
        let Some(changes) = collect_change_burst(source, event, debounce_window)? else {
            break;
        };
        runner.on_change_burst(&changes);
        reruns += 1;
        run_number += 1;
        last_status = runner.run(run_number, &changes);
    }

    Ok(WatchLoopOutcome {
        reruns,
        last_status,
    })
}

fn collect_change_burst<S: WatchEventSource>(
    source: &mut S,
    event: WatchEvent,
    debounce_window: Duration,
) -> Result<Option<Vec<PathBuf>>> {
    match event {
        WatchEvent::Interrupt | WatchEvent::Closed => Ok(None),
        WatchEvent::Changes(paths) => {
            let mut unique_paths: BTreeSet<PathBuf> = paths.into_iter().collect();
            let burst_started = Instant::now();
            loop {
                let elapsed = burst_started.elapsed();
                if elapsed >= debounce_window {
                    break;
                }
                let remaining = debounce_window - elapsed;
                match source.recv_timeout(remaining)? {
                    Some(WatchEvent::Changes(paths)) => unique_paths.extend(paths),
                    Some(WatchEvent::Interrupt) | Some(WatchEvent::Closed) => return Ok(None),
                    None => break,
                }
            }
            Ok(Some(unique_paths.into_iter().collect()))
        }
    }
}

struct NotifyEventSource {
    repo_root: PathBuf,
    receiver: Receiver<notify::Result<Event>>,
    interrupted: Arc<AtomicBool>,
    external_dirty: Option<Arc<AtomicBool>>,
    _watcher: RecommendedWatcher,
}

impl NotifyEventSource {
    fn new(repo_root: PathBuf, interrupted: Arc<AtomicBool>) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |event| {
                let _ = sender.send(event);
            },
            Config::default(),
        )
        .context("failed to start notify watcher for architecture watch mode")?;
        watcher
            .watch(&repo_root, RecursiveMode::Recursive)
            .with_context(|| {
                format!(
                    "failed to watch repository root {} for architecture changes",
                    repo_root.display()
                )
            })?;

        Ok(Self {
            repo_root,
            receiver,
            interrupted,
            external_dirty: None,
            _watcher: watcher,
        })
    }

    fn next_event(&mut self, timeout: Option<Duration>) -> Result<Option<WatchEvent>> {
        let deadline = timeout.map(|duration| Instant::now() + duration);
        loop {
            if self.interrupted.load(Ordering::SeqCst) {
                return Ok(Some(WatchEvent::Interrupt));
            }

            // Check if an external NotifyDirty arrived via socket.
            if let Some(flag) = &self.external_dirty {
                if flag.swap(false, Ordering::SeqCst) {
                    return Ok(Some(WatchEvent::Changes(Vec::new())));
                }
            }

            let wait = deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(INTERRUPT_POLL_INTERVAL)
                .min(INTERRUPT_POLL_INTERVAL);
            if wait.is_zero() {
                return Ok(None);
            }

            match self.receiver.recv_timeout(wait) {
                Ok(message) => {
                    if let Some(event) = self.translate_event(message)? {
                        return Ok(Some(event));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        return Ok(None);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(Some(WatchEvent::Closed)),
            }
        }
    }

    fn translate_event(&self, message: notify::Result<Event>) -> Result<Option<WatchEvent>> {
        let event = message.context("architecture watch backend reported an error")?;
        if event.kind.is_access() {
            return Ok(None);
        }

        let paths: BTreeSet<PathBuf> = event
            .paths
            .into_iter()
            .filter(|path| path_matches_boundaries(&self.repo_root, path))
            .collect();
        if paths.is_empty() {
            return Ok(None);
        }

        Ok(Some(WatchEvent::Changes(paths.into_iter().collect())))
    }
}

impl WatchEventSource for NotifyEventSource {
    fn recv(&mut self) -> Result<WatchEvent> {
        loop {
            if let Some(event) = self.next_event(None)? {
                return Ok(event);
            }
        }
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WatchEvent>> {
        self.next_event(Some(timeout))
    }

    fn set_external_dirty_flag(&mut self, flag: Arc<AtomicBool>) {
        self.external_dirty = Some(flag);
    }
}

struct ArchitectureWatchRunner {
    render: RenderFlags,
    context: ArchitectureContext,
}

struct ArchitectureRunOutcome {
    status: WatchRunStatus,
    report: super::boundaries::BoundariesRunReport,
}

impl ArchitectureWatchRunner {
    fn new(render: RenderFlags, context: ArchitectureContext) -> Self {
        Self { render, context }
    }

    fn repo_root(&self) -> &Path {
        self.context.repo_root()
    }

    fn run_once(&mut self, run_number: usize, changes: &[PathBuf]) -> ArchitectureRunOutcome {
        if !changes.is_empty() {
            self.context.record_changes(changes);
        }

        let started = Instant::now();
        let report = match run_boundaries_watch_report(&mut self.context, self.render) {
            Ok(report) => report,
            Err(error) => super::boundaries::BoundariesRunReport {
                success: false,
                rendered_output: String::new(),
                summary: Some(format!("{error:#}")),
                timings_output: None,
                violations: Vec::new(),
            },
        };
        let duration = started.elapsed();
        let status = if report.success {
            WatchRunStatus::Passed
        } else {
            WatchRunStatus::Failed
        };

        let verdict = if report.success { "PASS" } else { "FAIL" };
        eprintln!(
            "[run {run_number}] {verdict} {:<10} {:.2}s",
            "boundaries",
            duration.as_secs_f64()
        );
        if let Some(timings_output) = &report.timings_output {
            eprint!("{timings_output}");
        }
        if !report.success {
            eprintln!(
                "[run {run_number}] failure detail for boundaries:\n{}",
                render_failure_detail(&report)
            );
        }

        ArchitectureRunOutcome { status, report }
    }
}

impl WatchRunner for ArchitectureWatchRunner {
    fn run(&mut self, run_number: usize, changes: &[PathBuf]) -> WatchRunStatus {
        self.run_once(run_number, changes).status
    }

    fn on_waiting(&mut self) {
        eprintln!("[watch] waiting for changes...");
    }

    fn on_change_burst(&mut self, paths: &[PathBuf]) {
        eprintln!(
            "[watch] change burst: {}",
            paths
                .iter()
                .map(|path| display_watch_path(self.context.repo_root(), path))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn render_failure_detail(report: &super::boundaries::BoundariesRunReport) -> String {
    match (&report.rendered_output[..], report.summary.as_deref()) {
        ("", Some(summary)) => summary.to_string(),
        (body, Some(summary)) if !body.ends_with('\n') => format!("{body}\n{summary}"),
        (body, Some(summary)) => format!("{body}{summary}"),
        (body, None) => body.to_string(),
    }
}

fn path_matches_boundaries(repo_root: &Path, path: &Path) -> bool {
    let Ok(rel_path) = path.strip_prefix(repo_root) else {
        return false;
    };

    if rel_path.starts_with("target") {
        return false;
    }

    rel_path.starts_with("src") && rel_path.extension().is_some_and(|ext| ext == "rs")
        || rel_path.file_name().is_some_and(|name| {
            matches!(
                name.to_str(),
                Some("boundaries.toml" | "Cargo.toml" | "Cargo.lock" | "build.rs")
            )
        })
}

fn display_watch_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use anyhow::Result;

    use super::{
        WatchEvent, WatchEventSource, WatchLoopOutcome, WatchRunStatus, WatchRunner, host,
        path_matches_boundaries, run_noninteractive, run_watch_loop, start_watch_host,
    };

    #[test]
    fn architecture_watch_debounces_back_to_back_changes() {
        let mut source = FakeEventSource::new(
            vec![
                WatchEvent::Changes(vec![PathBuf::from("src/lib.rs")]),
                WatchEvent::Closed,
            ],
            vec![
                Some(WatchEvent::Changes(vec![PathBuf::from("src/lib.rs")])),
                None,
            ],
        );
        let mut runner = FakeRunner::new([WatchRunStatus::Passed, WatchRunStatus::Passed]);

        let outcome = run_watch_loop(&mut source, &mut runner, Duration::from_millis(350)).unwrap();

        assert_eq!(
            outcome,
            WatchLoopOutcome {
                reruns: 1,
                last_status: WatchRunStatus::Passed,
            }
        );
        assert_eq!(
            runner.observed_changes,
            vec![vec![], vec![PathBuf::from("src/lib.rs")]]
        );
    }

    #[test]
    fn architecture_watch_reports_last_failed_status() {
        let mut source = FakeEventSource::new(
            vec![
                WatchEvent::Changes(vec![PathBuf::from("src/lib.rs")]),
                WatchEvent::Interrupt,
            ],
            vec![None],
        );
        let mut runner = FakeRunner::new([WatchRunStatus::Passed, WatchRunStatus::Failed]);

        let outcome = run_watch_loop(&mut source, &mut runner, Duration::from_millis(350)).unwrap();

        assert_eq!(outcome.last_status, WatchRunStatus::Failed);
    }

    #[test]
    fn architecture_watch_noninteractive_run_preserves_failures() {
        let mut runner = FakeRunner::new([WatchRunStatus::Failed]);

        let error = run_noninteractive(&mut runner).unwrap_err();

        assert!(error.to_string().contains("last run failed"));
        assert_eq!(runner.observed_changes, vec![Vec::<PathBuf>::new()]);
    }

    #[test]
    fn watch_path_filter_tracks_boundaries_inputs() {
        assert!(path_matches_boundaries(
            Path::new("/repo"),
            Path::new("/repo/src/runtime/mod.rs")
        ));
        assert!(!path_matches_boundaries(
            Path::new("/repo"),
            Path::new("/repo/docs/architecture/dependency-rules.md")
        ));
        assert!(!path_matches_boundaries(
            Path::new("/repo"),
            Path::new("/repo/target/rust-analyzer/metadata/workspace/Cargo.lock")
        ));
    }

    #[test]
    fn watch_mode_writes_host_metadata_on_startup() {
        let mut harness = WatchHostHarness::new();

        harness.start();

        assert!(harness.metadata_path().exists());
        assert_eq!(harness.bind_calls(), 1);
    }

    #[test]
    fn watch_mode_uses_worktree_specific_metadata_and_transport_names() {
        let mut left = WatchHostHarness::new_for_worktree("feature-a");
        let mut right = WatchHostHarness::new_for_worktree("feature-b");

        left.start();
        right.start();

        let left_metadata = left.metadata();
        let right_metadata = right.metadata();
        assert_ne!(left_metadata.worktree_id, right_metadata.worktree_id);
        assert_ne!(
            left_metadata.metadata_path(),
            right_metadata.metadata_path()
        );
        assert_ne!(left_metadata.transport, right_metadata.transport);
    }

    #[test]
    fn watch_mode_removes_metadata_on_clean_shutdown() {
        let mut harness = WatchHostHarness::new();
        harness.start();
        let metadata_path = harness.metadata_path();

        harness.stop();

        assert!(!metadata_path.exists());
        assert_eq!(harness.cleanup_calls(), 1);
    }

    #[test]
    fn noninteractive_run_does_not_accidentally_spawn_host_metadata() {
        let harness = WatchHostHarness::new();
        let mut runner = FakeRunner::new([WatchRunStatus::Passed]);

        run_noninteractive(&mut runner).unwrap();

        assert!(!harness.metadata_path().exists());
        assert_eq!(harness.bind_calls(), 0);
    }

    #[derive(Debug)]
    struct FakeEventSource {
        events: VecDeque<WatchEvent>,
        timeouts: VecDeque<Option<WatchEvent>>,
    }

    impl FakeEventSource {
        fn new(events: Vec<WatchEvent>, timeouts: Vec<Option<WatchEvent>>) -> Self {
            Self {
                events: events.into(),
                timeouts: timeouts.into(),
            }
        }
    }

    impl WatchEventSource for FakeEventSource {
        fn recv(&mut self) -> Result<WatchEvent> {
            self.events
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("missing fake event"))
        }

        fn recv_timeout(&mut self, _timeout: Duration) -> Result<Option<WatchEvent>> {
            self.timeouts
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("missing fake timeout event"))
        }
    }

    #[derive(Debug)]
    struct FakeRunner {
        statuses: VecDeque<WatchRunStatus>,
        observed_changes: Vec<Vec<PathBuf>>,
    }

    impl FakeRunner {
        fn new(statuses: impl IntoIterator<Item = WatchRunStatus>) -> Self {
            Self {
                statuses: statuses.into_iter().collect(),
                observed_changes: Vec::new(),
            }
        }
    }

    impl WatchRunner for FakeRunner {
        fn run(&mut self, _run_number: usize, changes: &[PathBuf]) -> WatchRunStatus {
            self.observed_changes.push(changes.to_vec());
            self.statuses.pop_front().unwrap_or(WatchRunStatus::Passed)
        }
    }

    #[derive(Debug, Default, Clone)]
    struct FakeHostBinder {
        bind_calls: Arc<AtomicUsize>,
        cleanup_calls: Arc<AtomicUsize>,
    }

    impl host::HostTransportBinder for FakeHostBinder {
        type Endpoint = FakeHostEndpoint;

        fn bind(
            &self,
            _metadata: &host::HostMetadata,
            _state: host::SharedHostState,
            _render_options: host::HostRenderOptions,
        ) -> Result<Self::Endpoint> {
            self.bind_calls.fetch_add(1, Ordering::SeqCst);
            Ok(FakeHostEndpoint {
                cleanup_calls: Arc::clone(&self.cleanup_calls),
            })
        }
    }

    #[derive(Debug)]
    struct FakeHostEndpoint {
        cleanup_calls: Arc<AtomicUsize>,
    }

    impl host::HostEndpoint for FakeHostEndpoint {
        fn cleanup(&mut self) -> Result<()> {
            self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct WatchHostHarness {
        repo_root: PathBuf,
        binder: FakeHostBinder,
        host: Option<host::ArchitectureHost<FakeHostEndpoint>>,
    }

    impl WatchHostHarness {
        fn new() -> Self {
            Self::new_for_worktree("default")
        }

        fn new_for_worktree(name: &str) -> Self {
            let repo_root = unique_repo_root(name);
            fs::create_dir_all(&repo_root).unwrap();
            Self {
                repo_root,
                binder: FakeHostBinder::default(),
                host: None,
            }
        }

        fn start(&mut self) {
            self.host = Some(
                start_watch_host(
                    &self.repo_root,
                    &self.binder,
                    host::HostRenderOptions {
                        verbose: false,
                        timings: false,
                    },
                    Arc::new(AtomicBool::new(false)),
                )
                .unwrap(),
            );
        }

        fn stop(&mut self) {
            if let Some(host) = self.host.take() {
                host.shutdown();
            }
        }

        fn metadata_path(&self) -> PathBuf {
            host::HostMetadata::empty_for_repo(&self.repo_root).metadata_path()
        }

        fn metadata(&self) -> host::HostMetadata {
            let content = fs::read_to_string(self.metadata_path()).unwrap();
            serde_json::from_str(&content).unwrap()
        }

        fn bind_calls(&self) -> usize {
            self.binder.bind_calls.load(Ordering::SeqCst)
        }

        fn cleanup_calls(&self) -> usize {
            self.binder.cleanup_calls.load(Ordering::SeqCst)
        }
    }

    impl Drop for WatchHostHarness {
        fn drop(&mut self) {
            self.stop();
            let _ = fs::remove_dir_all(self.repo_root.parent().unwrap_or(&self.repo_root));
        }
    }

    fn unique_repo_root(name: &str) -> PathBuf {
        let unique = format!(
            "mmdflux-watch-host-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir()
            .join(unique)
            .join("worktrees")
            .join(name)
    }
}
