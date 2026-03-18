# AGENTS.md

This file provides guidance to AI code assistants when working with code in this repository.

## Project Overview

mmdflux is a Rust CLI tool and library that parses Mermaid diagrams and renders them as text (Unicode/ASCII), SVG, or MMDS JSON. Supported diagram types: flowchart, class, sequence. It converts Mermaid syntax into terminal-friendly visualizations using Unicode box-drawing characters, with support for multiple layout directions (TD, BT, LR, RL), node shapes, edge styles, subgraphs with direction overrides, and structured JSON output (MMDS format).

## Common Commands

Use `just` (see `Justfile`) for day-to-day work. Tests use `cargo-nextest` for parallel execution.

```bash
just test                      # Run all tests (nextest, parallel)
just test-file integration     # Run a specific test file
just test -E 'test(test_name)' # Run a specific test (nextest filter)
just lint                      # clippy + fmt check
just check                     # lint + test
just build                     # Debug build
just release                   # Release build
just boundaries-watch         # Watch semantic architecture boundaries
just run diagram.mmd           # Run the CLI
just fmt                       # Format code

# Run CLI directly
cargo run -- diagram.mmd
cargo run -- --debug diagram.mmd
cargo run -- --ascii diagram.mmd
echo 'graph LR\nA-->B' | cargo run
```

## Architecture

See `docs/architecture/dependency-rules.md` for the authoritative module ownership rules and public contract tiers. The repo-owned architecture gate is `cargo xtask architecture` or `just architecture`. Semantic module boundaries are enforced from `boundaries.toml` (override with `SEMANTIC_BOUNDARIES_CONFIG`); run `cargo xtask architecture boundaries` or `just boundaries` when you want only the semantic dependency check.

When editing imports, top-level wiring, or ownership boundaries, run
`cargo xtask architecture boundaries` before finishing. During larger boundary
refactors, consider keeping `just boundaries-watch` running in a separate
terminal and pay attention to its output while you work.

Pipeline: **Frontend → Diagrams → Engine → Render**

```
Input Text → frontends.rs (detect frontend: Mermaid or MMDS)
  → mermaid/ (parse to AST) → diagrams/ (compile to IR, build payload)
  → runtime/ (orchestrate: registry → engine → render dispatch)
  → engines/ (solve graph layout → GraphGeometry)
  → render/ (emit Text/SVG/MMDS output)
```

### Public Contract Tiers

1. **Runtime facade**: `render_diagram`, `detect_diagram`, `validate_diagram` + `RenderConfig`, `OutputFormat`, `RenderError` re-exported from `lib.rs`
2. **Low-level API**: `builtins`, `registry`, `payload`, `graph`, `timeline`, `mmds` for adapter-oriented workflows
3. **Internal implementation**: `diagrams`, `engines`, `render`, `mermaid` — documented for contributors but not part of the supported contract

### Module Structure

**`src/frontends.rs`** — Source-format detection (Mermaid vs MMDS)

**`src/mermaid/`** — Mermaid source ingestion

- `grammar.pest` — PEG grammar definition
- `ast.rs` — Flowchart AST types (`ShapeSpec`, `Vertex`, `ConnectorSpec`, `EdgeSpec`, `Statement`)
- `flowchart.rs` — `parse_flowchart()` entry point
- `class/`, `sequence/` — Per-type parsers
- `error.rs` — `ParseError`, `ParseDiagnostic`

**`src/graph/`** — Graph-family IR, float-space geometry, routing, and style

- `diagram.rs` — `Graph` struct (nodes, edges, subgraphs, direction)
- `node.rs` — `Node` with `Shape` enum
- `edge.rs` — `Edge` with `Stroke` and `Arrow`
- `style.rs` — `NodeStyle`, `ColorToken`, style statement parsing
- `geometry.rs` — `GraphGeometry`, `RoutedGraphGeometry` (float-space layout results)
- `grid/` — Float-to-grid conversion, grid routing, replay geometry contracts
- `routing/` — Shared routing helpers (orthogonal routing, float routing)
- `attachment.rs`, `direction_policy.rs`, `measure.rs`, `projection.rs`, `space.rs`

**`src/diagrams/`** — Diagram type implementations (detect, compile, build payload)

- `flowchart/` — Flowchart: compiler to `graph::Graph`, validation warnings
- `class/` — Class diagrams: compiler to `graph::Graph`
- `sequence/` — Sequence diagrams: compiler to `timeline::Sequence`

Diagrams stop at `into_payload()` — they produce a `payload::Diagram`, not rendered output.

**`src/engines/`** — Engine adapters and layout algorithms

- `graph/contracts.rs` — `GraphEngine` trait, `GraphSolveRequest`, `EngineConfig`
- `graph/flux.rs` — `FluxLayeredEngine` (all formats)
- `graph/mermaid.rs` — `MermaidLayeredEngine` (SVG/MMDS only)
- `graph/elk.rs` — ELK subprocess adapter (behind `engine-elk` feature flag)
- `graph/algorithms/layered/` — Sugiyama hierarchical layout (~95% dagre v0.8.5 parity)
- `graph/algorithms/layered/kernel/` — Pure graph-agnostic layered engine (internal boundary)
- `graph/registry.rs` — `GraphEngineRegistry` with `EngineAlgorithmId`

**`src/render/`** — Output production

- `graph/` — Shared graph-family text and SVG emission from `GraphGeometry`
- `graph/text/` — Text-pipeline edge/node/subgraph rendering
- `graph/svg/` — SVG-pipeline rendering
- `diagram/` — Family-local renderers (sequence text)
- `text/` — Text utilities (`Canvas`, `CharSet`)

**`src/runtime/`** — Pipeline orchestration and config

- `mod.rs` — `render_diagram`, `validate_diagram`, `detect_diagram` facade functions
- `config.rs` — `RenderConfig`, `LayoutConfig`, `LayoutDirection`, `Ranker`
- `config_input.rs` — `RuntimeConfigInput` (serde-friendly config for JSON/WASM consumers)
- `graph_family.rs` — Graph-family solve-result dispatch
- `mmds.rs` — MMDS replay rendering (hydrate → render dispatch)
- `payload.rs` — Payload rendering dispatch

**`src/mmds/`** — MMDS interchange format (pure — no render or engines imports)

- `output.rs` — MMDS JSON serialization for graph-family output
- `detect.rs`, `parse.rs`, `hydrate.rs`, `mermaid.rs`

**Other top-level modules:**
- `format.rs` — `OutputFormat`, `Curve`, `EdgePreset`, `RoutingStyle`, `ColorWhen`, `TextColorMode`
- `errors.rs` — `RenderError`, `ParseDiagnostic`
- `registry.rs` — `DiagramRegistry`, `DiagramInstance`, `ParsedDiagram`, `DiagramFamily`
- `builtins.rs` — `default_registry()` wiring
- `payload.rs` — `payload::Diagram` enum (`Flowchart`, `Class`, `Sequence`)
- `simplification.rs` — Path simplification
- `timeline/` — `timeline::Sequence` and sequence layout

### Key Data Flow

1. `parse_flowchart(input)` → `Flowchart` AST
2. `compile_to_graph(&flowchart)` → `graph::Graph` with nodes/edges/subgraphs
3. `GraphEngine::solve()` → `GraphGeometry` (float coordinates, edge topology)
4. `route_graph_geometry()` → `RoutedGraphGeometry` (edge paths, attachment ports)
5. Text: `geometry_to_grid_layout_with_routed()` → `GridLayout` → `route_all_edges()` → `Canvas` → String
6. SVG: `render_svg_from_routed_geometry()` → SVG string
7. MMDS: `mmds::output::to_mmds_json_typed_with_routing()` → structured JSON

## Testing

Test fixtures are organized by diagram type:

- `tests/fixtures/flowchart/*.mmd` — flowchart fixtures
- `tests/fixtures/class/*.mmd` — class diagram fixtures
- `tests/fixtures/sequence/*.mmd` — sequence diagram fixtures

Snapshots follow the same structure: `tests/snapshots/flowchart/*.txt`, `tests/svg-snapshots/flowchart/*.svg`.

Key test files:

- `tests/integration_full.rs` — full-pipeline rendering tests
- `tests/compliance_class.rs` — class diagram compliance
- `tests/compliance_sequence.rs` — sequence diagram compliance
- `tests/mmds_json.rs` — MMDS JSON contract tests
- `tests/svg_render.rs` — SVG rendering tests
- `tests/cli.rs` — CLI integration tests
- `cargo xtask architecture` — repo-owned architecture policy enforcement
- `src/internal_tests/` — crate-local cross-pipeline tests (engine + routing + render)

The xtask architecture command enforces the semantic boundary rules in
`docs/architecture/dependency-rules.md`. `boundaries` owns semantic top-level
dependency policy across production and test code.

## Debug Infrastructure

The project includes tooling to compare mmdflux layout against dagre.js v0.8.5.

### Setup

```bash
./scripts/setup-debug-deps.sh    # Clone dagre and mermaid to deps/
```

### Parity Tests

```bash
cargo nextest run -E 'test(dagre_parity)'  # Compare layout against dagre.js fixtures
```

### Refreshing Fixtures

```bash
./scripts/refresh-parity-fixtures.sh   # Regenerate from dagre.js
```

### Debug Environment Variables

- `MMDFLUX_DEBUG_LAYOUT=<file>` - Write layout JSON
- `MMDFLUX_DEBUG_PIPELINE=<file>` - Write pipeline stages (JSONL)
- `MMDFLUX_DEBUG_BORDER_NODES=1` - Print border node trace
- `MMDFLUX_DEBUG_ORDER=1` - Order debug tracing
- `MMDFLUX_DEBUG_BK_TRACE=1` - Brandes-Köpf coordinate assignment trace

### Debug Scripts

- `scripts/dump-dagre-layout.js` - Run dagre.js layout
- `scripts/dump-dagre-pipeline.js` - Trace dagre pipeline stages
- `scripts/dump-dagre-borders.js` - Extract dagre border nodes
- `scripts/dump-dagre-order.js` - Dump node order per rank

See `docs/DEBUG.md` for comprehensive documentation.
