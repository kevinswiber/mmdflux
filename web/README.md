# mmdflux Web Playground

Static Vite + TypeScript playground for `mmdflux-wasm`.

## Commands

```bash
npm install
npm run test
npm run build
npm run build:release
npm run dev
npm run benchmark:smoke
npm run benchmark:smoke:release
npm run benchmark:full
npm run benchmark:full:release
npm run benchmark:compare -- --baseline <path-to-baseline.json>
npm run benchmark:compare:release -- --baseline <path-to-baseline.json>
```

`npm run dev`, `npm run build`, and `npm run test` call `wasm-pack` to refresh `src/wasm-pkg` from `../crates/mmdflux-wasm`.

`npm run build:release` is the production build path and uses `wasm-pack --release` before `vite build` (used by the Pages deploy workflow).

`npm run benchmark:smoke` also refreshes `src/wasm-pkg`, then runs `scripts/benchmark-smoke.ts` to execute reduced benchmark scenarios against mmdflux and mermaid with conservative CI thresholds.

`npm run benchmark:smoke:release` does the same smoke run but builds WASM with `--release`, which is better for representative performance comparisons.

`benchmark:full*` commands include all scenarios (`small`, `medium`, `large`) and disable threshold enforcement by default so they act as non-gating exploratory runs.

`benchmark:compare*` commands run current smoke benchmarks and compare against a baseline JSON report, including delta metrics (`ΔMean`, `ΔP95`, and speedup).
These scripts tag reports with WASM profile metadata and fail on `dev` vs `release` mismatches (or one-sided missing metadata) to avoid apples-to-oranges deltas.

## Deploy Runbook

- Workflow: `.github/workflows/playground-deploy.yml`
- Trigger options:
  - `mmdflux-v*` tag push (created by `cog bump --package mmdflux`, e.g. `mmdflux-v2.1.0`)
  - Manual `workflow_dispatch`
- Deploys to Cloudflare Pages project `mmdflux-play`
- Production URL: `play.mmdflux.com`
- PR previews: deployed automatically by `playground-ci.yml` on pull requests

Operator sequence:

1. Ensure `web` tests/build and `just wasm-build` are green locally.
2. Run `cog bump --package mmdflux` to create and push the `mmdflux-v*` tag (or run manual workflow dispatch).
3. Confirm the `Build & Deploy Playground` job deploys to Cloudflare Pages.

## Benchmark Runbook

- Benchmark mode URL flag: `?benchmark=true`
- Local usage:
  1. Run `npm run dev`
  2. Open `http://localhost:5173/?benchmark=true`
  3. Click **Run Benchmark**
  4. Optionally click **Export JSON** for a schema-versioned report

The benchmark runner uses mmdflux WASM and mermaid through a shared `warm`/`render` contract and reports `mean`, `median`, `p95`, `min`, and `max` values.

For terminal readability, smoke and compare scripts print fixed-width tables with aligned numeric columns.

Delta workflow example:

1. Capture a baseline:
   `npm run benchmark:smoke:release -- --out .benchmarks/baseline-release.json`
2. Compare current run to baseline:
   `npm run benchmark:compare:release -- --baseline .benchmarks/baseline-release.json`
3. Optional regression gate (fail if either `ΔMean` or `ΔP95` exceeds threshold):
   `npm run benchmark:compare:release -- --baseline .benchmarks/baseline-release.json --max-regression-pct 25`

Full benchmark example (includes `flowchart-large`, non-gating):

`npm run benchmark:full:release -- --out .benchmarks/full-release.json`

Interpretation caveats:

- Results are machine/browser specific and should be compared on the same host/runtime.
- Smoke checks intentionally use reduced scenarios/iterations and are not a replacement for full benchmark studies.
- Benchmark loading is isolated behind route-gated lazy imports to avoid main playground overhead.

## Benchmark Smoke Policy

- Script: `web/scripts/benchmark-smoke.ts`
- Scenarios: `flowchart-small`, `flowchart-medium` (large is intentionally excluded from smoke checks)
- Iterations: `warmup=1`, `measured=3` per scenario/engine
- Thresholds:
  - `mmdflux`: `mean <= 500ms`, `p95 <= 1000ms`
  - `mermaid`: `mean <= 2000ms`, `p95 <= 3500ms`

The goal is to catch catastrophic regressions while avoiding noisy machine-specific failures.

Local benchmark outputs are intentionally untracked; `.benchmarks/` is gitignored for baseline/result files.

CI wiring is optional by default: `playground-ci.yml` exposes a `workflow_dispatch` input (`run_benchmark_smoke`) so operators can run the smoke step on demand.
When running optional smoke checks from `workflow_dispatch`, `benchmark_profile` can be set to `dev` or `release`.

## Included Examples

- Flowchart Basics
- Fan-out
- Sequence Basics
- Sequence Retry
- Class Basics
- Class Interfaces

Examples are wired to the live render pipeline and are useful as smoke fixtures for manual regression checks.
