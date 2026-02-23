# Changelog

## Unreleased

### Added

- `mermaid-layered` engine now ignores subgraph `direction` overrides when the
  subgraph has cross-boundary edges, matching Mermaid.js/dagre behavior.
  `flux-layered` continues to always respect direction overrides.

### Fixed

- Sibling subgraph bounds no longer overlap after sublayout reconciliation.
- Added margin between adjacent subgraph borders for visual breathing room.
- Text backward-edge routing now reuses shared routed paths for long TD/BT
  backward edges while preserving text-specific fallback heuristics for short
  cycles, fixing wrong-facing arrowheads and attachment/segment artifacts
  (for example in `complex.mmd` and `multiple_cycles.mmd`).
- SVG polyline rendering no longer injects tiny synthetic jogs on
  axis-to-diagonal turns (for example `ampersand.mmd`) in both
  `flux-layered` and `mermaid-layered`.
- Self-loop tail regression coverage now validates loop-lane drift without
  assuming a fixed elbow index, preventing false failures when valid polyline
  cleanup reduces intermediate points.

### Changed

- Routing semantics: `--edge-preset straight` now means direct routing
  (`Direct + Linear + Sharp`). Use `--edge-preset polyline` for prior straight semantics.
- Direct routing now uses a collision-aware fallback: when a single direct segment
  would cross node interiors, mmdflux preserves node-avoidance geometry.

### Refactor

- Renamed broad `dagre` terminology to `layered` across APIs, internals, and docs
  (plan-0082), including layout/routing config names and layered hint types.
- Reorganized `src/diagrams/flowchart/render/` to clearly separate text, SVG, and
  shared modules ([#13](https://github.com/kevinswiber/mmdflux/pull/13)):
  extracted shared layout building (`layout_building.rs`) and subgraph ops
  (`layout_subgraph_ops.rs`), moved text types to `text_types.rs`, renamed
  `layout.rs` to `text_layout.rs`, and added `text_` prefix to all text-only
  modules for naming symmetry with `svg_*`. Renamed `LayoutConfig` to
  `TextLayoutConfig`.
- `mermaid-layered` engine now only supports SVG and MMDS output, matching
  Mermaid.js which only renders to SVG ([#14](https://github.com/kevinswiber/mmdflux/pull/14)).
  Text/ASCII output uses `flux-layered` exclusively.
