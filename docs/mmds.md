# MMDS — Machine-Mediated Diagram Specification

MMDS is the structured JSON output format for graph-family diagrams produced by mmdflux. It is designed for machine consumption in LLM pipelines, adapter libraries, and agentic workflows.

## Contract Ownership and Parity Harness

Rust owns the canonical MMDS contract, MMDS input helpers, and document helpers under `src/mmds/`, while runtime MMDS frontend detection lives in `src/frontends.rs`.

Locked cross-language contract fixtures live under `tests/fixtures/mmds/contracts/`. They are consumed by:

- Rust contract tests in `tests/mmds_json.rs`
- `@mmds/core` parity tests in `packages/mmds-core/test/*.test.mjs`
- adapter package fixture tests in `packages/*/test/*.test.mjs`

Any intentional MMDS contract change should update those locked fixtures and the related Rust/TypeScript assertions in the same change.

## Supported Rust Entry Points

Most Rust callers should produce MMDS through the high-level runtime facade:

- `render_diagram(input, OutputFormat::Mmds, &RenderConfig::default())`
  (`render_diagram` auto-detects MMDS input and dispatches to the replay path)
- `materialize_diagram(input, &config)` for materializing a graph-family
  `mmds::Document` directly from Mermaid or MMDS input
- `render_document(&document, output_format, &config)` for rendering an
  already-parsed graph-family `mmds::Document` without a JSON round trip

Adapter-oriented workflows can use the low-level API:

- `mmdflux::builtins::default_registry()` for builtin registry wiring
- `mmdflux::registry` and `mmdflux::payload` for explicit payload flows
- `mmdflux::mmds` for parsing into `Document`, hydration, profile negotiation,
  and Mermaid generation
- `mmdflux::views` for materialized read-side views over canonical MMDS payloads

Rust examples for MMDS-oriented adapter workflows:

- `examples/mmds_replay.rs` — profile negotiation, replay, and Mermaid
  regeneration from an MMDS document
- `examples/materialized_view.rs` — focused read-side view projection and replay
- `examples/commands_events_views.rs` — command application, model events, view
  projection, and rendering
- `examples/snapshot_diff.rs` — snapshot comparison between two materialized
  MMDS documents

Fixture-backed payloads used throughout the Rust contract tests live at:

- `tests/fixtures/mmds/generation/basic-flow.json`
- `tests/fixtures/mmds/positioned/routed-basic.json`

## Input Status

MMDS input support is active:

- Runtime detects MMDS as an input frontend, resolves the logical diagram type from payload metadata, and dispatches through the existing family pipeline.
- Parse-time envelope validation is active (`MMDS parse error: ...` on invalid JSON/envelope).
- MMDS core hydration/validation contract is implemented (`MMDS validation error: ...` on invalid core payloads).
- Render runtime dispatches by `geometry_level` with an explicit capability matrix.

### MMDS Input Render Capability Matrix

| `geometry_level`      | text | ascii | svg | mmds/json |
| --------------------- | ---- | ----- | --- | --------- |
| `layout`              | ✅   | ✅    | ✅  | ✅        |
| `routed` (positioned) | ✅\* | ✅\*  | ✅  | ✅        |

\* For text/ascii, routed path fields are currently ignored and output is re-routed on the text grid from core topology.

## Materialized Views

The Rust `mmdflux::views` module provides a read-side contract for focused
diagram views over canonical graph-family MMDS payloads. It is intentionally a
small materialization API, not a general graph query language.

The primary entry point is:

- `mmdflux::views::project(canonical: &mmdflux::mmds::Document, spec: &ViewSpec) -> Result<(Document, Vec<ViewEvent>), ViewError>`

`ViewSpec` v1 supports:

- `Selector::All`
- `Selector::Anchor(AnchorRef::Node(id))`
- `Selector::Anchor(AnchorRef::Subgraph(id))` for the subgraph container only
- `Selector::Traversal` from a node anchor with upstream, downstream, or neighbor hops
- `Selector::Predicate(NodePredicate::Shape(shape))` with `mmdflux::graph::Shape`
- `Selector::Predicate(NodePredicate::Parent(subgraph_id))`
- `Selector::SubgraphDescendants(id)` for recursive subgraph contents
- ordered `Include` and `Exclude` statements

These primitives are present in the public types but intentionally deferred in
the v1 evaluator:

- tag predicates
- edge anchors
- boundary stubs
- compact, local-reflow, and incremental layout modes
- compound flattening

Unsupported primitives return `ViewError::NotImplementedYet` rather than
falling back silently.

### View Payload Contract

`project` returns a normal MMDS `Document` with retained nodes, retained
subgraphs, surviving edges, and a `Vec<ViewEvent>` describing omitted canonical
elements. Surviving edge IDs are preserved verbatim from the canonical payload;
filtered views may therefore contain sparse IDs such as `["e0", "e2", "e4"]`.
Consumers should not assume edge IDs are contiguous.

Materialized view payloads carry a top-level extension marker:

```json
{
  "extensions": {
    "org.mmdflux.view.v1": {
      "layout_mode": "shared_coordinates",
      "boundary_policy": "omit"
    }
  }
}
```

When the canonical payload contains the text renderer projection extension
(`org.mmdflux.render.text.v1`), v1 view materialization keeps `projection.node_ranks`
for retained nodes and drops edge-array-indexed projection maps such as
`edge_waypoints` and `label_positions`. This avoids re-indexing edge projection
data through sparse view edges.

To render a materialized view, pass the returned `Document` through the typed
runtime replay helper:

```rust
use mmdflux::{OutputFormat, RenderConfig, render_document};

let text = render_document(&view_payload, OutputFormat::Text, &RenderConfig::default())?;
println!("{text}");
```

## MMDS -> Mermaid Generation Contract

mmdflux provides deterministic Mermaid generation for graph-family MMDS payloads:

- `mmdflux::mmds::generate_mermaid_from_str(input: &str) -> Result<String, GenerationError>`
- `mmdflux::mmds::generate_mermaid(document: &Document) -> Result<String, GenerationError>`

### Canonical Document Rules

Generated Mermaid is canonicalized as:

1. Header first: `flowchart {direction}`
2. Subgraphs emitted in deterministic ID order, with nested `subgraph ... end` blocks and optional `direction` lines
3. Nodes emitted in deterministic ID order within each scope
4. Edges emitted in deterministic edge-ID order (`e{number}` before non-numeric IDs)
5. Generated Mermaid always ends with a trailing newline (`\n`)

### Identifier and Label Policy

- Node and subgraph identifiers are normalized to Mermaid-safe tokens:
  - keep `[A-Za-z0-9_]`
  - replace other characters with `_`
  - collapse repeated `_`, trim outer `_`
  - prefix with `node_` / `subgraph_` if empty or digit-leading
  - resolve collisions deterministically with suffixes (`_2`, `_3`, ...)
- Labels are quoted when needed for parser safety (for example spaces or `|`), with `\\` and `\"` escaping.
- Edge labels use pipe syntax (`A -->|label| B`) and escape `|` as `&#124;`.

Example (validated by tests):

- Input node ID `node 1` and label `A | B`
- Generated Mermaid node: `node_1["A | B"]`

### Connector / minlen Policy

`edge.minlen` is preserved by emitting connector length variants (`-->`, `--->`, `==>`, `===>`, `---`, `----`, etc.) so parse-back semantics stay stable.

### Known Non-Goals / Caveats

- Generation preserves semantics, not source formatting. Comments, original statement ordering, quoting style, and alias spellings are not reconstructed.
- Non-graph payloads (for example `diagram_type: "sequence"`) are rejected with `GenerationError`.
- IDs that are not Mermaid-safe are normalized; exact original ID text is not retained in generated Mermaid.
- Mermaid regeneration from MMDS does not yet emit style, class, or link directives.
- Node styles can still round-trip through the `mmdflux-node-style-v1` extension for MMDS, text, and SVG rendering.

## MMDS Input Validation Contract

Hydration follows a **strict-core / permissive-extensions** policy:

- **Strict core** (rejected):
  - unsupported `version`
  - invalid core enum values (`geometry_level`, directions, shapes, strokes, arrows)
  - missing required identifiers (`node.id`, `edge.id`, `subgraph.id`, edge endpoints)
  - dangling references (edge source/target, node parent, subgraph parent/children, endpoint-intent subgraph IDs)
  - cyclic subgraph parent chains
- **Permissive extensions** (tolerated):
  - unknown `profiles` values
  - unknown namespaces under `extensions`

Hydration also expands omitted node/edge fields from the document `defaults` block before mapping to internal graph types.

### Deterministic Ordering

Hydrated edge insertion order is deterministic:

1. sort by explicit edge ID when it matches `e{number}`
2. fallback to declaration order for ties/non-numeric IDs

### Canonical Error Example

`MMDS validation error: edge e0 target 'X' not found`

### Endpoint Intent Compatibility

MMDS edges may include optional endpoint intent fields:

- `from_subgraph`
- `to_subgraph`

When present, hydration preserves these into internal edge state and renderers can reproduce subgraph-as-endpoint behavior deterministically.

When absent (older payloads), hydration falls back to node-only endpoint semantics (`source`/`target`), which remains valid but may diverge from direct Mermaid replay in subgraph-edge cases.

The fixture-backed endpoint-intent cases above are exercised through the public
runtime and replay paths in the test suite rather than an older removed helper
directory.

## Geometry Levels

MMDS supports two geometry levels that control how much spatial detail is included:

### Layout (default)

The default `--format mmds` output. (`--format json` is an alias.) Includes:

- **Node geometry**: position (center x, y) and size (width, height) in unitless MMDS coordinate space (currently SVG-pixel-aligned in mmdflux output)
- **Edge topology**: source, target, label, stroke style, arrow types
- **Diagram bounds**: overall width and height in the same coordinate space
- **Subgraph structure**: id, title, direct children, parent, direction override

Does **not** include edge paths, waypoints, ports, or routing metadata.

```bash
mmdflux --format mmds diagram.mmd
```

### Routed (opt-in)

Explicit opt-in via `--geometry-level routed`. Includes everything from layout plus:

- **Edge paths**: `edge.path` polyline coordinates as `[x, y]` pairs; this is the authoritative visible geometry for routed edge shape and endpoints
- **Edge metadata**: `label_position`, `is_backward`, `source_port`, `target_port`; ports are logical route-intent anchors, not the visible path itself
- **Subgraph bounds**: width and height of each subgraph

```bash
mmdflux --format mmds --geometry-level routed diagram.mmd
```

## Document Envelope

```json
{
  "version": 1,
  "profiles": ["mmds-core-v1", "mmdflux-svg-v1"],
  "extensions": {
    "org.mmdflux.render.svg.v1": {
      "edge_style": "curved",
      "edge_radius": 5
    }
  },
  "defaults": {
    "node": { "shape": "rectangle" },
    "edge": { "stroke": "solid", "arrow_start": "none", "arrow_end": "normal", "minlen": 1 }
  },
  "geometry_level": "layout",
  "metadata": {
    "diagram_type": "flowchart",
    "direction": "TD",
    "bounds": { "width": 120.0, "height": 80.0 }
  },
  "nodes": [...],
  "edges": [...],
  "subgraphs": [...]
}
```

### Fields

| Field                   | Type                     | Description                                                                                                                          |
| ----------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `version`               | `1`                      | Integer schema version. Increment only for breaking MMDS changes.                                                                    |
| `profiles`              | string[]                 | Optional behavior bundles for capability negotiation.                                                                                |
| `extensions`            | object                   | Optional namespaced extension payloads keyed by versioned namespace ID (`*.v{number}`).                                              |
| `defaults`              | object                   | Document-level defaults for omitted node/edge fields                                                                                 |
| `geometry_level`        | `"layout"` or `"routed"` | Geometry detail level                                                                                                                |
| `metadata.diagram_type` | string                   | `"flowchart"`, `"class"`, `"state"`, or `"sequence"`                                                                                 |
| `metadata.direction`    | string                   | `"TD"`, `"TB"`, `"BT"`, `"LR"`, or `"RL"`; `"TB"` deserializes as canonical `"TD"`                                                   |
| `metadata.bounds`       | object                   | Overall diagram canvas extents (`width`, `height`) in unitless MMDS coordinate space (currently SVG-pixel-aligned in mmdflux output) |
| `metadata.engine`       | string?                  | Engine+algorithm that produced this output (e.g., `"flux-layered"`). Omitted when not produced via the solve pipeline.               |
| `subgraphs`             | array                    | Subgraph inventory (omitted when empty)                                                                                              |

## Profiles and Extensions Governance

MMDS keeps core graph semantics compact while allowing renderer- or adapter-specific controls through explicit governance fields.

### Initial Profile Vocabulary

- `mmds-core-v1` — baseline MMDS core behavior contract.
- `mmdflux-svg-v1` — SVG-oriented controls and expectations.
- `mmdflux-text-v1` — text/ASCII-oriented controls and expectations.
- `mmdflux-node-style-v1` — node style extension contract for `fill`, `stroke`, and `color` replay.
- `mmdflux-text-metrics-v1` — graph-family text metrics identity and layout text values for deterministic replay.

### Extension Namespace Rules

- Extension keys live under `extensions` and must be namespaced + versioned.
- Canonical namespace style: reverse-domain-like segments ending in `.v{number}`.
- Example: `org.mmdflux.render.svg.v1`
- Extension payload values must be JSON objects.

### Compatibility Rules

- Unknown `profiles` values are tolerated.
- Unknown extension namespaces are tolerated.
- Unsupported core `version` remains a hard validation error.

### Adapter Negotiation Checklist

1. Parse and validate MMDS core fields first.
2. Evaluate `profiles` into `{supported, unknown}` sets.
3. Apply only recognized extension namespaces.
4. Ignore unknown profiles/extensions without mutating core semantics.
5. If a required profile is missing, fall back deterministically or fail with a clear capability error.

### Node Style Extension

When at least one node carries a non-empty internal `NodeStyle`, mmdflux emits:

- profile: `mmdflux-node-style-v1`
- extension namespace: `org.mmdflux.node-style.v1`

Payload shape:

```json
{
  "profiles": ["mmds-core-v1", "mmdflux-node-style-v1"],
  "extensions": {
    "org.mmdflux.node-style.v1": {
      "nodes": {
        "A": {
          "fill": "#ffeeaa",
          "stroke": "#333",
          "color": "#111"
        }
      }
    }
  }
}
```

Rules:

- Omit the profile and extension entirely when no node styles are present.
- `fill`, `stroke`, and `color` preserve the raw Mermaid/MMDS color token.
- MMDS input hydration replays this extension back into internal node styles for text and SVG rendering.
- Mermaid generation from MMDS style extensions is still deferred.

### Text Metrics Extension

Graph-family MMDS output carries the effective text metrics contract used for
layout and replay:

- profile: `mmdflux-text-metrics-v1`
- extension namespace: `org.mmdflux.text-metrics.v1`
- default compatibility metrics profile: `mmdflux-heuristic-proportional-v1`
- opt-in recorded metrics profile: `mmdflux-sans-v1`

Payload shape:

```json
{
  "profiles": ["mmds-core-v1", "mmdflux-text-metrics-v1"],
  "extensions": {
    "org.mmdflux.text-metrics.v1": {
      "metricsProfile": {
        "id": "mmdflux-heuristic-proportional-v1",
        "source": "heuristic",
        "version": 1
      },
      "defaultTextStyle": {
        "font-family": "\"trebuchet ms\", verdana, arial, sans-serif",
        "font-size": 16,
        "font-style": "normal",
        "font-weight": "400",
        "line-height": 24
      },
      "layoutText": {
        "node-padding-x": 15,
        "node-padding-y": 15,
        "label-padding-x": 4,
        "label-padding-y": 2,
        "edge-label-max-width": 200
      }
    }
  }
}
```

Rules:

- New graph-family MMDS output emits this profile and extension.
- `mmdflux-heuristic-proportional-v1` remains the default. `mmdflux-sans-v1`
  is opt-in, graph-family-only, and backed by mmdflux-owned generated static
  width tables.
- `metricsProfile.source` is a provenance category: `heuristic` for the
  compatibility estimates, `recorded` for generated static tables, and
  reserved `dynamic` for future live measurement backends.
- The recorded profile source font is provenance, not an exact Mermaid or
  browser font claim. Browser `measureText` remains out of scope for this
  static profile.
- Replay uses `metricsProfile.id` plus `layoutText` node padding and edge-label wrap width when the extension is present.
- Replay is document-owned: a caller-supplied `fontMetricsProfile` must match the replay profile, and the replay profile plus persisted `layoutText` values override SVG font and node-padding config.
- Older MMDS documents without the extension replay with the `mmdflux-heuristic-proportional-v1` compatibility defaults.
- A recognized text metrics extension with an unsupported `metricsProfile.id` is a replay error.
- Sequence-family full text-metrics parity remains deferred.

### Node

| Field      | Type              | Level | Description                                                          |
| ---------- | ----------------- | ----- | -------------------------------------------------------------------- |
| `id`       | string            | both  | Node identifier                                                      |
| `label`    | string            | both  | Display label                                                        |
| `shape`    | string            | both  | Shape name (snake_case), omitted when equal to `defaults.node.shape` |
| `parent`   | string?           | both  | Parent subgraph ID                                                   |
| `position` | `{x, y}`          | both  | Center position (not top-left)                                       |
| `size`     | `{width, height}` | both  | Bounding box                                                         |

### Edge

| Field            | Type          | Level  | Description                                                                                                                                                |
| ---------------- | ------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `source`         | string        | both   | Source node ID                                                                                                                                             |
| `target`         | string        | both   | Target node ID                                                                                                                                             |
| `id`             | string        | both   | Deterministic edge ID (`e{declaration_index}`)                                                                                                             |
| `label`          | string?       | both   | Edge label                                                                                                                                                 |
| `from_subgraph`  | string?       | both   | Optional source subgraph endpoint intent (for subgraph-as-source edges)                                                                                    |
| `to_subgraph`    | string?       | both   | Optional target subgraph endpoint intent (for subgraph-as-target edges)                                                                                    |
| `stroke`         | string        | both   | `"solid"`, `"dotted"`, `"thick"`, `"invisible"`; omitted when equal to `defaults.edge.stroke`                                                              |
| `arrow_start`    | string        | both   | `"none"`, `"normal"`, `"cross"`, `"circle"`, `"open_triangle"`, `"diamond"`, `"open_diamond"`; omitted when equal to `defaults.edge.arrow_start`           |
| `arrow_end`      | string        | both   | `"none"`, `"normal"`, `"cross"`, `"circle"`, `"open_triangle"`, `"diamond"`, `"open_diamond"`; omitted when equal to `defaults.edge.arrow_end`             |
| `minlen`         | integer       | both   | Minimum rank separation; omitted when equal to `defaults.edge.minlen`                                                                                      |
| `path`           | `[[x,y],...]` | routed | Authoritative visible routed polyline path coordinates. Consumers should use `edge.path` for visible edge geometry and endpoints.                          |
| `label_position` | `{x, y}`      | routed | Label center                                                                                                                                               |
| `is_backward`    | boolean       | routed | Flows backward in layout                                                                                                                                   |
| `source_port`    | Port?         | routed | Logical source port anchor metadata describing route intent (see Port below)                                                                               |
| `target_port`    | Port?         | routed | Logical target port anchor metadata describing route intent (see Port below)                                                                               |
| `label_side`     | string?       | both   | `"above"`, `"below"`, or `"center"`; present at both layout and routed levels when the engine has assigned a side, omitted otherwise                       |
| `label_rect`     | Rect?         | routed | Padded label rectangle `{x, y, width, height}` including `label_padding_x`/`label_padding_y` padding; omitted when the engine has not assigned a rectangle |

### Port

Port metadata describes a logical route-intent anchor on a node boundary. It is
metadata for choosing and grouping a boundary face, not a replacement for
`edge.path` as visible geometry. A port may differ from `edge.path[0]` or
`edge.path[-1]`; consumers that need the visible rendered endpoint should read
the routed path.

| Field        | Type     | Description                                                                                                                    |
| ------------ | -------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `face`       | string   | Logical node boundary face for route intent: `"top"`, `"bottom"`, `"left"`, or `"right"`                                       |
| `fraction`   | number   | Position along the logical face (0.0 = start, 1.0 = end). Top/bottom: left-to-right. Left/right: top-to-bottom.                |
| `position`   | `{x, y}` | Logical anchor coordinate derived from face, fraction, and node bounds. It is not guaranteed to equal a visible path endpoint. |
| `group_size` | integer  | Number of edges sharing this face on this node                                                                                 |

### Subgraph

| Field       | Type              | Level  | Description                                           |
| ----------- | ----------------- | ------ | ----------------------------------------------------- |
| `id`        | string            | both   | Subgraph identifier                                   |
| `title`     | string            | both   | Display title                                         |
| `children`  | string[]          | both   | Direct child node IDs                                 |
| `parent`    | string?           | both   | Parent subgraph ID                                    |
| `direction` | string?           | both   | Direction override: `"TD"`, `"TB"`, `"BT"`, `"LR"`, or `"RL"`; `"TB"` deserializes as canonical `"TD"` |
| `bounds`    | `{width, height}` | routed | Bounding box dimensions                               |

## Schema

The formal JSON Schema is available at [`docs/mmds.schema.json`](./mmds.schema.json).

## Coordinate System

MMDS coordinates are unitless coordinate-space values.

In current mmdflux output, these values are SVG-pixel-aligned.

- `position.x` and `position.y` are node centers (not top-left anchors).
- `size.width` and `size.height` are node dimensions in the same coordinate space.
- `metadata.bounds.width` and `metadata.bounds.height` define full document extents in the same space.
- `metadata.bounds` is a canvas extent, not guaranteed to be a tight content bounding box.
- Current graph-family engines may include outer margin in `metadata.bounds`.
- Routed `path` points and `label_position` values also use this same coordinate space.

Consumers may scale these values to pixels, character cells, or any target render space.
Consumers SHOULD scale uniformly (same factor on both axes) to preserve the aspect ratio implied by `metadata.bounds`.
Consumers rendering top-left-anchored primitives should convert node placement as:

- `left = position.x - size.width / 2`
- `top = position.y - size.height / 2`

## Defaults and Omission

MMDS has a single JSON shape. Fields that match document defaults may be omitted.

- `defaults.node.shape` defines the implicit node shape when `node.shape` is absent.
- `defaults.edge.stroke`, `defaults.edge.arrow_start`, `defaults.edge.arrow_end`, and `defaults.edge.minlen` define implicit edge semantics when those fields are absent.
- `subgraphs` is omitted when there are no subgraphs.

Consumers should apply defaults before processing if they require explicit values.

## Conformance Tiers

MMDS roundtrip quality is measured across three conformance tiers, comparing the direct render pipeline (Mermaid text → Diagram → render) against the MMDS roundtrip pipeline (Mermaid text → MMDS JSON → hydrate → render).

### Semantic parity

Graph structure equivalence: nodes, edges, subgraphs, direction, labels, strokes, arrows, and minlen all survive the roundtrip. Subgraph child lists are normalized to direct children for comparison (the parser includes all descendants; MMDS uses direct children only).

### Nested subgraph membership parity strategy

MMDS keeps `subgraph.children` as direct children at the interchange boundary. This remains the canonical payload contract for validation, hydration, and downstream adapters.

For runtime/layout internals, mmdflux deterministically reconstructs any additional compound layout membership needed by the layout from parent links and subgraph topology. In other words:

- MMDS payload contract: direct children only.
- Runtime compound layout membership: reconstructed descendants as needed for compound layout membership parity with direct Mermaid parsing behavior.

This split preserves a stable external schema while allowing internal layout behavior to stay parity-aligned.

### Layout parity

Geometry equivalence: both pipelines produce the same layout — identical node positions, sizes, edge endpoints, waypoints, label positions, subgraph bounds, and overall bounds within float tolerance (0.01).

### Visual parity

Rendered output equivalence: both text and SVG output are byte-identical between direct and roundtrip paths.

### Current status

| Tier     | Flowchart      | Class        |
| -------- | -------------- | ------------ |
| Semantic | 32/32 fixtures | 1/1 fixtures |
| Layout   | 32/32 fixtures | 1/1 fixtures |
| Visual   | 32/32 fixtures | 1/1 fixtures |

Nested subgraph fixtures now pass visual parity after runtime compound-membership reconstruction. The MMDS contract remains direct children only, while runtime layout internals reconstruct descendants as needed for parity.

### Running conformance checks

```bash
just conformance
```

## Supported Diagram Types

| Type      | `diagram_type` | Family   | Status    |
| --------- | -------------- | -------- | --------- |
| Flowchart | `"flowchart"`  | Graph    | Supported |
| Class     | `"class"`      | Graph    | Supported |
| State     | `"state"`      | Graph    | Supported |
| Sequence  | `"sequence"`   | Timeline | Supported |

### Graph-Family vs Timeline-Family Document

Graph-family diagrams (flowchart, class, state) use the standard MMDS envelope with `nodes`, `edges`, `subgraphs`, and `defaults`. This is the typed Rust `mmdflux::mmds::Document` contract.

Timeline-family diagrams (sequence) use a separate internal serde envelope with the same top-level compatibility fields (`version`, `geometry_level`, `metadata`, `nodes`, `edges`), but `nodes` and `edges` are always empty arrays. The diagram content is in sequence-specific body fields described below.

## Sequence MMDS Profile

Sequence diagrams produce MMDS JSON with timeline-native fields. The envelope fields (`version`, `geometry_level`, `metadata`) follow the same contract as graph-family output. `metadata.diagram_type` is `"sequence"`. `metadata.direction` is omitted. `nodes` and `edges` are empty arrays.

Positions come from the proportional SVG layout engine and are in the same unitless MMDS coordinate space as graph-family output.

### Sequence-Specific Fields

#### `participants`

| Field        | Type              | Description                                     |
| ------------ | ----------------- | ----------------------------------------------- |
| `id`         | string            | Participant identifier from source              |
| `label`      | string            | Display label (alias if provided, otherwise id) |
| `kind`       | string            | `"participant"` or `"actor"`                    |
| `position`   | `{x, y}`          | Top-left of header box                          |
| `size`       | `{width, height}` | Header box dimensions                           |
| `lifeline_x` | number            | Center x of the vertical lifeline               |

#### `messages`

| Field        | Type   | Description                                        |
| ------------ | ------ | -------------------------------------------------- |
| `id`         | string | Deterministic message ID (`m0`, `m1`, ...)         |
| `from`       | number | Source participant index                           |
| `to`         | number | Target participant index (same as `from` for self) |
| `line_style` | string | `"solid"` or `"dashed"`                            |
| `arrow_head` | string | `"filled"`, `"none"`, `"cross"`, or `"async"`      |
| `text`       | string | Message label text                                 |
| `y`          | number | Vertical position of the arrow                     |

#### `notes`

| Field          | Type              | Description                             |
| -------------- | ----------------- | --------------------------------------- |
| `placement`    | string            | `"left_of"`, `"right_of"`, or `"over"`  |
| `participants` | number[]          | Participant indices the note relates to |
| `text`         | string            | Note text                               |
| `position`     | `{x, y}`          | Top-left of note box                    |
| `size`         | `{width, height}` | Note box dimensions                     |

#### `activations`

| Field         | Type   | Description                   |
| ------------- | ------ | ----------------------------- |
| `participant` | number | Participant index             |
| `y_start`     | number | Top of the activation bar     |
| `y_end`       | number | Bottom of the activation bar  |
| `depth`       | number | Nesting depth (0 = outermost) |

#### `blocks`

| Field      | Type                    | Description                                                            |
| ---------- | ----------------------- | ---------------------------------------------------------------------- |
| `kind`     | string                  | `"loop"`, `"alt"`, `"opt"`, `"par"`, `"critical"`, `"break"`, `"rect"` |
| `label`    | string                  | Block header label                                                     |
| `rect`     | `{x, y, width, height}` | Bounding rectangle                                                     |
| `dividers` | array                   | `[{y, kind, label}]` — `kind` is `"else"`, `"and"`, or `"option"`      |

#### `participant_boxes`

| Field          | Type                    | Description                          |
| -------------- | ----------------------- | ------------------------------------ |
| `label`        | string?                 | Optional grouping label              |
| `color`        | string?                 | Optional fill color                  |
| `participants` | number[]                | Participant indices in this grouping |
| `rect`         | `{x, y, width, height}` | Bounding rectangle                   |

### Sequence Example

```json
{
  "version": 1,
  "geometry_level": "layout",
  "metadata": {
    "diagram_type": "sequence",
    "bounds": { "width": 370.0, "height": 220.0 }
  },
  "nodes": [],
  "edges": [],
  "participants": [
    {
      "id": "Alice",
      "label": "Alice",
      "kind": "participant",
      "position": { "x": 60.0, "y": 20.0 },
      "size": { "width": 90.0, "height": 40.0 },
      "lifeline_x": 105.0
    },
    {
      "id": "Bob",
      "label": "Bob",
      "kind": "participant",
      "position": { "x": 210.0, "y": 20.0 },
      "size": { "width": 80.0, "height": 40.0 },
      "lifeline_x": 250.0
    }
  ],
  "messages": [
    {
      "id": "m0",
      "from": 0,
      "to": 1,
      "line_style": "solid",
      "arrow_head": "filled",
      "text": "hello",
      "y": 100.0
    }
  ]
}
```
