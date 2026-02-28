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
```

### Options

| Flag | Short | Values | Default | Description |
|------|-------|--------|---------|-------------|
| `--output` | `-o` | `tldr`, `json` | `tldr` | Output mode |
| `--scale` | | number | `1` | Scale MMDS coordinate space before conversion |

## Mapping

- MMDS nodes map to tldraw `geo` shapes with optional `text` labels.
- MMDS subgraphs map to `frame` shapes and preserve parent nesting via `subgraph.parent`.
- MMDS edges map to native `arrow` shapes. Routed polylines are approximated to tldraw arrow bend with deterministic heuristics.
- Endpoint intent (`from_subgraph` / `to_subgraph`) binds arrows to frames when possible.

## Fidelity caveat

tldraw arrows do not store arbitrary polyline waypoint lists. The adapter preserves edge endpoints, labels, and a deterministic best-fit bend/arc approximation.
