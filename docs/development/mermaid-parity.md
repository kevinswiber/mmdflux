# Debug Infrastructure

This document describes the debug and validation infrastructure for mmdflux,
including how to set up dependencies, run parity tests, and debug layout issues.

## Quick Start

1. Clone the repo
2. Run `./scripts/setup-debug-deps.sh` to set up dagre and mermaid
3. Run `cargo test --test dagre_parity` to verify layout parity

## Overview

mmdflux implements a Sugiyama-style hierarchical graph layout algorithm that aims
for parity with dagre.js v0.8.5. The debug infrastructure enables:

- **Parity testing**: Compare mmdflux layout output against dagre.js
- **Pipeline tracing**: Step through layout stages (rank, order, position)
- **Border node debugging**: Verify subgraph border handling

## Dependencies

The debug infrastructure requires two external repositories:

| Repo                                             | Version | Purpose                         |
| ------------------------------------------------ | ------- | ------------------------------- |
| [dagre](https://github.com/dagrejs/dagre)        | v0.8.5  | Reference layout implementation |
| [mermaid](https://github.com/mermaid-js/mermaid) | 09d0650 | Diagram parsing via getData()   |

### Setup Script

Run the bootstrap script to clone and configure dependencies:

```bash
./scripts/setup-debug-deps.sh
```

This creates a `deps/` directory (gitignored) containing:
- `deps/dagre/` - dagre v0.8.5 with npm dependencies
- `deps/mermaid/` - mermaid with pnpm dependencies and custom scripts

### Manual Setup

If you prefer manual setup or have existing checkouts:

```bash
export DAGRE_ROOT=/path/to/dagre
export MERMAID_ROOT=/path/to/mermaid
```

Ensure dagre is at v0.8.5 and mermaid has the patch scripts copied.

## Parity Tests

The `tests/dagre_parity.rs` tests compare mmdflux layout against dagre.js output:

```bash
cargo test dagre_parity
```

### Test Fixtures

Parity fixtures are stored in `tests/parity-fixtures/`. Each fixture contains:

| File                       | Description                        |
| -------------------------- | ---------------------------------- |
| `mmdflux-dagre-input.json` | Input graph in dagre format        |
| `dagre-layout.json`        | Expected layout from dagre.js      |
| `mmdflux-border-nodes.txt` | Border node positions from mmdflux |
| `dagre-border-nodes.txt`   | Border node positions from dagre   |

### Refreshing Fixtures

To regenerate fixtures after dagre changes:

```bash
./scripts/refresh-parity-fixtures.sh
```

## Debug Scripts

### dump-dagre-layout.js

Runs dagre.js layout and outputs the result:

```bash
node scripts/dump-dagre-layout.js input.json > output.json
```

### dump-dagre-pipeline.js

Traces dagre through all pipeline stages:

```bash
node scripts/dump-dagre-pipeline.js input.json > stages.jsonl
```

### dump-dagre-borders.js

Extracts border node positions:

```bash
MMDFLUX_DAGRE_SKIP_TRANSLATE=1 node scripts/dump-dagre-borders.js input.json
```

### dump-dagre-order.js

Dumps node order per rank after ordering phase:

```bash
node scripts/dump-dagre-order.js input.json
```

## Environment Variables

### Paths

| Variable       | Default        | Description               |
| -------------- | -------------- | ------------------------- |
| `DAGRE_ROOT`   | `deps/dagre`   | Path to dagre v0.8.5 repo |
| `MERMAID_ROOT` | `deps/mermaid` | Path to mermaid repo      |

### Debug Output

| Variable                        | Description                              |
| ------------------------------- | ---------------------------------------- |
| `MMDFLUX_DEBUG_LAYOUT=<file>`   | Write layout JSON to file                |
| `MMDFLUX_DEBUG_PIPELINE=<file>` | Write pipeline stages to JSONL           |
| `MMDFLUX_DEBUG_BORDER_NODES=1`  | Print border node trace                  |
| `MMDFLUX_DEBUG_ORDER=1`         | Enable order debug tracing               |
| `MMDFLUX_DEBUG_BK_TRACE=1`      | Trace Brandes-Köpf coordinate assignment |

## Orthogonal Routing Preview

Phase 4 of plan 0075 adds an explicit orthogonal-routing preview path for flowchart
SVG and routed MMDS outputs:

```bash
# Preview orthogonal float-first routing (SVG)
cargo run -- -f svg --edge-routing orthogonal-preview --edge-style straight diagram.mmd

# Use full-compute routing behavior (default)
cargo run -- -f svg --edge-style straight diagram.mmd
```

Notes:
- Default rendering remains `full-compute` unless `--edge-routing orthogonal-preview` is set.
- `--edge-routing full-compute` explicitly selects the default full-compute path.
- The parity harness currently tracks a known straight-SVG delta on
  `simple_cycle.mmd` for orthogonal preview.

### Preview and Rollback Gates

```bash
cargo test svg_snapshot_all_fixtures_straight --test svg_snapshots
cargo test svg_polyline_route_rollback_is_stable_across_repeated_renders --test svg_snapshots
cargo test orthogonal_route_preserves_core_routed_geometry_contracts --test routed_geometry
```

## Troubleshooting

### "Dagre not found" error

Run `./scripts/setup-debug-deps.sh` or set `DAGRE_ROOT` environment variable.

### Parity test failures

1. Check if dagre.js output changed
2. Run `./scripts/refresh-parity-fixtures.sh`
3. Compare diff output to identify divergence

### Missing mermaid.core.mjs

Ensure mermaid was built: `cd deps/mermaid && pnpm run build`
