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

/// Strategy for placing edge-label dummies within long edge chains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelDummyStrategy {
    #[default]
    Midpoint,
    WidestLayer,
}

impl From<LabelDummyStrategy> for crate::engines::graph::algorithms::layered::LabelDummyStrategy {
    fn from(value: LabelDummyStrategy) -> Self {
        match value {
            LabelDummyStrategy::Midpoint => Self::Midpoint,
            LabelDummyStrategy::WidestLayer => Self::WidestLayer,
        }
    }
}

impl From<crate::engines::graph::algorithms::layered::LabelDummyStrategy> for LabelDummyStrategy {
    fn from(value: crate::engines::graph::algorithms::layered::LabelDummyStrategy) -> Self {
        match value {
            crate::engines::graph::algorithms::layered::LabelDummyStrategy::Midpoint => {
                LabelDummyStrategy::Midpoint
            }
            crate::engines::graph::algorithms::layered::LabelDummyStrategy::WidestLayer => {
                LabelDummyStrategy::WidestLayer
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
    pub label_dummy_strategy: LabelDummyStrategy,
    pub edge_label_spacing: f64,
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
            label_dummy_strategy: LabelDummyStrategy::default(),
            edge_label_spacing: 2.0,
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
            label_dummy_strategy: value.label_dummy_strategy.into(),
            edge_label_spacing: value.edge_label_spacing,
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
            label_dummy_strategy: value.label_dummy_strategy.into(),
            edge_label_spacing: value.edge_label_spacing,
        }
    }
}
