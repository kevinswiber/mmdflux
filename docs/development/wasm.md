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
  Text and ASCII output ignore this setting and remain pinned to compatibility
  metrics for wrap preparation. Unsupported profile IDs are rejected.
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

## Browser Text Metrics

The existing `render` export remains static and deterministic. It never calls
browser measurement APIs, and importing the browser metrics export does not
change `render` output.

The browser-side adapter for `@mmds/wasm` lives in `@mmds/browser-text-metrics`
(`packages/mmds-browser-text-metrics/`). It exposes
`createMmdsBrowserTextMetricsClient`, `createMmdsMainThreadRenderer`, and
`createWorkerRequestHandler`. See the package README for the worker and
main-thread setup, the capability error contract, and the routing heuristic.
The core adapter invariants — `load(cssFont)` plus post-load `check(cssFont)`
(the adapter intentionally does NOT await `FontFaceSet.ready`), `metricsJson`
as the only dynamic-font source, graph-family SVG only, and a synchronous
finite non-negative `measureText` — are enforced by the package and pinned by
`crates/mmdflux-wasm/tests/web.rs`. Phrased as a one-line rule:
`load` plus post-load `check` is the requested-font contract; `check()`
alone is not proof that a requested font loaded.

`renderWithBrowserTextMetrics(input, format, configJson, metricsJson, measureText)`
is the wasm export used by the adapter. It supports SVG graph-family Mermaid input, dynamic graph-family MMDS output and provider-bound replay, and provider-bound dynamic MMDS-to-SVG replay.

Text and ASCII reject dynamic metrics because browser pixel widths do not
map to the monospaced terminal grid used by those outputs, and
sequence-family diagrams use a separate timeline-family layout path that
would need a dedicated metrics plan before browser measurement could be
honestly applied.

`metricsJson` carries provider identity
(`profileId = "mmdflux-browser-canvas-v1"`, `profileVersion = 1`),
`defaultStyle`, and a `textStyles` array; each style entry supplies
`fontFamily`, `fontSize`, `fontStyle`, `fontWeight`, `lineHeight`, and
`cssFont`. `renderWithBrowserTextMetrics uses metricsJson for font identity`
and is intentionally separate from `configJson`:
`configJson.fontFamily`, `configJson.fontSize`, and `configJson.themeVariables`
are rejected on this export even when they match `metricsJson`.

| Mode | Required browser capabilities | Notes |
| ---- | ----------------------------- | ----- |
| Worker dynamic metrics | worker `FontFaceSet` (`self.fonts`) and `OffscreenCanvas` | Preferred path. Layout and `measureText` stay inside the worker. |
| Main-thread dynamic metrics | `document.fonts` and a normal canvas | Fallback for worker font/canvas capability gaps. This can block the UI during render. |

Mermaid graph styles can override `font-family`, `font-size`, `font-style`, and
`font-weight`, but they do not carry a per-element line-height. mmdflux derives
a styled element's line-height from the default style's line-height ratio. For
example, a default style of `fontSize = 16` and `lineHeight = 24` gives an 8-px
Mermaid-styled label a required `lineHeight = 12` entry in `metricsJson`.
Supplying the same font identity with a different `lineHeight` is rejected so
layout and replay stay keyed to the same measured style.

The dynamic path does not fall back to `mmdflux-sans-v1` or
`mmdflux-heuristic-proportional-v1` after a measurement failure.
When the requested font is unavailable, rendering fails instead of measuring a fallback font and pretending it matched the request.
The `measureText` callback must return a synchronous finite non-negative width
number for each `(text, cssFont)` request; promises, objects, `NaN`,
`Infinity`, negative values, and thrown errors fail the render.

Dynamic MMDS output includes `org.mmdflux.text-measurements.v1`, a
measured-query sidecar keyed by `textStyles`, `lineWidths`, and
`scalarWidths`. That sidecar lets the static `render` export replay
graph-family dynamic MMDS to SVG provider-free when complete.
Missing measured queries remain explicit replay errors. Relayout after
changing constraints is still unsupported.

The playground routes graph-family SVG font styles through browser text metrics when Mermaid `style`, `classDef`/`class`, or `linkStyle` declarations include layout-affecting font properties. Static/default SVG renders without font styling continue to use the portable `render` path. Subgraph container visual styles can replay provider-free, but custom subgraph title font family or size requires dynamic metrics because it changes layout geometry. Text, ASCII, and sequence remain unsupported.

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

Two npm packages are published from this repo via tag-driven workflows:

- `mmdflux-v*` tags (`.github/workflows/wasm-release.yml`) publish the
  generated bundler artifact as `@mmds/wasm`. Triggered by
  `cog bump --package mmdflux`; tag version must equal crate version. Root
  `Cargo.toml` and `crates/mmdflux-wasm/Cargo.toml` versions must match.
- `mmds-browser-text-metrics-v*` tags
  (`.github/workflows/packages-release.yml`) publish the hand-written
  adapter as `@mmds/browser-text-metrics`. Triggered by
  `cog bump --package mmds-browser-text-metrics`. The adapter declares
  `@mmds/wasm` as a peer dependency; the host application installs both.

Required repository setup:

- Preferred (steady state): configure npm trusted publishing for both
  `@mmds/wasm` and `@mmds/browser-text-metrics` in npm package settings,
  linked to the matching GitHub repository workflow.
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
