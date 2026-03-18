use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::{
    ArchitectureContext, ArchitectureOptions, ArchitectureSuite, run_suites_collect,
    selected_suites, suite_name,
};

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
}

trait WatchRunner {
    fn run(&mut self, run_number: usize, changes: &[PathBuf]) -> WatchRunStatus;

    fn on_waiting(&mut self) {}

    fn on_change_burst(&mut self, _paths: &[PathBuf]) {}
}

pub(crate) fn run(options: ArchitectureOptions, context: ArchitectureContext) -> Result<()> {
    let mut runner = ArchitectureWatchRunner::new(options, context);
    if !std::io::stdin().is_terminal() {
        return run_noninteractive(&mut runner);
    }

    eprintln!("[watch] architecture ({})", runner.selection_label());
    eprintln!("[watch] initial run");

    let interrupted = Arc::new(AtomicBool::new(false));
    let mut source = NotifyEventSource::new(
        runner.repo_root().to_path_buf(),
        options.suite,
        Arc::clone(&interrupted),
    )?;
    ctrlc::set_handler(move || {
        interrupted.store(true, Ordering::SeqCst);
    })
    .context("failed to install Ctrl-C handler for architecture watch mode")?;

    let outcome = run_watch_loop(&mut source, &mut runner, DEBOUNCE_WINDOW)?;
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
        WatchEvent::Interrupt | WatchEvent::Closed => return Ok(None),
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
    selection: ArchitectureSuite,
    receiver: Receiver<notify::Result<Event>>,
    interrupted: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}

impl NotifyEventSource {
    fn new(
        repo_root: PathBuf,
        selection: ArchitectureSuite,
        interrupted: Arc<AtomicBool>,
    ) -> Result<Self> {
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
            selection,
            receiver,
            interrupted,
            _watcher: watcher,
        })
    }

    fn next_event(&mut self, timeout: Option<Duration>) -> Result<Option<WatchEvent>> {
        let deadline = timeout.map(|duration| Instant::now() + duration);
        loop {
            if self.interrupted.load(Ordering::SeqCst) {
                return Ok(Some(WatchEvent::Interrupt));
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
            .filter(|path| path_matches_selection(&self.repo_root, self.selection, path))
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
}

struct ArchitectureWatchRunner {
    options: ArchitectureOptions,
    context: ArchitectureContext,
}

impl ArchitectureWatchRunner {
    fn new(mut options: ArchitectureOptions, context: ArchitectureContext) -> Self {
        options.watch = false;
        Self { options, context }
    }

    fn repo_root(&self) -> &Path {
        self.context.repo_root()
    }

    fn selection_label(&self) -> String {
        selected_suites(self.options.suite)
            .iter()
            .map(|suite| suite_name(*suite))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl WatchRunner for ArchitectureWatchRunner {
    fn run(&mut self, run_number: usize, changes: &[PathBuf]) -> WatchRunStatus {
        if !changes.is_empty() {
            self.context.record_changes(changes);
        }

        let executions = run_suites_collect(&mut self.context, self.options, true);
        let has_failures = executions.iter().any(|execution| !execution.is_success());
        for execution in &executions {
            let verdict = if execution.is_success() {
                "PASS"
            } else {
                "FAIL"
            };
            eprintln!(
                "[run {run_number}] {verdict} {:<10} {:.2}s",
                suite_name(execution.suite),
                execution.duration.as_secs_f64()
            );
        }
        for execution in executions
            .into_iter()
            .filter(|execution| !execution.is_success())
        {
            let error = execution
                .error
                .expect("failed suite executions should capture their error");
            eprintln!(
                "[run {run_number}] failure detail for {}:\n{error}",
                suite_name(execution.suite)
            );
        }

        if has_failures {
            WatchRunStatus::Failed
        } else {
            WatchRunStatus::Passed
        }
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

fn path_matches_selection(repo_root: &Path, selection: ArchitectureSuite, path: &Path) -> bool {
    let Ok(rel_path) = path.strip_prefix(repo_root) else {
        return false;
    };
    selected_suites(selection)
        .iter()
        .any(|suite| path_matches_suite(*suite, rel_path))
}

fn path_matches_suite(suite: ArchitectureSuite, rel_path: &Path) -> bool {
    if rel_path.starts_with("target") {
        return false;
    }

    match suite {
        ArchitectureSuite::All => unreachable!("path matching expects a concrete suite"),
        ArchitectureSuite::Boundaries => {
            rel_path.starts_with("src") && rel_path.extension().is_some_and(|ext| ext == "rs")
                || rel_path.file_name().is_some_and(|name| {
                    matches!(
                        name.to_str(),
                        Some("boundaries.toml" | "Cargo.toml" | "Cargo.lock" | "build.rs")
                    )
                })
        }
    }
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
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use anyhow::Result;

    use super::{
        WatchEvent, WatchEventSource, WatchLoopOutcome, WatchRunStatus, WatchRunner,
        path_matches_suite, run_noninteractive, run_watch_loop,
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
    fn watch_path_filter_tracks_suite_specific_inputs() {
        assert!(path_matches_suite(
            super::ArchitectureSuite::Boundaries,
            Path::new("src/runtime/mod.rs")
        ));
        assert!(!path_matches_suite(
            super::ArchitectureSuite::Boundaries,
            Path::new("docs/architecture/dependency-rules.md")
        ));
        assert!(!path_matches_suite(
            super::ArchitectureSuite::Boundaries,
            Path::new("target/rust-analyzer/metadata/workspace/Cargo.lock")
        ));
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
}
