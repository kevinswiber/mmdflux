use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use annotate_snippets::renderer::DecorStyle;
use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use anyhow::{Context, Result};
use ra_ap_cfg::{CfgAtom, CfgDiff};
use ra_ap_hir::{
    Crate as HirCrate, Module, ModuleDef, PathResolution, ScopeDef, Semantics, Symbol,
};
use ra_ap_ide::{AnalysisHost, Edition, RootDatabase};
use ra_ap_ide_db::{ChangeWithProcMacros, EditionedFileId, FxHashMap};
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, ProjectFolders, SourceRootConfig};
use ra_ap_paths::{AbsPathBuf, Utf8PathBuf};
use ra_ap_project_model::{
    CargoConfig, CargoFeatures, CfgOverrides, InvocationStrategy, ProjectManifest,
    ProjectWorkspace, ProjectWorkspaceKind, RustLibSource, TargetData, TargetKind,
};
use ra_ap_syntax::{AstNode, SyntaxNode, TextRange, TextSize, ast};
use ra_ap_vfs::{Change, Vfs, VfsPath};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SemanticBoundariesSuiteOptions {
    pub(crate) timings: bool,
    pub(crate) quiet: bool,
    pub(crate) verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BoundariesRunReport {
    pub(crate) success: bool,
    pub(crate) rendered_output: String,
    pub(crate) summary: Option<String>,
    pub(crate) timings_output: Option<String>,
    pub(crate) violations: Vec<BoundaryViolation>,
}

/// Full result of a boundaries run, including the graph for inspection.
#[derive(Debug, Clone)]
pub(crate) struct BoundariesRunResult {
    pub(crate) report: BoundariesRunReport,
    pub(crate) graph: BoundaryGraph,
}

#[derive(Debug, Default)]
pub(crate) struct SemanticBoundariesContext {
    library: Option<LoadedLibrary>,
    pending_refresh: PendingRefresh,
}

impl SemanticBoundariesContext {
    pub(crate) fn record_changes(&mut self, paths: &[PathBuf]) {
        for path in paths {
            match classify_pending_refresh(path) {
                PendingRefreshKind::Ignore => {}
                PendingRefreshKind::IncrementalSource(path) => {
                    self.pending_refresh.note_incremental(path);
                }
                PendingRefreshKind::FullReload => {
                    self.pending_refresh = PendingRefresh::FullReload;
                }
            }
        }
    }

    fn workspace_status(&self) -> &'static str {
        match (&self.library, &self.pending_refresh) {
            (None, _) => "load mmdflux library target through rust-analyzer",
            (_, PendingRefresh::FullReload) => {
                "reload mmdflux library target through rust-analyzer"
            }
            (_, PendingRefresh::Incremental(_)) => {
                "incrementally refresh warmed semantic boundary context"
            }
            (_, PendingRefresh::None) => "reuse warmed semantic boundary context",
        }
    }

    fn load_library(&mut self) -> Result<&mut LoadedLibrary> {
        let pending_refresh = std::mem::take(&mut self.pending_refresh);

        if self.library.is_none() || matches!(pending_refresh, PendingRefresh::FullReload) {
            self.library = Some(load_library()?);
            return Ok(self.library.as_mut().expect("library should be loaded"));
        }

        if let PendingRefresh::Incremental(paths) = pending_refresh
            && let Some(library) = self.library.as_mut()
        {
            library.apply_source_changes(&paths)?;
        }

        Ok(self.library.as_mut().expect("library should be present"))
    }
}

#[derive(Debug)]
struct LoadedLibrary {
    krate: HirCrate,
    target: TargetData,
    host: AnalysisHost,
    vfs: Vfs,
    source_root_config: SourceRootConfig,
}

#[derive(Debug, Default)]
enum PendingRefresh {
    #[default]
    None,
    Incremental(BTreeSet<PathBuf>),
    FullReload,
}

impl PendingRefresh {
    fn note_incremental(&mut self, path: PathBuf) {
        match self {
            Self::None => {
                let mut paths = BTreeSet::new();
                paths.insert(path);
                *self = Self::Incremental(paths);
            }
            Self::Incremental(paths) => {
                paths.insert(path);
            }
            Self::FullReload => {}
        }
    }
}

#[derive(Debug)]
enum PendingRefreshKind {
    Ignore,
    IncrementalSource(PathBuf),
    FullReload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BoundaryViolation {
    pub(crate) source_boundary: String,
    pub(crate) target_boundary: String,
    pub(crate) symbol: String,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) line_text: Option<String>,
    pub(crate) underline_offset: Option<usize>,
    pub(crate) underline_len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rule_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rule_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

impl BoundaryViolation {
    fn from_sample(source: &str, target: &str, sample: &DependencySample) -> Self {
        Self {
            source_boundary: source.to_string(),
            target_boundary: target.to_string(),
            symbol: sample.symbol.clone(),
            file: sample.location.as_ref().map(|loc| loc.path.clone()),
            line: sample.location.as_ref().map(|loc| loc.line),
            column: sample.location.as_ref().map(|loc| loc.column),
            line_text: sample.location.as_ref().map(|loc| loc.line_text.clone()),
            underline_offset: sample.location.as_ref().map(|loc| loc.underline_offset),
            underline_len: sample.location.as_ref().map(|loc| loc.underline_len),
            rule_id: None,
            rule_type: None,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DependencySample {
    pub(crate) source: String,
    pub(crate) symbol: String,
    pub(crate) target: String,
    pub(crate) location: Option<SourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceLocation {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) line_text: String,
    pub(crate) underline_offset: usize,
    pub(crate) underline_len: usize,
}

// ---------------------------------------------------------------------------
// BoundaryGraph — reusable graph artifact for rule evaluation & inspection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BoundaryGraph {
    pub(crate) boundaries: BTreeSet<String>,
    #[serde(
        serialize_with = "serialize_edges",
        deserialize_with = "deserialize_edges"
    )]
    pub(crate) edges: BTreeMap<(String, String), BoundaryEdge>,
}

fn serialize_edges<S: serde::Serializer>(
    edges: &BTreeMap<(String, String), BoundaryEdge>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(edges.len()))?;
    for ((source, target), edge) in edges {
        seq.serialize_element(&(source, target, edge))?;
    }
    seq.end()
}

fn deserialize_edges<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<(String, String), BoundaryEdge>, D::Error> {
    let entries: Vec<(String, String, BoundaryEdge)> =
        serde::Deserialize::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .map(|(source, target, edge)| ((source, target), edge))
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BoundaryEdge {
    pub(crate) sample: DependencySample,
    pub(crate) provenance: EdgeProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum EdgeProvenance {
    ModuleScope,
    QualifiedPath,
    Mixed,
}

impl EdgeProvenance {
    fn merge(self, other: EdgeProvenance) -> EdgeProvenance {
        if self == other {
            self
        } else {
            EdgeProvenance::Mixed
        }
    }
}

impl BoundaryGraph {
    pub(crate) fn new(boundaries: BTreeSet<String>) -> Self {
        Self {
            boundaries,
            edges: BTreeMap::new(),
        }
    }

    pub(crate) fn insert_edge(
        &mut self,
        source: String,
        target: String,
        sample: DependencySample,
        provenance: EdgeProvenance,
    ) {
        let key = (source, target);
        match self.edges.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(BoundaryEdge { sample, provenance });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                existing.provenance = existing.provenance.merge(provenance);
                if should_replace_dependency_sample(&existing.sample, &sample) {
                    existing.sample = sample;
                }
            }
        }
    }

    pub(crate) fn edge(&self, source: &str, target: &str) -> Option<&BoundaryEdge> {
        self.edges.get(&(source.to_string(), target.to_string()))
    }

    /// Convert to the legacy edge map format for rendering code that
    /// still expects the old shape.
    pub(crate) fn to_legacy_edge_map(
        &self,
    ) -> BTreeMap<String, BTreeMap<String, DependencySample>> {
        let mut map = BTreeMap::<String, BTreeMap<String, DependencySample>>::new();
        for ((source, target), edge) in &self.edges {
            map.entry(source.clone())
                .or_default()
                .insert(target.clone(), edge.sample.clone());
        }
        map
    }
}

#[derive(Debug, Clone, Copy)]
struct LocatedModule {
    module: Module,
    range: TextRange,
}

#[derive(Debug, Default)]
struct ModuleLocator {
    modules: Vec<LocatedModule>,
}

#[derive(Debug, Clone)]
enum RelativePathSegment {
    CrateKw,
    SelfKw,
    SuperKw,
    Name(String),
    Unsupported,
}

#[derive(Debug, Default)]
struct TimingBreakdown {
    config_load: Duration,
    workspace_load: Duration,
    boundary_discovery: Duration,
    boundary_discovery_repeat: Duration,
    module_scope_scan: Duration,
    qualified_path_scan: Duration,
    qualified_path_candidate_file_filtering: Duration,
    qualified_path_file_reads: Duration,
    qualified_path_edition_attach: Duration,
    qualified_path_sema_parse: Duration,
    qualified_path_module_locator_setup: Duration,
    qualified_path_module_locator_repeat: Duration,
    qualified_path_use_tree_walk: Duration,
    qualified_path_path_walk: Duration,
    qualified_path_segment_extraction: Duration,
    qualified_path_fast_path_resolution: Duration,
    qualified_path_semantic_fallback_resolution: Duration,
    qualified_path_source_rs_files: usize,
    qualified_path_candidate_files: usize,
    qualified_path_parsed_files: usize,
    qualified_path_use_tree_candidates: usize,
    qualified_path_path_candidates: usize,
    qualified_path_text_hits: usize,
    qualified_path_duplicate_path_hits: usize,
    qualified_path_token_lookup: Duration,
    qualified_path_path_ascend: Duration,
    qualified_path_slowest_files: Vec<SlowPathFile>,
    qualified_path_slowest_module_locator_files: Vec<SlowModuleLocatorFile>,
    violation_analysis: Duration,
    reporting: Duration,
    total: Duration,
}

#[derive(Debug, Default)]
struct QualifiedPathScanBreakdown {
    source_rs_files: usize,
    candidate_files: usize,
    parsed_files: usize,
    use_tree_candidates: usize,
    path_candidates: usize,
    text_hits: usize,
    duplicate_path_hits: usize,
    direct_prefix_hits: usize,
    semantic_fallbacks: usize,
    candidate_file_filtering: Duration,
    file_reads: Duration,
    edition_attach: Duration,
    sema_parse: Duration,
    module_locator_setup: Duration,
    module_locator_repeat: Duration,
    use_tree_walk: Duration,
    path_walk: Duration,
    segment_extraction: Duration,
    token_lookup: Duration,
    path_ascend: Duration,
    fast_path_resolution: Duration,
    semantic_fallback_resolution: Duration,
    slowest_path_files: Vec<SlowPathFile>,
    slowest_module_locator_files: Vec<SlowModuleLocatorFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum PathResolutionKind {
    Ignored,
    SyntacticFastPath,
    SemanticFallback,
}

#[derive(Debug, Clone)]
struct SlowPathFile {
    path: String,
    parsed_ordinal: usize,
    path_walk: Duration,
    text_hits: usize,
    duplicate_path_hits: usize,
    use_tree_candidates: usize,
    path_candidates: usize,
    token_lookup: Duration,
    path_ascend: Duration,
}

#[derive(Debug, Clone)]
struct SlowModuleLocatorFile {
    path: String,
    parsed_ordinal: usize,
    locator_setup: Duration,
    module_count: usize,
}

struct EdgeCollectionContext<'a> {
    sema: &'a Semantics<'a, RootDatabase>,
    vfs: &'a Vfs,
    krate: HirCrate,
    root: Module,
    db: &'a RootDatabase,
    declared_boundaries: &'a BTreeSet<String>,
    exact_module_paths: Option<&'a BTreeSet<String>>,
    verbose: bool,
}

struct CollectedDependencyGraphs {
    boundary_graph: BoundaryGraph,
    exact_module_graph: Option<BoundaryGraph>,
}

pub(crate) fn run_with_context(
    context: &mut SemanticBoundariesContext,
    options: SemanticBoundariesSuiteOptions,
) -> Result<()> {
    let result = run_with_context_report(context, options)?;
    let report = &result.report;
    if let Some(timings_output) = &report.timings_output {
        eprint!("{timings_output}");
    }
    if report.success {
        return Ok(());
    }

    if options.quiet {
        anyhow::bail!(quiet_failure_message(report));
    }

    eprint!("{}", report.rendered_output);
    anyhow::bail!(
        report
            .summary
            .clone()
            .unwrap_or_else(|| "error: architecture boundaries failed".to_string())
    );
}

pub(crate) fn run_with_context_report(
    context: &mut SemanticBoundariesContext,
    options: SemanticBoundariesSuiteOptions,
) -> Result<BoundariesRunResult> {
    let started = Instant::now();
    let mut timings = TimingBreakdown::default();

    let phase_started = Instant::now();
    let config_path = super::policy::resolve_policy_path(&repo_root());
    let arch_policy = super::policy::parse_policy_file(&config_path)?;
    timings.config_load = phase_started.elapsed();
    if !options.quiet && options.verbose {
        log_info(format!(
            "load semantic boundaries policy from {}",
            display_repo_relative(&config_path)
        ));
    }
    // Extract legacy allow map for downstream rendering compatibility.
    // This will be replaced by rule-aware rendering in a later task.
    let policy = arch_policy.extract_allow_map();

    if !options.quiet && options.verbose {
        log_info(context.workspace_status());
    }
    let phase_started = Instant::now();
    let loaded = context.load_library()?;
    timings.workspace_load = phase_started.elapsed();
    let db = loaded.host.raw_database();
    let sema = Semantics::new(db);
    let root = loaded.krate.root_module();

    let policy_boundaries: BTreeSet<_> = arch_policy.modules.keys().cloned().collect();
    let phase_started = Instant::now();
    let discovered_boundaries = discover_top_level_boundaries(root, db);
    timings.boundary_discovery = phase_started.elapsed();
    if options.timings {
        let phase_started = Instant::now();
        let rediscovered_boundaries = discover_top_level_boundaries(root, db);
        timings.boundary_discovery_repeat = phase_started.elapsed();
        debug_assert_eq!(discovered_boundaries, rediscovered_boundaries);
    }
    let missing_boundaries: Vec<_> = policy_boundaries
        .difference(&discovered_boundaries)
        .cloned()
        .collect();
    if !missing_boundaries.is_empty() {
        anyhow::bail!(
            "{} declares missing top-level modules: {:?}",
            display_repo_relative(&config_path),
            missing_boundaries
        );
    }

    if !options.quiet && options.verbose {
        log_info(format!(
            "discover {} declared boundaries from semantic module tree",
            policy_boundaries.len()
        ));
    }
    let exact_module_paths = if arch_policy.uses_module_path_selectors() {
        let exact_module_paths = discover_module_paths(root, db);
        validate_declared_module_paths(&arch_policy, &exact_module_paths, &config_path)?;
        Some(exact_module_paths)
    } else {
        None
    };
    let edge_collection = EdgeCollectionContext {
        sema: &sema,
        vfs: &loaded.vfs,
        krate: loaded.krate,
        root,
        db,
        declared_boundaries: &policy_boundaries,
        exact_module_paths: exact_module_paths.as_ref(),
        verbose: !options.quiet && options.verbose,
    };
    let collected_graphs = collect_dependency_graphs(&edge_collection, &mut timings);
    let phase_started = Instant::now();
    let eval_result = super::rules_eval::evaluate_rules_with_module_graph(
        &collected_graphs.boundary_graph,
        collected_graphs.exact_module_graph.as_ref(),
        &arch_policy,
    );
    let mut boundary_violations: Vec<BoundaryViolation> = eval_result
        .violations
        .iter()
        .map(|v| {
            let mut bv =
                BoundaryViolation::from_sample(&v.source_boundary, &v.target_boundary, &v.sample);
            bv.rule_id = Some(v.rule_id.clone());
            bv.rule_type = Some(v.rule_type.clone());
            bv.detail = v.detail.clone();
            bv
        })
        .collect();
    boundary_violations.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.column.cmp(&b.column))
            .then(a.source_boundary.cmp(&b.source_boundary))
            .then(a.target_boundary.cmp(&b.target_boundary))
    });
    timings.violation_analysis = phase_started.elapsed();
    let phase_started = Instant::now();
    if !options.quiet && options.verbose {
        let actual = collected_graphs.boundary_graph.to_legacy_edge_map();
        report_boundary_results(&policy, &arch_policy, &actual);
        for suppressed in &eval_result.suppressed {
            log_info(format!(
                "suppressed: {} -> {} (exception: {})",
                suppressed.violation.source_boundary,
                suppressed.violation.target_boundary,
                suppressed.exception_id,
            ));
        }
        for unused in &eval_result.unused_exceptions {
            log_info(format!("unused exception: {unused}"));
        }
    }
    let report = if boundary_violations.is_empty() {
        BoundariesRunReport {
            success: true,
            rendered_output: String::new(),
            summary: None,
            timings_output: None,
            violations: Vec::new(),
        }
    } else {
        let actual = collected_graphs.boundary_graph.to_legacy_edge_map();
        BoundariesRunReport {
            success: false,
            rendered_output: render_violation_report(
                &policy,
                &boundary_violations,
                &actual,
                options.verbose,
                &diagnostic_renderer(),
                false,
            ),
            summary: Some(format_failure_summary(boundary_violations.len())),
            timings_output: None,
            violations: boundary_violations,
        }
    };
    timings.reporting = phase_started.elapsed();
    timings.total = started.elapsed();
    if !options.quiet && options.verbose {
        log_info(format!("finished in {:.2}s", timings.total.as_secs_f64()));
    }

    Ok(BoundariesRunResult {
        report: BoundariesRunReport {
            timings_output: options.timings.then(|| render_timing_breakdown(&timings)),
            ..report
        },
        graph: collected_graphs.boundary_graph,
    })
}

/// Load the library and collect the boundary graph for inspection commands.
/// Reuses the same semantic analysis as `run_with_context_report` but skips
/// violation analysis and reporting.
pub(crate) fn collect_graph_for_inspection(
    context: &mut SemanticBoundariesContext,
) -> Result<BoundaryGraph> {
    let config_path = super::policy::resolve_policy_path(&repo_root());
    let arch_policy = super::policy::parse_policy_file(&config_path)?;
    let policy_boundaries: BTreeSet<_> = arch_policy.modules.keys().cloned().collect();

    let loaded = context.load_library()?;
    let db = loaded.host.raw_database();
    let sema = Semantics::new(db);
    let root = loaded.krate.root_module();

    let discovered_boundaries = discover_top_level_boundaries(root, db);
    let missing: Vec<_> = policy_boundaries
        .difference(&discovered_boundaries)
        .cloned()
        .collect();
    if !missing.is_empty() {
        anyhow::bail!(
            "{} declares missing top-level modules: {:?}",
            display_repo_relative(&config_path),
            missing
        );
    }

    let edge_collection = EdgeCollectionContext {
        sema: &sema,
        vfs: &loaded.vfs,
        krate: loaded.krate,
        root,
        db,
        declared_boundaries: &policy_boundaries,
        exact_module_paths: None,
        verbose: false,
    };
    let mut timings = TimingBreakdown::default();
    Ok(collect_dependency_graphs(&edge_collection, &mut timings).boundary_graph)
}

fn load_library() -> Result<LoadedLibrary> {
    let manifest_path = repo_root().join("Cargo.toml");
    let manifest_path = manifest_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", manifest_path.display()))?;
    let cargo_config = cargo_config();
    let load_config = load_config();
    let progress = |_| {};

    let utf8_path = Utf8PathBuf::from_path_buf(manifest_path.clone())
        .map_err(|path| anyhow::anyhow!("non-utf8 manifest path: {}", path.display()))?;
    let root = AbsPathBuf::assert(utf8_path);
    let manifest = ProjectManifest::discover_single(root.as_path())
        .context("failed to discover Cargo manifest for semantic boundaries guard")?;
    let mut workspace = ProjectWorkspace::load(manifest, &cargo_config, &progress)
        .context("failed to load cargo workspace through rust-analyzer")?;

    if load_config.load_out_dirs_from_check {
        let build_scripts = workspace
            .run_build_scripts(&cargo_config, &progress)
            .context("failed to run build scripts for rust-analyzer workspace load")?;
        workspace.set_build_scripts(build_scripts);
    }

    let source_root_config =
        ProjectFolders::new(std::slice::from_ref(&workspace), &[], None).source_root_config;
    let target = select_library_target(&workspace)?;
    let (db, vfs, _proc_macro_client) =
        ra_ap_load_cargo::load_workspace(workspace, &cargo_config.extra_env, &load_config)
            .context("failed to load workspace into rust-analyzer database")?;
    let host = AnalysisHost::with_database(db);
    let krate = find_library_crate(host.raw_database(), &vfs, &target)?;

    Ok(LoadedLibrary {
        krate,
        target,
        host,
        vfs,
        source_root_config,
    })
}

impl LoadedLibrary {
    fn apply_source_changes(&mut self, paths: &BTreeSet<PathBuf>) -> Result<()> {
        let mut analysis_change = ChangeWithProcMacros::default();
        let mut changed_roots = false;
        let mut applied_changes = false;

        for path in paths {
            let abs_path = absolute_watch_path(path)?;
            let contents = read_watch_file_contents(abs_path.as_ref())?;
            self.vfs
                .set_file_contents(VfsPath::from(abs_path), contents);
        }

        for (_, changed_file) in self.vfs.take_changes() {
            changed_roots |= changed_file.is_created_or_deleted();
            match changed_file.change {
                Change::Create(bytes, _) | Change::Modify(bytes, _) => {
                    if let Ok(text) = String::from_utf8(bytes) {
                        analysis_change.change_file(changed_file.file_id, Some(text));
                        applied_changes = true;
                    }
                }
                Change::Delete => {
                    analysis_change.change_file(changed_file.file_id, None);
                    applied_changes = true;
                }
            }
        }

        if !applied_changes {
            return Ok(());
        }

        if changed_roots {
            analysis_change.set_roots(self.source_root_config.partition(&self.vfs));
        }
        self.host.apply_change(analysis_change);
        self.krate = find_library_crate(self.host.raw_database(), &self.vfs, &self.target)?;
        Ok(())
    }
}

fn classify_pending_refresh(path: &Path) -> PendingRefreshKind {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    };
    let Ok(rel_path) = absolute.strip_prefix(repo_root()) else {
        return PendingRefreshKind::Ignore;
    };
    let file_name = rel_path.file_name().and_then(|name| name.to_str());

    if rel_path.starts_with("target") {
        return PendingRefreshKind::Ignore;
    }
    if rel_path == Path::new("boundaries.toml") {
        return PendingRefreshKind::Ignore;
    }
    if matches!(file_name, Some("Cargo.toml" | "Cargo.lock" | "build.rs")) {
        return PendingRefreshKind::FullReload;
    }
    if rel_path.starts_with("src") && rel_path.extension().is_some_and(|ext| ext == "rs") {
        return PendingRefreshKind::IncrementalSource(absolute);
    }

    PendingRefreshKind::Ignore
}

fn absolute_watch_path(path: &Path) -> Result<AbsPathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    };
    let utf8_path = Utf8PathBuf::from_path_buf(absolute.clone())
        .map_err(|path| anyhow::anyhow!("non-utf8 watch path: {}", path.display()))?;
    Ok(AbsPathBuf::assert_utf8(utf8_path.into()))
}

fn read_watch_file_contents(path: &Path) -> Result<Option<Vec<u8>>> {
    // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn cargo_config() -> CargoConfig {
    CargoConfig {
        all_targets: false,
        cfg_overrides: CfgOverrides {
            global: CfgDiff::new(Vec::new(), vec![CfgAtom::Flag(Symbol::intern("test"))]),
            selective: Default::default(),
        },
        extra_args: Vec::new(),
        extra_env: FxHashMap::default(),
        extra_includes: Vec::new(),
        features: CargoFeatures::Selected {
            features: Vec::new(),
            no_default_features: false,
        },
        invocation_strategy: InvocationStrategy::PerWorkspace,
        no_deps: false,
        run_build_script_command: None,
        rustc_source: None,
        set_test: false,
        sysroot_src: None,
        sysroot: Some(RustLibSource::Discover),
        target_dir: None,
        target: None,
        wrap_rustc_in_build_scripts: true,
    }
}

fn load_config() -> LoadCargoConfig {
    LoadCargoConfig {
        load_out_dirs_from_check: false,
        prefill_caches: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
    }
}

fn select_library_target(workspace: &ProjectWorkspace) -> Result<TargetData> {
    let cargo = match &workspace.kind {
        ProjectWorkspaceKind::Cargo { cargo, .. } => cargo,
        _ => anyhow::bail!("semantic boundaries guard expected a cargo workspace"),
    };

    let package = cargo
        .packages()
        .find(|idx| cargo[*idx].is_member && cargo[*idx].name == "mmdflux")
        .context("failed to find mmdflux package in workspace")?;

    let target = cargo[package]
        .targets
        .iter()
        .copied()
        .find(|idx| matches!(cargo[*idx].kind, TargetKind::Lib { .. }))
        .context("failed to find library target for mmdflux package")?;

    Ok(cargo[target].clone())
}

fn find_library_crate(db: &RootDatabase, vfs: &Vfs, target: &TargetData) -> Result<HirCrate> {
    let target_root = target.root.as_path();

    ra_ap_hir::Crate::all(db)
        .into_iter()
        .find(|krate| {
            vfs.file_path(krate.root_file(db))
                .as_path()
                .is_some_and(|path| path == target_root)
        })
        .context("failed to map rust-analyzer target back to mmdflux library crate")
}

fn discover_top_level_boundaries(root: Module, db: &RootDatabase) -> BTreeSet<String> {
    root.children(db)
        .filter_map(|module| module_name(module, db))
        .collect()
}

fn discover_module_paths(root: Module, db: &RootDatabase) -> BTreeSet<String> {
    let mut module_paths = BTreeSet::new();
    for module in root.children(db) {
        collect_module_paths(module, db, &mut module_paths);
    }
    module_paths
}

fn collect_module_paths(module: Module, db: &RootDatabase, module_paths: &mut BTreeSet<String>) {
    let module_path = module_selector_path(module, db);
    if !module_path.is_empty() {
        module_paths.insert(module_path);
    }
    for child in module.children(db) {
        collect_module_paths(child, db, module_paths);
    }
}

fn validate_declared_module_paths(
    policy: &super::policy::ArchitecturePolicy,
    discovered_paths: &BTreeSet<String>,
    config_path: &Path,
) -> Result<()> {
    let mut missing = Vec::new();
    for selector in policy_module_path_selectors(policy) {
        if !discovered_paths.contains(&selector.path) {
            missing.push(format!("{} ({})", selector.path, selector.context));
        }
    }
    if missing.is_empty() {
        return Ok(());
    }

    anyhow::bail!(
        "{} declares missing module paths: {:?}",
        display_repo_relative(config_path),
        missing
    );
}

struct PolicyModuleSelector {
    path: String,
    context: String,
}

fn policy_module_path_selectors(
    policy: &super::policy::ArchitecturePolicy,
) -> Vec<PolicyModuleSelector> {
    let mut selectors = Vec::new();
    for rule in &policy.rules {
        match &rule.rule {
            super::policy::RuleKind::Allow(allow) => {
                push_module_selector(
                    &mut selectors,
                    &allow.source,
                    format!("rule {} source", rule.id),
                );
                for allowed in &allow.allowed {
                    push_module_selector(
                        &mut selectors,
                        allowed,
                        format!("rule {} allowed", rule.id),
                    );
                }
            }
            super::policy::RuleKind::Layers(layers) => {
                for member in &layers.order {
                    push_module_selector(&mut selectors, member, format!("rule {} order", rule.id));
                }
            }
            super::policy::RuleKind::Protected(protected) => {
                for target in &protected.targets {
                    push_module_selector(
                        &mut selectors,
                        target,
                        format!("rule {} targets", rule.id),
                    );
                }
                for importer in &protected.allowed_importers {
                    push_module_selector(
                        &mut selectors,
                        importer,
                        format!("rule {} allowed_importers", rule.id),
                    );
                }
            }
            super::policy::RuleKind::Independence(independence) => {
                for member in &independence.members {
                    push_module_selector(
                        &mut selectors,
                        member,
                        format!("rule {} members", rule.id),
                    );
                }
            }
            super::policy::RuleKind::Acyclic(acyclic) => {
                for member in &acyclic.members {
                    push_module_selector(
                        &mut selectors,
                        member,
                        format!("rule {} members", rule.id),
                    );
                }
            }
        }
    }
    for exception in &policy.exceptions {
        push_module_selector(
            &mut selectors,
            &exception.source,
            format!("exception {} source", exception.id),
        );
        push_module_selector(
            &mut selectors,
            &exception.target,
            format!("exception {} target", exception.id),
        );
    }
    selectors
}

fn push_module_selector(
    selectors: &mut Vec<PolicyModuleSelector>,
    selector: &str,
    context: String,
) {
    if selector.contains("::") {
        selectors.push(PolicyModuleSelector {
            path: selector.to_string(),
            context,
        });
    }
}

fn collect_dependency_graphs(
    ctx: &EdgeCollectionContext<'_>,
    timings: &mut TimingBreakdown,
) -> CollectedDependencyGraphs {
    let mut graphs = CollectedDependencyGraphs {
        boundary_graph: BoundaryGraph::new(ctx.declared_boundaries.clone()),
        exact_module_graph: ctx.exact_module_paths.cloned().map(BoundaryGraph::new),
    };

    if ctx.verbose {
        log_info("collect semantic module-scope edges");
    }
    let phase_started = Instant::now();
    for top_level_module in ctx.root.children(ctx.db) {
        let Some(source_boundary) = module_name(top_level_module, ctx.db) else {
            continue;
        };
        if !ctx.declared_boundaries.contains(&source_boundary) {
            continue;
        }
        if ctx.verbose {
            log_info(format!("scan {source_boundary}"));
        }
        collect_module_scope_edges(
            top_level_module,
            &source_boundary,
            ctx.root,
            ctx.db,
            &mut graphs,
        );
    }
    timings.module_scope_scan = phase_started.elapsed();

    if ctx.verbose {
        log_info("resolve qualified crate/self/super paths");
    }
    let phase_started = Instant::now();
    let breakdown = collect_qualified_path_edges(ctx, &mut graphs);
    timings.qualified_path_scan = phase_started.elapsed();
    timings.qualified_path_candidate_file_filtering = breakdown.candidate_file_filtering;
    timings.qualified_path_file_reads = breakdown.file_reads;
    timings.qualified_path_edition_attach = breakdown.edition_attach;
    timings.qualified_path_sema_parse = breakdown.sema_parse;
    timings.qualified_path_module_locator_setup = breakdown.module_locator_setup;
    timings.qualified_path_module_locator_repeat = breakdown.module_locator_repeat;
    timings.qualified_path_use_tree_walk = breakdown.use_tree_walk;
    timings.qualified_path_path_walk = breakdown.path_walk;
    timings.qualified_path_segment_extraction = breakdown.segment_extraction;
    timings.qualified_path_fast_path_resolution = breakdown.fast_path_resolution;
    timings.qualified_path_semantic_fallback_resolution = breakdown.semantic_fallback_resolution;
    timings.qualified_path_source_rs_files = breakdown.source_rs_files;
    timings.qualified_path_candidate_files = breakdown.candidate_files;
    timings.qualified_path_parsed_files = breakdown.parsed_files;
    timings.qualified_path_use_tree_candidates = breakdown.use_tree_candidates;
    timings.qualified_path_path_candidates = breakdown.path_candidates;
    timings.qualified_path_text_hits = breakdown.text_hits;
    timings.qualified_path_duplicate_path_hits = breakdown.duplicate_path_hits;
    timings.qualified_path_token_lookup = breakdown.token_lookup;
    timings.qualified_path_path_ascend = breakdown.path_ascend;
    timings.qualified_path_slowest_files = breakdown.slowest_path_files;
    timings.qualified_path_slowest_module_locator_files = breakdown.slowest_module_locator_files;

    graphs
}

fn collect_module_scope_edges(
    module: Module,
    source_boundary: &str,
    root: Module,
    db: &RootDatabase,
    graphs: &mut CollectedDependencyGraphs,
) {
    let source_selector_path = module_selector_path(module, db);
    let source_module_path = module_path(module, db);

    for (_name, scope_def) in module.scope(db, None) {
        let ScopeDef::ModuleDef(def) = scope_def else {
            continue;
        };
        let Some(target_module) = owning_module(def, db) else {
            continue;
        };
        if target_module.krate() != module.krate() {
            continue;
        }
        if def.module(db) == Some(module) {
            continue;
        }

        let Some(target_boundary) = top_level_boundary(target_module, root, db) else {
            continue;
        };
        let target_selector_path = module_selector_path(target_module, db);
        let symbol = def
            .canonical_path(db, Edition::CURRENT)
            .or_else(|| {
                def.name(db)
                    .map(|name| name.display(db, Edition::CURRENT).to_string())
            })
            .unwrap_or_else(|| module_path(target_module, db));
        let sample = DependencySample {
            source: source_module_path.clone(),
            symbol,
            target: module_path(target_module, db),
            location: None,
        };

        if let Some(exact_graph) = &mut graphs.exact_module_graph
            && !source_selector_path.is_empty()
            && !target_selector_path.is_empty()
            && source_selector_path != target_selector_path
        {
            exact_graph.insert_edge(
                source_selector_path.clone(),
                target_selector_path,
                sample.clone(),
                EdgeProvenance::ModuleScope,
            );
        }

        if target_boundary == source_boundary {
            continue;
        }

        graphs.boundary_graph.insert_edge(
            source_boundary.to_string(),
            target_boundary,
            sample,
            EdgeProvenance::ModuleScope,
        );
    }

    for child in module.children(db) {
        collect_module_scope_edges(child, source_boundary, root, db, graphs);
    }
}

fn collect_qualified_path_edges(
    ctx: &EdgeCollectionContext<'_>,
    graphs: &mut CollectedDependencyGraphs,
) -> QualifiedPathScanBreakdown {
    let src_root = AbsPathBuf::assert_utf8(repo_root().join("src"));
    let boundary_names = discover_top_level_boundaries(ctx.root, ctx.db);
    let root_export_boundaries = root_export_boundaries(ctx.root, ctx.db, ctx.krate);
    let crate_edition = ctx.krate.edition(ctx.db);
    let mut breakdown = QualifiedPathScanBreakdown::default();
    for (file_id, vfs_path) in ctx.vfs.iter() {
        let filter_started = Instant::now();
        let Some(abs_path) = vfs_path.as_path() else {
            breakdown.candidate_file_filtering += filter_started.elapsed();
            continue;
        };
        if !abs_path.starts_with(src_root.as_path()) || abs_path.extension() != Some("rs") {
            breakdown.candidate_file_filtering += filter_started.elapsed();
            continue;
        }
        breakdown.source_rs_files += 1;
        breakdown.candidate_file_filtering += filter_started.elapsed();
        let abs_std_path: &Path = abs_path.as_ref();
        let filter_started = Instant::now();
        let rel_src_path = match abs_std_path.strip_prefix(src_root.as_path()) {
            Ok(path) => path,
            Err(_) => {
                breakdown.candidate_file_filtering += filter_started.elapsed();
                continue;
            }
        };
        let Some(probable_boundary) = probable_source_boundary(rel_src_path) else {
            breakdown.candidate_file_filtering += filter_started.elapsed();
            continue;
        };
        if !ctx.declared_boundaries.contains(probable_boundary) {
            breakdown.candidate_file_filtering += filter_started.elapsed();
            continue;
        }
        breakdown.candidate_file_filtering += filter_started.elapsed();
        let read_started = Instant::now();
        let Ok(file_text) = std::fs::read_to_string(abs_std_path) else {
            breakdown.file_reads += read_started.elapsed();
            continue;
        };
        breakdown.file_reads += read_started.elapsed();
        let filter_started = Instant::now();
        if !contains_relative_path_candidate(&file_text) {
            breakdown.candidate_file_filtering += filter_started.elapsed();
            continue;
        }
        breakdown.candidate_file_filtering += filter_started.elapsed();
        breakdown.candidate_files += 1;
        let attach_started = Instant::now();
        let editioned_file_id = EditionedFileId::new(ctx.db, file_id, crate_edition);
        breakdown.edition_attach += attach_started.elapsed();
        let parse_started = Instant::now();
        let source_file = ctx.sema.parse(editioned_file_id);
        breakdown.sema_parse += parse_started.elapsed();
        let locator_started = Instant::now();
        let module_locator = ModuleLocator::for_file(ctx.sema, ctx.db, file_id, ctx.krate);
        let locator_elapsed = locator_started.elapsed();
        breakdown.module_locator_setup += locator_elapsed;
        breakdown.parsed_files += 1;
        let parsed_ordinal = breakdown.parsed_files;
        if parsed_ordinal == 1 {
            let repeat_started = Instant::now();
            let _ = ModuleLocator::for_file(ctx.sema, ctx.db, file_id, ctx.krate);
            breakdown.module_locator_repeat += repeat_started.elapsed();
        }
        update_slowest_module_locator_files(
            &mut breakdown.slowest_module_locator_files,
            SlowModuleLocatorFile {
                path: display_repo_relative(abs_path.as_ref()),
                parsed_ordinal,
                locator_setup: locator_elapsed,
                module_count: module_locator.modules.len(),
            },
        );

        let walk_started = Instant::now();
        let mut file_use_tree_candidates = 0usize;
        for use_tree in source_file
            .syntax()
            .descendants()
            .filter_map(ast::UseTree::cast)
        {
            if use_tree.use_tree_list().is_some() || use_tree.star_token().is_some() {
                continue;
            }
            let Some(path) = use_tree.path() else {
                continue;
            };
            let segment_started = Instant::now();
            let segments = use_tree_segments(&use_tree);
            breakdown.segment_extraction += segment_started.elapsed();
            if !starts_with_relative_qualifier(&segments) {
                continue;
            }
            breakdown.use_tree_candidates += 1;
            file_use_tree_candidates += 1;
            let resolution_started = Instant::now();
            let resolution = record_relative_path_edge(
                ctx.sema,
                ctx.krate,
                ctx.root,
                ctx.db,
                &boundary_names,
                &root_export_boundaries,
                ctx.declared_boundaries,
                abs_path.as_ref(),
                &module_locator,
                use_tree.syntax(),
                &path,
                &segments,
                &file_text,
                render_relative_path(&segments),
                graphs,
            );
            let elapsed = resolution_started.elapsed();
            match resolution {
                PathResolutionKind::Ignored => {}
                PathResolutionKind::SyntacticFastPath => {
                    breakdown.direct_prefix_hits += 1;
                    breakdown.fast_path_resolution += elapsed;
                }
                PathResolutionKind::SemanticFallback => {
                    breakdown.semantic_fallbacks += 1;
                    breakdown.semantic_fallback_resolution += elapsed;
                }
            }
        }
        breakdown.use_tree_walk += walk_started.elapsed();

        let walk_started = Instant::now();
        let mut file_path_candidates = 0usize;
        let mut file_text_hits = 0usize;
        let mut file_duplicate_path_hits = 0usize;
        let mut file_token_lookup = Duration::ZERO;
        let mut file_path_ascend = Duration::ZERO;
        let mut seen_ranges = BTreeSet::new();
        for match_offset in relative_module_qualifier_offsets(&file_text) {
            breakdown.text_hits += 1;
            file_text_hits += 1;

            let token_lookup_started = Instant::now();
            let Some(mut path) = top_level_path_for_offset(source_file.syntax(), match_offset)
            else {
                let elapsed = token_lookup_started.elapsed();
                breakdown.token_lookup += elapsed;
                file_token_lookup += elapsed;
                continue;
            };
            let token_lookup_elapsed = token_lookup_started.elapsed();
            breakdown.token_lookup += token_lookup_elapsed;
            file_token_lookup += token_lookup_elapsed;

            let ascend_started = Instant::now();
            while let Some(parent) = path.parent_path() {
                path = parent;
            }
            let ascend_elapsed = ascend_started.elapsed();
            breakdown.path_ascend += ascend_elapsed;
            file_path_ascend += ascend_elapsed;

            let range = path.syntax().text_range();
            let range_key = (u32::from(range.start()), u32::from(range.end()));
            if !seen_ranges.insert(range_key) {
                breakdown.duplicate_path_hits += 1;
                file_duplicate_path_hits += 1;
                continue;
            }
            if path
                .syntax()
                .parent()
                .and_then(ast::UseTree::cast)
                .is_some()
            {
                continue;
            }
            let segment_started = Instant::now();
            let segments = path_segments(&path);
            breakdown.segment_extraction += segment_started.elapsed();
            if !starts_with_relative_qualifier(&segments) {
                continue;
            }
            breakdown.path_candidates += 1;
            file_path_candidates += 1;
            let resolution_started = Instant::now();
            let resolution = record_relative_path_edge(
                ctx.sema,
                ctx.krate,
                ctx.root,
                ctx.db,
                &boundary_names,
                &root_export_boundaries,
                ctx.declared_boundaries,
                abs_path.as_ref(),
                &module_locator,
                path.syntax(),
                &path,
                &segments,
                &file_text,
                path.syntax().text().to_string(),
                graphs,
            );
            let elapsed = resolution_started.elapsed();
            match resolution {
                PathResolutionKind::Ignored => {}
                PathResolutionKind::SyntacticFastPath => {
                    breakdown.direct_prefix_hits += 1;
                    breakdown.fast_path_resolution += elapsed;
                }
                PathResolutionKind::SemanticFallback => {
                    breakdown.semantic_fallbacks += 1;
                    breakdown.semantic_fallback_resolution += elapsed;
                }
            }
        }
        let path_walk_elapsed = walk_started.elapsed();
        breakdown.path_walk += path_walk_elapsed;
        update_slowest_path_files(
            &mut breakdown.slowest_path_files,
            SlowPathFile {
                path: display_repo_relative(abs_path.as_ref()),
                parsed_ordinal,
                path_walk: path_walk_elapsed,
                text_hits: file_text_hits,
                duplicate_path_hits: file_duplicate_path_hits,
                use_tree_candidates: file_use_tree_candidates,
                path_candidates: file_path_candidates,
                token_lookup: file_token_lookup,
                path_ascend: file_path_ascend,
            },
        );
    }

    if ctx.verbose {
        log_info(format!(
            "qualified path scan: {} direct boundary prefixes, {} semantic fallbacks",
            breakdown.direct_prefix_hits, breakdown.semantic_fallbacks
        ));
    }

    breakdown
}

fn update_slowest_path_files(files: &mut Vec<SlowPathFile>, candidate: SlowPathFile) {
    const LIMIT: usize = 10;

    files.push(candidate);
    files.sort_by_key(|right| std::cmp::Reverse(right.path_walk));
    files.truncate(LIMIT);
}

fn update_slowest_module_locator_files(
    files: &mut Vec<SlowModuleLocatorFile>,
    candidate: SlowModuleLocatorFile,
) {
    const LIMIT: usize = 10;

    files.push(candidate);
    files.sort_by_key(|right| std::cmp::Reverse(right.locator_setup));
    files.truncate(LIMIT);
}

#[allow(clippy::too_many_arguments)]
fn record_relative_path_edge(
    sema: &Semantics<'_, RootDatabase>,
    krate: HirCrate,
    root: Module,
    db: &RootDatabase,
    boundary_names: &BTreeSet<String>,
    root_export_boundaries: &BTreeMap<String, String>,
    declared_boundaries: &BTreeSet<String>,
    abs_path: &Path,
    module_locator: &ModuleLocator,
    scope_node: &SyntaxNode,
    path: &ast::Path,
    segments: &[RelativePathSegment],
    file_text: &str,
    symbol: String,
    graphs: &mut CollectedDependencyGraphs,
) -> PathResolutionKind {
    let Some(source_module) = module_locator.locate(scope_node) else {
        return PathResolutionKind::Ignored;
    };
    let Some(source_boundary) = top_level_boundary(source_module, root, db) else {
        return PathResolutionKind::Ignored;
    };
    if !declared_boundaries.contains(&source_boundary) {
        return PathResolutionKind::Ignored;
    }
    let source_selector_path = module_selector_path(source_module, db);
    let source_module_segments = module_segments(source_module, db);
    let mut resolved_target_module = if graphs.exact_module_graph.is_some() {
        resolve_target_module(sema, path, db, krate)
    } else {
        None
    };

    let (target_boundary, target_path, resolution_kind) = if let Some(target_boundary) =
        syntactic_target_boundary(
            segments,
            &source_module_segments,
            boundary_names,
            root_export_boundaries,
        ) {
        (
            target_boundary.clone(),
            format!("crate::{target_boundary}"),
            PathResolutionKind::SyntacticFastPath,
        )
    } else {
        let Some(target_module) = resolved_target_module
            .take()
            .or_else(|| resolve_target_module(sema, path, db, krate))
        else {
            return PathResolutionKind::Ignored;
        };
        let Some(target_boundary) = top_level_boundary(target_module, root, db) else {
            return PathResolutionKind::Ignored;
        };
        resolved_target_module = Some(target_module);
        (
            target_boundary,
            module_path(target_module, db),
            PathResolutionKind::SemanticFallback,
        )
    };

    let sample = DependencySample {
        source: display_repo_relative(abs_path),
        symbol,
        target: target_path,
        location: Some(source_location_for_range(
            abs_path,
            file_text,
            path.syntax().text_range(),
        )),
    };

    if let (Some(exact_graph), Some(target_module)) =
        (&mut graphs.exact_module_graph, resolved_target_module)
    {
        let target_selector_path = module_selector_path(target_module, db);
        if !source_selector_path.is_empty()
            && !target_selector_path.is_empty()
            && source_selector_path != target_selector_path
        {
            exact_graph.insert_edge(
                source_selector_path,
                target_selector_path,
                sample.clone(),
                EdgeProvenance::QualifiedPath,
            );
        }
    }

    if target_boundary == source_boundary {
        return resolution_kind;
    }

    graphs.boundary_graph.insert_edge(
        source_boundary,
        target_boundary,
        sample,
        EdgeProvenance::QualifiedPath,
    );

    resolution_kind
}

fn should_replace_dependency_sample(
    existing: &DependencySample,
    candidate: &DependencySample,
) -> bool {
    existing.location.is_none() && candidate.location.is_some()
}

fn source_location_for_range(path: &Path, source: &str, range: TextRange) -> SourceLocation {
    let start = usize::try_from(u32::from(range.start())).expect("range start should fit usize");
    let end = usize::try_from(u32::from(range.end())).expect("range end should fit usize");
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[end..]
        .find('\n')
        .map_or(source.len(), |offset| end + offset);
    let line_number = source[..start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let line_text = source[line_start..line_end].to_string();
    let underline_offset = source[line_start..start].chars().count();
    let underline_len = source[start..end].chars().count().max(1);

    SourceLocation {
        path: display_repo_relative(path),
        line: line_number,
        column: underline_offset + 1,
        line_text,
        underline_offset,
        underline_len,
    }
}

fn resolve_target_module(
    sema: &Semantics<'_, RootDatabase>,
    path: &ast::Path,
    db: &RootDatabase,
    krate: HirCrate,
) -> Option<Module> {
    let PathResolution::Def(def) = sema.resolve_path(path)? else {
        return None;
    };
    let target_module = owning_module(def, db)?;
    (target_module.krate() == krate).then_some(target_module)
}

fn syntactic_target_boundary(
    segments: &[RelativePathSegment],
    source_module_segments: &[String],
    boundary_names: &BTreeSet<String>,
    root_export_boundaries: &BTreeMap<String, String>,
) -> Option<String> {
    let mut current = source_module_segments.to_vec();
    let mut anchored = false;

    for segment in segments {
        match segment {
            RelativePathSegment::CrateKw => {
                current.clear();
                anchored = true;
            }
            RelativePathSegment::SelfKw => anchored = true,
            RelativePathSegment::SuperKw => {
                current.pop();
                anchored = true;
            }
            RelativePathSegment::Name(name) => {
                if !anchored {
                    return None;
                }
                current.push(name.clone());
                if let Some(boundary) = current
                    .first()
                    .filter(|segment| boundary_names.contains(*segment))
                    .cloned()
                {
                    return Some(boundary);
                }
                if current.len() == 1 {
                    return root_export_boundaries.get(name).cloned();
                }
                return None;
            }
            RelativePathSegment::Unsupported => return None,
        }
    }

    None
}

fn top_level_boundary(module: Module, root: Module, db: &RootDatabase) -> Option<String> {
    if module == root {
        return None;
    }

    let mut current = module;
    loop {
        let parent = current.parent(db)?;
        if parent == root {
            return module_name(current, db);
        }
        current = parent;
    }
}

fn module_name(module: Module, db: &RootDatabase) -> Option<String> {
    module
        .name(db)
        .map(|name| name.display(db, Edition::CURRENT).to_string())
}

fn owning_module(def: ModuleDef, db: &RootDatabase) -> Option<Module> {
    match def {
        ModuleDef::Module(module) => Some(module),
        other => other.module(db),
    }
}

fn module_path(module: Module, db: &RootDatabase) -> String {
    let segments = module_segments(module, db);
    if segments.is_empty() {
        "crate".to_string()
    } else {
        format!("crate::{}", segments.join("::"))
    }
}

fn module_selector_path(module: Module, db: &RootDatabase) -> String {
    module_segments(module, db).join("::")
}

fn module_segments(module: Module, db: &RootDatabase) -> Vec<String> {
    let mut segments: Vec<_> = module
        .path_to_root(db)
        .into_iter()
        .filter_map(|module| module_name(module, db))
        .collect();
    segments.reverse();
    segments
}

fn root_export_boundaries(
    root: Module,
    db: &RootDatabase,
    krate: HirCrate,
) -> BTreeMap<String, String> {
    let mut exports = BTreeMap::new();

    for (name, scope_def) in root.scope(db, None) {
        let ScopeDef::ModuleDef(def) = scope_def else {
            continue;
        };
        let Some(target_module) = owning_module(def, db) else {
            continue;
        };
        if target_module.krate() != krate {
            continue;
        }
        let Some(target_boundary) = top_level_boundary(target_module, root, db) else {
            continue;
        };
        exports.insert(
            name.display(db, Edition::CURRENT).to_string(),
            target_boundary,
        );
    }

    exports
}

impl ModuleLocator {
    fn for_file(
        sema: &Semantics<'_, RootDatabase>,
        db: &RootDatabase,
        file_id: ra_ap_vfs::FileId,
        krate: HirCrate,
    ) -> Self {
        let mut modules = sema
            .file_to_module_defs(file_id)
            .filter(|module| module.krate() == krate)
            .filter_map(|module| {
                let range = module.definition_source_range(db);
                let original_file = range
                    .file_id
                    .original_file_respecting_includes(db)
                    .file_id(db);
                (original_file == file_id).then_some(LocatedModule {
                    module,
                    range: range.value,
                })
            })
            .collect::<Vec<_>>();
        modules.sort_by_key(|entry| entry.range.len());
        Self { modules }
    }

    fn locate(&self, node: &SyntaxNode) -> Option<Module> {
        let range = node.text_range();
        self.modules
            .iter()
            .find(|entry| entry.range.contains_range(range))
            .map(|entry| entry.module)
    }
}

fn path_segments(path: &ast::Path) -> Vec<RelativePathSegment> {
    path.segments().map(map_path_segment).collect()
}

fn use_tree_segments(use_tree: &ast::UseTree) -> Vec<RelativePathSegment> {
    let mut trees: Vec<_> = use_tree
        .syntax()
        .ancestors()
        .filter_map(ast::UseTree::cast)
        .collect();
    trees.reverse();

    let mut segments = Vec::new();
    for tree in trees {
        if let Some(path) = tree.path() {
            segments.extend(path_segments(&path));
        }
    }
    segments
}

fn map_path_segment(segment: ast::PathSegment) -> RelativePathSegment {
    match segment.kind() {
        Some(ast::PathSegmentKind::CrateKw) => RelativePathSegment::CrateKw,
        Some(ast::PathSegmentKind::SelfKw) => RelativePathSegment::SelfKw,
        Some(ast::PathSegmentKind::SuperKw) => RelativePathSegment::SuperKw,
        Some(ast::PathSegmentKind::Name(name_ref)) => {
            RelativePathSegment::Name(name_ref.text().to_string())
        }
        _ => RelativePathSegment::Unsupported,
    }
}

fn starts_with_relative_qualifier(segments: &[RelativePathSegment]) -> bool {
    let Some(first) = segments.first() else {
        return false;
    };

    matches!(
        first,
        RelativePathSegment::CrateKw | RelativePathSegment::SelfKw | RelativePathSegment::SuperKw
    )
}

fn render_relative_path(segments: &[RelativePathSegment]) -> String {
    segments
        .iter()
        .map(|segment| match segment {
            RelativePathSegment::CrateKw => "crate".to_string(),
            RelativePathSegment::SelfKw => "self".to_string(),
            RelativePathSegment::SuperKw => "super".to_string(),
            RelativePathSegment::Name(name) => name.clone(),
            RelativePathSegment::Unsupported => "<unsupported>".to_string(),
        })
        .collect::<Vec<_>>()
        .join("::")
}

fn contains_relative_path_candidate(source: &str) -> bool {
    source.contains("crate::") || source.contains("self::") || source.contains("super::")
}

fn probable_source_boundary(rel_src_path: &Path) -> Option<&str> {
    let first = rel_src_path.iter().next()?.to_str()?;
    if let Some(stem) = first.strip_suffix(".rs") {
        return Some(stem);
    }
    Some(first)
}

fn relative_module_qualifier_offsets(source: &str) -> Vec<TextSize> {
    const NEEDLES: [&str; 3] = ["crate::", "self::", "super::"];

    let mut offsets = Vec::new();
    for needle in NEEDLES {
        let mut start = 0usize;
        while let Some(found) = source[start..].find(needle) {
            let offset = start + found;
            start = offset + needle.len();
            if offset > 0 && is_identifier_continue(source.as_bytes()[offset - 1]) {
                continue;
            }
            offsets.push(TextSize::from(offset as u32));
        }
    }
    offsets.sort_unstable();
    offsets
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn top_level_path_for_offset(root: &SyntaxNode, offset: TextSize) -> Option<ast::Path> {
    root.token_at_offset(offset)
        .find_map(|token| token.parent()?.ancestors().find_map(ast::Path::cast))
}

fn render_violation_report(
    policy: &BTreeMap<String, BTreeSet<String>>,
    violations: &[BoundaryViolation],
    actual: &BTreeMap<String, BTreeMap<String, DependencySample>>,
    verbose: bool,
    renderer: &Renderer,
    include_summary: bool,
) -> String {
    let mut output = Vec::new();
    write_violation_report(
        &mut output,
        policy,
        violations,
        actual,
        verbose,
        renderer,
        include_summary,
    )
    .expect("writing a violation report into memory should not fail");
    String::from_utf8(output).expect("violation reports should be valid UTF-8")
}

fn quiet_failure_message(report: &BoundariesRunReport) -> String {
    match (&report.rendered_output[..], report.summary.as_deref()) {
        ("", Some(summary)) => summary.to_string(),
        (body, Some(summary)) => format!("{body}\n{summary}"),
        (body, None) => body.to_string(),
    }
}

fn render_violation(
    violation: &BoundaryViolation,
    allowed_targets: Option<&BTreeSet<String>>,
    renderer: &Renderer,
) -> String {
    let source = &violation.source_boundary;
    let target = &violation.target_boundary;
    let title = format!("forbidden dependency from `{source}` to `{target}`");
    let label = format!("forbidden dependency on `{target}`");

    let mut group = Group::with_title(Level::ERROR.primary_title(title).id("boundaries"));
    if let (Some(file), Some(line), Some(line_text)) =
        (&violation.file, violation.line, &violation.line_text)
    {
        let char_start = violation.underline_offset.unwrap_or(0);
        let char_end = char_start + violation.underline_len.unwrap_or(1);
        let byte_start = char_offset_to_byte_index(line_text, char_start);
        let byte_end = char_offset_to_byte_index(line_text, char_end);
        let min_end = next_char_end_byte_index(line_text, byte_start);
        let span = byte_start..byte_end.max(min_end);
        group = group.element(
            Snippet::source(line_text.clone())
                .line_start(line)
                .path(file.clone())
                .fold(false)
                .annotation(AnnotationKind::Primary.span(span).label(label)),
        );
    } else {
        group = group.element(Level::NOTE.message(format!(
            "observed via {} (`{}`)",
            violation.source_boundary, violation.symbol
        )));
    }

    group = group.element(Level::NOTE.message(format!("imported symbol: `{}`", violation.symbol)));

    // Dispatch help text based on rule type
    match violation.rule_type.as_deref() {
        Some("allow") | None => {
            if let Some(allowed_targets) = allowed_targets {
                let allowed = format_allowed_targets(allowed_targets);
                group = group.element(Level::HELP.message(format!(
                    "allowed top-level dependencies for `{source}`: {allowed}"
                )));
            }
        }
        Some(_) => {
            if let Some(detail) = &violation.detail {
                group = group.element(Level::HELP.message(detail.clone()));
            }
            if let Some(rule_id) = &violation.rule_id {
                let rule_type = violation.rule_type.as_deref().unwrap_or("unknown");
                group =
                    group.element(Level::NOTE.message(format!("rule: {rule_id} ({rule_type})")));
            }
        }
    }

    renderer.render(&[group]).trim_end().to_string()
}

fn write_violation_report<W: Write>(
    writer: &mut W,
    policy: &BTreeMap<String, BTreeSet<String>>,
    violations: &[BoundaryViolation],
    actual: &BTreeMap<String, BTreeMap<String, DependencySample>>,
    verbose: bool,
    renderer: &Renderer,
    include_summary: bool,
) -> std::io::Result<()> {
    for (index, violation) in violations.iter().enumerate() {
        if index > 0 {
            writer.write_all(b"\n")?;
        }
        writer.write_all(
            render_violation(violation, policy.get(&violation.source_boundary), renderer)
                .as_bytes(),
        )?;
    }

    if include_summary {
        writer.write_all(b"\n")?;
        writer.write_all(format_failure_summary(violations.len()).as_bytes())?;
    }

    if verbose {
        if include_summary {
            writer.write_all(b"\n\n")?;
        } else {
            writer.write_all(b"\n")?;
        }
        writer.write_all(b"Actual top-level edges:\n")?;
        for (source, targets) in actual {
            writeln!(
                writer,
                "  {source} -> {:?}",
                targets.keys().collect::<Vec<_>>()
            )?;
        }
    }

    Ok(())
}

fn diagnostic_renderer() -> Renderer {
    let term_width = diagnostic_term_width();
    if styled_diagnostics_enabled() {
        Renderer::styled()
            .decor_style(DecorStyle::Unicode)
            .term_width(term_width)
    } else {
        Renderer::plain()
            .decor_style(DecorStyle::Unicode)
            .term_width(term_width)
    }
}

fn styled_diagnostics_enabled() -> bool {
    std::io::stderr().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("TERM").is_none_or(|term| term != "dumb")
}

fn diagnostic_term_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 60)
        .unwrap_or(140)
}

fn format_allowed_targets(allowed_targets: &BTreeSet<String>) -> String {
    if allowed_targets.is_empty() {
        "<none>".to_string()
    } else {
        allowed_targets
            .iter()
            .map(|target| format!("`{target}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_failure_summary(violation_count: usize) -> String {
    format!(
        "error: architecture boundaries failed with {} violation{}",
        violation_count,
        if violation_count == 1 { "" } else { "s" }
    )
}

fn char_offset_to_byte_index(text: &str, char_offset: usize) -> usize {
    if char_offset == 0 {
        return 0;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .nth(char_offset)
        .unwrap_or(text.len())
}

fn next_char_end_byte_index(text: &str, start: usize) -> usize {
    text.get(start..)
        .and_then(|tail| tail.chars().next())
        .map(|ch| start + ch.len_utf8())
        .unwrap_or(start)
}

fn report_boundary_results(
    allow_map: &BTreeMap<String, BTreeSet<String>>,
    arch_policy: &super::policy::ArchitecturePolicy,
    actual: &BTreeMap<String, BTreeMap<String, DependencySample>>,
) {
    // Report allow rule results per boundary
    for (boundary, allowed) in allow_map {
        let unexpected: Vec<_> = actual
            .get(boundary)
            .into_iter()
            .flat_map(|targets| targets.keys())
            .filter(|target| !allowed.contains(*target))
            .cloned()
            .collect();

        if unexpected.is_empty() {
            eprintln!("[PASS] {boundary}");
        } else {
            eprintln!("[FAIL] {boundary} unexpected: {}", unexpected.join(", "));
        }
    }

    // Report non-allow rules
    for rule in &arch_policy.rules {
        match &rule.rule {
            super::policy::RuleKind::Allow(_) => {} // already reported above
            super::policy::RuleKind::Layers(l) => {
                eprintln!("[INFO] layers rule {:?}: order = {:?}", rule.id, l.order);
            }
            super::policy::RuleKind::Independence(i) => {
                eprintln!(
                    "[INFO] independence rule {:?}: members = {:?}",
                    rule.id, i.members
                );
            }
            super::policy::RuleKind::Protected(p) => {
                eprintln!(
                    "[INFO] protected rule {:?}: targets = {:?}, allowed_importers = {:?}",
                    rule.id, p.targets, p.allowed_importers
                );
            }
            super::policy::RuleKind::Acyclic(a) => {
                eprintln!(
                    "[INFO] acyclic rule {:?}: members = {:?}",
                    rule.id, a.members
                );
            }
        }
    }
}

fn log_info(message: impl AsRef<str>) {
    eprintln!("[I] {}", message.as_ref());
}

fn render_timing_breakdown(timings: &TimingBreakdown) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "[T] config load: {:.2}s",
        timings.config_load.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] rust-analyzer workspace load: {:.2}s",
        timings.workspace_load.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] top-level boundary discovery: {:.2}s",
        timings.boundary_discovery.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] repeated top-level boundary discovery: {:.2}s",
        timings.boundary_discovery_repeat.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] semantic module-scope scan: {:.2}s",
        timings.module_scope_scan.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path scan: {:.2}s",
        timings.qualified_path_scan.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path files: {} src rs, {} candidate, {} parsed",
        timings.qualified_path_source_rs_files,
        timings.qualified_path_candidate_files,
        timings.qualified_path_parsed_files
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path candidates: {} use trees, {} top-level paths",
        timings.qualified_path_use_tree_candidates, timings.qualified_path_path_candidates
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path text hits: {} ({} duplicate top-level path hits)",
        timings.qualified_path_text_hits, timings.qualified_path_duplicate_path_hits
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path candidate-file filtering: {:.2}s",
        timings
            .qualified_path_candidate_file_filtering
            .as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path file reads: {:.2}s",
        timings.qualified_path_file_reads.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path edition attach: {:.2}s",
        timings.qualified_path_edition_attach.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path sema.parse: {:.2}s",
        timings.qualified_path_sema_parse.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path module locator setup: {:.2}s",
        timings.qualified_path_module_locator_setup.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path repeated first module locator: {:.2}s",
        timings.qualified_path_module_locator_repeat.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path use-tree descendant walk (inclusive): {:.2}s",
        timings.qualified_path_use_tree_walk.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path text-seeded lookup (inclusive): {:.2}s",
        timings.qualified_path_path_walk.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path segment extraction: {:.2}s",
        timings.qualified_path_segment_extraction.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path token lookup: {:.2}s",
        timings.qualified_path_token_lookup.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path path ascend: {:.2}s",
        timings.qualified_path_path_ascend.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path syntactic fast-path resolution: {:.2}s",
        timings.qualified_path_fast_path_resolution.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] qualified path semantic fallback resolution: {:.2}s",
        timings
            .qualified_path_semantic_fallback_resolution
            .as_secs_f64()
    )
    .unwrap();
    if !timings.qualified_path_slowest_files.is_empty() {
        writeln!(output, "[T] slowest qualified-path files:").unwrap();
        for file in &timings.qualified_path_slowest_files {
            writeln!(
                output,
                "[T]   {} [parsed #{}]: {:.2}s ({} text hits, {} duplicate hits, {} use trees, {} top-level paths, token lookup {:.2}s, path ascend {:.2}s)",
                file.path,
                file.parsed_ordinal,
                file.path_walk.as_secs_f64(),
                file.text_hits,
                file.duplicate_path_hits,
                file.use_tree_candidates,
                file.path_candidates,
                file.token_lookup.as_secs_f64(),
                file.path_ascend.as_secs_f64()
            )
            .unwrap();
        }
    }
    if !timings
        .qualified_path_slowest_module_locator_files
        .is_empty()
    {
        writeln!(output, "[T] slowest module-locator files:").unwrap();
        for file in &timings.qualified_path_slowest_module_locator_files {
            writeln!(
                output,
                "[T]   {} [parsed #{}]: {:.2}s ({} located modules)",
                file.path,
                file.parsed_ordinal,
                file.locator_setup.as_secs_f64(),
                file.module_count
            )
            .unwrap();
        }
    }
    writeln!(
        output,
        "[T] policy violation analysis: {:.2}s",
        timings.violation_analysis.as_secs_f64()
    )
    .unwrap();
    writeln!(
        output,
        "[T] result reporting: {:.2}s",
        timings.reporting.as_secs_f64()
    )
    .unwrap();
    writeln!(output, "[T] total: {:.2}s", timings.total.as_secs_f64()).unwrap();
    output
}

fn display_repo_relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn repo_root() -> PathBuf {
    crate::repo_root()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use annotate_snippets::Renderer;
    use annotate_snippets::renderer::DecorStyle;

    use super::{
        BoundariesRunReport, BoundaryGraph, BoundaryViolation, DependencySample, EdgeProvenance,
        PendingRefresh, SemanticBoundariesContext, SourceLocation, quiet_failure_message,
        render_violation_report, write_violation_report,
    };

    fn test_violation(source: &str, target: &str, sample: DependencySample) -> BoundaryViolation {
        BoundaryViolation::from_sample(source, target, &sample)
    }

    #[test]
    fn boundary_violation_round_trips_through_json() {
        let violation = BoundaryViolation {
            source_boundary: "diagrams".to_string(),
            target_boundary: "engines".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            file: Some("src/diagrams/flowchart/compiler.rs".to_string()),
            line: Some(5),
            column: Some(5),
            line_text: Some("use crate::EngineAlgorithmId;".to_string()),
            underline_offset: Some(4),
            underline_len: Some(24),
            rule_id: None,
            rule_type: None,
            detail: None,
        };

        let json = serde_json::to_string(&violation).unwrap();
        let decoded: BoundaryViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, violation);
    }

    #[test]
    fn boundary_violation_handles_missing_location() {
        let violation = BoundaryViolation {
            source_boundary: "diagrams".to_string(),
            target_boundary: "engines".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            file: None,
            line: None,
            column: None,
            line_text: None,
            underline_offset: None,
            underline_len: None,
            rule_id: None,
            rule_type: None,
            detail: None,
        };

        let json = serde_json::to_string(&violation).unwrap();
        let decoded: BoundaryViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, violation);
    }

    #[test]
    fn boundary_violation_converts_from_dependency_sample_with_location() {
        let sample = DependencySample {
            source: "diagrams".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            target: "engines".to_string(),
            location: Some(SourceLocation {
                path: "src/diagrams/flowchart/compiler.rs".to_string(),
                line: 5,
                column: 5,
                line_text: "use crate::EngineAlgorithmId;".to_string(),
                underline_offset: 4,
                underline_len: 24,
            }),
        };

        let violation = BoundaryViolation::from_sample("diagrams", "engines", &sample);
        assert_eq!(violation.source_boundary, "diagrams");
        assert_eq!(violation.target_boundary, "engines");
        assert_eq!(
            violation.file,
            Some("src/diagrams/flowchart/compiler.rs".to_string())
        );
        assert_eq!(violation.line, Some(5));
    }

    #[test]
    fn boundary_violation_converts_from_dependency_sample_without_location() {
        let sample = DependencySample {
            source: "diagrams".to_string(),
            symbol: "crate::EngineAlgorithmId".to_string(),
            target: "engines".to_string(),
            location: None,
        };

        let violation = BoundaryViolation::from_sample("diagrams", "engines", &sample);
        assert_eq!(violation.file, None);
        assert_eq!(violation.line, None);
    }

    #[test]
    fn semantic_boundary_context_batches_incremental_source_changes() {
        let mut context = SemanticBoundariesContext::default();

        context.record_changes(&[
            PathBuf::from("src/runtime/mod.rs"),
            PathBuf::from("src/runtime/mod.rs"),
            PathBuf::from("src/graph/mod.rs"),
        ]);

        match context.pending_refresh {
            PendingRefresh::Incremental(paths) => {
                assert_eq!(
                    paths.into_iter().collect::<Vec<_>>(),
                    vec![
                        super::repo_root().join("src/graph/mod.rs"),
                        super::repo_root().join("src/runtime/mod.rs"),
                    ]
                );
            }
            other => panic!("expected incremental refresh, found {other:?}"),
        }
    }

    #[test]
    fn semantic_boundary_context_promotes_workspace_shape_changes_to_full_reload() {
        let mut context = SemanticBoundariesContext::default();

        context.record_changes(&[PathBuf::from("src/runtime/mod.rs")]);
        context.record_changes(&[PathBuf::from("Cargo.toml")]);

        assert!(matches!(
            context.pending_refresh,
            PendingRefresh::FullReload
        ));
    }

    #[test]
    fn semantic_boundary_context_ignores_policy_only_changes() {
        let mut context = SemanticBoundariesContext::default();

        context.record_changes(&[PathBuf::from("boundaries.toml")]);

        assert!(matches!(context.pending_refresh, PendingRefresh::None));
    }

    #[test]
    fn semantic_boundary_context_ignores_generated_target_metadata() {
        let mut context = SemanticBoundariesContext::default();

        context.record_changes(&[PathBuf::from(
            "target/rust-analyzer/metadata/workspace/Cargo.lock",
        )]);

        assert!(matches!(context.pending_refresh, PendingRefresh::None));
    }

    #[test]
    fn violation_report_defaults_to_compiler_style_output() {
        let violations = vec![test_violation(
            "graph",
            "diagrams",
            DependencySample {
                source: "src/graph/direction_policy.rs".to_string(),
                symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                target: "crate::diagrams".to_string(),
                location: Some(SourceLocation {
                    path: "src/graph/direction_policy.rs".to_string(),
                    line: 133,
                    column: 9,
                    line_text: "    use crate::diagrams::flowchart::compile_to_graph;".to_string(),
                    underline_offset: 8,
                    underline_len: 45,
                }),
            },
        )];
        let report = render_violation_report(
            &BTreeMap::from([(
                "graph".to_string(),
                BTreeSet::from(["errors".to_string(), "format".to_string()]),
            )]),
            &violations,
            &BTreeMap::new(),
            false,
            &Renderer::plain()
                .decor_style(DecorStyle::Unicode)
                .term_width(140),
            true,
        );

        assert!(
            report.contains("error[boundaries]: forbidden dependency from `graph` to `diagrams`")
        );
        assert!(report.contains("src/graph/direction_policy.rs:133:9"));
        assert!(report.contains("forbidden dependency on `diagrams`"));
        assert!(report.contains("imported symbol: `crate::diagrams::flowchart::compile_to_graph`"));
        assert!(report.contains("allowed top-level dependencies for `graph`: `errors`, `format`"));
        assert!(report.contains("error: architecture boundaries failed with 1 violation"));
        assert!(!report.contains("Actual top-level edges"));
    }

    #[test]
    fn violation_report_verbose_includes_edge_dump() {
        let violations = vec![test_violation(
            "graph",
            "diagrams",
            DependencySample {
                source: "src/graph/direction_policy.rs".to_string(),
                symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                target: "crate::diagrams".to_string(),
                location: None,
            },
        )];
        let report = render_violation_report(
            &BTreeMap::from([("graph".to_string(), BTreeSet::from(["errors".to_string()]))]),
            &violations,
            &BTreeMap::from([(
                "graph".to_string(),
                BTreeMap::from([(
                    "diagrams".to_string(),
                    DependencySample {
                        source: "src/graph/direction_policy.rs".to_string(),
                        symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                        target: "crate::diagrams".to_string(),
                        location: None,
                    },
                )]),
            )]),
            true,
            &Renderer::plain()
                .decor_style(DecorStyle::Unicode)
                .term_width(140),
            true,
        );

        assert!(report.contains("Actual top-level edges"));
        assert!(report.contains("graph -> [\"diagrams\"]"));
    }

    #[test]
    fn streamed_violation_report_omits_summary_when_requested() {
        let policy = BTreeMap::from([(
            "graph".to_string(),
            BTreeSet::from(["errors".to_string(), "format".to_string()]),
        )]);
        let violations = vec![test_violation(
            "graph",
            "diagrams",
            DependencySample {
                source: "src/graph/direction_policy.rs".to_string(),
                symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                target: "crate::diagrams".to_string(),
                location: Some(SourceLocation {
                    path: "src/graph/direction_policy.rs".to_string(),
                    line: 133,
                    column: 9,
                    line_text: "    use crate::diagrams::flowchart::compile_to_graph;".to_string(),
                    underline_offset: 8,
                    underline_len: 45,
                }),
            },
        )];
        let mut output = Vec::new();

        write_violation_report(
            &mut output,
            &policy,
            &violations,
            &BTreeMap::new(),
            false,
            &Renderer::plain()
                .decor_style(DecorStyle::Unicode)
                .term_width(140),
            false,
        )
        .unwrap();

        let report = String::from_utf8(output).unwrap();
        assert!(
            report.contains("error[boundaries]: forbidden dependency from `graph` to `diagrams`")
        );
        assert!(!report.contains("error: architecture boundaries failed with 1 violation"));
    }

    #[test]
    fn boundaries_run_report_contains_rendered_output_and_summary() {
        let report = BoundariesRunReport {
            success: false,
            rendered_output: "error[boundaries]: forbidden dependency ...".into(),
            summary: Some("error: architecture boundaries failed with 1 violation".into()),
            timings_output: None,
            violations: Vec::new(),
        };

        assert!(!report.success);
        assert!(report.rendered_output.contains("error[boundaries]"));
        assert!(report.summary.as_deref().unwrap().contains("1 violation"));
    }

    #[test]
    fn quiet_and_streaming_paths_share_the_same_report_body() {
        let report = violation_report_fixture();
        assert_eq!(streamed_body(&report), quiet_body(&report));
    }

    #[test]
    fn location_backed_samples_replace_module_only_samples() {
        let boundaries = BTreeSet::from(["graph".to_string(), "diagrams".to_string()]);
        let mut graph = BoundaryGraph::new(boundaries);
        graph.insert_edge(
            "graph".to_string(),
            "diagrams".to_string(),
            DependencySample {
                source: "crate::graph".to_string(),
                symbol: "crate::diagrams".to_string(),
                target: "crate::diagrams".to_string(),
                location: None,
            },
            EdgeProvenance::ModuleScope,
        );
        graph.insert_edge(
            "graph".to_string(),
            "diagrams".to_string(),
            DependencySample {
                source: "src/graph/direction_policy.rs".to_string(),
                symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                target: "crate::diagrams".to_string(),
                location: Some(SourceLocation {
                    path: "src/graph/direction_policy.rs".to_string(),
                    line: 133,
                    column: 9,
                    line_text: "    use crate::diagrams::flowchart::compile_to_graph;".to_string(),
                    underline_offset: 8,
                    underline_len: 45,
                }),
            },
            EdgeProvenance::QualifiedPath,
        );

        let edge = graph.edge("graph", "diagrams").unwrap();
        assert_eq!(edge.sample.source, "src/graph/direction_policy.rs");
        assert!(edge.sample.location.is_some());
        assert_eq!(edge.provenance, EdgeProvenance::Mixed);
    }

    #[test]
    fn boundary_graph_single_pass_retains_provenance() {
        let boundaries = BTreeSet::from(["a".to_string(), "b".to_string()]);
        let mut graph = BoundaryGraph::new(boundaries);
        graph.insert_edge(
            "a".to_string(),
            "b".to_string(),
            DependencySample {
                source: "crate::a".to_string(),
                symbol: "crate::b::Foo".to_string(),
                target: "crate::b".to_string(),
                location: None,
            },
            EdgeProvenance::ModuleScope,
        );
        let edge = graph.edge("a", "b").unwrap();
        assert_eq!(edge.provenance, EdgeProvenance::ModuleScope);
    }

    #[test]
    fn boundary_graph_to_legacy_edge_map_preserves_edge_pairs() {
        let boundaries = BTreeSet::from(["a".to_string(), "b".to_string(), "c".to_string()]);
        let mut graph = BoundaryGraph::new(boundaries);
        graph.insert_edge(
            "a".to_string(),
            "b".to_string(),
            DependencySample {
                source: "crate::a".to_string(),
                symbol: "crate::b::X".to_string(),
                target: "crate::b".to_string(),
                location: None,
            },
            EdgeProvenance::ModuleScope,
        );
        graph.insert_edge(
            "a".to_string(),
            "c".to_string(),
            DependencySample {
                source: "src/a.rs".to_string(),
                symbol: "crate::c::Y".to_string(),
                target: "crate::c".to_string(),
                location: Some(SourceLocation {
                    path: "src/a.rs".to_string(),
                    line: 1,
                    column: 1,
                    line_text: "use crate::c::Y;".to_string(),
                    underline_offset: 4,
                    underline_len: 12,
                }),
            },
            EdgeProvenance::QualifiedPath,
        );

        let legacy = graph.to_legacy_edge_map();
        assert_eq!(legacy.len(), 1); // one source boundary: "a"
        let a_targets = &legacy["a"];
        assert_eq!(a_targets.len(), 2); // two targets: "b" and "c"
        assert!(a_targets.contains_key("b"));
        assert!(a_targets.contains_key("c"));
    }

    fn violation_report_fixture() -> BoundariesRunReport {
        let violations = vec![test_violation(
            "graph",
            "diagrams",
            DependencySample {
                source: "src/graph/direction_policy.rs".to_string(),
                symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                target: "crate::diagrams".to_string(),
                location: Some(SourceLocation {
                    path: "src/graph/direction_policy.rs".to_string(),
                    line: 133,
                    column: 9,
                    line_text: "    use crate::diagrams::flowchart::compile_to_graph;".to_string(),
                    underline_offset: 8,
                    underline_len: 45,
                }),
            },
        )];
        BoundariesRunReport {
            success: false,
            rendered_output: render_violation_report(
                &BTreeMap::from([(
                    "graph".to_string(),
                    BTreeSet::from(["errors".to_string(), "format".to_string()]),
                )]),
                &violations,
                &BTreeMap::new(),
                false,
                &Renderer::plain()
                    .decor_style(DecorStyle::Unicode)
                    .term_width(140),
                false,
            ),
            summary: Some("error: architecture boundaries failed with 1 violation".to_string()),
            timings_output: None,
            violations: Vec::new(),
        }
    }

    fn streamed_body(report: &BoundariesRunReport) -> String {
        report.rendered_output.clone()
    }

    fn quiet_body(report: &BoundariesRunReport) -> String {
        let summary = report.summary.as_deref().unwrap();
        quiet_failure_message(report)
            .strip_suffix(summary)
            .and_then(|body| body.strip_suffix('\n'))
            .unwrap()
            .to_string()
    }
}
