# AGENTS.md

This file provides guidance to AI code assistants when working with code in this repository.

## Project Overview

mmdflux is a Rust CLI tool and library that parses Mermaid flowchart diagrams and renders them as text. It converts Mermaid syntax into terminal-friendly visualizations using Unicode box-drawing characters, with support for multiple layout directions (TD, BT, LR, RL), node shapes (rectangle, rounded, diamond), and edge styles (solid, dotted, thick).

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

Four-stage pipeline: **Parser → Graph → Layout → Render**

```
Mermaid Text → Parser (pest PEG) → AST → Graph Builder → Diagram → Layered Layout → Router → Canvas → Text
```

### Module Structure

**`src/parser/`** - Mermaid parsing

- `grammar.pest` - PEG grammar definition (header, nodes, edges, connectors)
- `ast.rs` - AST types: `ShapeSpec`, `Vertex`, `ConnectorSpec`, `EdgeSpec`, `Statement`
- `flowchart.rs` - `parse_flowchart()` entry point, converts pest tree to AST

**`src/graph/`** - Graph data structures

- `diagram.rs` - `Diagram` struct (nodes HashMap, edges Vec, direction)
- `node.rs` - `Node` with `Shape` enum (Rectangle, Round, Diamond)
- `edge.rs` - `Edge` with `Stroke` (Solid, Dotted, Thick) and `Arrow` (Normal, None)
- `builder.rs` - `build_diagram()` converts AST to Diagram

**`src/layered/`** - Hierarchical graph layout (Sugiyama framework, based on dagre.js)

- `mod.rs` - `layout()` entry point, orchestrates the layout phases
- `graph.rs` - `DiGraph` input graph, `LayoutGraph` internal representation
- `acyclic.rs` - Cycle removal via DFS, tracks reversed edges
- `rank.rs` - Layer assignment using longest-path algorithm
- `normalize.rs` - Long edge normalization (dummy nodes), edge label positioning
- `order.rs` - Crossing reduction via barycenter heuristic
- `position.rs` - Coordinate assignment using Brandes-Köpf algorithm
- `bk.rs` - Brandes-Köpf horizontal coordinate assignment with vertical alignment
- `types.rs` - `LayoutConfig`, `LayoutResult`, `Rect`, `Point`, `Direction`

**`src/render/`** - Text rendering

- `layout.rs` - `compute_layout()` bridges Diagram to layered layout, converts to draw coordinates
- `router.rs` - `route_edge()` and `route_backward_edge()` compute paths using waypoints
- `edge.rs` - `render_edge()` draws edges with arrows and labels
- `shape.rs` - `render_node()` draws node shapes
- `canvas.rs` - 2D character grid with `strip_common_leading_whitespace()`
- `chars.rs` - `CharSet` for box-drawing characters (Unicode default, ASCII via `--ascii`)

### Key Data Flow

1. `parse_flowchart(input)` → `Flowchart` AST
2. `build_diagram(&flowchart)` → `Diagram` with nodes/edges
3. `layered::layout()` → Sugiyama layout (acyclic → rank → normalize → order → position)
4. `compute_layout(&diagram, &config)` → `Layout` with draw coordinates and waypoints
5. `route_edge()` / `route_backward_edge()` → edge paths using waypoints
6. `render()` → `Canvas` → String output

## Testing

Test fixtures are organized by diagram type:

- `tests/fixtures/flowchart/*.mmd` — flowchart fixtures (70 files)
- `tests/fixtures/class/*.mmd` — class diagram fixtures
- `tests/fixtures/sequence/*.mmd` — sequence diagram fixtures

Flowchart fixture categories:

- **Basic flows**: `simple.mmd`, `chain.mmd`, `ampersand.mmd`
- **Node shapes**: `decision.mmd`, `shapes.mmd`, `diamond_fan.mmd`
- **Backward edges/cycles**: `simple_cycle.mmd`, `multiple_cycles.mmd`
- **Edge variations**: `labeled_edges.mmd`, `edge_styles.mmd`, `label_spacing.mmd`
- **Directions**: `left_right.mmd`, `right_left.mmd`, `bottom_top.mmd`
- **Fan patterns**: `fan_in.mmd`, `fan_out.mmd`, `fan_in_lr.mmd`, `five_fan_in.mmd`, `narrow_fan_in.mmd`, `stacked_fan_in.mmd`
- **Long edges**: `double_skip.mmd`, `skip_edge_collision.mmd`
- **Complex examples**: `complex.mmd`, `http_request.mmd`, `ci_pipeline.mmd`, `git_workflow.mmd`

Snapshots follow the same structure: `tests/snapshots/flowchart/*.txt`, `tests/svg-snapshots/flowchart/*.svg`.

Integration tests in `tests/integration.rs` verify parsing, building, and rendering.

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
