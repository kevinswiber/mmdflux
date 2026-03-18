use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{IsTerminal, Write};
use std::ops::Range;
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

const BOUNDARIES_CONFIG_ENV: &str = "SEMANTIC_BOUNDARIES_CONFIG";
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SemanticBoundariesSuiteOptions {
    pub(crate) timings: bool,
    pub(crate) quiet: bool,
    pub(crate) verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundariesRunReport {
    pub(crate) success: bool,
    pub(crate) rendered_output: String,
    pub(crate) summary: Option<String>,
    pub(crate) timings_output: Option<String>,
    pub(crate) violations: Vec<BoundaryViolation>,
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

#[derive(Debug, Deserialize)]
struct BoundariesConfig {
    #[serde(default = "default_boundaries_config_version")]
    version: u32,
    #[serde(default)]
    modules: BTreeMap<String, BTreeSet<String>>,
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
        }
    }
}

#[derive(Debug, Clone)]
struct DependencySample {
    source: String,
    symbol: String,
    target: String,
    location: Option<SourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceLocation {
    path: String,
    line: usize,
    column: usize,
    line_text: String,
    underline_offset: usize,
    underline_len: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    verbose: bool,
}

pub(crate) fn run_with_context(
    context: &mut SemanticBoundariesContext,
    options: SemanticBoundariesSuiteOptions,
) -> Result<()> {
    let report = run_with_context_report(context, options)?;
    if let Some(timings_output) = &report.timings_output {
        eprint!("{timings_output}");
    }
    if report.success {
        return Ok(());
    }

    if options.quiet {
        anyhow::bail!(quiet_failure_message(&report));
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
) -> Result<BoundariesRunReport> {
    let started = Instant::now();
    let mut timings = TimingBreakdown::default();

    let phase_started = Instant::now();
    let (config_path, boundaries_config) = load_boundaries_config()?;
    timings.config_load = phase_started.elapsed();
    if !options.quiet && options.verbose {
        log_info(format!(
            "load semantic boundaries policy from {}",
            display_repo_relative(&config_path)
        ));
    }
    if boundaries_config.version != 1 {
        anyhow::bail!(
            "unsupported semantic boundaries config version {} in {}",
            boundaries_config.version,
            config_path.display()
        );
    }
    let policy = boundaries_config.modules;

    if !options.quiet && options.verbose {
        log_info(context.workspace_status());
    }
    let phase_started = Instant::now();
    let loaded = context.load_library()?;
    timings.workspace_load = phase_started.elapsed();
    let db = loaded.host.raw_database();
    let sema = Semantics::new(db);
    let root = loaded.krate.root_module();

    let policy_boundaries: BTreeSet<_> = policy.keys().cloned().collect();
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
    let edge_collection = EdgeCollectionContext {
        sema: &sema,
        vfs: &loaded.vfs,
        krate: loaded.krate,
        root,
        db,
        declared_boundaries: &policy_boundaries,
        verbose: !options.quiet && options.verbose,
    };
    let actual = collect_actual_edges(&edge_collection, &mut timings);
    let phase_started = Instant::now();
    let violations = find_policy_violations(&policy, &actual);
    timings.violation_analysis = phase_started.elapsed();
    let phase_started = Instant::now();
    if !options.quiet && options.verbose {
        report_boundary_results(&policy, &actual);
    }
    let report = if violations.is_empty() {
        BoundariesRunReport {
            success: true,
            rendered_output: String::new(),
            summary: None,
            timings_output: None,
            violations: Vec::new(),
        }
    } else {
        let boundary_violations = violations
            .iter()
            .map(|(source, target, sample)| BoundaryViolation::from_sample(source, target, sample))
            .collect();
        BoundariesRunReport {
            success: false,
            rendered_output: render_violation_report(
                &policy,
                &violations,
                &actual,
                options.verbose,
                &diagnostic_renderer(),
                false,
            ),
            summary: Some(format_failure_summary(violations.len())),
            timings_output: None,
            violations: boundary_violations,
        }
    };
    timings.reporting = phase_started.elapsed();
    timings.total = started.elapsed();
    if !options.quiet && options.verbose {
        log_info(format!("finished in {:.2}s", timings.total.as_secs_f64()));
    }

    Ok(BoundariesRunReport {
        timings_output: options.timings.then(|| render_timing_breakdown(&timings)),
        ..report
    })
}

fn default_boundaries_config_version() -> u32 {
    1
}

fn load_boundaries_config() -> Result<(PathBuf, BoundariesConfig)> {
    let path = resolve_boundaries_config_path();
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok((path, config))
}

fn resolve_boundaries_config_path() -> PathBuf {
    std::env::var_os(BOUNDARIES_CONFIG_ENV)
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_root().join(path)
            }
        })
        .unwrap_or_else(|| repo_root().join("boundaries.toml"))
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

fn collect_actual_edges(
    ctx: &EdgeCollectionContext<'_>,
    timings: &mut TimingBreakdown,
) -> BTreeMap<String, BTreeMap<String, DependencySample>> {
    let mut edges = BTreeMap::<String, BTreeMap<String, DependencySample>>::new();

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
            &mut edges,
        );
    }
    timings.module_scope_scan = phase_started.elapsed();

    if ctx.verbose {
        log_info("resolve qualified crate/self/super paths");
    }
    let phase_started = Instant::now();
    let breakdown = collect_qualified_path_edges(ctx, &mut edges);
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

    edges
}

fn collect_module_scope_edges(
    module: Module,
    source_boundary: &str,
    root: Module,
    db: &RootDatabase,
    edges: &mut BTreeMap<String, BTreeMap<String, DependencySample>>,
) {
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
        if target_boundary == source_boundary {
            continue;
        }

        let symbol = def
            .canonical_path(db, Edition::CURRENT)
            .or_else(|| {
                def.name(db)
                    .map(|name| name.display(db, Edition::CURRENT).to_string())
            })
            .unwrap_or_else(|| module_path(target_module, db));

        insert_dependency_sample(
            edges,
            source_boundary.to_string(),
            target_boundary.clone(),
            DependencySample {
                source: source_module_path.clone(),
                symbol,
                target: module_path(target_module, db),
                location: None,
            },
        );
    }

    for child in module.children(db) {
        collect_module_scope_edges(child, source_boundary, root, db, edges);
    }
}

fn collect_qualified_path_edges(
    ctx: &EdgeCollectionContext<'_>,
    edges: &mut BTreeMap<String, BTreeMap<String, DependencySample>>,
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
                edges,
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
                edges,
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
    files.sort_by(|left, right| right.path_walk.cmp(&left.path_walk));
    files.truncate(LIMIT);
}

fn update_slowest_module_locator_files(
    files: &mut Vec<SlowModuleLocatorFile>,
    candidate: SlowModuleLocatorFile,
) {
    const LIMIT: usize = 10;

    files.push(candidate);
    files.sort_by(|left, right| right.locator_setup.cmp(&left.locator_setup));
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
    edges: &mut BTreeMap<String, BTreeMap<String, DependencySample>>,
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
    let source_module_segments = module_segments(source_module, db);

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
        let Some(target_module) = resolve_target_module(sema, path, db, krate) else {
            return PathResolutionKind::Ignored;
        };
        let Some(target_boundary) = top_level_boundary(target_module, root, db) else {
            return PathResolutionKind::Ignored;
        };
        (
            target_boundary,
            module_path(target_module, db),
            PathResolutionKind::SemanticFallback,
        )
    };

    if target_boundary == source_boundary {
        return resolution_kind;
    }

    insert_dependency_sample(
        edges,
        source_boundary,
        target_boundary,
        DependencySample {
            source: display_repo_relative(abs_path),
            symbol,
            target: target_path,
            location: Some(source_location_for_range(
                abs_path,
                file_text,
                path.syntax().text_range(),
            )),
        },
    );

    resolution_kind
}

fn insert_dependency_sample(
    edges: &mut BTreeMap<String, BTreeMap<String, DependencySample>>,
    source_boundary: String,
    target_boundary: String,
    sample: DependencySample,
) {
    let target_entry = edges.entry(source_boundary).or_default();
    match target_entry.entry(target_boundary) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(sample);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if should_replace_dependency_sample(entry.get(), &sample) {
                entry.insert(sample);
            }
        }
    }
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

fn find_policy_violations(
    policy: &BTreeMap<String, BTreeSet<String>>,
    actual: &BTreeMap<String, BTreeMap<String, DependencySample>>,
) -> Vec<(String, String, DependencySample)> {
    let mut violations = Vec::new();

    for (source, targets) in actual {
        let allowed = policy.get(source);
        for (target, sample) in targets {
            if !allowed.is_some_and(|deps| deps.contains(target)) {
                violations.push((source.clone(), target.clone(), sample.clone()));
            }
        }
    }

    violations.sort_by(|left, right| {
        compare_dependency_samples(&left.2, &right.2)
            .then(left.0.cmp(&right.0))
            .then(left.1.cmp(&right.1))
    });
    violations
}

fn compare_dependency_samples(
    left: &DependencySample,
    right: &DependencySample,
) -> std::cmp::Ordering {
    match (&left.location, &right.location) {
        (Some(left), Some(right)) => left
            .path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.column.cmp(&right.column)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left
            .source
            .cmp(&right.source)
            .then(left.symbol.cmp(&right.symbol))
            .then(left.target.cmp(&right.target)),
    }
}

fn render_violation_report(
    policy: &BTreeMap<String, BTreeSet<String>>,
    violations: &[(String, String, DependencySample)],
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
    source: &str,
    target: &str,
    sample: &DependencySample,
    allowed_targets: Option<&BTreeSet<String>>,
    renderer: &Renderer,
) -> String {
    let title = format!("forbidden dependency from `{source}` to `{target}`");
    let label = format!("forbidden dependency on `{target}`");

    let mut group = Group::with_title(Level::ERROR.primary_title(title).id("boundaries"));
    if let Some(location) = &sample.location {
        let span = line_span(location);
        group = group.element(
            Snippet::source(location.line_text.clone())
                .line_start(location.line)
                .path(location.path.clone())
                .fold(false)
                .annotation(AnnotationKind::Primary.span(span).label(label)),
        );
    } else {
        group = group.element(Level::NOTE.message(format!(
            "observed via {} (`{}` -> {})",
            sample.source, sample.symbol, sample.target
        )));
    }

    group = group.element(Level::NOTE.message(format!("imported symbol: `{}`", sample.symbol)));
    if let Some(allowed_targets) = allowed_targets {
        let allowed = format_allowed_targets(allowed_targets);
        group = group.element(Level::HELP.message(format!(
            "allowed top-level dependencies for `{source}`: {allowed}"
        )));
    }

    renderer.render(&[group]).trim_end().to_string()
}

fn write_violation_report<W: Write>(
    writer: &mut W,
    policy: &BTreeMap<String, BTreeSet<String>>,
    violations: &[(String, String, DependencySample)],
    actual: &BTreeMap<String, BTreeMap<String, DependencySample>>,
    verbose: bool,
    renderer: &Renderer,
    include_summary: bool,
) -> std::io::Result<()> {
    for (index, (source, target, sample)) in violations.iter().enumerate() {
        if index > 0 {
            writer.write_all(b"\n")?;
        }
        writer.write_all(
            render_violation(source, target, sample, policy.get(source), renderer).as_bytes(),
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

fn line_span(location: &SourceLocation) -> Range<usize> {
    let start = char_offset_to_byte_index(&location.line_text, location.underline_offset);
    let end = char_offset_to_byte_index(
        &location.line_text,
        location.underline_offset + location.underline_len,
    );
    let min_end = next_char_end_byte_index(&location.line_text, start);
    start..end.max(min_end)
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
    policy: &BTreeMap<String, BTreeSet<String>>,
    actual: &BTreeMap<String, BTreeMap<String, DependencySample>>,
) {
    for (boundary, allowed) in policy {
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should live at the workspace root")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use annotate_snippets::Renderer;
    use annotate_snippets::renderer::DecorStyle;

    use super::{
        BoundariesRunReport, BoundaryViolation, DependencySample, PendingRefresh,
        SemanticBoundariesContext, SourceLocation, insert_dependency_sample, quiet_failure_message,
        render_violation_report, write_violation_report,
    };

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
        let report = render_violation_report(
            &BTreeMap::from([(
                "graph".to_string(),
                BTreeSet::from(["errors".to_string(), "format".to_string()]),
            )]),
            &[(
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
                        line_text: "    use crate::diagrams::flowchart::compile_to_graph;"
                            .to_string(),
                        underline_offset: 8,
                        underline_len: 45,
                    }),
                },
            )],
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
        let report = render_violation_report(
            &BTreeMap::from([("graph".to_string(), BTreeSet::from(["errors".to_string()]))]),
            &[(
                "graph".to_string(),
                "diagrams".to_string(),
                DependencySample {
                    source: "src/graph/direction_policy.rs".to_string(),
                    symbol: "crate::diagrams::flowchart::compile_to_graph".to_string(),
                    target: "crate::diagrams".to_string(),
                    location: None,
                },
            )],
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
        let violations = vec![(
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
        let mut edges = BTreeMap::new();
        insert_dependency_sample(
            &mut edges,
            "graph".to_string(),
            "diagrams".to_string(),
            DependencySample {
                source: "crate::graph".to_string(),
                symbol: "crate::diagrams".to_string(),
                target: "crate::diagrams".to_string(),
                location: None,
            },
        );
        insert_dependency_sample(
            &mut edges,
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
        );

        let sample = edges
            .get("graph")
            .and_then(|targets| targets.get("diagrams"))
            .unwrap();
        assert_eq!(sample.source, "src/graph/direction_policy.rs");
        assert!(sample.location.is_some());
    }

    fn violation_report_fixture() -> BoundariesRunReport {
        BoundariesRunReport {
            success: false,
            rendered_output: render_violation_report(
                &BTreeMap::from([(
                    "graph".to_string(),
                    BTreeSet::from(["errors".to_string(), "format".to_string()]),
                )]),
                &[(
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
                            line_text: "    use crate::diagrams::flowchart::compile_to_graph;"
                                .to_string(),
                            underline_offset: 8,
                            underline_len: 45,
                        }),
                    },
                )],
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
