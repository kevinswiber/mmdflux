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
just wasm-build-release
just wasm-test
just wasm-size
```

- `just wasm-build` compiles the library for `wasm32-unknown-unknown` with
  both `web` and `bundler` wasm-pack targets in dev mode for local browser testing.
- `just wasm-build-release` compiles the shipped `web` and `bundler` artifacts with
  the size-optimized wasm release profile used by CI and npm publishing.
- `just wasm-test` runs browser-executed wasm-bindgen contract tests for
  `crates/mmdflux-wasm`.
- `just wasm-size` builds size-optimized release artifacts (unless `--no-build`
  is supplied) and enforces the CI-equivalent raw/gzip budgets.

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
- `edgePreset` (`straight`, `polyline`, `step`, `smooth-step`, `curved-step`, `basis`)
- `routingStyle` (`direct`, `polyline`, `orthogonal`)
- `curve` (`basis`, `linear`, `linear-sharp`, `linear-rounded`)
- `edgeRadius`
- `svgDiagramPadding`
- `svgNodePaddingX`
- `svgNodePaddingY`
- `showIds`
- `color` (`off`, `auto`, `always`)
- `geometryLevel` (`layout`, `routed`)
- `pathSimplification` (`none`, `lossless`, `lossy`, `minimal`)
- `layout` object:
  - `nodeSep`, `edgeSep`, `rankSep`, `margin`, `ranker`

Notes:

- For SVG output, if `layoutEngine` is omitted, WASM defaults to `flux-layered`.
- `color` only affects text/ascii output. `always` forces ANSI escapes, while `auto`
  resolves to plain text in WASM because there is no terminal-capability probe.
- Release wasm artifacts use a size-optimized Cargo profile:
  `opt-level=z`, `codegen-units=1`, `lto=fat`, `panic=abort`.
- Legacy keys such as `edgeRouting`, `edgeStyle`, `svgEdgeCurve`, and
  `svgEdgeCurveRadius` are rejected.

Example:

```json
{
  "layoutEngine": "flux-layered",
  "edgePreset": "curved-step",
  "edgeRadius": 6,
  "geometryLevel": "routed",
  "pathSimplification": "lossless",
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
just wasm-build-release
just wasm-test
just wasm-size --no-build
```
