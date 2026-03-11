# Architecture Dependency Rules

This document defines the steady-state dependency and ownership rules for the
mmdflux architecture. It is the architectural contract enforced by the guard
tests and should remain accurate as the codebase evolves.

## Dependency Rules

These rules define forbidden cross-layer imports and ownership boundaries.
Violating any of them is a structural regression.

### Core rules

1. **diagrams do not render** — Diagram modules (`src/diagrams/`) compile input
   into family IR (graph geometry or timeline model). They never invoke format
   emitters or write output directly.

2. **formats do not parse** — Format emitters (`src/formats/`) consume family
   geometry and produce output. They never parse Mermaid text or touch ASTs.

3. **engines do not know about diagram types** — Engine implementations
   (`src/engines/`) solve generic graph layout problems. They never reference
   flowchart, class, sequence, or any diagram-specific type.

4. **api/ owns the stable public contract** — All supported public types,
   traits, configs, and error types live in `src/api/`. Other modules expose
   internals only through `pub(crate)` or narrowly scoped re-exports.

5. **runtime/ is orchestration only** — The runtime layer (`src/runtime/`)
   detects diagram types, manages the registry, and wires the pipeline. It does
   not contain business logic, layout algorithms, or rendering code.

6. **render/ is shared primitives only** — `src/render.rs` and its sibling
   files contain shared primitives like `Canvas`, `CharSet`, and `intersect`
   utilities. They do not orchestrate rendering or re-export diagram-specific
   types.

### Adapter rules

7. **web main.ts is composition only** — The web playground's `main.ts` is a
   composition root that wires stores, services, and controllers. It does not
   contain application logic, state management, or rendering orchestration.

8. **wasm adapter is a thin boundary** — `crates/mmdflux-wasm` deserializes
   JS requests, calls the Rust facade, and serializes responses. It does not
   duplicate config parsing, registry logic, or format selection.

9. **CLI adapter is a thin boundary** — `src/main.rs` maps CLI flags to the
   Rust facade contract and formats output. It does not contain business logic
   beyond argument mapping.

### Package rules

10. **npm library entrypoints are pure** — `packages/*/src/index.ts` exports
    only pure conversion functions. Side effects (stdin, stdout, file I/O,
    browser open, upload) live in dedicated `cli.ts` or adapter modules.

11. **MMDS is the graph-family interchange contract** — MMDS JSON is the
    shared contract between Rust, wasm, TypeScript packages, and the web UI
    for graph-family diagrams. Schema ownership lives in one place.
