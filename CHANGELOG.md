# Changelog

## Unreleased

### Added

- `mermaid-layered` engine now ignores subgraph `direction` overrides when the
  subgraph has cross-boundary edges, matching Mermaid.js/dagre behavior.
  `flux-layered` continues to always respect direction overrides.

### Fixed

- Sibling subgraph bounds no longer overlap after sublayout reconciliation.
- Added margin between adjacent subgraph borders for visual breathing room.

### Changed

- Routing semantics: `--edge-preset straight` now means direct routing
  (`Direct + Linear + Sharp`). Use `--edge-preset polyline` for prior straight semantics.
- Direct routing now uses a collision-aware fallback: when a single direct segment
  would cross node interiors, mmdflux preserves node-avoidance geometry.

### Refactor

- Renamed broad `dagre` terminology to `layered` across APIs, internals, and docs
  (plan-0082), including layout/routing config names and layered hint types.
