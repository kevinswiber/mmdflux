//! mmdflux — Mermaid diagrams to text, SVG, and MMDS
//!
//! mmdflux parses [Mermaid](https://mermaid.js.org/) diagram syntax and
//! renders it as Unicode/ASCII text, SVG, or structured JSON ([MMDS](https://mmds.dev/)).
//! Supported diagram types: **flowchart**, **class**, and **sequence**.
//!
//! # High-Level API
//!
//! Most consumers only need three functions and two config types:
//!
//! - [`render_diagram`] — detect, parse, and render in one call
//! - [`detect_diagram`] — detect the diagram type without rendering
//! - [`validate_diagram`] — parse and return structured JSON diagnostics
//! - [`OutputFormat`] — `Text`, `Ascii`, `Svg`, or `Mmds`
//! - [`RenderConfig`] — layout engine, routing, padding, color, and more
//!
//! ```
//! use mmdflux::{OutputFormat, RenderConfig, render_diagram};
//!
//! let input = "graph TD\n    A[Collect] --> B[Render]";
//!
//! // Render as Unicode text
//! let text = render_diagram(input, OutputFormat::Text, &RenderConfig::default()).unwrap();
//! println!("{text}");
//!
//! // Render as SVG
//! let svg = render_diagram(input, OutputFormat::Svg, &RenderConfig::default()).unwrap();
//! assert!(svg.contains("<svg"));
//!
//! // Render as MMDS JSON (structured interchange format)
//! let json = render_diagram(input, OutputFormat::Mmds, &RenderConfig::default()).unwrap();
//! assert!(json.contains("\"diagram_type\""));
//! ```
//!
//! ## Customizing output
//!
//! Use [`RenderConfig`] to control layout direction, engine selection, edge
//! routing, padding, color mode, and more:
//!
//! ```
//! use mmdflux::{OutputFormat, RenderConfig, render_diagram};
//! use mmdflux::LayoutConfig;
//! use mmdflux::format::RoutingStyle;
//!
//! let config = RenderConfig {
//!     routing_style: Some(RoutingStyle::Direct),
//!     padding: Some(2),
//!     layout: LayoutConfig {
//!         rank_sep: 30.0,
//!         ..LayoutConfig::default()
//!     },
//!     ..RenderConfig::default()
//! };
//!
//! let output = render_diagram("graph LR\n    A-->B-->C", OutputFormat::Text, &config).unwrap();
//! println!("{output}");
//! ```
//!
//! ## Validation
//!
//! [`validate_diagram`] returns structured JSON diagnostics suitable for
//! editor integrations and CI pipelines:
//!
//! ```
//! use mmdflux::validate_diagram;
//!
//! let result = validate_diagram("graph TD\n    A-->B");
//! let json: serde_json::Value = serde_json::from_str(&result).unwrap();
//! assert_eq!(json["valid"], true);
//! ```
//!
//! # Low-Level API
//!
//! For adapters, tooling, or workflows that need explicit control over the
//! detect → parse → payload → render pipeline, the low-level API provides:
//!
//! - [`builtins::default_registry`] — the built-in diagram registry
//! - [`registry`] — [`DiagramRegistry`](registry::DiagramRegistry),
//!   [`DiagramInstance`](registry::DiagramInstance), and
//!   [`ParsedDiagram`](registry::ParsedDiagram) traits
//! - [`payload`] — the [`payload::Diagram`] enum returned by
//!   [`ParsedDiagram::into_payload`](registry::ParsedDiagram::into_payload)
//! - [`graph`] — graph-family IR types ([`Graph`](graph::Graph),
//!   [`Node`](graph::Node), [`Edge`](graph::Edge),
//!   [`Shape`](graph::Shape), [`Direction`](graph::Direction))
//! - [`timeline`] — timeline-family types
//!   ([`Sequence`](timeline::Sequence))
//! - [`mmds`] — MMDS parsing, hydration to [`graph::Graph`],
//!   profile negotiation, and Mermaid regeneration
//!
//! ```no_run
//! use mmdflux::builtins::default_registry;
//! use mmdflux::payload::Diagram;
//!
//! let input = "graph TD\n    A[Draft] --> B[Published]";
//! let registry = default_registry();
//!
//! // Detect diagram type
//! let resolved = registry.resolve(input).expect("should detect diagram type");
//! println!("detected: {} ({:?})", resolved.diagram_id(), resolved.family());
//!
//! // Parse and build payload
//! let instance = registry.create(resolved.diagram_id()).unwrap();
//! let payload = instance
//!     .parse(input).unwrap()
//!     .into_payload().unwrap();
//!
//! // Inspect the payload
//! match payload {
//!     Diagram::Flowchart(graph) => {
//!         println!("flowchart with {} nodes", graph.nodes.len());
//!     }
//!     Diagram::Class(graph) => {
//!         println!("class diagram with {} nodes", graph.nodes.len());
//!     }
//!     Diagram::Sequence(seq) => {
//!         println!("sequence with {} participants", seq.participants.len());
//!     }
//! }
//! ```
//!
//! ## MMDS interchange
//!
//! [MMDS](https://mmds.dev/) is a structured JSON format for diagram geometry.
//! Use the [`mmds`] module to parse MMDS input, hydrate it to a
//! [`graph::Graph`], or regenerate Mermaid source. To render MMDS input to
//! text/SVG, pass it to [`render_diagram`] which auto-detects MMDS:
//!
//! ```
//! use mmdflux::mmds::{from_str, generate_mermaid_from_str};
//!
//! let mmds_json = r#"{
//!   "version": 1,
//!   "profiles": ["mmds-core-v1"],
//!   "defaults": {
//!     "node": { "shape": "rectangle" },
//!     "edge": { "stroke": "solid", "arrow_start": "none", "arrow_end": "normal", "minlen": 1 }
//!   },
//!   "geometry_level": "layout",
//!   "metadata": {
//!     "diagram_type": "flowchart",
//!     "direction": "TD",
//!     "bounds": { "width": 100.0, "height": 80.0 }
//!   },
//!   "nodes": [
//!     { "id": "A", "label": "Start", "position": { "x": 50.0, "y": 20.0 },
//!       "size": { "width": 50.0, "height": 20.0 } },
//!     { "id": "B", "label": "End", "position": { "x": 50.0, "y": 60.0 },
//!       "size": { "width": 50.0, "height": 20.0 } }
//!   ],
//!   "edges": [{ "id": "e0", "source": "A", "target": "B" }]
//! }"#;
//!
//! // Hydrate to graph IR
//! let graph = from_str(mmds_json).unwrap();
//! assert_eq!(graph.nodes.len(), 2);
//!
//! // Regenerate Mermaid source
//! let mermaid = generate_mermaid_from_str(mmds_json).unwrap();
//! assert!(mermaid.contains("flowchart TD"));
//! ```

pub mod builtins;
mod diagrams;
mod engines;
pub mod errors;
pub mod format;
mod frontends;
pub mod graph;
mod mermaid;
pub mod mmds;
pub mod payload;
pub mod registry;
mod render;
// Facade functions and config_input are re-exported below as public API.
pub(crate) mod runtime;
pub mod simplification;
pub mod timeline;

// Public re-exports from public modules (convenience aliases).
// Re-exports from public modules for convenience at crate root.
/// Algorithm identifier (e.g., `Layered`, `Mrtree`) used in engine selection.
pub use engines::graph::AlgorithmId;
/// Combined engine + algorithm identifier for explicit layout engine selection.
pub use engines::graph::EngineAlgorithmId;
/// Engine identifier (e.g., `Flux`, `Mermaid`, `Elk`).
pub use engines::graph::EngineId;
pub use errors::RenderError;
/// Policy for resolving `--color auto` in CLI/WASM adapters.
pub use format::ColorWhen;
pub use format::OutputFormat;
/// Text output color mode (plain, styled, or ANSI).
pub use format::TextColorMode;
pub use runtime::config::RenderConfig;
/// Layout configuration for the Sugiyama hierarchical engine.
pub use runtime::config::{LabelDummyStrategy, LayoutConfig, LayoutDirection, Ranker};
/// Serde-friendly config input for JSON consumers (WASM, API).
pub use runtime::config_input::RuntimeConfigInput;
/// Apply default SVG surface settings (curve, engine) when format is SVG.
pub use runtime::config_input::apply_svg_surface_defaults;
/// Detect the diagram type from input text.
pub use runtime::detect_diagram;
/// Detect, parse, and render a diagram in one call.
pub use runtime::render_diagram;
/// Validate input and return structured JSON diagnostics.
pub use runtime::validate_diagram;

// Residual crate-local tests stay narrowly scoped to cross-pipeline coverage.
#[cfg(test)]
mod internal_tests;
