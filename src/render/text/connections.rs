//! Shared connection metadata for text-grid junction resolution.

/// Tracks connections in four directions for a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Connections {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl Connections {
    /// Create connections with no directions set.
    pub fn none() -> Self {
        Self::default()
    }

    /// Merge another set of connections into this one.
    pub fn merge(&mut self, other: Connections) {
        self.up |= other.up;
        self.down |= other.down;
        self.left |= other.left;
        self.right |= other.right;
    }

    /// Count how many directions are connected.
    #[allow(dead_code)]
    pub fn count(&self) -> u8 {
        self.up as u8 + self.down as u8 + self.left as u8 + self.right as u8
    }
}
