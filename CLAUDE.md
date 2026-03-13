# AGENTS.md

This file provides guidance to AI code assistants when working with code in this repository.

## Project Overview

mmdflux is a Rust CLI tool and library that parses Mermaid diagrams and renders them as text (Unicode/ASCII) or SVG. Supported diagram types: flowchart, class, sequence, pie, info, packet. It converts Mermaid syntax into terminal-friendly visualizations using Unicode box-drawing characters, with support for multiple layout directions (TD, BT, LR, RL), node shapes, edge styles, subgraphs with direction overrides, and structured JSON output (MMDS format).

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
just run diagram.mmd           # Run the CLI
just fmt                       # Format code

# Run CLI directly
cargo run -- diagram.mmd
cargo run -- --debug diagram.mmd
cargo run -- --ascii diagram.mmd
echo 'graph LR\nA-->B' | cargo run
```

## Architecture

See `docs/architecture/dependency-rules.md` for the authoritative module ownership rules and public contract tiers.

Pipeline: **Frontend → Diagrams → Engine → Render**

```
Input Text → frontends.rs (detect frontend: Mermaid or MMDS)
  → mermaid/ (parse to AST) → diagrams/ (compile to IR, prepare payload)
  → runtime/ (orchestrate: registry → engine → render dispatch)
  → engines/ (solve graph layout → GraphGeometry)
  → render/ (emit Text/SVG/MMDS output)
```

### Public Contract Tiers

1. **Runtime facade**: `render_diagram`, `detect_diagram`, `validate_diagram` + flat config/format/error types re-exported from `lib.rs`
2. **Supported expert tier**: `builtins`, `registry`, `prepared`, `mmds` for adapter-oriented workflows
3. **Internal implementation**: `diagrams`, `engines`, `graph`, `render`, `mermaid`, `timeline` — documented for contributors but not part of the supported contract

### Module Structure

**`src/frontends.rs`** — Source-format detection (Mermaid vs MMDS)

**`src/mermaid/`** — Mermaid source ingestion (all diagram types)

- `grammar.pest` — PEG grammar definition
- `ast.rs` — Flowchart AST types (`ShapeSpec`, `Vertex`, `ConnectorSpec`, `EdgeSpec`, `Statement`)
- `flowchart.rs` — `parse_flowchart()` entry point
- `class.rs`, `sequence.rs`, `pie.rs`, `info.rs`, `packet.rs` — Per-type parsers
- `error.rs` — `ParseError`, `ParseDiagnostic`

**`src/graph/`** — Graph-family IR, float-space geometry, and shared helpers

- `diagram.rs` — `Diagram` struct (nodes, edges, subgraphs, direction)
- `node.rs` — `Node` with `Shape` enum
- `edge.rs` — `Edge` with `Stroke` and `Arrow`
- `geometry.rs` — `GraphGeometry`, `RoutedGraphGeometry` (float-space layout results)
- `grid/` — Float-to-grid conversion, grid routing, replay geometry contracts
- `routing/` — Shared routing helpers (orthogonal routing, float routing)
- `attachment.rs`, `direction_policy.rs`, `measure.rs`, `projection.rs`, `space.rs`

**`src/diagrams/`** — Diagram type implementations (detect, parse delegation, compile, prepare)

- `flowchart/` — Flowchart: compiler to `graph::Diagram`, validation warnings
- `class/` — Class diagrams: compiler to `graph::Diagram`
- `sequence/` — Sequence diagrams: timeline-family pipeline (compiler → model → layout → text renderer)
- `pie.rs`, `info.rs`, `packet.rs` — Simple diagram types

Diagrams stop at `prepare()` — they produce a `PreparedDiagram` payload, not rendered output.

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
- `diagram/` — Family-local renderers (sequence text, pie text, packet text)
- `text/` — Text utilities (`Canvas`, `CharSet`, color)

**`src/runtime/`** — Pipeline orchestration

- `mod.rs` — `render_diagram`, `validate_diagram`, `detect_diagram` facade functions
- `graph_family.rs` — Graph-family solve-result dispatch
- `prepared.rs` — Prepared-diagram rendering dispatch

**`src/mmds/`** — MMDS contract and output

- `output.rs` — MMDS JSON serialization for graph-family output
- `detect.rs`, `parse.rs`, `hydrate.rs`, `replay.rs`, `mermaid.rs`

**Other top-level modules:**
- `config.rs` — `RenderConfig`
- `format.rs` — `OutputFormat` enum
- `errors.rs` — `RenderError`
- `diagnostics.rs` — `ParseDiagnostic` re-export
- `family.rs` — `DiagramFamily` enum
- `registry.rs` — `DiagramRegistry`, `DiagramInstance`, `ParsedDiagram` traits
- `builtins.rs` — `default_registry()` wiring
- `prepared.rs` — `PreparedDiagram` payload enum
- `simplification.rs` — Path simplification
- `style.rs` — Style parsing
- `timeline/sequence/` — Shared sequence-family model and layout types

### Key Data Flow

1. `parse_flowchart(input)` → `Flowchart` AST
2. `compile_to_graph(&flowchart)` → `Diagram` with nodes/edges/subgraphs
3. `GraphEngine::solve()` → `GraphGeometry` (float coordinates, edge topology)
4. `route_graph_geometry()` → `RoutedGraphGeometry` (edge paths, attachment ports)
5. Text: `geometry_to_grid_layout_with_routed()` → `GridLayout` → `route_all_edges()` → `Canvas` → String
6. SVG: `render_svg_from_routed_geometry()` → SVG string
7. MMDS: `mmds::output::render_mmds()` → structured JSON

## Testing

Test fixtures are organized by diagram type:

- `tests/fixtures/flowchart/*.mmd` — flowchart fixtures
- `tests/fixtures/class/*.mmd` — class diagram fixtures
- `tests/fixtures/sequence/*.mmd` — sequence diagram fixtures

Snapshots follow the same structure: `tests/snapshots/flowchart/*.txt`, `tests/svg-snapshots/flowchart/*.svg`.

Key test files:

- `tests/integration_full.rs` — full-pipeline rendering tests
- `tests/dagre_parity.rs` — layout comparison against dagre.js fixtures
- `tests/compliance_class.rs` — class diagram compliance
- `tests/compliance_sequence.rs` — sequence diagram compliance
- `tests/mmds_json.rs` — MMDS JSON contract tests
- `tests/svg_render.rs` — SVG rendering tests
- `tests/cli.rs` — CLI integration tests
- `tests/architecture_guards.rs` — module boundary and dependency rule enforcement
- `src/internal_tests/` — crate-local cross-pipeline tests (engine + routing + render)

Architecture guard tests enforce the rules in `docs/architecture/dependency-rules.md`. They verify module boundaries in both production code and test code.

## Debug Infrastructure

The project includes tooling to compare mmdflux layout against dagre.js v0.8.5.

### Setup

```bash
./scripts/setup-debug-deps.sh    # Clone dagre and mermaid to deps/
```

### Parity Tests

```bash
cargo test --test dagre_parity          # Compare layout against dagre.js fixtures
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
