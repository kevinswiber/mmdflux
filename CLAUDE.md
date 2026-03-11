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

Pipeline: **Parser → Graph → Engine → Render**

```
Mermaid Text → Parser (pest PEG) → AST → Graph Builder → Diagram
  → GraphEngine::solve() → GraphGeometry → Router → Renderer (Text/SVG/MMDS)
```

### Module Structure

**`src/parser/`** - Mermaid parsing

- `grammar.pest` - PEG grammar definition (header, nodes, edges, connectors)
- `ast.rs` - AST types: `ShapeSpec`, `Vertex`, `ConnectorSpec`, `EdgeSpec`, `Statement`
- `flowchart.rs` - `parse_flowchart()` entry point, converts pest tree to AST

**`src/graph/`** - Graph data structures

- `diagram.rs` - `Diagram` struct (nodes HashMap, edges Vec, direction)
- `node.rs` - `Node` with `Shape` enum (Rectangle, Round, Diamond, etc.)
- `edge.rs` - `Edge` with `Stroke` (Solid, Dotted, Thick) and `Arrow` (Normal, None)
- `builder.rs` - `build_diagram()` converts AST to Diagram

**`src/diagram.rs`** - Engine abstractions (`GraphEngine` trait, `EngineConfig`, `RenderConfig`, `GraphSolveRequest`/`Result`)

**`src/diagrams/`** - Diagram type implementations

- `flowchart/` - Flowchart: engine (`FluxLayeredEngine` for all formats, `MermaidLayeredEngine` for SVG/MMDS only), geometry IR, routing, render modules
- `class/` - Class diagrams: parser, compiler to `graph::Diagram`, renders through shared engine pipeline
- `sequence/` - Sequence diagrams: independent timeline-family pipeline (parser→compiler→model→layout→text renderer)
- `pie.rs`, `info.rs`, `packet.rs` - Simple diagram types

**`src/diagrams/flowchart/render/`** - Flowchart rendering (text + SVG)

Modules are prefixed by pipeline: `text_*` for character-grid rendering, `svg*` for SVG, unprefixed for shared.

- *Shared:* `layout_building.rs` (layered layout bridge), `layout_subgraph_ops.rs` (float-coord subgraph reconciliation), `orthogonal_router.rs`, `route_policy.rs`
- *Text pipeline:* `text_types.rs` (Layout, GridLayoutConfig, SubgraphBounds, etc.), `text_layout.rs` (text-specific layout logic), `text_adapter.rs` (engine geometry → text Layout), `text_edge.rs`, `text_shape.rs`, `text_router.rs`, `text_subgraph.rs`, `text_routing_core.rs`
- *SVG pipeline:* `svg.rs` (SVG rendering + layout), `svg_router.rs` (SVG edge routing), `svg_metrics.rs` (font metrics)

**`src/layered/`** - Hierarchical graph layout (Sugiyama framework, ~95% dagre v0.8.5 parity)

- `mod.rs` - `layout()` entry point, orchestrates the layout phases
- `graph.rs` - `DiGraph` input graph, `LayoutGraph` internal representation
- `acyclic.rs` - Cycle removal via DFS, tracks reversed edges
- `rank.rs` - Layer assignment using longest-path or network simplex
- `normalize.rs` - Long edge normalization (dummy nodes), edge label positioning
- `order.rs` - Crossing reduction via barycenter heuristic
- `position.rs` - Coordinate assignment using Brandes-Köpf algorithm
- `bk.rs` - Brandes-Köpf horizontal coordinate assignment with vertical alignment
- `types.rs` - `LayoutConfig`, `LayoutResult`, `Rect`, `Point`, `Direction`

**`src/render/`** - Top-level render orchestration

- `mod.rs` - `render()` entry point, dispatches to text or SVG pipeline; re-exports key types
- `canvas.rs` - 2D character grid with `strip_common_leading_whitespace()`
- `chars.rs` - `CharSet` for box-drawing characters (Unicode default, ASCII via `--ascii`)
- `intersect.rs` - Shared node-face intersection calculations

**`src/engines/`** - Engine adapters (ELK subprocess adapter behind `engine-elk` feature flag)

**`src/mmds.rs`** - MMDS JSON output (structured geometry export, version 2)

### Key Data Flow

1. `parse_flowchart(input)` → `Flowchart` AST
2. `build_diagram(&flowchart)` → `Diagram` with nodes/edges
3. `GraphEngine::solve()` → `GraphGeometry` (float coordinates, edge topology, subgraph bounds)
4. Text: `geometry_to_text_layout()` → `Layout` (integer character-grid coordinates)
5. Text: `route_all_edges()` → routed edge paths; `render_text_from_layout()` → `Canvas` → String
6. SVG: `render_svg_from_geometry()` → SVG string
7. MMDS: `mmds::render_json()` → structured JSON

## Testing

Test fixtures are organized by diagram type:

- `tests/fixtures/flowchart/*.mmd` — flowchart fixtures
- `tests/fixtures/class/*.mmd` — class diagram fixtures
- `tests/fixtures/sequence/*.mmd` — sequence diagram fixtures

Snapshots follow the same structure: `tests/snapshots/flowchart/*.txt`, `tests/svg-snapshots/flowchart/*.svg`.

Key test files:

- `tests/integration.rs` — flowchart parsing, building, rendering
- `tests/dagre_parity.rs` — layout comparison against dagre.js fixtures
- `tests/compliance_class.rs` — class diagram compliance
- `tests/compliance_sequence.rs` — sequence diagram compliance
- `tests/mmds_json.rs` — MMDS JSON contract tests
- `tests/svg_render.rs` — SVG rendering tests
- `tests/cli.rs` — CLI integration tests

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
