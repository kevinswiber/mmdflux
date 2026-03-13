# RoutingStyle vs EdgeRouting

## Purpose

Record whether `RoutingStyle` (intent) and `EdgeRouting` (execution mode) should be consolidated now, after introducing `RoutingStyle::Direct` and `EdgeRouting::DirectRoute`.

## Current State Data Flow

```text
CLI/RenderConfig
  -> GraphSolveRequest::from_config()
     - routing_style: Option<RoutingStyle>
     - path_simplification: PathSimplification
  -> EngineAlgorithmCapabilities (supported_routing_styles, route_ownership)
  -> EngineAlgorithmId::edge_routing_for_style()
     - resolves to EdgeRouting::{DirectRoute, PolylineRoute, OrthogonalRoute, EngineProvided}
  -> route_graph_geometry()/SVG/MMDS rendering pipeline
```

## Call Site Inventory

### `RoutingStyle` (user intent + capability contract)

- `src/main.rs` parses `--routing-style` (`direct`, `polyline`, `orthogonal`).
- `src/format.rs` defines the public `RoutingStyle` vocabulary.
- `src/config.rs` and `src/engines/graph/contracts.rs` own capability lists and style-to-route mapping.
- `src/render/graph/mod.rs` resolves style precedence: explicit style > preset > engine defaults.
- `src/engines/graph/flux.rs` and `src/engines/graph/mermaid.rs` advertise supported styles per engine adapter.

Classification:
- User configuration concern: CLI parsing and preset/style resolution.
- Engine capability concern: supported style sets and validation.

### `EdgeRouting` (runtime execution concern)

- `src/engines/graph/contracts.rs` defines execution modes and style-to-route selection.
- `src/render/graph/routing.rs` executes route-building by mode.
- `src/render/graph/svg/edges.rs` applies mode-specific post-processing.
- `src/mmds/replay.rs` and `src/mmds/hydrate.rs` select replay/runtime execution behavior.

Classification:
- Runtime routing execution concern.
- Post-routing/render-pipeline concern.

## Pain Points and Duplication

- There is one explicit conversion boundary (`RoutingStyle` -> `EdgeRouting`) in `EngineAlgorithmId::edge_routing_for_style`.
- This adds one extra type hop, but keeps capability validation separate from runtime routing branches.
- Current duplication is low; most logic is centralized in the conversion function and route-mode match arms.

## Options

### Option A: Keep Split Types (current model)

Pros:
- Preserves clear taxonomy: requested topology vs executable routing mode.
- Fits engine ownership model (`Native`, `HintDriven`, `EngineProvided`) cleanly.
- Keeps `PathSimplification` orthogonal to style/preset resolution.

Cons:
- Requires conversion step and maintaining two enums.

### Option B: Consolidate into One Enum Now

Pros:
- Fewer type conversions.
- Slightly simpler mental model in small call paths.

Cons:
- Blurs user intent and engine/runtime ownership concerns.
- Increases breakage risk across CLI, engine capability checks, and rendering adapters.
- Complicates handling of engine-provided routing versus caller-requested style.

## Scorecard

| Criterion | Keep Split | Consolidate Now |
| --- | --- | --- |
| API clarity | High | Medium |
| Breakage blast radius | Low | High |
| Test complexity | Medium | High |
| Engine extensibility | High | Medium |

## Recommendation

**Defer consolidation.** Keep `RoutingStyle` and `EdgeRouting` separate for now.

Rationale:
- The split maps directly to how the pipeline currently works (intent -> capability validation -> execution mode).
- Direct routing support was added cleanly without adding scattered conversion logic.
- Immediate consolidation would be a broad refactor with limited near-term value.

## Revisit Triggers

Re-open consolidation only when one or more of these occur:

1. More than one conversion boundary appears outside `edge_routing_for_style` (conversion logic duplication).
2. A new engine family requires repeated special-case bridging between intent and execution mode.
3. A new routing mode requires adding parallel variants in both enums with no ownership distinction.
4. Cross-family routing APIs (beyond flowchart/MMDS) need a single public routing abstraction.

## References

- `.gumbo/research/0039-architecture-extensibility-readiness/q4-routing-abstraction-boundary.md`
- `.gumbo/research/0047-orthogonal-routing-unification/synthesis.md`
