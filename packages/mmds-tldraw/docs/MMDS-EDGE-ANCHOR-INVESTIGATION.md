# MMDS Edge Path & Anchor Investigation

## Summary

Investigation into how mmdflux generates MMDS edge paths, whether endpoints target node centers vs. edges, and implications for the tldraw adapter.

## Key Findings

### 1. MMDS Position Semantics: Spec vs. Implementation

**Spec (docs/mmds.md, mmds.schema.json):**

- `position.x` and `position.y` are **node centers** (not top-left)
- Consumers should use: `left = position.x - size.width / 2`, `top = position.y - size.height / 2`

**mmdflux implementation (src/mmds.rs:264-266):**

```rust
position: MmdsPosition {
    x: pn.rect.x,
    y: pn.rect.y,
},
```

- Outputs `rect.x`, `rect.y` directly from `PositionedNode`
- The flux-layered geometry uses **top-left** rects (from `layered::Rect` in `position.rs`: `x = center_x - w/2`)

**Conclusion:** mmdflux outputs `rect.x`, `rect.y` which are **top-left** (from `layered::Rect`). The spec declares **center**. This is a conformance bug. The tldraw adapter interprets `position` as center per spec. Path endpoints (e.g. A→B start at 255.25, 62) align with rect center x and bottom edge—so paths are in the same coordinate space as the rects. If mmdflux output center instead of top-left, adapters would get correct semantics. The fix: output `position.x = rect.x + rect.width/2`, `position.y = rect.y + rect.height/2`.

### 2. Edge Path Endpoint Semantics

**Routing pipeline (src/diagrams/flowchart/routing.rs, orthogonal_router.rs):**

| Stage                                                       | Endpoint semantics                                                           |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `build_path_from_nodes_and_waypoints`                       | Starts with **rect center** (`rect.center_x()`, `rect.center_y()`)           |
| `snap_path_endpoints_to_faces` (DirectRoute)                | Snaps to **rect boundary** (primary departure/arrival face)                  |
| `anchor_path_endpoints_to_endpoint_faces` (OrthogonalRoute) | Projects to **rect boundary** using path direction                           |
| `snap_backward_endpoints_to_shape`                          | For backward edges: projects to **diamond/hexagon boundary** (not just rect) |

**Path generation flow:**

1. **DirectRoute / PolylineRoute:** `build_direct_path` or `build_path_from_hints` → center-to-center initially → `snap_path_endpoints_to_faces` moves endpoints to rect edges
2. **OrthogonalRoute:** `build_path_from_hints` (center-based) → `anchor_path_endpoints_to_endpoint_faces` → endpoints on rect faces; backward edges get `snap_backward_endpoints_to_shape` for diamond/hexagon

**Conclusion:** Routed paths target **shape boundaries** (rect edges, or diamond/ellipse boundaries for backward edges), not centers. The path coordinates are in the same space as the geometry rects (top-left based in flux).

### 3. Why "More Data?" Edges May Connect

The two edges exiting "More Data?" (E→A "yes", E→F "no") may appear to connect because:

1. **Path starts outside the diamond:** The orthogonal router places path start on the diamond's **rect** boundary. For a diamond, the visible shape is smaller than the bounding rect (corners of rect are outside the diamond). If the path start lands on a rect edge that maps to a diamond vertex (e.g. right edge center), it can look correct.
2. **Center-targeting fallback:** When `build_path_from_hints` is used without a valid hint, it uses `rect_center` (center-to-center). If the routing falls back to center-based paths for some edges, those would connect at center—which tldraw can render correctly with `isPrecise: false` or when the anchor happens to align.

### 4. Dimension Mismatch: mmdflux vs. tldraw

**mmdflux node dimensions:** Come from layout (text measurement, node label length, etc.). The rect is the layout's bounding box.

**tldraw adapter (`scaleNodeRect`):**

- Uses `node.size.width/height` from MMDS
- Applies `MIN_LABEL_PAD_X`, `MIN_LABEL_PAD_Y`, `CHAR_WIDTH_EST` to **enlarge** nodes so labels don't wrap
- For diamonds: forces square via `Math.max(width, height)`

**Effect:** tldraw shapes can be **larger** than the mmdflux rects. Path endpoints were computed for the smaller mmdflux rect. When we use larger tldraw rects:

- Endpoints may fall **inside** the tldraw shape (causing "stray marks")
- Or the path may not intersect the expanded rect (ray extension helps but can pick wrong edge)

### 5. Excalidraw Adapter Pattern

The Excalidraw adapter (adapters/excalidraw/src/convert.ts:196-219) has `adjustEndpoint`:

- "MMDS path endpoints sit at (or near) the original node boundary, which is inside the padded shape"
- Snaps path endpoints to the **padded** boundary using the adjacent path segment direction
- Uses `adjustEndpoint(pt, adjacentPt, nodeCx, nodeCy, paddedW, paddedH)` to move endpoints to the correct edge of the padded rect

This suggests: (a) MMDS paths target boundaries, and (b) when the consumer uses different (e.g. padded) dimensions, endpoints must be re-projected.

## Recommendations

### A. Fix mmdflux MMDS output (position = center)

In `src/mmds.rs`, change `mmds_node` to output center:

```rust
position: MmdsPosition {
    x: pn.rect.x + pn.rect.width / 2.0,
    y: pn.rect.y + pn.rect.height / 2.0,
},
```

This aligns with the spec and with adapter expectations. Path coordinates remain in layout space; they were computed against the same rects, so no path changes needed.

### B. Add scaling / padding options to tldraw adapter

Options to consider:

- `--node-padding`: Extra padding added to node dimensions (default 0) to compensate for tldraw font/size differences
- `--scale`: Already exists; document that it affects both positions and path points uniformly
- `--preserve-mmds-dimensions`: When set, skip `MIN_LABEL_PAD_*` and `CHAR_WIDTH_EST` so tldraw rects match mmdflux exactly (may cause label wrapping)

### C. Endpoint re-projection when dimensions differ

When the adapter enlarges nodes (e.g. for label fit), re-project path endpoints onto the new boundary—similar to Excalidraw's `adjustEndpoint`. The current `edgeAnchor` logic does this using the path direction, but it uses the **enlarged** rect. The path was computed for the **original** rect. If we have access to the original MMDS size, we could:

1. Compute anchor on the original rect (where the path actually hits)
2. Map that normalized anchor to the enlarged rect (same relative position on the shape)

### D. Document coordinate system for adapter authors

Add to docs/mmds.md or a separate adapter guide:

- Position is center; rect = `[position.x - size.width/2, position.y - size.height/2, size.width, size.height]`
- Path points are in the same coordinate space
- Path endpoints target shape boundaries (rect or diamond/ellipse)
- Adapters that change node dimensions should re-project path endpoints
