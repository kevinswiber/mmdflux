# Wasm Build and Test Commands

Use these reproducible local entrypoints when validating Wasm readiness.

## Prerequisite

Install the Wasm target and wasm-pack once per environment:

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
- `renderWithBrowserTextMetrics(input, format, configJson, metricsJson, measureText)`
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
- `fontMetricsProfile` (default: `mmdflux-sans-v1`; compatibility:
  `mmdflux-heuristic-proportional-v1`)
- `fontFamily`
- `fontSize`
- `themeVariables` object:
  - `fontFamily`
  - `fontSize`
- `showIds`
- `color` (`off`, `auto`, `always`)
- `geometryLevel` (`layout`, `routed`)
- `pathSimplification` (`none`, `lossless`, `lossy`, `minimal`)
- `layout` object:
  - `nodeSep`, `edgeSep`, `rankSep`, `margin`, `ranker`

Notes:

- For SVG output, if `layoutEngine` is omitted, Wasm defaults to `flux-layered`.
- When SVG rendering uses `flux-layered` and no explicit edge style is provided,
  Wasm defaults to the `smooth-step` preset.
- `color` only affects text/ascii output. `always` forces ANSI escapes, while `auto`
  resolves to plain text in Wasm because there is no terminal-capability probe.
- `fontMetricsProfile` defaults to `mmdflux-sans-v1` for direct graph-family
  rendering. The compatibility profile `mmdflux-heuristic-proportional-v1`
  remains available for callers that need the previous heuristic geometry.
  Text and ASCII output ignore this setting and remain pinned to
  compatibility metrics for wrap preparation. Unsupported profile IDs are
  rejected.
- Wasm uses the same static profile tables as native rendering; browser
  `measureText` is not used by these profiles.
- `fontFamily` and `fontSize` are graph text-style inputs, not styling-only SVG
  overrides. Provider-free static rendering accepts them only when they
  normalize to the selected static profile descriptor. A different
  custom graph font style requires dynamic text metrics through the separate dynamic
  export.
- `themeVariables` is intentionally narrow. Only `themeVariables.fontFamily`
  and `themeVariables.fontSize` are accepted; other Mermaid theme variables are
  rejected.
- SVG font-family and metrics profile are intentionally decoupled for static
  profiles: the default recorded profile uses Liberation Sans Regular advances,
  while emitted SVG continues to use the existing Mermaid-style font stack.
- Release wasm artifacts use a size-optimized Cargo profile:
  `opt-level=z`, `codegen-units=1`, `lto=fat`, `panic=abort`.
- Legacy keys such as `edgeRouting`, `edgeStyle`, `svgEdgeCurve`, and
  `svgEdgeCurveRadius` are rejected.

## Experimental Browser Text Metrics

The existing `render` export remains static and deterministic. It never calls
browser measurement APIs, and importing the browser metrics export does not
change `render` output.

`renderWithBrowserTextMetrics(input, format, configJson, metricsJson, measureText)`
is a separate experimental export for browser-owned font measurement. It
supports SVG graph-family Mermaid input, dynamic graph-family MMDS output, and
provider-bound dynamic MMDS-to-SVG replay. Text, ASCII, and sequence-family
diagrams are rejected instead of silently falling back to a static profile.

`metricsJson` is intentionally separate from `configJson`:

```json
{
  "profileId": "mmdflux-browser-canvas-v1",
  "profileVersion": 1,
  "defaultStyle": "s0",
  "textStyles": [
    {
      "id": "s0",
      "fontFamily": "Verdana",
      "fontSize": 8,
      "fontStyle": "normal",
      "fontWeight": "400",
      "lineHeight": 12,
      "cssFont": "normal 400 8px Verdana"
    },
    {
      "id": "s1",
      "fontFamily": "Times New Roman",
      "fontSize": 32,
      "fontStyle": "normal",
      "fontWeight": "400",
      "lineHeight": 48,
      "cssFont": "normal 400 32px Times New Roman"
    }
  ]
}
```

`renderWithBrowserTextMetrics uses metricsJson for font identity`.
`configJson.fontFamily`, `configJson.fontSize`, and `configJson.themeVariables`
are rejected on this export even when they match `metricsJson`.

The JavaScript adapter must complete async font preflight before Rust layout
starts. Both browser modes call `load(cssFont)`, await `ready`, and then use a
post-load `check(cssFont)` validity gate for every entry in `textStyles` before
invoking Rust. `check()` alone is not proof that a requested font loaded.

| Mode | Required browser capabilities | Notes |
| ---- | ----------------------------- | ----- |
| Worker dynamic metrics | worker `FontFaceSet` (`self.fonts`) and `OffscreenCanvas` | Preferred path. Layout and `measureText` stay inside the worker. |
| Main-thread dynamic metrics | `document.fonts` and a normal canvas | Fallback for worker font/canvas capability gaps. This can block the UI during render. |

Main-thread fallback is used only when worker dynamic metrics fail because the
worker lacks font or canvas capabilities. Missing fonts, failed post-load
`check()`, invalid `measureText` output, and Rust render errors remain explicit
failures; they do not retry through another mode or fall back to static
profiles.

The `measureText` callback itself is synchronous from Rust's perspective and
must return a finite non-negative width number for each `(text, cssFont)`
request. Promises, objects, `NaN`, `Infinity`, negative values, and thrown errors
fail the render.

The dynamic path does not fall back to `mmdflux-sans-v1` or
`mmdflux-heuristic-proportional-v1` after a measurement failure. For dynamic
MMDS output and provider-bound replay, `metricsJson` carries provider identity
(`profileId = "mmdflux-browser-canvas-v1"`, `profileVersion = 1`) plus a
style set. Each style supplies `fontFamily`, `fontSize`, `fontStyle`,
`fontWeight`, `lineHeight`, and `cssFont`; the browser callback receives the
CSS font for each measured text query. Dynamic MMDS output includes
`org.mmdflux.text-measurements.v1`, a measured-query sidecar keyed by
`textStyles`, `lineWidths`, and `scalarWidths`. That sidecar lets the static
`render` export replay graph-family dynamic MMDS to SVG provider-free when it
is complete; this is the static `render` export replay path for dynamic MMDS.
Missing measured queries remain explicit replay errors;
Text/ASCII, sequence, and relayout after changing constraints are still
unsupported.

Text and ASCII reject dynamic metrics because browser pixel widths do not map to
the monospaced terminal grid used by those outputs. Sequence diagrams also
reject dynamic metrics: they use a separate timeline-family layout path and
would need a dedicated timeline metrics plan before browser measurement could be
honestly applied.

## Tracing and Diagnostics

The wasm adapter depends on the root `mmdflux` crate with
`default-features = false`, and it does not install a tracing subscriber or
export an `init_logging()` function. Subscriber setup is owned by native
entrypoints such as the CLI, `xtask`, and test harnesses.

Keep `tracing-subscriber`, wasm-specific subscriber setup, and log-interop
features out of the default wasm dependency path unless a browser consumer has
a concrete need for them. For parity dumpers and trace-stream diagnostics, use
the native CLI workflows documented in
[mermaid-parity.md](./mermaid-parity.md).

Example:

```json
{
  "layoutEngine": "flux-layered",
  "edgePreset": "smooth-step",
  "fontMetricsProfile": "mmdflux-sans-v1",
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

Wasm publishing is tag-driven via:

- `.github/workflows/wasm-release.yml`

Rules enforced by the workflow:

- Release is triggered by the `mmdflux-v*` tag (created by `cog bump --package mmdflux`). Tag version must equal crate version.
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
