# mmdflux

[![Crates.io](https://img.shields.io/crates/v/mmdflux)](https://crates.io/crates/mmdflux)
[![docs.rs](https://img.shields.io/docsrs/mmdflux)](https://docs.rs/mmdflux)
[![CI](https://github.com/kevinswiber/mmdflux/actions/workflows/ci.yml/badge.svg)](https://github.com/kevinswiber/mmdflux/actions/workflows/ci.yml)

Render Mermaid diagrams as terminal text, SVG, and structured JSON.

mmdflux is a diagram rendering toolkit built in Rust. It ships as a CLI, a
Rust library, and a WebAssembly package — all from the same codebase. It
includes its own graph layout engine with native orthogonal routing, a
character-grid renderer for terminal output, and
[MMDS](docs/mmds.md) — a structured JSON format designed for tooling, adapters,
and LLM/agent pipelines.

[Playground](https://play.mmdflux.com) · [Releases](https://github.com/kevinswiber/mmdflux/releases) · [Gallery](docs/gallery.md) · [MMDS Spec](docs/mmds.md)

## At a glance

One Mermaid source, multiple outputs: terminal text, SVG, and machine-readable JSON.

**Mermaid source** ([`docs/assets/readme/at-a-glance.mmd`](docs/assets/readme/at-a-glance.mmd))

```
graph TD
    subgraph sg1[Horizontal Section]
        direction LR
        A[Step 1] --> B[Step 2] --> C[Step 3]
    end
    Start --> A
    C --> End
```

**SVG output** (`mmdflux --format svg --layout-engine flux-layered --curve linear-rounded ...`)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/kevinswiber/mmdflux/main/docs/assets/readme/at-a-glance-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/kevinswiber/mmdflux/main/docs/assets/readme/at-a-glance-light.svg">
  <img alt="mmdflux at-a-glance SVG output" src="https://raw.githubusercontent.com/kevinswiber/mmdflux/main/docs/assets/readme/at-a-glance-light.svg" width="360">
</picture>

**Text output** (`mmdflux --format text ...`)

```text
 ┌───────┐
 │ Start │
 └───────┘
     │
     └─┐
       │
       │
       │
       │
       │
       │
       │
       │
       │
       │
┌──────┼─ Horizontal Section ────────┐
│      ▼                             │
│ ┌────────┐  ┌────────┐  ┌────────┐ │
│ │ Step 1 │─►│ Step 2 │─►│ Step 3 │ │
│ └────────┘  └────────┘  └────────┘ │
│                     ┌────┘         │
└─────────────────────┼──────────────┘
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      │
                      ▼
                 ┌─────┐
                 │ End │
                 └─────┘
```

**MMDS JSON output**: [`docs/assets/readme/at-a-glance.mmds.json`](docs/assets/readme/at-a-glance.mmds.json)

## Why mmdflux

**No runtime dependencies.** A single compiled binary. No Node.js, no
headless browser, no Puppeteer.

**Terminal text is a first-class output.** Text rendering isn't a
secondary mode — it has its own grid layout system, orthogonal edge routing,
and Unicode box-drawing characters designed to be readable in a terminal.

**Native orthogonal routing.** The `flux-layered` engine treats layout and
routing as one solve contract. Edges follow right-angle paths with
deterministic fan-in/fan-out policies and shape-aware attachment points —
addressing one of the most common complaints about Mermaid's rendering.

**Structured JSON for tooling and agents.** MMDS (Machine-Mediated Diagram
Specification) outputs positioned graph data — node coordinates, routed edge
paths, subgraph bounds — so downstream tools can consume diagram geometry
without parsing SVG or scraping pixels.

**Compound graph layout.** Subgraphs are laid out as part of a single
compound graph, not rendered recursively. This produces globally optimized
positioning and consistent cross-boundary edge routing.

**Multiple engines for graph-family diagrams.** The default `flux-layered`
engine handles flowchart/class text, SVG, and MMDS output. Switch to
`mermaid-layered` for Mermaid-compatible graph output.

## Ecosystem

| Package                                                              | Description                                                   |
| -------------------------------------------------------------------- | ------------------------------------------------------------- |
| [`mmdflux`](https://crates.io/crates/mmdflux)                        | CLI and Rust library (crates.io)                              |
| [`@mmds/wasm`](https://www.npmjs.com/package/@mmds/wasm)             | WebAssembly bindings (npm)                                    |
| [`@mmds/core`](https://www.npmjs.com/package/@mmds/core)             | MMDS normalization, traversal, and validation utilities (npm) |
| [`@mmds/excalidraw`](https://www.npmjs.com/package/@mmds/excalidraw) | MMDS to Excalidraw `.excalidraw` JSON (npm)                   |
| [`@mmds/tldraw`](https://www.npmjs.com/package/@mmds/tldraw)         | MMDS to tldraw `.tldr` JSON (npm)                             |
| [Playground](https://play.mmdflux.com)                               | Interactive browser editor (WASM-powered)                     |

## Install

### Homebrew (recommended)

```bash
brew tap kevinswiber/mmdflux
brew install mmdflux
```

### Cargo

```bash
cargo install mmdflux
```

### Prebuilt binaries

Download platform binaries from [GitHub Releases](https://github.com/kevinswiber/mmdflux/releases).

## Quick Start

```bash
# Render a Mermaid file to text (default format)
mmdflux diagram.mmd

# Read Mermaid from stdin
printf 'graph LR\nA-->B\n' | mmdflux

# Disable ANSI color for text/ascii output
NO_COLOR=1 mmdflux --format text diagram.mmd

# Override NO_COLOR for a single invocation
NO_COLOR=1 mmdflux --format text --color always diagram.mmd

# SVG output
mmdflux --format svg diagram.mmd -o diagram.svg

# MMDS JSON output with routed geometry detail
mmdflux --format mmds --geometry-level routed diagram.mmd

# Lint mode (validate input and print diagnostics)
mmdflux --lint diagram.mmd
```

See more examples in the sections below.

## What It Supports

- **Diagram types:** flowchart, class, sequence
- **Output formats:** Unicode text, ASCII text, SVG, MMDS JSON
- **Layout directions:** `TD`, `BT`, `LR`, `RL` (with per-subgraph overrides)
- **Edge styles:** solid, dotted, thick, invisible, cross-arrow, circle-arrow
- **Routing:** orthogonal, polyline, direct (with curve options: basis, linear, linear-rounded, linear-sharp)
- **Round-trip conversion:** Mermaid to MMDS and MMDS back to Mermaid

## Graph-Family Engines

|            | `flux-layered`                  | `mermaid-layered`         |
| ---------- | ------------------------------- | ------------------------- |
| Applies to | Flowchart/class text, SVG, MMDS | Flowchart/class SVG, MMDS |
| Routing    | Orthogonal, polyline, direct    | Polyline                  |
| Subgraphs  | Compound graph (global layout)  | Compound graph            |
| Best fit   | Deterministic routed output     | Mermaid-compatible output |

Sequence diagrams use a separate timeline renderer, currently support text/ascii
output only, and do not accept `--layout-engine`.

### SVG edge presets

| Preset                  | Routing    | Curve         |
| ----------------------- | ---------- | ------------- |
| `smooth-step` (default) | Orthogonal | Rounded arcs  |
| `curved-step`           | Orthogonal | Basis spline  |
| `step`                  | Orthogonal | Sharp corners |
| `polyline`              | Polyline   | Sharp corners |
| `straight`              | Direct     | Sharp corners |

```bash
# Smooth orthogonal corners (default)
mmdflux --format svg diagram.mmd -o diagram.svg

# Curved orthogonal basis paths
mmdflux --format svg --edge-preset curved-step diagram.mmd -o diagram.svg

# Explicit curve control
mmdflux --format svg --curve linear-rounded diagram.mmd -o diagram.svg
```

## Documentation

- [Gallery](docs/gallery.md) — rendered output for 110 fixtures
- [MMDS specification](docs/mmds.md) — structured JSON format
- [Edge routing design](docs/edge-routing-heuristics.md) — routing internals

## Rust API Surface

Most Rust integrations should stay on the high-level runtime facade:

- `render_diagram`
- `detect_diagram`
- `validate_diagram`

Pair those entrypoints with `RenderConfig` and `OutputFormat` unless you are
building an adapter or tooling layer that needs explicit preparation control.

The low-level API is smaller and adapter-focused:

- `mmdflux::builtins::default_registry()` for the builtin diagram registry
- `registry` and `payload` for explicit detect/parse/payload flows
- `mmds` for MMDS parsing, replay, and Mermaid generation

The rest of the crate tree (`diagrams`, `engines`, `graph`, `render`,
`mermaid`, and `timeline`) consists of internal implementation modules and is
not part of the supported public contract.

### Examples

- [`examples/high_level_render.rs`](examples/high_level_render.rs) — top-level
  `render_diagram` workflow
- [`examples/registry_adapter.rs`](examples/registry_adapter.rs) — explicit
  registry-driven detection and preparation
- [`examples/mmds_replay.rs`](examples/mmds_replay.rs) — MMDS profile
  negotiation, replay, and Mermaid regeneration

Verify the examples compile with `cargo test --examples`.

## License

MIT
