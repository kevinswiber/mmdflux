# Architecture Dependency Rules

This document defines the steady-state dependency and ownership rules for the
mmdflux Rust crate. The module tree should tell one coherent story for
contributors:

- `frontends.rs` owns source-format detection
- `mermaid/` owns Mermaid source ingestion
- `diagrams/` own compilation and instance behavior
- `payload/` owns the runtime payload contract
- `builtins/` owns default registry wiring for the supported low-level API
- `graph/` owns graph-family IR, float-space geometry, and shared policy/measurement helpers
- `engines/` own engine adapters and internal algorithm boundaries such as `algorithms::layered::kernel`
- `render/` owns output production
- `mmds/` owns the MMDS contract and output helpers

The repo-owned architecture gate should fail when the code drifts away from
these rules.

The repo-owned architecture gate is `cargo xtask architecture` or
`just architecture`.

The semantic boundaries guard reads its declarative module-dependency policy
from the repo-root
[`boundaries.toml`](/Users/kevin/src/mmdflux-semantic-architecture/boundaries.toml).
Set `SEMANTIC_BOUNDARIES_CONFIG` to point at a different file when reusing the
checker outside this repo. The semantic-only command is
`cargo xtask architecture boundaries` or `just boundaries`.
Performance investigation notes for the xtask guard live in
[`docs/architecture/semantic-boundaries-guard-performance.md`](/Users/kevin/src/mmdflux-semantic-architecture/docs/architecture/semantic-boundaries-guard-performance.md).

## Public Contract Tiers

- The high-level runtime facade is `render_diagram`, `detect_diagram`,
  `validate_diagram`, plus the flat config/format/error types re-exported from
  `lib.rs`.
- The supported low-level API is `builtins`, `registry`, `payload`, `graph`,
  `timeline`, and `mmds` for adapter-oriented workflows that need explicit
  payload construction, graph IR inspection, or MMDS replay control.
- `diagrams`, `engines`, `render`, and `mermaid` are internal implementation modules.
  They are intentionally documented here for contributors, but they are not part
  of the supported low-level contract.

The repo also locks in three directory-module shell replacements for former
mega-files. These shells are part of the steady-state layout and should not be
collapsed back into singleton roots:

- `src/render/graph/svg/edges/mod.rs` is the directory-module shell replacing
  the removed `src/render/graph/svg/edges.rs`
- `src/graph/grid/routing/mod.rs` is the directory-module shell replacing the
  removed `src/graph/grid/routing.rs`
- `src/graph/routing/orthogonal/mod.rs` is the directory-module shell
  replacing the removed `src/graph/routing/orthogonal.rs`

## Core Rules

1. **frontends own input formats** — Source-format detection lives in
   `src/frontends.rs`. Runtime detects the frontend first (`mermaid`, `mmds`),
   then resolves the logical diagram type and family pipeline. Mermaid parsing
   itself lives in the separate top-level `src/mermaid/` namespace.

2. **diagrams do not parse source text directly** — Diagram modules
   (`src/diagrams/`) consume frontend-owned models and compile them into
   logical family IR or family-local runtime models.

3. **diagrams do not render** — `src/diagrams/` stop at detection, parse
   delegation, compilation, and `into_payload()` orchestration. Diagram
   instances hand runtime a `payload::Diagram` instead of calling
   renderers directly. Output production lives under `src/render/`, not under
   diagram modules.

4. **render/ owns output production** — All rendering code lives under
   `src/render/`. There is no top-level `formats/` ownership boundary and no
   graph render tree under `src/graph/`.

5. **render::graph owns geometry-based graph-family emitters** — Shared
   graph-family text and SVG emission lives under `src/render/graph/` and
   consumes `GraphGeometry`, `RoutedGraphGeometry`, or graph-owned
   `graph::grid` layouts. High-level geometry entrypoints stay at the
   `render::graph` root, low-level text drawing lives under
   `render::graph::text`, and routed SVG emission is explicit through
   `render_svg_from_routed_geometry`. Render code does not take
   `GraphSolveResult` or instantiate engines.

6. **runtime owns graph-family solve-result dispatch** — `src/runtime/`
   resolves graph-family output formats from engine solve results and owns the
   final dispatch to MMDS serialization or geometry-based renderers. Runtime
   does not own renderer implementations.

7. **render::diagram owns family-local renderers** — Timeline/chart/table
   renderers that do not use the shared graph-family pipeline live under
   `src/render/diagram/`.

8. **graph/ owns graph-family IR, float-space geometry, and shared policy/measurement helpers** —
   `src/graph/` contains reusable graph-family models, solved and routed
   geometry, direction policy, and shared graph-family grid/proportional
   measurement and routing helpers. The graph-owned `graph::grid` namespace is
   the derived grid-space layer for float-to-grid conversion, grid routing, and
   replay geometry contracts. Output emission does not live under `src/graph/`,
   but graph-family routing, derived grid geometry, and shared sizing/policy do.

9. **mmds/ is a pure interchange format layer** — `src/mmds/` owns the
   typed MMDS envelope, profile vocabulary, Mermaid regeneration helpers,
   hydration to graph IR, and MMDS serialization for graph-family output.
   mmds must not import from `render` or `engines`. Replay rendering
   (hydrate→render dispatch) lives in `runtime/mmds.rs`.

10. **MMDS is a frontend, not a logical diagram type** — MMDS input handling
   is detected through `src/frontends.rs`, while the MMDS parse, hydration,
   and output helpers live under `src/mmds/`. MMDS is not registered
   in the logical diagram registry.

11. **engines do not know about diagram types or output formats** — Engine
    implementations (`src/engines/`) solve generic graph layout problems and
    own layout building / measurement adapters. They may use shared
    graph-family helpers, but they never reference flowchart, class, sequence,
    or other logical diagram types, and they do not import render-owned
    modules. Engine solve requests stay in engine-owned vocabulary:
    grid/proportional measurement plus canonical/visual geometry contracts.
    Render-format mapping (`Text`, `Svg`, `Mmds`) happens above the engine
    layer in graph-family orchestration, and path simplification remains a
    downstream render/MMDS consumer concern. Within graph-family engines,
    `algorithms::layered::kernel` is the pure graph-agnostic layered engine
    boundary, while the outer `algorithms::layered` root owns the graph-family
    bridge code such as layout building / measurement adapters, float layout,
    and float routing. `layered::kernel` stays internal; it is a contributor
    boundary, not a supported public contract.

12. **flat top-level contract modules own the stable public contract** —
    Stable public format and error vocabulary live in `src/format.rs` and
    `src/errors.rs`. `RenderConfig` lives in `src/runtime/config.rs`
    (re-exported from `lib.rs`). Diagnostics live in `errors`, family
    classification in `registry`, and style types in `graph/style`.
    Adapter orchestration entrypoints are curated runtime facade re-exports
    from `lib.rs`. Other namespaces are either part of the supported
    low-level API or internal implementation modules.

13. **runtime/ is orchestration only** — The runtime layer detects input
    frontends, resolves logical diagram types, manages the registry, consumes
    runtime payloads, and wires the pipeline. Graph-family runtime
    dispatch lives under `src/runtime/`; runtime itself does not own Mermaid
    grammars, layout algorithms, or renderer implementations.

14. **registry is contract-only infrastructure** — `src/registry.rs` defines
    reusable registry contracts (`DiagramRegistry`, `DiagramDefinition`,
    `DiagramInstance`) and does not import concrete diagram modules. Built-in
    diagram wiring lives in the separate public `builtins` namespace.

15. **timeline::sequence owns shared sequence runtime types** — Shared
    sequence-family model and layout types live under `src/timeline/sequence/`
    so the final text renderer can depend on a neutral timeline namespace
    instead of importing `diagrams::sequence`.

## Adapter Rules

16. **web main.ts is composition only** — The web playground's `main.ts` is a
    composition root that wires stores, services, and controllers. It does not
    contain application logic, state management, or rendering orchestration.

17. **wasm adapter is a thin boundary** — `crates/mmdflux-wasm` deserializes JS
    requests, calls the Rust facade, and serializes responses. It does not
    duplicate config parsing, registry logic, or format selection.

18. **CLI adapter is a thin boundary** — `src/main.rs` maps CLI flags to the
    Rust facade contract and formats output. It does not contain business logic
    beyond argument mapping.

## Deferred Friction

See [deferred-friction.md](./deferred-friction.md) for architecture friction items
that have been reviewed and deliberately deferred, each with a specific trigger
condition for when to revisit.
