# mmdflux SVG output contract

This document describes the parts of mmdflux's SVG output that downstream
consumers can rely on for styling and DOM targeting. Use it as the source of
truth when writing CSS against rendered diagrams or when embedding mmdflux
output in another application.

## Subgraph wrapper

Each subgraph (flowchart `subgraph ... end`, class-diagram namespace, state
composite region) is rendered as:

```html
<g class="cluster {userClasses}" id="{subgraphId}" data-id="{subgraphId}" data-look="classic">
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

### `id` and `data-id` attributes

The wrapper `id` is derived from the Mermaid subgraph identifier via XML-safe
escaping: `"`, `&`, `<`, `>` are entity-escaped, and whitespace characters are
replaced with `_`. Unicode letters pass through unchanged.

The `data-id` attribute carries the original (un-substituted) subgraph
identifier, XML-attribute-safe but with whitespace preserved. Use it when the
caller needs to recover the identifier verbatim.

Both attributes are stable as long as the source identifier in the Mermaid
input is stable.

### `data-look` attribute

`data-look="classic"` is emitted on every wrapper. mmdflux currently renders
only the classic look, but the attribute is present unconditionally so
Mermaid-targeted CSS that scopes by `[data-look="classic"]` continues to
match.

## Node wrapper

Each node is rendered as:

```html
<g class="node default {userClasses}" id="{nodeId}" data-id="{nodeId}" data-look="classic">
  <!-- shape primitives (rect, polygon, circle, …) and label text -->
</g>
```

The `<g class="node default">` wrapper is the canonical CSS hook for styling
individual nodes. `default` mirrors Mermaid's wrapper class so existing
Mermaid-targeted CSS that uses `g.node.default` keeps working.

### User class names

Mermaid `class A blueFill`, inline `A:::blueFill`, and `class A,B foo`
annotations append user class identifiers to the wrapper's `class` attribute
in application order. Duplicates are suppressed.

`classDef blueFill fill:#9cf` defines what `blueFill` *means* visually. The
resolved style continues to flow onto the inner shape primitives as inline
attributes; the class name itself reaches the wrapper so external CSS can
extend or override.

### `id`, `data-id`, and `data-look`

The node `id` is derived from the Mermaid node identifier via the same
XML-safe escaping used for subgraph ids. `data-id` carries the original
identifier with whitespace preserved. `data-look="classic"` is emitted
unconditionally.

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

/* Target every node */
g.node.default rect,
g.node.default polygon,
g.node.default circle {
  cursor: pointer;
}

/* Target a node user class */
g.node.default.highlight rect {
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.3));
}
```

## MMDS round-trip

The user class list for nodes and subgraphs survives serialization to MMDS
JSON. The `org.mmdflux.node-style.v1.nodes[<id>].classNames` and
`.subgraphs[<id>].classNames` arrays preserve applied class names (in order)
and are replayed onto their respective wrappers when the document is hydrated
back into a diagram. See [`mmds.md`](./mmds.md) for the full extension shape.

## What is intentionally preserved

- `<rect class="subgraph">` on the inner rect. mmdflux's internal dynamic
  text-metrics path relies on this class for width measurement; removing it
  would silently break that contract. Downstream code may continue to use it
  as a fallback hook but should prefer `g.cluster` for new styling work.
