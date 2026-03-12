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
//! # Advanced API
//!
//! Low-level parsing, compilation, MMDS hydration, and render-only geometry
//! emission remain available through explicit namespaces such as `frontends`,
//! `diagrams`, `render`, `registry`, and `registry_builtins`.

pub mod config;
pub mod diagnostics;
// Core modules
pub mod diagrams;
pub mod engines;
pub mod errors;
pub mod family;
pub mod format;
pub mod frontends;
pub mod graph;
pub mod lint;
pub mod mmds;
pub mod prepared;
pub mod registry;
pub mod registry_builtins;
pub mod render;
pub mod request;
pub(crate) mod runtime;
pub mod style;
mod timeline;

// Re-export commonly used types for convenience
pub use config::{
    AlgorithmId, ColorWhen, EngineAlgorithmId, EngineId, GeometryLevel, PathSimplification,
    RenderConfig, TextColorMode,
};
pub use diagnostics::ParseDiagnostic;
pub use errors::RenderError;
pub use family::DiagramFamily;
pub use format::{CornerStyle, Curve, EdgePreset, OutputFormat, RoutingStyle};
pub use graph::{Diagram, Direction, Edge, Node, Shape};
pub use mmds::{MmdsGenerationError, generate_mermaid_from_mmds, generate_mermaid_from_mmds_str};
pub use request::RenderRequest;
// Runtime facade re-exports — curated entrypoints for adapters (CLI, WASM).
pub use runtime::config_input::{RuntimeConfigInput, apply_svg_surface_defaults};
pub use runtime::{detect_diagram, render_diagram, validate_diagram};
pub use style::{ColorToken, NodeStyle};
