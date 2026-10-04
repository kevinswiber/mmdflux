//! Fit text output to a terminal width budget.
//!
//! A fitted render tries a fixed ladder of layout levers, in order, and keeps
//! the first drawing that fits the budget without breaking anything the
//! as-authored drawing showed (labels, arrowheads, rank order, node
//! separation, label placement). When nothing fits it returns the narrowest
//! valid drawing and says so in the [`FitReport`].
//!
//! Only text and ASCII output are fitted. Other formats render exactly as the
//! non-fitted entry points do, with [`FitOutcome::NotApplied`].

use std::fmt;

use serde::{Deserialize, Serialize};

use super::config::RenderConfig;
use super::graph_family::{TextSolve, render_graph_family_text, solve_graph_family_text};
use super::mmds::{prepare_text_replay, render_text_replay};
use super::{PreparedInput, payload, prepare_input, validate_render_config};
use crate::errors::RenderError;
use crate::format::OutputFormat;
use crate::graph::fit_levers::{
    elide_class_members, flip_graph_direction, strip_horizontal_subgraph_overrides,
    transpose_direction, truncate_labels, wrap_class_members, wrap_edge_labels, wrap_node_labels,
};
use crate::graph::grid::{GridGaps, GridSpacingOverrides};
use crate::graph::{Direction, Graph};
use crate::payload::Diagram;
use crate::render::graph::GraphTextDrawing;
use crate::render::graph::text::audit::DrawingAudit;
use crate::render::text::{CellExtent, CharSet};
use crate::render::timeline;
use crate::timeline::sequence::layout as sequence_layout;

/// What a fitted render may do to meet a budget.
///
/// Build with [`FitOptions::max_width`] (or [`Default`] for no budget) and the
/// `with_*` setters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FitOptions {
    /// Width budget in terminal cells; must be at least 1 when set.
    pub max_width: Option<usize>,
    /// Height budget in rows; must be at least 1 when set. No lever targets
    /// height yet: it only decides whether a candidate fits.
    pub max_height: Option<usize>,
    /// Whether the fit may change the layout direction.
    pub direction_change: DirectionChange,
    /// Whether the fit may truncate labels and elide class members.
    pub truncation: bool,
}

impl FitOptions {
    /// Options with a width budget of `cols` terminal cells.
    pub fn max_width(cols: usize) -> Self {
        Self {
            max_width: Some(cols),
            ..Self::default()
        }
    }

    /// Set a height budget of `rows`.
    #[must_use]
    pub fn with_max_height(mut self, rows: usize) -> Self {
        self.max_height = Some(rows);
        self
    }

    /// Set whether the fit may change the layout direction.
    #[must_use]
    pub fn with_direction_change(mut self, policy: DirectionChange) -> Self {
        self.direction_change = policy;
        self
    }

    /// Enable or disable the lossy levers (label truncation, member elision).
    #[must_use]
    pub fn with_truncation(mut self, enabled: bool) -> Self {
        self.truncation = enabled;
        self
    }

    pub(in crate::runtime) fn validate(&self) -> Result<(), RenderError> {
        if self.max_width == Some(0) {
            return Err(RenderError {
                message: "max width must be at least 1".to_string(),
            });
        }
        if self.max_height == Some(0) {
            return Err(RenderError {
                message: "max height must be at least 1".to_string(),
            });
        }
        Ok(())
    }
}

/// Whether a fit may change the layout direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum DirectionChange {
    /// Try the transposed direction as a late, validated rung.
    #[default]
    Allow,
    /// Never change the authored direction.
    Keep,
}

/// A fitted render: the output and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Fitted {
    pub output: String,
    pub report: FitReport,
}

/// How a fitted render was chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FitReport {
    /// The budget asked for.
    pub budget: FitBudget,
    pub outcome: FitOutcome,
    /// Size of the output; `None` only when the fit was not applied.
    pub size: Option<CellSize>,
    /// Size of the as-authored drawing; `None` only when not applied.
    pub as_authored: Option<CellSize>,
    /// Levers applied to the output, in canonical order.
    pub applied: Vec<FitLever>,
    /// Authored layout direction; `None` for sequence diagrams and when not
    /// applied.
    pub authored_direction: Option<Direction>,
    /// Direction actually drawn; same `None` rule as `authored_direction`.
    pub direction: Option<Direction>,
    /// Width budgets that give this same output; `None` when a height budget
    /// was given or the fit was not applied.
    pub stable_for: Option<StableRange>,
    pub lever_scope: FitLeverScope,
    /// Renders performed.
    pub attempts: u32,
    /// Engine solves performed.
    pub solves: u32,
    /// Every evaluated candidate, in ladder order.
    #[serde(skip)]
    pub trail: Vec<FitAttempt>,
}

/// A width and height budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FitBudget {
    pub max_width: Option<usize>,
    pub max_height: Option<usize>,
}

/// A drawing size in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct CellSize {
    pub width: usize,
    pub height: usize,
}

/// The range of width budgets that select the same output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StableRange {
    pub min_width: usize,
    /// `None` when every wider budget gives the same output.
    pub max_width: Option<usize>,
}

/// How a fitted render ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum FitOutcome {
    /// The as-authored drawing fits.
    AsAuthored,
    /// A compacted drawing fits.
    Fitted,
    /// Nothing fits; the output is the narrowest valid drawing.
    BestAttempt,
    /// The format is not fitted (SVG, MMDS, Mermaid).
    NotApplied,
}

/// Which levers were available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum FitLeverScope {
    /// Every lever, including re-solves and direction changes.
    Full,
    /// Only render-time spacing levers (replayed MMDS geometry).
    RenderOnly,
    /// No levers (sequence diagrams, non-text formats).
    Unavailable,
}

/// One evaluated candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FitAttempt {
    pub levers: Vec<FitLever>,
    pub size: CellSize,
    /// Whether the drawing kept everything the as-authored drawing showed.
    pub valid: bool,
    pub fits: bool,
}

/// A layout lever a fit applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "lever", rename_all = "camelCase")]
#[non_exhaustive]
pub enum FitLever {
    /// The layout direction was transposed.
    Direction { from: Direction, to: Direction },
    /// Horizontal subgraph direction overrides were dropped.
    SubgraphDirectionStrip,
    /// Rank gaps were sized from the labels in each gap.
    LabelAwareSpacing,
    /// Rank and node gaps were set in cells.
    GridGap {
        #[serde(rename = "rankGap")]
        rank_gap: usize,
        #[serde(rename = "nodeGap")]
        node_gap: usize,
    },
    /// Edge labels were wrapped at this many cells.
    EdgeLabelWrap { cells: usize },
    /// Node labels were wrapped at this many cells.
    NodeLabelWrap { cells: usize },
    /// Class members were wrapped at this many cells.
    MemberWrap { cells: usize },
    /// Labels were truncated to this many cells.
    LabelTruncation { cells: usize },
    /// Class member sections were cut to this many members.
    MemberElision { keep: usize },
}

impl fmt::Display for FitLever {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direction { from, to } => {
                write!(
                    f,
                    "direction {}→{}",
                    direction_code(*from),
                    direction_code(*to)
                )
            }
            Self::SubgraphDirectionStrip => f.write_str("subgraph directions relaxed"),
            Self::LabelAwareSpacing => f.write_str("label-aware spacing"),
            Self::GridGap { rank_gap, node_gap } => {
                write!(f, "gaps rank {rank_gap} node {node_gap}")
            }
            Self::EdgeLabelWrap { cells } => write!(f, "edge labels wrapped at {cells}"),
            Self::NodeLabelWrap { cells } => write!(f, "node labels wrapped at {cells}"),
            Self::MemberWrap { cells } => write!(f, "class members wrapped at {cells}"),
            Self::LabelTruncation { cells } => write!(f, "labels truncated to {cells}"),
            Self::MemberElision { keep } => write!(f, "class members elided to {keep}"),
        }
    }
}

fn direction_code(direction: Direction) -> &'static str {
    match direction {
        Direction::TopDown => "TD",
        Direction::BottomTop => "BT",
        Direction::LeftRight => "LR",
        Direction::RightLeft => "RL",
    }
}

impl From<CellExtent> for CellSize {
    fn from(extent: CellExtent) -> Self {
        Self {
            width: extent.width,
            height: extent.height,
        }
    }
}

// ---------------------------------------------------------------------------
// Ladder
// ---------------------------------------------------------------------------

/// The settings one candidate applies, cumulative over its rung sequence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::runtime) struct FitSettings {
    pub(in crate::runtime) flip: bool,
    pub(in crate::runtime) strip_horizontal_subgraph_overrides: bool,
    pub(in crate::runtime) label_aware_spacing: bool,
    pub(in crate::runtime) grid_gaps: Option<GridGaps>,
    pub(in crate::runtime) edge_label_wrap: Option<usize>,
    pub(in crate::runtime) node_label_wrap: Option<usize>,
    pub(in crate::runtime) member_wrap: Option<usize>,
    pub(in crate::runtime) truncate: Option<usize>,
    pub(in crate::runtime) member_keep: Option<usize>,
}

impl FitSettings {
    /// The levers these settings apply, in canonical order.
    pub(in crate::runtime) fn levers(&self, authored: Direction) -> Vec<FitLever> {
        let mut levers = Vec::new();
        if self.flip {
            levers.push(FitLever::Direction {
                from: authored,
                to: transpose_direction(authored),
            });
        }
        if self.strip_horizontal_subgraph_overrides {
            levers.push(FitLever::SubgraphDirectionStrip);
        }
        if self.label_aware_spacing {
            levers.push(FitLever::LabelAwareSpacing);
        }
        if let Some(gaps) = self.grid_gaps {
            levers.push(FitLever::GridGap {
                rank_gap: gaps.rank_gap,
                node_gap: gaps.node_gap,
            });
        }
        if let Some(cells) = self.edge_label_wrap {
            levers.push(FitLever::EdgeLabelWrap { cells });
        }
        if let Some(cells) = self.node_label_wrap {
            levers.push(FitLever::NodeLabelWrap { cells });
        }
        if let Some(cells) = self.member_wrap {
            levers.push(FitLever::MemberWrap { cells });
        }
        if let Some(cells) = self.truncate {
            levers.push(FitLever::LabelTruncation { cells });
        }
        if let Some(keep) = self.member_keep {
            levers.push(FitLever::MemberElision { keep });
        }
        levers
    }

    /// The grid spacing overrides these settings render with.
    pub(in crate::runtime) fn grid_spacing(&self) -> GridSpacingOverrides {
        GridSpacingOverrides {
            label_aware: self.label_aware_spacing,
            gaps: self.grid_gaps,
        }
    }
}

/// One candidate in the ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct Rung {
    pub(in crate::runtime) settings: FitSettings,
    /// Index (in the ladder) of the rung whose solve this rung reuses, or
    /// `None` when it needs its own solve.
    pub(in crate::runtime) reuses_solve_of: Option<usize>,
}

/// Apply a candidate's graph transforms in the fixed order: flip, strip,
/// truncate, elide, member wrap, node wrap, edge wrap. Returns whether any
/// changed the graph.
pub(in crate::runtime) fn apply_graph_transforms(
    graph: &mut Graph,
    settings: &FitSettings,
) -> bool {
    let mut changed = false;
    if settings.flip {
        changed |= flip_graph_direction(graph);
    }
    if settings.strip_horizontal_subgraph_overrides {
        changed |= strip_horizontal_subgraph_overrides(graph);
    }
    if let Some(cells) = settings.truncate {
        changed |= truncate_labels(graph, cells);
    }
    if let Some(keep) = settings.member_keep {
        changed |= elide_class_members(graph, keep);
    }
    if let Some(cells) = settings.member_wrap {
        changed |= wrap_class_members(graph, cells);
    }
    if let Some(cells) = settings.node_label_wrap {
        changed |= wrap_node_labels(graph, cells);
    }
    if let Some(cells) = settings.edge_label_wrap {
        changed |= wrap_edge_labels(graph, cells);
    }
    changed
}

/// Rung kinds: render-only rungs reuse the solve of their sequence's base.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RungKind {
    Solve,
    RenderOnly,
}

/// Adds one rung's settings to the cumulative settings before it.
type RungStep = fn(&mut FitSettings);

/// The unflipped rung deltas U1..U9, each cumulative over the previous.
fn unflipped_steps() -> Vec<(RungKind, RungStep)> {
    vec![
        (RungKind::RenderOnly, |s| s.label_aware_spacing = true),
        (RungKind::RenderOnly, |s| {
            s.grid_gaps = Some(GridGaps {
                rank_gap: 2,
                node_gap: 2,
            })
        }),
        (RungKind::RenderOnly, |s| {
            s.grid_gaps = Some(GridGaps {
                rank_gap: 2,
                node_gap: 1,
            })
        }),
        (RungKind::Solve, |s| s.edge_label_wrap = Some(16)),
        (RungKind::Solve, |s| s.edge_label_wrap = Some(10)),
        (RungKind::Solve, |s| s.node_label_wrap = Some(24)),
        (RungKind::Solve, |s| s.node_label_wrap = Some(16)),
        (RungKind::Solve, |s| s.node_label_wrap = Some(10)),
        (RungKind::Solve, |s| s.member_wrap = Some(16)),
    ]
}

/// What the transforms of `settings` make of the pristine graph, for
/// detecting rungs that change nothing.
#[derive(PartialEq)]
struct GraphSignature {
    direction: Direction,
    subgraph_dirs: Vec<(String, Option<Direction>)>,
    node_labels: Vec<(String, String)>,
    edge_labels: Vec<(Option<String>, Option<Vec<String>>)>,
}

fn graph_signature(pristine: &Graph, settings: &FitSettings) -> GraphSignature {
    let mut graph = pristine.clone();
    apply_graph_transforms(&mut graph, settings);
    let mut subgraph_dirs: Vec<(String, Option<Direction>)> = graph
        .subgraphs
        .iter()
        .map(|(id, sg)| (id.clone(), sg.dir))
        .collect();
    subgraph_dirs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut node_labels: Vec<(String, String)> = graph
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node.label.clone()))
        .collect();
    node_labels.sort();
    GraphSignature {
        direction: graph.direction,
        subgraph_dirs,
        node_labels,
        edge_labels: graph
            .edges
            .iter()
            .map(|edge| (edge.label.clone(), edge.wrapped_label_lines.clone()))
            .collect(),
    }
}

/// Builds a ladder, dropping re-solve rungs that change nothing relative to
/// the rung they extend.
struct LadderBuilder<'a> {
    pristine: &'a Graph,
    rungs: Vec<Rung>,
}

impl LadderBuilder<'_> {
    /// Push a solve rung unless its graph equals `extends`'s; returns the
    /// rung's index when kept.
    fn push_solve(
        &mut self,
        settings: FitSettings,
        extends: Option<&FitSettings>,
    ) -> Option<usize> {
        if let Some(base) = extends
            && graph_signature(self.pristine, &settings) == graph_signature(self.pristine, base)
        {
            return None;
        }
        self.rungs.push(Rung {
            settings,
            reuses_solve_of: None,
        });
        Some(self.rungs.len() - 1)
    }

    fn push_render_only(&mut self, settings: FitSettings, base: usize) {
        self.rungs.push(Rung {
            settings,
            reuses_solve_of: Some(base),
        });
    }

    /// Push one rung sequence (U or F) starting from `start`; returns the
    /// settings of its last kept step.
    fn push_sequence(&mut self, start: FitSettings) -> FitSettings {
        let base = self
            .push_solve(start.clone(), None)
            .expect("a sequence base is never skipped");
        let mut settings = start;
        for (kind, step) in unflipped_steps() {
            let previous = settings.clone();
            step(&mut settings);
            match kind {
                RungKind::RenderOnly => self.push_render_only(settings.clone(), base),
                RungKind::Solve => {
                    // A skipped rung's lever changed nothing, so later rungs
                    // do not carry (or report) it.
                    if self.push_solve(settings.clone(), Some(&previous)).is_none() {
                        settings = previous;
                    }
                }
            }
        }
        settings
    }
}

/// The graph-family ladder: U0..U10, F0..F9 with direction changes
/// allowed, then the lossy T rungs when truncation is enabled.
pub(in crate::runtime) fn build_graph_ladder(
    _diagram_id: &str,
    pristine: &Graph,
    fit: &FitOptions,
) -> Vec<Rung> {
    let allow_flip = fit.direction_change == DirectionChange::Allow;
    let mut builder = LadderBuilder {
        pristine,
        rungs: Vec::new(),
    };

    let u9 = builder.push_sequence(FitSettings::default());
    let mut u_last = u9.clone();
    if allow_flip {
        let u10 = FitSettings {
            strip_horizontal_subgraph_overrides: true,
            ..u9.clone()
        };
        if builder.push_solve(u10.clone(), Some(&u9)).is_some() {
            u_last = u10;
        }
    }

    let f9 = allow_flip.then(|| {
        builder.push_sequence(FitSettings {
            flip: true,
            ..FitSettings::default()
        })
    });

    if fit.truncation {
        let mut truncated_8: Vec<FitSettings> = Vec::new();
        for cells in [20, 12, 8] {
            for base in std::iter::once(&u_last).chain(f9.as_ref()) {
                let settings = FitSettings {
                    truncate: Some(cells),
                    ..base.clone()
                };
                let kept = builder.push_solve(settings.clone(), Some(base)).is_some();
                if cells == 8 {
                    truncated_8.push(if kept { settings } else { base.clone() });
                }
            }
        }
        for base in &truncated_8 {
            let settings = FitSettings {
                member_keep: Some(3),
                ..base.clone()
            };
            builder.push_solve(settings, Some(base));
        }
    }

    builder.rungs
}

/// The replay ladder: U0..U3, render-only rungs over the stored geometry.
pub(in crate::runtime) fn build_replay_ladder() -> Vec<Rung> {
    let mut rungs = vec![Rung {
        settings: FitSettings::default(),
        reuses_solve_of: None,
    }];
    let mut settings = FitSettings::default();
    for (kind, step) in unflipped_steps() {
        if kind != RungKind::RenderOnly {
            break;
        }
        step(&mut settings);
        rungs.push(Rung {
            settings: settings.clone(),
            reuses_solve_of: Some(0),
        });
    }
    rungs
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// One rendered candidate.
struct Evaluated {
    drawing: GraphTextDrawing,
    direction: Option<Direction>,
}

/// The search result, including the chosen settings for internal checks.
pub(in crate::runtime) struct SearchResult {
    pub(in crate::runtime) fitted: Fitted,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::runtime) chosen: FitSettings,
}

fn fits_budget(extent: CellExtent, fit: &FitOptions) -> bool {
    fit.max_width.is_none_or(|w| extent.width <= w)
        && fit.max_height.is_none_or(|h| extent.height <= h)
}

fn overflow(extent: CellExtent, fit: &FitOptions) -> (usize, usize) {
    (
        fit.max_width.map_or(0, |w| extent.width.saturating_sub(w)),
        fit.max_height
            .map_or(0, |h| extent.height.saturating_sub(h)),
    )
}

fn budget_of(fit: &FitOptions) -> FitBudget {
    FitBudget {
        max_width: fit.max_width,
        max_height: fit.max_height,
    }
}

/// Run the first-fit search over `rungs`, rendering each with `evaluate`.
/// The first rung is the as-authored reference for validity.
fn search(
    rungs: &[Rung],
    fit: &FitOptions,
    authored_direction: Option<Direction>,
    lever_scope: FitLeverScope,
    mut evaluate: impl FnMut(usize, &Rung) -> Result<Evaluated, RenderError>,
    solves: impl Fn() -> u32,
) -> Result<SearchResult, RenderError> {
    let levers_of = |settings: &FitSettings| {
        authored_direction
            .map(|direction| settings.levers(direction))
            .unwrap_or_default()
    };
    let mut trail: Vec<FitAttempt> = Vec::new();
    let mut candidates: Vec<(usize, Evaluated, bool)> = Vec::new();
    let mut reference: Option<DrawingAudit> = None;
    let mut chosen: Option<usize> = None;

    for (index, rung) in rungs.iter().enumerate() {
        let evaluated = evaluate(index, rung)?;
        let audit = evaluated.drawing.audit.clone().unwrap_or_default();
        let valid = match &reference {
            None => {
                reference = Some(audit);
                true
            }
            Some(authored) => audit.regressions_against(authored).is_empty(),
        };
        let fits = valid && fits_budget(evaluated.drawing.extent, fit);
        let levers = levers_of(&rung.settings);
        tracing::debug!(
            event = "fit_attempt",
            attempt = index + 1,
            levers = ?levers,
            width = evaluated.drawing.extent.width,
            height = evaluated.drawing.extent.height,
            valid,
            fits,
            "fit attempt"
        );
        trail.push(FitAttempt {
            levers,
            size: evaluated.drawing.extent.into(),
            valid,
            fits,
        });
        candidates.push((index, evaluated, valid));
        if fits {
            chosen = Some(candidates.len() - 1);
            break;
        }
    }

    let (pick, outcome) = match chosen {
        Some(pick) if pick == 0 => (pick, FitOutcome::AsAuthored),
        Some(pick) => (pick, FitOutcome::Fitted),
        None => {
            let best = candidates
                .iter()
                .enumerate()
                .filter(|(_, (_, _, valid))| *valid)
                .min_by_key(|(position, (_, evaluated, _))| {
                    let (w, h) = overflow(evaluated.drawing.extent, fit);
                    (w, h, *position)
                })
                .map(|(position, _)| position)
                .unwrap_or(0);
            (best, FitOutcome::BestAttempt)
        }
    };

    let stable_for = fit.max_height.is_none().then(|| match outcome {
        FitOutcome::BestAttempt => {
            let narrowest = candidates
                .iter()
                .filter(|(_, _, valid)| *valid)
                .map(|(_, evaluated, _)| evaluated.drawing.extent.width)
                .min()
                .unwrap_or(0);
            StableRange {
                min_width: 1,
                max_width: Some(narrowest.saturating_sub(1)),
            }
        }
        _ => StableRange {
            min_width: candidates[pick].1.drawing.extent.width,
            max_width: candidates[..pick]
                .iter()
                .filter(|(_, _, valid)| *valid)
                .map(|(_, evaluated, _)| evaluated.drawing.extent.width)
                .min()
                .map(|width| width.saturating_sub(1)),
        },
    });

    let as_authored = candidates[0].1.drawing.extent;
    let (rung_index, evaluated, _) = candidates.swap_remove(pick);
    let settings = rungs[rung_index].settings.clone();
    tracing::debug!(
        event = "fit_outcome",
        outcome = ?outcome,
        width = evaluated.drawing.extent.width,
        height = evaluated.drawing.extent.height,
        attempts = trail.len(),
        "fit outcome"
    );
    let report = FitReport {
        budget: budget_of(fit),
        outcome,
        size: Some(evaluated.drawing.extent.into()),
        as_authored: Some(as_authored.into()),
        applied: levers_of(&settings),
        authored_direction,
        direction: evaluated.direction,
        stable_for,
        lever_scope,
        attempts: trail.len() as u32,
        solves: solves(),
        trail,
    };
    Ok(SearchResult {
        fitted: Fitted {
            output: evaluated.drawing.text,
            report,
        },
        chosen: settings,
    })
}

/// Fit a prepared graph-family graph with the full ladder.
pub(in crate::runtime) fn fit_graph(
    diagram_id: &str,
    pristine: &Graph,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<SearchResult, RenderError> {
    let rungs = build_graph_ladder(diagram_id, pristine, fit);
    let base_options = config.text_render_options(format);
    let solves = std::cell::Cell::new(0u32);
    let mut solved: Vec<Option<(Graph, TextSolve)>> = Vec::new();

    search(
        &rungs,
        fit,
        Some(pristine.direction),
        FitLeverScope::Full,
        |index, rung| {
            solved.push(None);
            let source = match rung.reuses_solve_of {
                Some(base) => base,
                None => {
                    let mut graph = pristine.clone();
                    apply_graph_transforms(&mut graph, &rung.settings);
                    let solve = solve_graph_family_text(diagram_id, &mut graph, format, config)?;
                    solves.set(solves.get() + 1);
                    solved[index] = Some((graph, solve));
                    index
                }
            };
            let (graph, solve) = solved[source]
                .as_ref()
                .expect("a reused solve precedes its render-only rungs");
            let options = crate::render::graph::TextRenderOptions {
                grid_spacing: rung.settings.grid_spacing(),
                ..base_options.clone()
            };
            Ok(Evaluated {
                drawing: render_graph_family_text(graph, solve, &options, true),
                direction: Some(graph.direction),
            })
        },
        || solves.get(),
    )
}

fn is_text_format(format: OutputFormat) -> bool {
    matches!(format, OutputFormat::Text | OutputFormat::Ascii)
}

fn not_applied(output: String, fit: &FitOptions) -> Fitted {
    Fitted {
        output,
        report: FitReport {
            budget: budget_of(fit),
            outcome: FitOutcome::NotApplied,
            size: None,
            as_authored: None,
            applied: Vec::new(),
            authored_direction: None,
            direction: None,
            stable_for: None,
            lever_scope: FitLeverScope::Unavailable,
            attempts: 0,
            solves: 0,
            trail: Vec::new(),
        },
    }
}

/// Fit a sequence diagram: one as-authored candidate, measured.
fn fit_sequence(
    model: &crate::timeline::sequence::model::Sequence,
    format: OutputFormat,
    fit: &FitOptions,
) -> Fitted {
    let layout = sequence_layout::layout(model);
    let charset = match format {
        OutputFormat::Ascii => CharSet::ascii(),
        _ => CharSet::unicode(),
    };
    let (output, extent) = timeline::render_measured(&layout, &charset);
    let fits = fits_budget(extent, fit);
    let stable_for = fit.max_height.is_none().then(|| {
        if fits {
            StableRange {
                min_width: extent.width,
                max_width: None,
            }
        } else {
            StableRange {
                min_width: 1,
                max_width: Some(extent.width.saturating_sub(1)),
            }
        }
    });
    Fitted {
        output,
        report: FitReport {
            budget: budget_of(fit),
            outcome: if fits {
                FitOutcome::AsAuthored
            } else {
                FitOutcome::BestAttempt
            },
            size: Some(extent.into()),
            as_authored: Some(extent.into()),
            applied: Vec::new(),
            authored_direction: None,
            direction: None,
            stable_for,
            lever_scope: FitLeverScope::Unavailable,
            attempts: 1,
            solves: 0,
            trail: vec![FitAttempt {
                levers: Vec::new(),
                size: extent.into(),
                valid: true,
                fits,
            }],
        },
    }
}

/// Fit text input with the internal search result, for crate checks.
pub(in crate::runtime) fn search_diagram(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<SearchResult, RenderError> {
    match prepare_input(input, format, config)? {
        PreparedInput::Mmds(document) => search_document(&document, format, config, fit),
        PreparedInput::Mermaid {
            payload,
            effective_config,
        } => match payload::prepare_payload_for_render(payload, &effective_config) {
            Diagram::Flowchart(graph) => {
                fit_graph("flowchart", &graph, format, &effective_config, fit)
            }
            Diagram::Class(graph) => fit_graph("class", &graph, format, &effective_config, fit),
            Diagram::State(graph) => fit_graph("state", &graph, format, &effective_config, fit),
            Diagram::Sequence(model) => Ok(SearchResult {
                fitted: fit_sequence(&model, format, fit),
                chosen: FitSettings::default(),
            }),
        },
    }
}

/// Fit a document by replaying its geometry with render-only levers.
fn search_document(
    document: &crate::mmds::Document,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<SearchResult, RenderError> {
    let replay = prepare_text_replay(
        document,
        format,
        &config.text_render_options(format),
        config.font_metrics_profile.as_deref(),
    )?;
    let direction = document.metadata.direction;
    search(
        &build_replay_ladder(),
        fit,
        Some(direction),
        FitLeverScope::RenderOnly,
        |_, rung| {
            Ok(Evaluated {
                drawing: render_text_replay(&replay, rung.settings.grid_spacing(), true),
                direction: Some(direction),
            })
        },
        || 0,
    )
}

/// Detect, parse and render text input fitted to `fit`'s budget.
///
/// The fitted counterpart of [`crate::render_diagram`]: with no budget, or when
/// the as-authored drawing fits, the output is byte-identical to
/// `render_diagram(input, format, config)`. Formats other than text and ASCII
/// render exactly as `render_diagram` does, with
/// [`FitOutcome::NotApplied`].
///
/// # Errors
///
/// Everything [`crate::render_diagram`] rejects, plus a budget of 0.
pub fn render_diagram_fitted(
    input: &str,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<Fitted, RenderError> {
    validate_render_config(config)?;
    fit.validate()?;
    if !is_text_format(format) {
        return Ok(not_applied(
            super::render_diagram(input, format, config)?,
            fit,
        ));
    }
    Ok(search_diagram(input, format, config, fit)?.fitted)
}

/// Render a graph-family MMDS document fitted to `fit`'s budget.
///
/// The fitted counterpart of [`crate::render_document`]. Replay keeps the
/// document's geometry, so only render-time spacing levers apply
/// ([`FitLeverScope::RenderOnly`]).
///
/// # Errors
///
/// Everything [`crate::render_document`] rejects, plus a budget of 0.
pub fn render_document_fitted(
    document: &crate::mmds::Document,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<Fitted, RenderError> {
    validate_render_config(config)?;
    fit.validate()?;
    if !is_text_format(format) {
        return Ok(not_applied(
            super::render_document(document, format, config)?,
            fit,
        ));
    }
    Ok(search_document(document, format, config, fit)?.fitted)
}

/// Lay out a graph-family MMDS document afresh and render it fitted to
/// `fit`'s budget.
///
/// The fitted counterpart of [`crate::render_document_with_relayout`], with
/// every lever available ([`FitLeverScope::Full`]).
///
/// # Errors
///
/// Everything [`crate::render_document_with_relayout`] rejects, plus a budget
/// of 0.
pub fn render_document_with_relayout_fitted(
    document: &crate::mmds::Document,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<Fitted, RenderError> {
    Ok(search_document_with_relayout(document, format, config, fit)?.fitted)
}

pub(in crate::runtime) fn search_document_with_relayout(
    document: &crate::mmds::Document,
    format: OutputFormat,
    config: &RenderConfig,
    fit: &FitOptions,
) -> Result<SearchResult, RenderError> {
    validate_render_config(config)?;
    fit.validate()?;
    let diagram = crate::mmds::from_document(document).map_err(|error| RenderError {
        message: format!("MMDS validation error: {error}"),
    })?;
    let mut config = config.clone();
    if let Some(engine) = document.metadata.engine.as_deref() {
        config.layout_engine = Some(engine.parse()?);
    }
    if !is_text_format(format) {
        return Ok(SearchResult {
            fitted: not_applied(
                super::render_document_with_relayout(document, format, &config)?,
                fit,
            ),
            chosen: FitSettings::default(),
        });
    }
    fit_graph(
        &document.metadata.diagram_type,
        &diagram,
        format,
        &config,
        fit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse Mermaid source into its prepared graph and diagram id.
    fn prepared_graph(input: &str, config: &RenderConfig) -> (&'static str, Graph) {
        let PreparedInput::Mermaid {
            payload,
            effective_config,
        } = prepare_input(input, OutputFormat::Text, config).unwrap()
        else {
            panic!("expected Mermaid input");
        };
        match payload::prepare_payload_for_render(payload, &effective_config) {
            Diagram::Flowchart(graph) => ("flowchart", graph),
            Diagram::Class(graph) => ("class", graph),
            Diagram::State(graph) => ("state", graph),
            Diagram::Sequence(_) => panic!("expected a graph family"),
        }
    }

    fn ladder(input: &str, fit: &FitOptions) -> Vec<Rung> {
        let (diagram_id, graph) = prepared_graph(input, &RenderConfig::default());
        build_graph_ladder(diagram_id, &graph, fit)
    }

    fn fixture(family: &str, name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{family}/{name}.mmd",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// Name each rung by the settings it adds, for readable assertions.
    fn names(rungs: &[Rung]) -> Vec<String> {
        rungs
            .iter()
            .map(|rung| {
                let levers = rung.settings.levers(Direction::LeftRight);
                let mut name = levers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if name.is_empty() {
                    name.push_str("as authored");
                }
                if let Some(base) = rung.reuses_solve_of {
                    name.push_str(&format!(" (reuses {base})"));
                }
                name
            })
            .collect()
    }

    const LONG_LABELED_LR: &str = "graph LR\n\
        A[An exceedingly long first node label here] -->|a fairly long edge label| \
        B[Another exceedingly long node label text]\n\
        B -->|second long edge label| C[Short]";

    #[test]
    fn levers_follow_the_canonical_order() {
        let settings = FitSettings {
            flip: true,
            strip_horizontal_subgraph_overrides: true,
            label_aware_spacing: true,
            grid_gaps: Some(GridGaps {
                rank_gap: 2,
                node_gap: 1,
            }),
            edge_label_wrap: Some(10),
            node_label_wrap: Some(16),
            member_wrap: Some(16),
            truncate: Some(8),
            member_keep: Some(3),
        };
        assert_eq!(
            settings.levers(Direction::BottomTop),
            vec![
                FitLever::Direction {
                    from: Direction::BottomTop,
                    to: Direction::RightLeft,
                },
                FitLever::SubgraphDirectionStrip,
                FitLever::LabelAwareSpacing,
                FitLever::GridGap {
                    rank_gap: 2,
                    node_gap: 1,
                },
                FitLever::EdgeLabelWrap { cells: 10 },
                FitLever::NodeLabelWrap { cells: 16 },
                FitLever::MemberWrap { cells: 16 },
                FitLever::LabelTruncation { cells: 8 },
                FitLever::MemberElision { keep: 3 },
            ]
        );
        assert!(FitSettings::default().levers(Direction::TopDown).is_empty());
    }

    #[test]
    fn labeled_lr_chain_ladder() {
        let rungs = ladder(LONG_LABELED_LR, &FitOptions::max_width(80));
        let u = [
            "as authored",
            "label-aware spacing (reuses 0)",
            "label-aware spacing, gaps rank 2 node 2 (reuses 0)",
            "label-aware spacing, gaps rank 2 node 1 (reuses 0)",
            "label-aware spacing, gaps rank 2 node 1, edge labels wrapped at 16",
            "label-aware spacing, gaps rank 2 node 1, edge labels wrapped at 10",
            "label-aware spacing, gaps rank 2 node 1, edge labels wrapped at 10, node labels wrapped at 24",
            "label-aware spacing, gaps rank 2 node 1, edge labels wrapped at 10, node labels wrapped at 16",
            "label-aware spacing, gaps rank 2 node 1, edge labels wrapped at 10, node labels wrapped at 10",
        ];
        let mut expected: Vec<String> = u.iter().map(|name| name.to_string()).collect();
        for (index, name) in u.iter().enumerate() {
            let flipped = if index == 0 {
                "direction LR→TD".to_string()
            } else {
                format!("direction LR→TD, {name}").replace("(reuses 0)", "(reuses 9)")
            };
            expected.push(flipped);
        }
        assert_eq!(names(&rungs), expected);
        assert!(
            rungs.iter().all(|rung| rung.settings.member_wrap.is_none()),
            "a skipped rung's lever is not carried forward"
        );
    }

    #[test]
    fn keep_drops_the_strip_and_flipped_rungs() {
        let fit = FitOptions::max_width(80).with_direction_change(DirectionChange::Keep);
        let rungs = ladder(LONG_LABELED_LR, &fit);
        assert_eq!(rungs.len(), 9);
        assert!(rungs.iter().all(|rung| !rung.settings.flip));
        assert!(
            rungs
                .iter()
                .all(|rung| !rung.settings.strip_horizontal_subgraph_overrides)
        );
    }

    #[test]
    fn truncation_rungs_follow_the_flipped_rungs() {
        let fit = FitOptions::max_width(80).with_truncation(true);
        let rungs = ladder(LONG_LABELED_LR, &fit);
        let truncated: Vec<(bool, Option<usize>, Option<usize>)> = rungs
            .iter()
            .skip_while(|rung| rung.settings.truncate.is_none())
            .map(|rung| {
                (
                    rung.settings.flip,
                    rung.settings.truncate,
                    rung.settings.member_keep,
                )
            })
            .collect();
        assert_eq!(
            truncated,
            vec![
                (false, Some(20), None),
                (true, Some(20), None),
                (false, Some(12), None),
                (true, Some(12), None),
                (false, Some(8), None),
                (true, Some(8), None),
            ],
            "a flowchart has no member sections to elide"
        );
        assert_eq!(rungs.len(), 18 + 6);
    }

    #[test]
    fn member_elision_rungs_need_a_long_class_section() {
        let class = "classDiagram\n\
            class Account {\n  +id\n  +owner\n  +balance\n  +currency\n  +open()\n}\n\
            class Ledger\n\
            Account --> Ledger";
        let rungs = ladder(class, &FitOptions::max_width(40).with_truncation(true));
        let elided: Vec<bool> = rungs
            .iter()
            .filter(|rung| rung.settings.member_keep == Some(3))
            .map(|rung| rung.settings.flip)
            .collect();
        assert_eq!(elided, vec![false, true]);
    }

    #[test]
    fn edge_wrap_rungs_depend_on_label_widths() {
        let has = |rungs: &[Rung], cells| {
            rungs
                .iter()
                .any(|rung| !rung.settings.flip && rung.settings.edge_label_wrap == Some(cells))
        };
        let fit = FitOptions::max_width(40);

        let unlabeled = ladder("graph LR\nA --> B", &fit);
        assert!(!has(&unlabeled, 16) && !has(&unlabeled, 10));

        let short = ladder("graph LR\nA -->|ten cells| B", &fit);
        assert!(!has(&short, 16) && !has(&short, 10));

        let twelve = ladder("graph LR\nA -->|twelve cells| B", &fit);
        assert!(!has(&twelve, 16));
        assert!(has(&twelve, 10));
    }

    #[test]
    fn member_wrap_and_strip_rungs_appear_when_they_change_the_graph() {
        let fit = FitOptions::max_width(40);
        let class = "classDiagram\n\
            class Account {\n  +String a_rather_long_member_name\n}";
        assert!(
            ladder(class, &fit)
                .iter()
                .any(|rung| !rung.settings.flip && rung.settings.member_wrap == Some(16))
        );

        let nested = "graph TD\n\
            subgraph S\n  direction LR\n  A --> B\nend\n\
            B --> C";
        assert!(
            ladder(nested, &fit)
                .iter()
                .any(|rung| rung.settings.strip_horizontal_subgraph_overrides)
        );
        assert!(
            ladder("graph TD\nA --> B", &fit)
                .iter()
                .all(|rung| !rung.settings.strip_horizontal_subgraph_overrides)
        );
    }

    #[test]
    fn skipped_edge_wraps_leave_the_engine_pixel_wrap_alone() {
        let mut config = RenderConfig::default();
        config.layout.edge_label_max_width = Some(40.0);
        let input = "graph LR\nA -->|go on| B -->|then stop| C";
        let (diagram_id, pristine) = prepared_graph(input, &config);
        let rungs = build_graph_ladder(diagram_id, &pristine, &FitOptions::max_width(20));
        assert!(
            rungs
                .iter()
                .all(|rung| rung.settings.edge_label_wrap.is_none())
        );

        let solved_lines = |settings: &FitSettings| {
            let mut graph = pristine.clone();
            apply_graph_transforms(&mut graph, settings);
            solve_graph_family_text(diagram_id, &mut graph, OutputFormat::Text, &config).unwrap();
            graph
                .edges
                .iter()
                .map(|edge| edge.wrapped_label_lines.clone())
                .collect::<Vec<_>>()
        };
        let authored = solved_lines(&rungs[0].settings);
        assert_eq!(solved_lines(&rungs[3].settings), authored);
        assert!(
            authored
                .iter()
                .any(|lines| lines.as_ref().is_some_and(|lines| lines.len() > 1)),
            "the pixel wrap should split a label: {authored:?}"
        );
    }

    #[test]
    fn transforms_apply_in_order() {
        let input = "graph LR\n\
            subgraph S\n  direction TB\n  A[alpha beta gamma delta] --> B\nend";
        let (_, mut graph) = prepared_graph(input, &RenderConfig::default());
        let settings = FitSettings {
            flip: true,
            strip_horizontal_subgraph_overrides: true,
            node_label_wrap: Some(10),
            ..FitSettings::default()
        };
        assert!(apply_graph_transforms(&mut graph, &settings));
        assert_eq!(graph.direction, Direction::TopDown);
        // The flip turns the TB override into LR under a TD root, and the strip
        // that runs after it drops that horizontal override.
        assert_eq!(graph.subgraphs["S"].dir, None);
        assert_eq!(graph.nodes["A"].label, "alpha beta\ngamma\ndelta");
    }

    #[test]
    fn ladders_are_deterministic() {
        let fit = FitOptions::max_width(40).with_truncation(true);
        let input = fixture("flowchart", "ci_pipeline");
        assert_eq!(ladder(&input, &fit), ladder(&input, &fit));
    }

    #[test]
    fn replay_ladder_is_render_only() {
        assert_eq!(
            names(&build_replay_ladder()),
            vec![
                "as authored",
                "label-aware spacing (reuses 0)",
                "label-aware spacing, gaps rank 2 node 2 (reuses 0)",
                "label-aware spacing, gaps rank 2 node 1 (reuses 0)",
            ]
        );
    }

    #[test]
    fn report_serializes_with_contract_field_names() {
        let report = FitReport {
            budget: FitBudget {
                max_width: Some(40),
                max_height: None,
            },
            outcome: FitOutcome::BestAttempt,
            size: Some(CellSize {
                width: 43,
                height: 47,
            }),
            as_authored: Some(CellSize {
                width: 169,
                height: 9,
            }),
            applied: vec![
                FitLever::Direction {
                    from: Direction::LeftRight,
                    to: Direction::TopDown,
                },
                FitLever::GridGap {
                    rank_gap: 2,
                    node_gap: 1,
                },
            ],
            authored_direction: Some(Direction::LeftRight),
            direction: Some(Direction::TopDown),
            stable_for: Some(StableRange {
                min_width: 1,
                max_width: Some(42),
            }),
            lever_scope: FitLeverScope::Full,
            attempts: 21,
            solves: 15,
            trail: vec![FitAttempt {
                levers: Vec::new(),
                size: CellSize {
                    width: 1,
                    height: 1,
                },
                valid: true,
                fits: false,
            }],
        };
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({
                "budget": {"maxWidth": 40, "maxHeight": null},
                "outcome": "bestAttempt",
                "size": {"width": 43, "height": 47},
                "asAuthored": {"width": 169, "height": 9},
                "applied": [
                    {"lever": "direction", "from": "LR", "to": "TD"},
                    {"lever": "gridGap", "rankGap": 2, "nodeGap": 1}
                ],
                "authoredDirection": "LR",
                "direction": "TD",
                "stableFor": {"minWidth": 1, "maxWidth": 42},
                "leverScope": "full",
                "attempts": 21,
                "solves": 15
            })
        );
        assert_eq!(
            serde_json::to_value(FitLever::LabelAwareSpacing).unwrap(),
            serde_json::json!({"lever": "labelAwareSpacing"})
        );
    }

    /// Render `input` with `settings` and no budget.
    fn render_with(input: &str, settings: &FitSettings, config: &RenderConfig) -> String {
        let (diagram_id, mut graph) = prepared_graph(input, config);
        apply_graph_transforms(&mut graph, settings);
        let solve =
            solve_graph_family_text(diagram_id, &mut graph, OutputFormat::Text, config).unwrap();
        let options = crate::render::graph::TextRenderOptions {
            grid_spacing: settings.grid_spacing(),
            ..config.text_render_options(OutputFormat::Text)
        };
        render_graph_family_text(&graph, &solve, &options, false).text
    }

    /// Rendering with the chosen settings and no budget reproduces the
    /// fitted output.
    #[test]
    fn chosen_settings_replay_the_fitted_output() {
        let config = RenderConfig::default();
        let cases = [
            ("flowchart", "ci_pipeline"),
            ("flowchart", "git_workflow"),
            ("flowchart", "multi_subgraph"),
            ("class", "all_relations"),
        ];
        for (family, name) in cases {
            let input = fixture(family, name);
            for width in [40, 80] {
                let fit = FitOptions::max_width(width).with_truncation(true);
                let result = search_diagram(&input, OutputFormat::Text, &config, &fit).unwrap();
                assert_eq!(
                    render_with(&input, &result.chosen, &config),
                    result.fitted.output,
                    "{family}/{name} at {width}"
                );
            }
        }
    }

    /// The replay invariant over the whole graph-family corpus.
    #[test]
    fn chosen_settings_replay_the_fitted_output_across_the_corpus() {
        let config = RenderConfig::default();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for family in ["flowchart", "class", "state", "fit"] {
            let mut paths: Vec<_> = std::fs::read_dir(root.join(family))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "mmd"))
                .collect();
            paths.sort();
            for path in paths {
                let input = std::fs::read_to_string(&path).unwrap();
                for width in [40, 80] {
                    let fit = FitOptions::max_width(width);
                    let result = search_diagram(&input, OutputFormat::Text, &config, &fit).unwrap();
                    assert_eq!(
                        render_with(&input, &result.chosen, &config),
                        result.fitted.output,
                        "{} at {width}",
                        path.display()
                    );
                }
            }
        }
    }
}
