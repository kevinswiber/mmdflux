# WASM Build and Test Commands

Use these reproducible local entrypoints when validating WASM readiness.

## Prerequisite

Install the WASM target and wasm-pack once per environment:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked
```

`just wasm-test` runs browser tests in headless Chrome. The helper script
`scripts/run-wasm-browser-tests.sh` will auto-detect Chrome/Chromium and
download a matching Chromedriver into `target/chromedriver/` when needed.
You can override detection with `BROWSER=/path/to/chrome` and
`CHROMEDRIVER=/path/to/chromedriver`.

## Commands

```bash
just wasm-build
just wasm-test
```

- `just wasm-build` compiles the library for `wasm32-unknown-unknown` with
  both `web` and `bundler` wasm-pack targets.
- `just wasm-test` runs browser-executed wasm-bindgen contract tests for
  `crates/mmdflux-wasm`.

## Runtime Config Contract

`mmdflux-wasm` exports:

- `render(input, format, configJson)`
- `detect(input)`
- `version()`

`configJson` uses a **strict** camelCase schema. Unknown or legacy keys are
rejected.

Supported top-level keys:

- `layoutEngine` (`flux-layered`, `mermaid-layered`, ...)
- `clusterRanksep`
- `padding`
- `svgScale`
- `edgePreset` (`straight`, `polyline`, `step`, `smoothstep`, `bezier`)
- `routingStyle` (`direct`, `polyline`, `orthogonal`)
- `interpolationStyle` (`linear`, `bezier`)
- `cornerStyle` (`sharp`, `rounded`)
- `edgeRadius`
- `svgDiagramPadding`
- `svgNodePaddingX`
- `svgNodePaddingY`
- `showIds`
- `geometryLevel` (`layout`, `routed`)
- `pathDetail` (`full`, `compact`, `simplified`, `endpoints`)
- `layout` object:
  - `nodeSep`, `edgeSep`, `rankSep`, `margin`, `ranker`

Notes:

- For SVG output, if `layoutEngine` is omitted, WASM defaults to `flux-layered`.
- Legacy keys such as `edgeRouting`, `edgeStyle`, `svgEdgeCurve`, and
  `svgEdgeCurveRadius` are rejected.

Example:

```json
{
  "layoutEngine": "flux-layered",
  "edgePreset": "smoothstep",
  "edgeRadius": 6,
  "geometryLevel": "routed",
  "pathDetail": "compact",
  "layout": {
    "nodeSep": 40,
    "rankSep": 50
  }
}
```

## npm Release Contract

WASM publishing is tag-driven via:

- `.github/workflows/wasm-release.yml`

Rules enforced by the workflow:

- Release tags must be `v*` and tag version must equal crate version.
- Root `Cargo.toml` version and `crates/mmdflux-wasm/Cargo.toml` version must match.
- Bundler package is published to npm as `@mmds/wasm`.

Required repository setup:

- Preferred (steady state): configure npm trusted publishing for
  `@mmds/wasm` in npm package settings, linked to this GitHub repository
  workflow (`.github/workflows/wasm-release.yml`).
- Bootstrap (first publish, before package settings exist): publish once
  manually from a maintainer machine.
- After first publish succeeds: configure trusted publisher in npm settings.
- CI publishing is trusted-publisher only and does not use `NPM_TOKEN`.

Local preflight before tagging:

```bash
cargo test --features cli
just wasm-build
just wasm-test
```
