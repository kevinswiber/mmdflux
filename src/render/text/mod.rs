//! Shared text-output infrastructure reused across graph and diagram renderers.

pub(crate) mod canvas;
pub(crate) mod chars;
pub(crate) mod connections;

pub use canvas::Canvas;
pub use chars::CharSet;
