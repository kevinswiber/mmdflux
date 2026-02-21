# Changelog

## Unreleased

- Routing semantics: `--edge-preset straight` now means direct routing, with
  a collision-aware fallback that preserves node-avoidance paths when a
  single direct segment would cross node interiors. Use `--edge-preset polyline`
  for prior straight semantics.
