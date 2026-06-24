//! mmdflux — Mermaid diagrams to text, SVG, and MMDS
//!
//! mmdflux parses [Mermaid](https://mermaid.js.org/) diagram syntax and
//! renders it as Unicode/ASCII text, SVG, or structured JSON (MMDS).
//! Supported diagram types: **flowchart**, **class**, **state**, and **sequence**.
//!
//! # High-Level API
//!
//! Most consumers only need the facade functions and two config types:
//!
//! - [`render_diagram`] — render Mermaid source or MMDS JSON in one call
//! - [`materialize_diagram`] — materialize Mermaid source or MMDS JSON as a
//!   graph-family [`mmds::Document`]
//! - [`render_document`] — render an already-parsed [`mmds::Document`]
//! - [`detect_diagram`] — detect the diagram type without rendering
//! - [`validate_diagram`] — parse and return structured JSON diagnostics
//! - [`OutputFormat`] — `Text`, `Ascii`, `Svg`, or `Mmds`
//! - [`RenderConfig`] — layout engine, routing, padding, color, and more
//!
//! Use [`render_diagram`] when you have text input. It auto-detects Mermaid
//! source vs MMDS JSON because both represent the same abstract diagram. Use
//! [`materialize_diagram`] when you want the typed [`mmds::Document`], and use
//! [`render_document`] when you already have one, such as after
//! [`views::project`].
//! For document-level workflows, use `commands::apply` to mutate a document,
//! `mmds::diff::diff_documents` to compare two snapshots, and
//! `views::project` to produce a read-side view.
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
//!
//! // MMDS JSON is also accepted as text input.
//! let replayed = render_diagram(&json, OutputFormat::Text, &RenderConfig::default()).unwrap();
//! assert!(!replayed.trim().is_empty());
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
//! - [`views`] — materialized read-side views over canonical
//!   [`mmds::Document`] payloads
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
//!     Diagram::State(graph) => {
//!         println!("state diagram with {} nodes", graph.nodes.len());
//!     }
//!     Diagram::Sequence(seq) => {
//!         println!("sequence with {} participants", seq.participants.len());
//!     }
//! }
//! ```
//!
//! ## MMDS interchange
//!
//! MMDS is mmdflux's structured JSON format for diagram geometry.
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
//!
//! ## Commands and Model Events
//!
//! Public MMDS commands mutate a document and emit model events that describe
//! the accepted state transition:
//!
//! ```
//! use mmdflux::commands::{Command, apply};
//! use mmdflux::graph::Shape;
//! use mmdflux::mmds::Subject;
//! use mmdflux::mmds::events::ModelEventKind;
//! use mmdflux::{RenderConfig, materialize_diagram};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut document = materialize_diagram(
//!     "graph TD\n    A[Gateway] --> B[Billing]",
//!     &RenderConfig::default(),
//! )?;
//!
//! let model_events = apply(
//!     &Command::AddNode {
//!         id: "C".to_string(),
//!         label: "Auth".to_string(),
//!         shape: Shape::Rectangle,
//!         parent: None,
//!     },
//!     &mut document,
//! )?;
//!
//! assert!(model_events.iter().any(|event| {
//!     event.kind == ModelEventKind::NodeAdded
//!         && matches!(&event.subject, Subject::Node(id) if id == "C")
//! }));
//! assert!(document.nodes.iter().any(|node| node.id == "C"));
//! # Ok(())
//! # }
//! ```
//!
//! ## Snapshot Diffs
//!
//! [`mmds::diff::diff_documents`] compares two document states and reports a
//! snapshot diff. Diff changes describe what is different between snapshots;
//! they do not describe edit intent or command history:
//!
//! ```
//! use mmdflux::mmds::Subject;
//! use mmdflux::mmds::diff::{ChangeKind, diff_documents};
//! use mmdflux::{RenderConfig, materialize_diagram};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let before = materialize_diagram(
//!     "graph TD\n    A[Gateway] --> B[Billing]",
//!     &RenderConfig::default(),
//! )?;
//! let after = materialize_diagram(
//!     "graph TD\n    A[Gateway] --> B[Billing]\n    A --> C[Auth]",
//!     &RenderConfig::default(),
//! )?;
//!
//! let diff = diff_documents(&before, &after);
//! assert!(diff.changes.iter().any(|change| {
//!     change.kind == ChangeKind::NodeAdded
//!         && matches!(&change.subject, Subject::Node(id) if id == "C")
//! }));
//! # Ok(())
//! # }
//! ```
//!
//! ## Views
//!
//! Use [`views`] when an adapter needs a focused read model over a canonical
//! MMDS payload. V1 views preserve shared coordinates, keep surviving edge IDs
//! sparse and stable, and return [`views::ViewEvent`] values for omitted
//! elements:
//!
//! ```
//! use mmdflux::mmds::Document;
//! use mmdflux::views::{
//!     AnchorRef, Selector, TraversalDirection, ViewSpec, ViewStatement, project,
//! };
//! use mmdflux::{OutputFormat, RenderConfig, materialize_diagram, render_document};
//!
//! let source = "\
//! graph TD
//! A[Gateway] --> B[Auth]
//! B --> C[Database]
//! A --> D[Audit]
//! ";
//! let canonical: Document = materialize_diagram(source, &RenderConfig::default()).unwrap();
//! let spec = ViewSpec::new(vec![ViewStatement::Include(Selector::Traversal {
//!         anchor: AnchorRef::Node("A".to_string()),
//!         direction: TraversalDirection::Downstream,
//!         hops: 1,
//!     })]);
//!
//! let (view, events) = project(&canonical, &spec).unwrap();
//! let text = render_document(&view, OutputFormat::Text, &RenderConfig::default()).unwrap();
//! assert!(text.contains("Gateway"));
//! assert_eq!(view.nodes.len(), 3);
//! assert!(events.iter().any(|event| matches!(
//!     event,
//!     mmdflux::views::ViewEvent::NodeLeftView { id, .. } if id == "C"
//! )));
//! ```
//!
//! # Stability
//!
//! The `commands`, `mmds::events`, `mmds::diff`, and `views` modules are early
//! surfaces. The following kinds of changes will land in minor versions and are
//! not considered breaking under this crate's SemVer policy:
//!
//! - new variants on the early-surface enums marked `#[non_exhaustive]`
//!   (including `commands::Command`, `commands::EdgeSelector`,
//!   `commands::CommandApplyError`, `mmds::events::ModelEventKind`,
//!   `mmds::diff::ChangeKind`, `mmds::Subject`, `views::ViewStatement`,
//!   `views::Selector`, `views::ViewEvent`, `views::ViewError`, and the
//!   supporting view vocabulary)
//! - new fields on early-surface structs marked `#[non_exhaustive]` (including
//!   `mmds::events::ModelEvent`, `mmds::diff::Change`, `mmds::diff::Diff`,
//!   `mmds::MmdsTokenError`, and `views::ViewSpec`)
//!
//! What is not covered:
//!
//! - Renaming, removing, or changing the meaning of an existing variant or field
//!   remains a breaking change.
//! - Adding fields to existing struct-like enum variants
//!   (for example, `Command::AddNode { ... }`) is still breaking; individual
//!   variants are not currently marked `#[non_exhaustive]`.
//! - The runtime facade (`render_diagram`, `materialize_diagram`,
//!   `render_document`, `detect_diagram`, `validate_diagram`, `OutputFormat`,
//!   `RenderConfig`, `RenderError`) follows standard SemVer.
//! - `views::TraversalDirection` is intentionally exhaustive because its
//!   vocabulary is closed.

pub mod builtins;
pub mod commands;
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
pub mod views;

// Public re-exports from public modules (convenience aliases).
// Re-exports from public modules for convenience at crate root.
/// Algorithm identifier (e.g., `Layered`, `Mrtree`) used in engine selection.
pub use engines::graph::AlgorithmId;
/// Combined engine + algorithm identifier for explicit layout engine selection.
pub use engines::graph::EngineAlgorithmId;
/// Engine identifier (e.g., `Flux`, `Mermaid`, `Elk`).
pub use engines::graph::EngineId;
pub use errors::RenderError;
/// Policy for resolving `--color auto` in CLI/Wasm adapters.
pub use format::ColorWhen;
pub use format::OutputFormat;
/// Text output color mode (plain, styled, or ANSI).
pub use format::TextColorMode;
pub use runtime::config::{GraphTextStyleConfig, RenderConfig, SvgThemeConfig, SvgThemeMode};
/// Layout configuration for the Sugiyama hierarchical engine.
pub use runtime::config::{
    LabelDummyPlacement, LabelDummyRouting, LayoutConfig, LayoutDirection, Ranker,
    SubgraphTitleMargin,
};
/// Serde-friendly config input for JSON consumers (Wasm, API).
pub use runtime::config_input::RuntimeConfigInput;
/// Apply default SVG surface settings (curve, engine) when format is SVG.
pub use runtime::config_input::apply_svg_surface_defaults;
/// Detect the diagram type from input text.
pub use runtime::detect_diagram;
// Keep this re-export gate in lockstep with the runtime module gate so the
// experimental dynamic metrics bridge stays opt-in and doc-hidden.
#[cfg(feature = "unstable-text-metrics-provider")]
#[doc(hidden)]
pub use runtime::dynamic_text_metrics;
/// Direct graph-in / geometry-out layout facade
/// ([`layout::layout_graph`], [`layout::LaidOutGraph`]).
pub use runtime::layout;
/// Detect, parse, solve, and materialize Mermaid source or MMDS JSON as MMDS.
pub use runtime::materialize_diagram;
/// Detect, parse, and render Mermaid source or MMDS JSON in one call.
pub use runtime::render_diagram;
/// Render a parsed graph-family MMDS document.
pub use runtime::render_document;
/// Validate input and return structured JSON diagnostics.
pub use runtime::validate_diagram;

// Residual crate-local tests stay narrowly scoped to cross-pipeline coverage.
#[cfg(test)]
mod test_tracing;

#[cfg(test)]
mod internal_tests;
