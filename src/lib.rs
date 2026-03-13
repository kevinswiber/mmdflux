//! mmdflux - Convert Mermaid diagrams to text/SVG
//!
//! This library provides parsing and rendering for Mermaid diagram syntax.
//! Currently supports flowcharts with text (Unicode/ASCII) output.
//!
//! # Quick Start
//!
//! ```
//! use mmdflux::{OutputFormat, RenderConfig, render_diagram};
//!
//! let input = "graph TD\n    A-->B";
//! let output = render_diagram(input, OutputFormat::Text, &RenderConfig::default()).unwrap();
//! println!("{}", output);
//! ```
//!
//! # Expert API
//!
//! The supported expert tier is organized around adapter workflows:
//! - `builtins` for the default diagram registry
//! - `registry` and `prepared` for explicit detect/parse/prepare flows
//! - `mmds` for MMDS parsing, replay, hydration to `Diagram`, and Mermaid generation
//!
//! The rest of the implementation tree stays internal to the crate.

extern crate self as mmdflux;

pub mod builtins;
pub mod config;
pub mod diagnostics;
// Core modules
mod diagrams;
mod engines;
pub mod errors;
pub mod family;
pub mod format;
mod frontends;
mod graph;
mod lint;
mod mermaid;
pub mod mmds;
pub mod prepared;
pub mod registry;
mod render;
pub(crate) mod runtime;
pub mod simplification;
pub mod style;
mod timeline;

// Re-export commonly used types for convenience
pub use config::RenderConfig;
pub use diagnostics::ParseDiagnostic;
pub use engines::graph::{AlgorithmId, EngineAlgorithmId, EngineId};
pub use errors::RenderError;
pub use family::DiagramFamily;
pub use format::{CornerStyle, Curve, EdgePreset, OutputFormat, RoutingStyle};
pub use graph::{Diagram, Direction, Edge, GeometryLevel, Node, Shape};
pub use mmds::{MmdsGenerationError, generate_mermaid_from_mmds, generate_mermaid_from_mmds_str};
pub use render::text::{ColorWhen, TextColorMode};
// Runtime facade re-exports — curated entrypoints for adapters (CLI, WASM).
pub use runtime::config_input::{RuntimeConfigInput, apply_svg_surface_defaults};
pub use runtime::{detect_diagram, render_diagram, validate_diagram};
pub use simplification::PathSimplification;
pub use style::{ColorToken, NodeStyle};

#[cfg(test)]
mod internal_tests;
