//! mmdflux - Convert Mermaid diagrams to text/SVG
//!
//! This library provides parsing and rendering for Mermaid diagram syntax.
//! Currently supports flowcharts with text (Unicode/ASCII) output.
//!
//! # Quick Start (Direct Render API)
//!
//! ```
//! use mmdflux::{parse_flowchart, build_diagram};
//! use mmdflux::render::{render, RenderOptions};
//!
//! let input = "graph TD\n    A-->B";
//! let flowchart = parse_flowchart(input).unwrap();
//! let diagram = build_diagram(&flowchart);
//! let output = render(&diagram, &RenderOptions::default());
//! println!("{}", output);
//! ```
//!
//! # Using the Registry API
//!
//! The registry provides a unified interface for all diagram types:
//!
//! ```
//! use mmdflux::registry::default_registry;
//! use mmdflux::diagram::{OutputFormat, RenderConfig};
//!
//! let registry = default_registry();
//! let input = "graph TD\n    A-->B";
//!
//! if let Some(diagram_id) = registry.detect(input) {
//!     let mut instance = registry.create(diagram_id).unwrap();
//!     instance.parse(input).unwrap();
//!     let output = instance.render(OutputFormat::Text, &RenderConfig::default()).unwrap();
//!     println!("{}", output);
//! }
//! ```

// Core modules
pub mod diagram;
pub mod diagrams;
pub mod engines;
pub mod graph;
pub mod layered;
pub mod lint;
pub mod mmds;
pub mod parser;
pub mod registry;
pub mod render;
pub mod style;

// Re-export commonly used types for convenience
pub use diagram::{
    AlgorithmId, ColorWhen, EdgeRouting, EngineAlgorithmCapabilities, EngineAlgorithmId,
    EngineConfig, EngineId, GeometryLevel, GraphEngine, GraphSolveRequest, GraphSolveResult,
    OutputFormat, PathSimplification, RenderConfig, RenderError, RouteOwnership, TextColorMode,
};
pub use graph::{Diagram, Direction, Edge, Node, Shape, build_diagram};
pub use mmds::{MmdsGenerationError, generate_mermaid_from_mmds, generate_mermaid_from_mmds_str};
pub use parser::{
    DiagramType, Flowchart, ParseDiagnostic, ParseError, detect_diagram_type, parse_flowchart,
};
pub use registry::default_registry;
pub use style::{ColorToken, NodeStyle};
