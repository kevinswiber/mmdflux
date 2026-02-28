# Changelog

## Unreleased

### Fixed

- Fixed backward edges detaching from source nodes in SVG when the backward
  path's first horizontal segment fell exactly on the source node's bottom
  boundary.
- Fixed tiny sub-pixel cross-axis jogs on forward edges caused by
  `collapse_tiny_cross_axis_jog` misidentifying short orthogonal segments in
  the SVG orthogonal router.
- Fixed backward edges in LR layouts entering the target node's east face
  instead of the south face when `align_backward_outer_lane_to_hint` pulled
  the outer lane inside node boundaries using layout hint waypoints that pass
  through node centers.
- Fixed `render_svg()` (library/test path) producing different layouts than the
  CLI by replacing hardcoded flux flags with calls to the canonical
  `flux_layout_profile()` and `adapt_flux_profile_for_reversed_chain_crowding()`
  from the engine module.
- Fixed `render_svg()` ignoring `routing_style` when deriving `edge_routing`,
  causing basis and straight preset snapshots to use orthogonal routing paths
  instead of polyline and direct routing respectively.

### Added

- Added `scripts/svg-gallery-diff` for side-by-side before/after HTML gallery
  of changed SVG snapshots versus a base ref.

## v1.3.1

### Added

- Added `--version` flag to the CLI.

## v1.3.0

### Breaking

- Removed edge preset token `bezier`; use `basis` (`--edge-preset basis`).
- SVG curve control is now a clean-break contract via
  `--curve basis|linear|linear-sharp|linear-rounded`.
- Removed legacy CLI flags `--interpolation-style` and `--corner-style`.
- Removed legacy WASM/web config fields `interpolationStyle` and `cornerStyle`;
  use `curve`.

### Added

- Implemented plan-0088 model-order tie-breaking across layered ordering paths
  to preserve source insertion order deterministically.
- Implemented plan-0089 greedy-switch two-sided post-pass crossing reduction,
  plus crossing baselines and quality regression checks.
- Implemented plan-0090 per-gap rank-separation overrides for `flux-layered`
  based on gap edge density and crossing pressure.
- Implemented plan-0091 per-edge label spacing features, including label dummy
  insertion, label side selection, label-layer switching, thickness offset, and
  HEAD/TAIL label support.
- Expanded layout and routing non-regression coverage (ordering, spacing,
  routing topology, and engine behavior).

### Fixed

- Fixed multiple backward-edge routing regressions in text and SVG, including
  corridor-aware channeling, face attachment consistency, and subgraph override
  cases.
- Fixed SVG edge rendering regressions around arrowhead visibility, reciprocal
  two-point curve separation, and shape-border lane attachment.
- Fixed label/spacing regressions in layered layout, including restored
  unlabeled-edge rank separation and corrected label-gap accounting.
- Fixed reversed long-edge chain accounting leakage into forward-edge density
  metrics.

### Changed

- Implemented plan-0092 curve taxonomy clean break and removed transitional
  interpolation bridge behavior in favor of `Curve`.
- Renamed SVG snapshot bucket `flowchart-bezier` to `flowchart-basis`.
- Updated web playground preset vocabulary from `bezier` to `basis`.
- Updated `scripts/svg-gallery` and `scripts/view` defaults/examples to use
  `basis`; `svg-gallery` now also exports fixture source copies.
- Removed web CSS `!important` cursor overrides and rely on panzoom cursor
  config and normal cascade precedence.

## v1.2.0

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
