//! Compatibility shim — canonical home is `crate::graph::geometry`.
//!
//! This re-export keeps existing `diagrams::flowchart::geometry::*` imports
//! working during the architecture transition. Will be removed in Task 2.3.

pub use crate::graph::geometry::*;
