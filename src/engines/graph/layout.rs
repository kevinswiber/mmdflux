//! Graph-family layout configuration types (ranker, direction, label strategy).

use std::collections::HashMap;

/// Ranking algorithm selection for the public graph-layout config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ranker {
    #[default]
    NetworkSimplex,
    LongestPath,
}

impl From<Ranker> for crate::engines::graph::algorithms::layered::Ranker {
    fn from(value: Ranker) -> Self {
        match value {
            Ranker::NetworkSimplex => Self::NetworkSimplex,
            Ranker::LongestPath => Self::LongestPath,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::Ranker> for Ranker {
    fn from(value: crate::engines::graph::algorithms::layered::Ranker) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::Ranker::NetworkSimplex => {
                Ranker::NetworkSimplex
            }
            crate::engines::graph::algorithms::layered::Ranker::LongestPath => Ranker::LongestPath,
        }
    }
}

/// Direction for the public graph-layout config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutDirection {
    #[default]
    TopBottom,
    BottomTop,
    LeftRight,
    RightLeft,
}

impl From<LayoutDirection> for crate::engines::graph::algorithms::layered::Direction {
    fn from(value: LayoutDirection) -> Self {
        match value {
            LayoutDirection::TopBottom => Self::TopBottom,
            LayoutDirection::BottomTop => Self::BottomTop,
            LayoutDirection::LeftRight => Self::LeftRight,
            LayoutDirection::RightLeft => Self::RightLeft,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::Direction> for LayoutDirection {
    fn from(value: crate::engines::graph::algorithms::layered::Direction) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::Direction::TopBottom => {
                LayoutDirection::TopBottom
            }
            crate::engines::graph::algorithms::layered::Direction::BottomTop => {
                LayoutDirection::BottomTop
            }
            crate::engines::graph::algorithms::layered::Direction::LeftRight => {
                LayoutDirection::LeftRight
            }
            crate::engines::graph::algorithms::layered::Direction::RightLeft => {
                LayoutDirection::RightLeft
            }
        }
    }
}

/// Placement strategy for edge-label dummies within long edge chains.
/// Orthogonal to [`LabelDummyRouting`] — plan 0147 Task 2.3 split the
/// legacy `LabelDummyStrategy` into placement + routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelDummyPlacement {
    #[default]
    Midpoint,
    WidestLayer,
}

impl From<LabelDummyPlacement> for crate::engines::graph::algorithms::layered::LabelDummyPlacement {
    fn from(value: LabelDummyPlacement) -> Self {
        match value {
            LabelDummyPlacement::Midpoint => Self::Midpoint,
            LabelDummyPlacement::WidestLayer => Self::WidestLayer,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::LabelDummyPlacement> for LabelDummyPlacement {
    fn from(value: crate::engines::graph::algorithms::layered::LabelDummyPlacement) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::LabelDummyPlacement::Midpoint => {
                LabelDummyPlacement::Midpoint
            }
            crate::engines::graph::algorithms::layered::LabelDummyPlacement::WidestLayer => {
                LabelDummyPlacement::WidestLayer
            }
        }
    }
}

/// Routing strategy for how an edge path traverses its label dummy's rect.
/// Orthogonal to [`LabelDummyPlacement`] — plan 0147 Task 2.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelDummyRouting {
    #[default]
    Center,
    Bend,
}

impl From<LabelDummyRouting> for crate::engines::graph::algorithms::layered::LabelDummyRouting {
    fn from(value: LabelDummyRouting) -> Self {
        match value {
            LabelDummyRouting::Center => Self::Center,
            LabelDummyRouting::Bend => Self::Bend,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::LabelDummyRouting> for LabelDummyRouting {
    fn from(value: crate::engines::graph::algorithms::layered::LabelDummyRouting) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::LabelDummyRouting::Center => {
                LabelDummyRouting::Center
            }
            crate::engines::graph::algorithms::layered::LabelDummyRouting::Bend => {
                LabelDummyRouting::Bend
            }
        }
    }
}

/// Strategy for assigning Above/Below sides to edge label dummies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelSideStrategy {
    #[default]
    FirstLast,
    DirectionDown,
}

impl From<LabelSideStrategy> for crate::engines::graph::algorithms::layered::LabelSideStrategy {
    fn from(value: LabelSideStrategy) -> Self {
        match value {
            LabelSideStrategy::FirstLast => Self::FirstLast,
            LabelSideStrategy::DirectionDown => Self::DirectionDown,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::LabelSideStrategy> for LabelSideStrategy {
    fn from(value: crate::engines::graph::algorithms::layered::LabelSideStrategy) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::LabelSideStrategy::FirstLast => {
                LabelSideStrategy::FirstLast
            }
            crate::engines::graph::algorithms::layered::LabelSideStrategy::DirectionDown => {
                LabelSideStrategy::DirectionDown
            }
        }
    }
}

/// Canonical caller-facing graph layout configuration.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    pub direction: LayoutDirection,
    pub node_sep: f64,
    pub edge_sep: f64,
    pub rank_sep: f64,
    pub rank_sep_overrides: HashMap<i32, f64>,
    pub margin: f64,
    pub acyclic: bool,
    pub ranker: Ranker,
    pub greedy_switch: bool,
    pub model_order_tiebreak: bool,
    pub variable_rank_spacing: bool,
    pub always_compound_ordering: bool,
    pub track_reversed_chains: bool,
    pub per_edge_label_spacing: bool,
    pub label_side_selection: bool,
    pub label_side_strategy: LabelSideStrategy,
    /// Plan 0147 Task 2.3: orthogonal placement + routing enums replaced
    /// the legacy `label_dummy_strategy`.
    pub label_dummy_placement: LabelDummyPlacement,
    pub label_dummy_routing: LabelDummyRouting,
    /// Pixel spacing between edge line and label, mirroring ELK
    /// `edgeLabelSpacing`. Applied by proportional measurement modes
    /// (SVG rendering and MMDS output at either geometry level), which
    /// pad the label dummy by `edge_label_spacing + thickness` in pixels.
    ///
    /// Text output uses a grid projection that snaps labels to
    /// rank-interpolated layer starts, so this knob does **not** affect
    /// Text rendering today. Adding Text support requires teaching
    /// `graph::grid::derive::waypoints::transform_label_positions_direct`
    /// to account for padded dummy heights — tracked in GitHub issue
    /// #238.
    pub edge_label_spacing: f64,
    pub backward_edge_side_grouping: bool,
    /// Maximum edge-label width in pixels before greedy wrap kicks in.
    /// `None` disables wrap (dagre-parity fallback). Plan 0147 Task 1.7.
    pub edge_label_max_width: Option<f64>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            direction: LayoutDirection::default(),
            node_sep: 50.0,
            edge_sep: 20.0,
            rank_sep: 50.0,
            rank_sep_overrides: HashMap::new(),
            margin: 8.0,
            acyclic: true,
            ranker: Ranker::default(),
            greedy_switch: false,
            model_order_tiebreak: false,
            variable_rank_spacing: false,
            always_compound_ordering: false,
            track_reversed_chains: false,
            per_edge_label_spacing: false,
            label_side_selection: false,
            label_side_strategy: LabelSideStrategy::default(),
            label_dummy_placement: LabelDummyPlacement::default(),
            label_dummy_routing: LabelDummyRouting::default(),
            edge_label_spacing: 2.0,
            backward_edge_side_grouping: false,
            // Plan 0147 Task 1.7: user-facing default enables wrap at 200 px so
            // long labels render wrapped out of the box. Set to `None` to opt
            // out (dagre-parity / unwrapped measurement).
            edge_label_max_width: Some(200.0),
        }
    }
}

impl LayoutConfig {
    pub fn rank_sep_for_gap(&self, rank: i32) -> f64 {
        self.rank_sep_overrides
            .get(&rank)
            .copied()
            .unwrap_or(self.rank_sep)
    }
}

impl From<LayoutConfig> for crate::engines::graph::algorithms::layered::LayoutConfig {
    fn from(value: LayoutConfig) -> Self {
        Self {
            direction: value.direction.into(),
            node_sep: value.node_sep,
            edge_sep: value.edge_sep,
            rank_sep: value.rank_sep,
            rank_sep_overrides: value.rank_sep_overrides,
            margin: value.margin,
            acyclic: value.acyclic,
            ranker: value.ranker.into(),
            greedy_switch: value.greedy_switch,
            model_order_tiebreak: value.model_order_tiebreak,
            variable_rank_spacing: value.variable_rank_spacing,
            always_compound_ordering: value.always_compound_ordering,
            track_reversed_chains: value.track_reversed_chains,
            per_edge_label_spacing: value.per_edge_label_spacing,
            label_side_selection: value.label_side_selection,
            label_side_strategy: value.label_side_strategy.into(),
            label_dummy_placement: value.label_dummy_placement.into(),
            label_dummy_routing: value.label_dummy_routing.into(),
            edge_label_spacing: value.edge_label_spacing,
            backward_edge_side_grouping: value.backward_edge_side_grouping,
            edge_label_max_width: value.edge_label_max_width,
        }
    }
}

impl From<&LayoutConfig> for crate::engines::graph::algorithms::layered::LayoutConfig {
    fn from(value: &LayoutConfig) -> Self {
        value.clone().into()
    }
}

impl From<crate::engines::graph::algorithms::layered::LayoutConfig> for LayoutConfig {
    fn from(value: crate::engines::graph::algorithms::layered::LayoutConfig) -> Self {
        Self {
            direction: value.direction.into(),
            node_sep: value.node_sep,
            edge_sep: value.edge_sep,
            rank_sep: value.rank_sep,
            rank_sep_overrides: value.rank_sep_overrides,
            margin: value.margin,
            acyclic: value.acyclic,
            ranker: value.ranker.into(),
            greedy_switch: value.greedy_switch,
            model_order_tiebreak: value.model_order_tiebreak,
            variable_rank_spacing: value.variable_rank_spacing,
            always_compound_ordering: value.always_compound_ordering,
            track_reversed_chains: value.track_reversed_chains,
            per_edge_label_spacing: value.per_edge_label_spacing,
            label_side_selection: value.label_side_selection,
            label_side_strategy: value.label_side_strategy.into(),
            label_dummy_placement: value.label_dummy_placement.into(),
            label_dummy_routing: value.label_dummy_routing.into(),
            edge_label_spacing: value.edge_label_spacing,
            backward_edge_side_grouping: value.backward_edge_side_grouping,
            edge_label_max_width: value.edge_label_max_width,
        }
    }
}
