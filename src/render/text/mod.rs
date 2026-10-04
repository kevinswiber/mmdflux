//! Shared text-output infrastructure reused across graph and diagram renderers.

pub(crate) mod canvas;
pub(crate) mod chars;
pub(crate) mod connections;

pub use canvas::Canvas;
pub use chars::CharSet;

/// Size of a painted text drawing in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CellExtent {
    pub(crate) width: usize,
    pub(crate) height: usize,
}

impl From<(usize, usize)> for CellExtent {
    fn from((width, height): (usize, usize)) -> Self {
        Self { width, height }
    }
}
