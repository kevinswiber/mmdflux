# AGENTS.md

This file provides guidance to AI code assistants when working with code in this repository.

## Project Overview

mmdflux is a Rust CLI tool and library that parses Mermaid diagrams and renders them as text (Unicode/ASCII), SVG, or MMDS JSON. Supported diagram types: flowchart, class, sequence. It converts Mermaid syntax into terminal-friendly visualizations using Unicode box-drawing characters, with support for multiple layout directions (TD, BT, LR, RL), node shapes, edge styles, subgraphs with direction overrides, and structured JSON output (MMDS format).

## Commit Conventions

This project uses [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), enforced by [cocogitto](https://docs.cocogitto.io/) via a `commit-msg` git hook.

Format: `<type>(<optional scope>): <subject>`

Types: `feat`, `fix`, `perf`, `revert`, `docs`, `test`, `build`, `ci`, `refactor`, `chore`, `style`

Scopes: `wasm`, `xtask`, `web`, `mmds-core`, `excalidraw`, `tldraw` (match monorepo packages). Omit scope for changes to the root `mmdflux` crate.

Rules:
- Header must be 100 characters or fewer
- Subject must start with a lowercase letter
- Subject must not end with a period
- Use imperative mood ("add feature" not "added feature")

For non-trivial changes, include a body after a blank line explaining **what** changed and **why**. A one-liner is fine for truly simple changes (typo fixes, version bumps), but multi-file changes, bug fixes, and new features should have a body.

Use `cog check` to validate commit history and `cog changelog` to preview changelog output. Use `git commit` (not `cog commit`) for creating commits — the commit-msg hook handles validation automatically.

## Common Commands

Use `just` (see `Justfile`) for day-to-day work. Tests use `cargo-nextest` for parallel execution.

```bash
just test                      # Run all tests (nextest, parallel)
just test-file integration     # Run a specific test file
just test -E 'test(test_name)' # Run a specific test (nextest filter)
just lint                      # clippy + fmt check
just check                     # lint + test + architecture
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

See `docs/architecture/dependency-rules.md` for the authoritative module ownership rules and public contract tiers. The repo-owned architecture gate is `cargo xtask architecture` or `just architecture`. Semantic module boundaries are enforced from `boundaries.toml` (override with `SEMANTIC_BOUNDARIES_CONFIG`); run `cargo xtask architecture check` or `just boundaries` for the semantic dependency check.

When editing imports, top-level wiring, or ownership boundaries, run
`cargo xtask architecture check` before finishing. During larger boundary
refactors, keep `just boundaries-watch` running in a separate terminal.

Inspection commands:
- `just boundaries-graph` — print dependency graph as Mermaid
- `just boundaries-explain --edge <source> <target>` — inspect a specific edge
- `just boundaries-explain --boundary <name>` — inspect a boundary

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
