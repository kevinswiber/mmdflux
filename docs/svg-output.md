# mmdflux SVG output contract

This document describes the parts of mmdflux's SVG output that downstream
consumers can rely on for styling and DOM targeting. Use it as the source of
truth when writing CSS against rendered diagrams or when embedding mmdflux
output in another application.

## Subgraph wrapper

Each subgraph (flowchart `subgraph ... end`, class-diagram namespace, state
composite region) is rendered as:

```html
<g class="cluster {userClasses}" id="{subgraphId}">
  <rect class="subgraph" ... />
  <text>{title}</text>
</g>
```

The `<g class="cluster">` wrapper is the canonical CSS hook for styling whole
subgraphs. The inner `<rect class="subgraph">` keeps its class so internal
measurement code and existing CSS that targets the rect directly continue to
work.

### User class names

Mermaid `class lr blueFill` and inline `lr:::blueFill` annotations that target
a subgraph identifier append `blueFill` to the wrapper's `class` attribute, in
application order. Re-applying the same class is a no-op (duplicates are
suppressed).

`classDef blueFill fill:#9cf` defines what `blueFill` *means* visually. The
resolved style continues to flow onto the inner rect as inline attributes; the
class name itself reaches the wrapper so external CSS can extend or override.

### `id` attribute

The wrapper `id` is derived from the Mermaid subgraph identifier via XML-safe
escaping: `"`, `&`, `<`, `>` are entity-escaped, and whitespace characters are
replaced with `_`. Unicode letters pass through unchanged.

The `id` is stable as long as the source identifier in the Mermaid input is
stable.

## CSS targeting

```css
/* Target every subgraph */
g.cluster > rect {
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.2));
}

/* Target a user class */
g.cluster.blueFill rect {
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.25));
}

g.cluster.blueFill > text {
  letter-spacing: 0.05em;
}

/* Target a specific subgraph by id */
g.cluster#lr > rect {
  outline: 2px dashed currentColor;
}
```

## MMDS round-trip

The user class list survives serialization to MMDS JSON. The
`org.mmdflux.node-style.v1.subgraphs[<id>].classNames` array preserves the
applied class names (in order) and is replayed onto the wrapper when the
document is hydrated back into a diagram. See [`mmds.md`](./mmds.md) for the
full extension shape.

## Not yet supported

- Node-level `<g>` wrappers with user-class propagation. Subgraph wrappers
  are the first surface; node-level parity is tracked under the per-element
  styling umbrella (GitHub issue
  [#333](https://github.com/kevinswiber/mmdflux/issues/333)).
- `data-look`, `data-id`, and other Mermaid attribute hooks. These are
  deferred to follow-up work on the same umbrella.

## What is intentionally preserved

- `<rect class="subgraph">` on the inner rect. mmdflux's internal dynamic
  text-metrics path relies on this class for width measurement; removing it
  would silently break that contract. Downstream code may continue to use it
  as a fallback hook but should prefer `g.cluster` for new styling work.
