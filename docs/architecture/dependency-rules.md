# Architecture Dependency Rules

This document defines the steady-state dependency and ownership rules for the
mmdflux Rust crate. The module tree should tell one coherent story for
contributors:

- `frontends/` own source ingestion
- `diagrams/` own compilation and instance behavior
- `graph/` owns graph-family IR, routed geometry, and shared policy/measurement helpers
- `render/` owns output production
- `mmds/` owns the MMDS contract and output helpers

Guard tests should fail when the code drifts away from these rules.

## Core Rules

1. **frontends own input formats** — Source-format ingestion lives under
   `src/frontends/`. Runtime detects the frontend first (`mermaid`, `mmds`),
   then resolves the logical diagram type and family pipeline.

2. **diagrams do not parse source text directly** — Diagram modules
   (`src/diagrams/`) consume frontend-owned models and compile them into
   logical family IR or family-local runtime models.

3. **diagrams do not render** — `src/diagrams/` stop at detection, parse
   delegation, compilation, and instance orchestration. Output production lives
   under `src/render/`, not under diagram modules.

4. **render/ owns output production** — All rendering code lives under
   `src/render/`. There is no top-level `formats/` ownership boundary and no
   graph render tree under `src/graph/`.

5. **render::graph owns geometry-based graph-family emitters** — Shared
   graph-family text and SVG emission lives under `src/render/graph/` and
   consumes `GraphGeometry` or `RoutedGraphGeometry`. Render code does not
   take `GraphSolveResult` or instantiate engines.

6. **runtime owns graph-family solve-result dispatch** — `src/runtime/`
   resolves graph-family output formats from engine solve results and owns the
   final dispatch to MMDS serialization or geometry-based renderers. Runtime
   does not own renderer implementations.

7. **render::diagram owns family-local renderers** — Timeline/chart/table
   renderers that do not use the shared graph-family pipeline live under
   `src/render/diagram/`.

8. **graph/ owns graph-family IR, routed geometry, and shared policy/measurement helpers** —
   `src/graph/` contains reusable graph-family models, solved and routed
   geometry, direction policy, and shared graph-family measurement/routing
   helpers. Output emission does not live under `src/graph/`, but graph-family
   routing and shared sizing/policy do.

9. **mmds/ is the MMDS contract and output namespace** — `src/mmds/` owns the
   typed MMDS envelope, profile vocabulary, Mermaid regeneration helpers, and
   MMDS serialization for graph-family output.

10. **MMDS is a frontend, not a logical diagram type** — MMDS input handling
   lives under `src/frontends/mmds/`. MMDS is not registered in the logical
   diagram registry.

11. **engines do not know about diagram types** — Engine implementations
    (`src/engines/`) solve generic graph layout problems and own layout building / measurement adapters.
    They may use shared graph-family helpers, but they never reference flowchart,
    class, sequence, or other logical diagram types, and they do not import
    render-owned modules.

12. **flat top-level contract modules own the stable public contract** —
    Stable public config types, request/response types, diagnostics, and error
    vocabulary live in `src/config.rs`, `src/format.rs`, `src/request.rs`,
    `src/errors.rs`, `src/diagnostics.rs`, and `src/family.rs`. Other
    namespaces are either advanced APIs or internal helpers.

13. **runtime/ is orchestration only** — The runtime layer detects input
    frontends, resolves logical diagram types, manages the registry, and wires
    the pipeline. It does not own parsing grammars, layout algorithms, or
    renderer implementations.

14. **registry is advanced infrastructure** — `src/registry.rs` remains public
    for power-user flows, but it is not the default onboarding path. The
    default public story is crate-root facade first.

## Adapter Rules

15. **web main.ts is composition only** — The web playground's `main.ts` is a
    composition root that wires stores, services, and controllers. It does not
    contain application logic, state management, or rendering orchestration.

16. **wasm adapter is a thin boundary** — `crates/mmdflux-wasm` deserializes JS
    requests, calls the Rust facade, and serializes responses. It does not
    duplicate config parsing, registry logic, or format selection.

17. **CLI adapter is a thin boundary** — `src/main.rs` maps CLI flags to the
    Rust facade contract and formats output. It does not contain business logic
    beyond argument mapping.
