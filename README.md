# mmdflux

Render diagrams as Unicode text, ASCII text, SVG, or JSON. Supports Mermaid syntax.

`mmdflux` is built for diagram-as-code pipelines: fast rendering, terminal-friendly output, linting, and machine-readable graph data for tooling and agents.

[Playground](https://play.mmdflux.com) • [Releases](https://github.com/kevinswiber/mmdflux/releases) • [MMDS Spec](docs/mmds.md)

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
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/at-a-glance-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="docs/assets/readme/at-a-glance-light.svg">
  <img alt="mmdflux at-a-glance SVG output" src="docs/assets/readme/at-a-glance-light.svg" width="360">
</picture>

<details>
<summary>Text output (<code>mmdflux --format text ...</code>)</summary>

```text
     ┌───────┐
     │ Start │
     └───────┘
          │
          │
          │
          │
┌──────── Horizontal Section ────────┐
│         ▼                          │
│ ┌────────┐  ┌────────┐  ┌────────┐ │
│ │ Step 1 │─►│ Step 2 │─►│ Step 3 │ │
│ └────────┘  └────────┘  └────────┘ │
│                         ┌┘         │
└─────────────────────────┼──────────┘
                          │
                          │
                          │
                          ▼
                     ┌─────┐
                     │ End │
                     └─────┘
```

</details>

**MMDS JSON output**: [`docs/assets/readme/at-a-glance.mmds.json`](docs/assets/readme/at-a-glance.mmds.json)

## Why mmdflux

- Terminal-native output that still preserves structure and readability
- SVG and MMDS JSON output for web tooling, automation, and data pipelines
- Native `flux-layered` engine with deterministic routing policies
- Compatibility `mermaid-layered` engine when Mermaid-style behavior is preferred

## Flux Layered (Native Engine)

`flux-layered` is the default graph engine for flowchart/class SVG and MMDS output.
It keeps the layered (Sugiyama) foundation but adds a native routing contract and
policy-driven geometry decisions that are hard to get from layout-only engines.

### What makes it distinct

- Layered layout + routing are treated as one solve contract (not disconnected phases)
- Rank-annotated waypoint metadata is preserved for downstream routing decisions
- Float-space orthogonal routing with deterministic fan-in/fan-out overflow policies
- Explicit backward-edge channel/face precedence rules
- Per-node effective direction in subgraphs and cross-boundary routing behavior
- Shape-aware attachment and clipping for non-rectangular nodes (for example, diamond/hexagon)
- Self-edge loops are emitted as explicit multi-point orthogonal paths
- The same graph model feeds text, SVG, and MMDS pipelines

### Engine snapshot

| Capability           | `flux-layered`                             | `mermaid-layered`                    |
| -------------------- | ------------------------------------------ | ------------------------------------ |
| Route ownership      | Native                                     | Hint-driven                          |
| Routing styles       | `direct`, `orthogonal`, `polyline`         | `polyline`                           |
| Default SVG behavior | Orthogonal topology + basis curve | Mermaid-compatible polyline defaults |
| Subgraph support     | Yes                                        | Yes                                  |
| Best fit             | Deterministic routed SVG/MMDS output       | Mermaid-style compatibility output   |

Routing semantics note:
`--edge-preset straight` now maps to direct routing (`Direct + linear-sharp`).
Direct routing prefers a single segment when unobstructed, and falls back to
node-avoidance geometry when a direct segment would cross node interiors.
Use `--edge-preset polyline` for the old straight/passthrough behavior.
Curve treatment is controlled independently via
`--curve basis|linear|linear-sharp|linear-rounded`.

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

# Text output (default)
mmdflux --format text diagram.mmd

# SVG output (flowchart/class)
mmdflux --format svg diagram.mmd -o diagram.svg

# Native flux layered (default) SVG with smooth orthogonal corners
mmdflux --format svg --layout-engine flux-layered --edge-preset smooth-step diagram.mmd -o diagram.svg

# Native flux layered SVG with curved orthogonal basis paths
mmdflux --format svg --layout-engine flux-layered --edge-preset curved-step diagram.mmd -o diagram.svg

# SVG with explicit curve contract
mmdflux --format svg --layout-engine flux-layered --curve linear-rounded diagram.mmd -o diagram.svg

# MMDS JSON output with routed geometry detail
mmdflux --format mmds --layout-engine flux-layered --geometry-level routed --path-detail compact diagram.mmd

# Lint mode (validate input and print diagnostics)
mmdflux --lint diagram.mmd
```

## What It Supports

- Flowchart rendering in Unicode/ASCII/SVG/MMDS
- Class diagram rendering in Unicode/ASCII/SVG/MMDS
- Mermaid-to-MMDS and MMDS-to-Mermaid conversion
- Layout directions: `TD`, `BT`, `LR`, `RL`
- Edge styles: solid, dotted, thick, invisible, cross-arrow, circle-arrow
- Engine selection: `flux-layered`, `mermaid-layered` (ELK engines available when built with `engine-elk`)

## Documentation

- [Gallery](docs/gallery.md)
- [MMDS specification](docs/mmds.md)
- [Edge routing design](docs/edge-routing-heuristics.md)

## Adapter Packages

- `@mmds/excalidraw` — MMDS to Excalidraw `.excalidraw` JSON.
- `@mmds/tldraw` — MMDS to tldraw `.tldr` JSON.

Adapter fidelity note:
MMDS routed polylines can include many waypoints, while tldraw arrows use native arc/elbow models. The tldraw adapter preserves endpoints and label intent, then applies deterministic best-fit arrow geometry.

## License

MIT
