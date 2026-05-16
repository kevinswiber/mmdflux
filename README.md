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

<!-- mmdflux-readme-assets:source begin -->
```
graph TD
    subgraph sg1[Horizontal Section]
        direction LR
        A[Step 1] --> B[Step 2] --> C[Step 3]
    end
    Start --> A
    C --> End
```
<!-- mmdflux-readme-assets:source end -->

**SVG output** (`mmdflux --format svg --layout-engine flux-layered --curve linear-rounded ...`)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/kevinswiber/mmdflux/main/docs/assets/readme/at-a-glance-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/kevinswiber/mmdflux/main/docs/assets/readme/at-a-glance-light.svg">
  <img alt="mmdflux at-a-glance SVG output" src="https://raw.githubusercontent.com/kevinswiber/mmdflux/main/docs/assets/readme/at-a-glance-light.svg" width="360">
</picture>

**Text output** (`mmdflux --format text ...`)

<!-- mmdflux-readme-assets:text begin -->
```text
               ┌───────┐
               │ Start │
               └───────┘
                   │
      ┌────────────┘
      │
      │
┌─────┼── Horizontal Section ────────┐
│     ▼                              │
│ ┌────────┐  ┌────────┐  ┌────────┐ │
│ │ Step 1 │─►│ Step 2 │─►│ Step 3 │ │
│ └────────┘  └────────┘  └────────┘ │
│                             │      │
└─────────────────────────────┼──────┘
                              │
                   ┌──────────┘
                   │
                   ▼
                ┌─────┐
                │ End │
                └─────┘
```
<!-- mmdflux-readme-assets:text end -->

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

**Typed diagram edits and events.** Rust adapters can materialize MMDS, apply
`Command` values, inspect accepted `ModelEvent`s, and hand the edited document
back to the renderer without a JSON round trip.

**Materialized diagram views.** Rust adapters can filter canonical MMDS payloads
into read-side views for focused rendering, traversal, or event correlation
without inventing a separate graph query language.

**Compound graph layout.** Subgraphs are laid out as part of a single
compound graph, not rendered recursively. This produces globally optimized
positioning and consistent cross-boundary edge routing.

**Multiple engines for graph-family diagrams.** The default `flux-layered`
engine handles flowchart/class/state text, SVG, and MMDS output. Switch to
`mermaid-layered` for Mermaid-compatible graph output.

## Ecosystem

| Package                                                              | Description                                                   |
| -------------------------------------------------------------------- | ------------------------------------------------------------- |
| [`mmdflux`](https://crates.io/crates/mmdflux)                        | CLI and Rust library (crates.io)                              |
| [`@mmds/wasm`](https://www.npmjs.com/package/@mmds/wasm)             | WebAssembly bindings (npm)                                    |
| [`@mmds/core`](https://www.npmjs.com/package/@mmds/core)             | MMDS normalization, traversal, and validation utilities (npm) |
| [`@mmds/excalidraw`](https://www.npmjs.com/package/@mmds/excalidraw) | MMDS to Excalidraw `.excalidraw` JSON (npm)                   |
| [`@mmds/tldraw`](https://www.npmjs.com/package/@mmds/tldraw)         | MMDS to tldraw `.tldr` JSON (npm)                             |
| [Playground](https://play.mmdflux.com)                               | Interactive browser editor (Wasm-powered)                     |

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

# SVG output with a named theme
mmdflux --format svg --svg-theme dark diagram.mmd -o diagram.svg

# SVG output with terminal-aware light/dark theme selection
mmdflux --format svg --svg-theme-auto diagram.mmd -o diagram.svg

# Browser-oriented SVG with CSS variables plus hex fallbacks
mmdflux --format svg --svg-theme dark --svg-theme-mode dynamic diagram.mmd -o diagram.svg

# MMDS JSON output with routed geometry detail
mmdflux --format mmds --geometry-level routed diagram.mmd

# Lint mode (validate input and print diagnostics)
mmdflux --lint diagram.mmd
```

With ANSI enabled, text/ascii output maps Mermaid styling to terminal colors where it has a clear analogue: node `style`/`classDef` `fill`, `stroke`, and `color` drive node background, border, and label color; flowchart `linkStyle ... stroke:<color>` drives edge and arrow foreground color; SVG-specific properties such as `stroke-width` and `stroke-dasharray` remain no-ops in text/ascii output.

The SVG output exposes Mermaid-compatible CSS hooks: each subgraph is wrapped in `<g class="cluster {userClasses}" id="{id}">` so external stylesheets can target whole subgraphs and the user classes applied via `class A foo` or `A:::foo`. See [`docs/svg-output.md`](./docs/svg-output.md) for the full hook surface.

See more examples in the sections below.

## What It Supports

- **Diagram types:** flowchart, class, sequence, state
- **Output formats:** Unicode text, ASCII text, SVG, MMDS JSON
- **Layout directions:** `TD`, `BT`, `LR`, `RL` (with per-subgraph overrides)
- **Edge styles:** solid, dotted, thick, invisible, cross-arrow, circle-arrow
- **Routing:** orthogonal, polyline, direct (with curve options: basis, linear, linear-rounded, linear-sharp)
- **Round-trip conversion:** Mermaid to MMDS and MMDS back to Mermaid

## Graph-Family Engines

|            | `flux-layered`                        | `mermaid-layered`         |
| ---------- | ------------------------------------- | ------------------------- |
| Applies to | Flowchart/class/state text, SVG, MMDS | Flowchart/class SVG, MMDS |
| Routing    | Orthogonal, polyline, direct          | Polyline                  |
| Subgraphs  | Compound graph (global layout)        | Compound graph            |
| Best fit   | Deterministic routed output           | Mermaid-compatible output |

Sequence diagrams use a separate timeline renderer, support text/ascii, SVG, and
MMDS output, and do not accept `--layout-engine`.

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

## SVG Theming

SVG theming is opt-in and affects SVG output only.

- **Explicit config wins.** CLI flags and `RenderConfig.svg_theme` take precedence over Mermaid source hints.
- **Auto-theme is explicit.** `--svg-theme-auto` resolves to a concrete named theme before render, so Mermaid source hints stay suppressed when auto-theme is enabled.
- **Mermaid hints are supported.** `config.theme` in YAML frontmatter and `%%{init: {"theme": "..."}}%%` both select a named SVG theme when no explicit SVG theme is supplied.
- **Un-themed output stays available.** If no explicit theme or Mermaid hint is present, SVG rendering keeps the existing un-themed palette.
- **Static mode is the default.** Static mode emits concrete hex colors for maximum rasterizer compatibility.
- **Dynamic mode is additive.** `--svg-theme-mode dynamic` emits the same hex fallbacks plus root CSS variables and a `<style>` block for browser embedding.
- **Default auto-theme mapping is `light:default,dark:dark`.** Override it with `--svg-theme-auto=light:zinc-light,dark:dracula` when a different light/dark pair is a better fit.

#### Detecting terminal appearance inside tmux/screen

`--svg-theme-auto` probes the terminal's background color via OSC 11. Inside a
multiplexer (tmux/screen) the query is intercepted by default, so mmdflux falls
back to `$COLORFGBG` and then to the OS-level theme — none of which track a
terminal profile switch. For tmux ≥ 3.3, enable passthrough so OSC 11 reaches
the outer terminal:

```tmux
set -g allow-passthrough on
set -as terminal-features ',*:RGB:OSC11'
```

This fixes background detection for every tool that queries OSC 11, not just
mmdflux.

### Built-in themes

| Theme               | Family            |
| ------------------- | ----------------- |
| `zinc-light`        | beautiful-mermaid |
| `zinc-dark`         | beautiful-mermaid |
| `tokyo-night`       | beautiful-mermaid |
| `tokyo-night-storm` | beautiful-mermaid |
| `tokyo-night-light` | beautiful-mermaid |
| `catppuccin-mocha`  | beautiful-mermaid |
| `catppuccin-latte`  | beautiful-mermaid |
| `nord`              | beautiful-mermaid |
| `nord-light`        | beautiful-mermaid |
| `dracula`           | beautiful-mermaid |
| `github-light`      | beautiful-mermaid |
| `github-dark`       | beautiful-mermaid |
| `solarized-light`   | beautiful-mermaid |
| `solarized-dark`    | beautiful-mermaid |
| `one-dark`          | beautiful-mermaid |
| `default`           | mermaid           |
| `dark`              | mermaid           |
| `forest`            | mermaid           |
| `neutral`           | mermaid           |

The beautiful-mermaid themes are adapted from [beautiful-mermaid](https://github.com/lukilabs/beautiful-mermaid) by lukilabs.
Mermaid-family themes (`default`, `dark`, `forest`, `neutral`) are also resolved
from `%%{init: {"theme": "..."}}%%` directives and YAML frontmatter when no
explicit `--svg-theme` is provided.

Supported theme slots for per-slot overrides: `bg`, `fg`, `line`, `accent`, `muted`, `surface`, and `border`.

```rust
use mmdflux::{OutputFormat, RenderConfig, SvgThemeConfig, SvgThemeMode, render_diagram};

let svg = render_diagram(
    "graph TD\nA-->B\n",
    OutputFormat::Svg,
    &RenderConfig {
        svg_theme: Some(SvgThemeConfig {
            name: Some("dark".into()),
            mode: SvgThemeMode::Dynamic,
            ..Default::default()
        }),
        ..Default::default()
    },
)?;
```

SVG `<defs>` blocks are also pruned to the markers each diagram actually uses, so simple flowcharts and sequence diagrams no longer carry unused arrowhead definitions.

### Graph font config

Rust and JSON/Wasm config can carry graph-family `fontFamily` and `fontSize`
values, including the narrow Mermaid aliases `themeVariables.fontFamily` and
`themeVariables.fontSize`. These fields describe text measurement identity, not
SVG-only styling. Provider-free static rendering accepts only values that match
the selected static profile descriptor; arbitrary browser fonts require the
separate browser dynamic metrics export.

The web playground uses that browser metrics path automatically for graph-family
SVG diagrams whose Mermaid `style`, `classDef`/`class`, or `linkStyle`
declarations set layout-affecting font properties. Node labels, edge labels, and
subgraph titles can each use their effective `font-family`, `font-size`,
`font-style`, and `font-weight` while still measuring with the same browser font
identity used for SVG output. Text, ASCII, sequence diagrams, and provider-free
static rendering keep the deterministic static-profile behavior.

## Adapter workflows

mmdflux is not only a one-shot renderer. Advanced Rust integrations can treat
MMDS as a live document model. Condensed from the full example, the flow is:

```rust
let source = r#"graph TD
api[API] --> auth[Auth]
api --> billing[Billing]
auth --> users[(Users)]
billing --> ledger[(Ledger)]
"#;
let mut document = materialize_diagram(source, &RenderConfig::default())?;

let model_events = apply(
    &Command::ChangeNodeLabel {
        node: "billing".into(),
        label: "Billing API".into(),
    },
    &mut document,
)?;

let spec = ViewSpec::new(vec![ViewStatement::Include(Selector::Traversal {
    anchor: AnchorRef::Node("api".into()),
    direction: TraversalDirection::Downstream,
    hops: 1,
})]);
let (view, view_events) = project(&document, &spec)?;
let text = render_document(&view, OutputFormat::Text, &RenderConfig::default())?;
```

See [`examples/commands_events_views.rs`](examples/commands_events_views.rs) for
the full flow, or the rustdocs for
[`commands`](https://docs.rs/mmdflux/latest/mmdflux/commands/),
[`mmds::events`](https://docs.rs/mmdflux/latest/mmdflux/mmds/events/), and
[`views`](https://docs.rs/mmdflux/latest/mmdflux/views/).

## Documentation

- [Developer setup](docs/development/setup.md) — prerequisites and first-time setup
- [Gallery](docs/gallery.md) — rendered output for 152 fixtures
- [MMDS specification](docs/mmds.md) — structured JSON format
- [Edge routing design](docs/edge-routing-heuristics.md) — routing internals

## Rust API Surface

Most Rust integrations should stay on the high-level runtime facade:

- `render_diagram`
- `materialize_diagram`
- `render_document`
- `detect_diagram`
- `validate_diagram`

Pair those entrypoints with `RenderConfig` and `OutputFormat` unless you are
building an adapter or tooling layer that needs explicit preparation control.

The low-level API is smaller and adapter-focused:

- `mmdflux::builtins::default_registry()` for the builtin diagram registry
- `registry` and `payload` for explicit detect/parse/payload flows
- `graph` and `timeline` for family-specific IR inspection
- `mmds` for MMDS `Document` parsing, replay, and Mermaid generation
- `commands` and `mmds::events` for typed MMDS edits and accepted model
  transition events
- `views` for materializing filtered read-side MMDS payloads

The rest of the crate tree (`diagrams`, `engines`, `render`, and `mermaid`)
consists of internal implementation modules and is not part of the supported
public contract.

### Examples

- [`examples/high_level_render.rs`](examples/high_level_render.rs) — top-level
  `render_diagram` workflow
- [`examples/registry_adapter.rs`](examples/registry_adapter.rs) — explicit
  registry-driven detection and preparation
- [`examples/mmds_replay.rs`](examples/mmds_replay.rs) — MMDS profile
  negotiation, replay, and Mermaid regeneration
- [`examples/materialized_view.rs`](examples/materialized_view.rs) —
  materialize a focused MMDS view and replay it as text
- [`examples/commands_events_views.rs`](examples/commands_events_views.rs) —
  apply MMDS commands, inspect model events, project a view, and render it
- [`examples/snapshot_diff.rs`](examples/snapshot_diff.rs) — compare two
  materialized MMDS documents with the snapshot diff API

Verify the examples compile with `cargo test --examples`.

## License

MIT
