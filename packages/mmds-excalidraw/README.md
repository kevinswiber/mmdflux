# @mmds/excalidraw

Converts [MMDS](https://github.com/mmdflux/mmdflux/blob/main/docs/mmds.md) JSON (mmdflux's intermediate format) into Excalidraw `.excalidraw` files. Nodes become rectangles, diamonds, or ellipses; edges become polyline arrows with labels. Subgraph membership is preserved as Excalidraw groups.

## Library Usage

```ts
import { convert } from "@mmds/excalidraw";

const { elements, bounds } = convert(mmds);
```

The package root is library-only. CLI behavior lives in the `mmds-to-excalidraw` bin and the `@mmds/excalidraw/cli` entrypoint.

## Install

```bash
npm install -g @mmds/excalidraw
```

Or use directly with npx (no install needed):

```bash
mmdflux --format mmds diagram.mmd | npx @mmds/excalidraw > out.excalidraw
```

## Usage

Pipe MMDS JSON from [mmdflux](https://github.com/mmdflux/mmdflux) into the adapter:

```bash
# Layout-level (straight center-to-center arrows)
mmdflux --format mmds diagram.mmd | npx mmds-to-excalidraw > out.excalidraw

# Routed-level (polyline edge paths from mmdflux's router)
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-excalidraw > out.excalidraw
```

### Options

| Flag | Short | Values | Default | Description |
|------|-------|--------|---------|-------------|
| `--output` | `-o` | `json`, `url` | `json` | Output format — Excalidraw JSON or a shareable excalidraw.com URL |
| `--open` | | | `false` | Upload and open the diagram in your browser |

```bash
# Get a shareable URL instead of JSON
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-excalidraw -o url

# Open directly in browser (also prints JSON to stdout)
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-excalidraw --open

# Open in browser, only print the URL
mmdflux --format mmds --geometry-level routed diagram.mmd | npx mmds-to-excalidraw -o url --open
```

Open the resulting `.excalidraw` file in [excalidraw.com](https://excalidraw.com) or the Excalidraw VS Code extension.

### Geometry levels

- **layout** (default) — node positions and sizes only; edges are drawn as straight lines between node centers.
- **routed** — includes full edge paths with waypoints, producing right-angle polyline arrows that match mmdflux's text output.

### Scale

Node and edge coordinates are scaled from layout units to pixel space. The default scale factor is 3. Override it with the `SCALE` environment variable:

```bash
mmdflux --format mmds diagram.mmd | SCALE=5 npx mmds-to-excalidraw > out.excalidraw
```

## How it works

1. Reads MMDS JSON from stdin
2. Maps MMDS node shapes to Excalidraw types (rectangle, diamond, ellipse) with text-aware sizing
3. Converts edges to Excalidraw arrows, snapping endpoints to node boundaries
4. Computes viewport zoom/scroll to fit the diagram
5. Writes a complete `.excalidraw` JSON document to stdout (or uploads to excalidraw.com with `--output url` / `--open`)
