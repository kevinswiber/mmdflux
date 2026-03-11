# Hard-Cutover Target Architecture

This document defines the target module layout and dependency rules for the
mmdflux architecture overhaul. It is the contract that Phase 2+ tasks must
satisfy before the cutover merges to main.

## Target Module Layout

```text
mmdflux
├── src/api/                 # stable public request/config/error surface
│   ├── config.rs
│   ├── diagnostics.rs
│   ├── errors.rs
│   ├── format.rs
│   └── request.rs
├── src/runtime/             # detection, registry, facade orchestration
│   ├── detect.rs
│   ├── facade.rs
│   └── registry.rs
├── src/graph/               # graph-family model, geometry, routing contracts
│   ├── model/
│   ├── geometry/
│   └── routing/
├── src/timeline/            # sequence/timeline family contracts
│   ├── model/
│   └── layout/
├── src/engines/
│   └── graph/               # graph-family engine implementations
│       ├── registry.rs
│       ├── elk.rs
│       └── layered/         # Sugiyama engine decomposition
├── src/diagrams/            # diagram-specific parse/compile/hydrate
│   ├── flowchart/
│   ├── class/
│   ├── sequence/
│   ├── pie.rs
│   ├── info.rs
│   └── packet.rs
├── src/formats/             # text/svg/mmds/mermaid emitters
│   ├── text/
│   ├── svg/
│   ├── mmds/
│   └── mermaid/
├── src/render/              # shared primitives only: canvas, chars, intersect
│   ├── canvas.rs
│   ├── chars.rs
│   └── intersect.rs
└── src/lib.rs               # curated facade re-exports only
```

## Dependency Rules

These rules define forbidden cross-layer imports. Violating any of these
rules is a blocking issue for the cutover.

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

6. **render/ is shared primitives only** — After the cutover, `src/render/`
   contains only `Canvas`, `CharSet`, and `intersect` utilities. It does not
   orchestrate rendering or re-export diagram-specific types.

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

## Current Module Mapping

| Current | Target | Change |
| ------- | ------ | ------ |
| `src/diagram.rs` | `src/api/` + `src/runtime/` | Split monolith into contract + orchestration |
| `src/render/mod.rs` | `src/formats/` + `src/render/` | Move rendering to formats, keep shared primitives |
| `src/layered/` | `src/engines/graph/layered/` | Relocate under engine namespace |
| `src/diagrams/flowchart/render/` | `src/formats/text/` + `src/formats/svg/` | Move to format-owned emitters |
| `src/lib.rs` | `src/lib.rs` (curated) | Prune to stable facade only |

## Validation

These rules are verified by:
- `tests/cutover_baseline.rs` — manifest and document existence checks
- `tests/cutover_harness.rs` — output comparison against frozen baselines
- `tests/lib_exports.rs` — public API surface contract
- Existing test suites: integration, dagre parity, compliance, MMDS, CLI, SVG snapshots
