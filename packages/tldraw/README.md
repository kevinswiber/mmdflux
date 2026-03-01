# @mmds/tldraw

Converts [MMDS](https://github.com/mmdflux/mmdflux/blob/main/docs/mmds.md) JSON into tldraw `.tldr` files.

## Install

```bash
npm install -g @mmds/tldraw
```

Or with npx:

```bash
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-tldraw > out.tldr
```

## Usage

```bash
# Default output is a .tldr envelope
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-tldraw > out.tldr

# Emit raw tldraw store JSON instead of .tldr envelope
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-tldraw --output json > out.store.json

# Emit preview URL (requires preview server running)
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-tldraw -o url

# Open diagram in browser (requires preview server running)
# Terminal 1: npm run preview
# Terminal 2: mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-tldraw --open
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-tldraw --open
```

### Preview server

The `--open` flag sends the diagram to a Vite-based preview server. Start it first:

```bash
cd packages/tldraw && npm run preview
```

Then in another terminal, pipe MMDS to the converter with `--open`. The CLI POSTs the diagram, receives a content-based ID, and opens `http://localhost:5173/?id=<id>`. Same diagram content yields the same ID, so repeated runs don't create duplicates.

You can also POST JSON to `http://localhost:5173/api/diagram` (returns `{ok, id}`), then open `/?id=<id>` or use `?data=<base64>` for inline data. Set `PREVIEW_URL` if the server runs on a different port.

### Options

| Flag             | Short | Values         | Default | Description                                                                            |
| ---------------- | ----- | -------------- | ------- | -------------------------------------------------------------------------------------- |
| `--output`       | `-o`  | `tldr`, `json`, `url` | `tldr`  | Output mode; `url` prints preview URL to stdout                                       |
| `--scale`        |       | number         | `1`     | Scale MMDS coordinate space before conversion                                          |
| `--node-spacing` |       | number         | `1.2`   | Multiplier for spacing between nodes (positions and paths); does not change node sizes |
| `--open`         |       | boolean        | `false` | POST diagram to preview server and open in browser (run `npm run preview` first)       |

## Mapping

- MMDS nodes map to tldraw `geo` shapes with optional `text` labels.
- MMDS subgraphs map to `frame` shapes and preserve parent nesting via `subgraph.parent`.
- MMDS edges map to native `arrow` shapes. Routed polylines are approximated to tldraw arrow bend with deterministic heuristics.
- Endpoint intent (`from_subgraph` / `to_subgraph`) binds arrows to frames when possible.

## Fidelity caveat

tldraw arrows do not store arbitrary polyline waypoint lists. The adapter preserves edge endpoints, labels, and a deterministic best-fit bend/arc approximation.
